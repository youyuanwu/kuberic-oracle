use std::ffi::CString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use kuberic_runtime::host::ApplicationStorageState;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::config::OracleRuntimeHostConfig;
use crate::{ObservationTarget, OracleSupportConfig};

pub const RUNTIME_BINDING_SCHEMA_VERSION: u32 = 1;
const BINDING_FILE: &str = "runtime-binding.json";
const MAX_BINDING_BYTES: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBindingIdentity {
    schema_version: u32,
    resource_uid: String,
    replica_id: i64,
    support: OracleSupportConfig,
    target: ObservationTarget,
    application_root: String,
}

impl RuntimeBindingIdentity {
    pub fn from_config(config: &OracleRuntimeHostConfig) -> Result<Self, RuntimeBindingError> {
        let application_root = canonical_root(config.application_root())?;
        Ok(Self {
            schema_version: RUNTIME_BINDING_SCHEMA_VERSION,
            resource_uid: config.resource_uid().as_str().to_owned(),
            replica_id: config.replica_id().value(),
            support: config.service().support().clone(),
            target: config.service().target().clone(),
            application_root: application_root
                .to_str()
                .ok_or(RuntimeBindingError::InvalidPath)?
                .to_owned(),
        })
    }

    pub fn resource_uid(&self) -> &str {
        &self.resource_uid
    }

    pub const fn replica_id(&self) -> i64 {
        self.replica_id
    }

    pub fn support(&self) -> &OracleSupportConfig {
        &self.support
    }

    pub fn target(&self) -> &ObservationTarget {
        &self.target
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeBindingStore {
    root: PathBuf,
    path: PathBuf,
    expected: RuntimeBindingIdentity,
}

impl RuntimeBindingStore {
    pub fn new(config: &OracleRuntimeHostConfig) -> Result<Self, RuntimeBindingError> {
        let root = canonical_root(config.application_root())?;
        let expected = RuntimeBindingIdentity::from_config(config)?;
        Ok(Self {
            path: root.join(BINDING_FILE),
            root,
            expected,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn expected(&self) -> &RuntimeBindingIdentity {
        &self.expected
    }

    pub fn storage_state(&self) -> Result<ApplicationStorageState, RuntimeBindingError> {
        let root = match BoundRoot::open(&self.root) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ApplicationStorageState::FreshEmpty);
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(RuntimeBindingError::UnsafeState);
            }
            Err(_) => return Err(RuntimeBindingError::Io),
        };
        root.verify_path()?;
        let mut entries = root.entries()?;
        let mut recovered = false;
        for entry in &entries {
            let Some(name) = entry.to_str() else {
                return Err(RuntimeBindingError::UnsafeState);
            };
            if name == BINDING_FILE {
                continue;
            }
            if recover_temporary(&root, name)? {
                recovered = true;
            } else {
                return Err(RuntimeBindingError::UnsafeState);
            }
        }
        if recovered {
            root.file.sync_all().map_err(|_| RuntimeBindingError::Io)?;
            root.verify_path()?;
            entries = root.entries()?;
        }
        if entries.is_empty() {
            return Ok(ApplicationStorageState::FreshEmpty);
        }
        if entries.len() != 1 || entries[0] != BINDING_FILE {
            return Err(RuntimeBindingError::UnsafeState);
        }
        let actual = self.load(&root)?;
        root.verify_path()?;
        if actual != self.expected {
            return Err(RuntimeBindingError::IdentityMismatch);
        }
        Ok(ApplicationStorageState::Established)
    }

    pub fn initialize(&self) -> Result<(), RuntimeBindingError> {
        if self.storage_state()? != ApplicationStorageState::FreshEmpty {
            return Err(RuntimeBindingError::AlreadyInitialized);
        }
        if !self.root.exists() {
            DirBuilder::new()
                .recursive(false)
                .mode(0o700)
                .create(&self.root)
                .map_err(|_| RuntimeBindingError::Io)?;
        }
        let root = BoundRoot::open(&self.root).map_err(map_open_error)?;
        root.verify_path()?;
        self.save(&root)
    }

    pub fn validate(&self) -> Result<(), RuntimeBindingError> {
        if self.storage_state()? == ApplicationStorageState::Established {
            Ok(())
        } else {
            Err(RuntimeBindingError::NotInitialized)
        }
    }

    fn load(&self, root: &BoundRoot) -> Result<RuntimeBindingIdentity, RuntimeBindingError> {
        let file = root
            .openat(
                BINDING_FILE,
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
            .map_err(|_| RuntimeBindingError::Io)?;
        let metadata = file.metadata().map_err(|_| RuntimeBindingError::Io)?;
        if !metadata.is_file()
            || metadata.len() > MAX_BINDING_BYTES
            || metadata.uid() != effective_uid()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(RuntimeBindingError::UnsafeState);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BINDING_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RuntimeBindingError::Io)?;
        if bytes.len() as u64 > MAX_BINDING_BYTES {
            return Err(RuntimeBindingError::UnsafeState);
        }
        let identity: RuntimeBindingIdentity =
            serde_json::from_slice(&bytes).map_err(|_| RuntimeBindingError::Malformed)?;
        if identity.schema_version != RUNTIME_BINDING_SCHEMA_VERSION {
            return Err(RuntimeBindingError::UnsupportedSchema(
                identity.schema_version,
            ));
        }
        Ok(identity)
    }

    fn save(&self, root: &BoundRoot) -> Result<(), RuntimeBindingError> {
        let bytes = serde_json::to_vec_pretty(&self.expected)
            .map_err(|_| RuntimeBindingError::Malformed)?;
        let temporary = format!(".{BINDING_FILE}.{}.new", std::process::id());
        let mut file = root
            .openat(
                &temporary,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
            .map_err(|_| RuntimeBindingError::Io)?;
        let result = (|| {
            file.write_all(&bytes)
                .map_err(|_| RuntimeBindingError::Io)?;
            file.sync_all().map_err(|_| RuntimeBindingError::Io)?;
            root.verify_path()?;
            root.renameat(&temporary, BINDING_FILE)?;
            root.file.sync_all().map_err(|_| RuntimeBindingError::Io)
        })();
        if result.is_err() {
            let _ = root.unlinkat(&temporary);
        }
        result
    }
}

struct BoundRoot {
    path: PathBuf,
    file: File,
    device: u64,
    inode: u64,
}

impl BoundRoot {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != effective_uid()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        Ok(Self {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
            file,
        })
    }

    fn verify_path(&self) -> Result<(), RuntimeBindingError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| RuntimeBindingError::Io)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || self
                .path
                .canonicalize()
                .map_err(|_| RuntimeBindingError::Io)?
                != self.path
        {
            return Err(RuntimeBindingError::UnsafeState);
        }
        let bound = self.file.metadata().map_err(|_| RuntimeBindingError::Io)?;
        if bound.dev() != self.device || bound.ino() != self.inode {
            return Err(RuntimeBindingError::UnsafeState);
        }
        Ok(())
    }

    fn openat(&self, name: &str, flags: i32, mode: libc::mode_t) -> std::io::Result<File> {
        let name = CString::new(name)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let descriptor = unsafe { libc::openat(self.file.as_raw_fd(), name.as_ptr(), flags, mode) };
        if descriptor < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(descriptor) })
        }
    }

    fn entries(&self) -> Result<Vec<std::ffi::OsString>, RuntimeBindingError> {
        fs::read_dir(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
            .map_err(|_| RuntimeBindingError::Io)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RuntimeBindingError::Io)
    }

    fn renameat(&self, from: &str, to: &str) -> Result<(), RuntimeBindingError> {
        let from = CString::new(from).map_err(|_| RuntimeBindingError::Io)?;
        let to = CString::new(to).map_err(|_| RuntimeBindingError::Io)?;
        if unsafe {
            libc::renameat(
                self.file.as_raw_fd(),
                from.as_ptr(),
                self.file.as_raw_fd(),
                to.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(RuntimeBindingError::Io)
        }
    }

    fn unlinkat(&self, name: &str) -> Result<(), RuntimeBindingError> {
        let name = CString::new(name).map_err(|_| RuntimeBindingError::Io)?;
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } == 0 {
            Ok(())
        } else {
            Err(RuntimeBindingError::Io)
        }
    }
}

fn recover_temporary(root: &BoundRoot, name: &str) -> Result<bool, RuntimeBindingError> {
    let prefix = format!(".{BINDING_FILE}.");
    let Some(pid) = name
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(".new"))
        .and_then(|value| value.parse::<libc::pid_t>().ok())
        .filter(|pid| *pid > 0)
    else {
        return Ok(false);
    };
    if !process_is_gone(pid)? {
        return Err(RuntimeBindingError::UnsafeState);
    }
    let temporary = root
        .openat(name, libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW, 0)
        .map_err(|_| RuntimeBindingError::UnsafeState)?;
    let metadata = temporary
        .metadata()
        .map_err(|_| RuntimeBindingError::UnsafeState)?;
    if !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(RuntimeBindingError::UnsafeState);
    }
    drop(temporary);
    root.unlinkat(name)?;
    Ok(true)
}

fn process_is_gone(pid: libc::pid_t) -> Result<bool, RuntimeBindingError> {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return Ok(false);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(true),
        Some(libc::EPERM) => Ok(false),
        _ => Err(RuntimeBindingError::Io),
    }
}

fn map_open_error(error: std::io::Error) -> RuntimeBindingError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        RuntimeBindingError::UnsafeState
    } else {
        RuntimeBindingError::Io
    }
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn canonical_root(path: &Path) -> Result<PathBuf, RuntimeBindingError> {
    if !path.is_absolute() {
        return Err(RuntimeBindingError::InvalidPath);
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|_| RuntimeBindingError::InvalidPath)?;
        if metadata.file_type().is_symlink() {
            return Err(RuntimeBindingError::InvalidPath);
        }
        return path
            .canonicalize()
            .map_err(|_| RuntimeBindingError::InvalidPath);
    }
    let parent = path.parent().ok_or(RuntimeBindingError::InvalidPath)?;
    let parent = parent
        .canonicalize()
        .map_err(|_| RuntimeBindingError::InvalidPath)?;
    let name = path.file_name().ok_or(RuntimeBindingError::InvalidPath)?;
    Ok(parent.join(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RuntimeBindingError {
    #[error("runtime application root is invalid")]
    InvalidPath,
    #[error("runtime application state is unsafe")]
    UnsafeState,
    #[error("runtime application binding identity differs")]
    IdentityMismatch,
    #[error("runtime application binding is already initialized")]
    AlreadyInitialized,
    #[error("runtime application binding is not initialized")]
    NotInitialized,
    #[error("runtime application binding is malformed")]
    Malformed,
    #[error("unsupported runtime application binding schema version {0}")]
    UnsupportedSchema(u32),
    #[error("runtime application binding I/O failed")]
    Io,
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    use kuberic_runtime::protocol::types::{PodUid, PvcUid, ReplicaId, ResourceUid};

    use super::*;
    use crate::kuberic::OracleServiceConfig;
    use crate::runtime_host::config::OracleRuntimeHostInput;
    use crate::{
        DatabaseId, DatabaseIncarnationIdentity, DecimalScn, EditionProfile, NativeName,
        ObservationTarget, OracleSupportConfig, PinnedImage, ReplicationProfile,
    };

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn config(root: &Path, database_id: u64) -> OracleRuntimeHostConfig {
        let replication_endpoint = "http://127.0.0.1:45102";
        let service = OracleServiceConfig::new(
            ResourceUid::new("oracle-resource"),
            OracleSupportConfig {
                edition: EditionProfile::Free,
                replication: ReplicationProfile::Standalone,
                replica_count: 1,
                expected_native_major_version: 23,
                image: PinnedImage::parse(format!(
                    "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
                ))
                .unwrap(),
            },
            ObservationTarget {
                database_id: DatabaseId::new(database_id).unwrap(),
                database_unique_name: NativeName::new("unique name", "FREE").unwrap(),
                incarnation: DatabaseIncarnationIdentity {
                    incarnation_number: 3,
                    resetlogs_id: 4,
                    resetlogs_change: DecimalScn::parse("5").unwrap(),
                },
            },
            Duration::from_secs(5),
            1_000,
            replication_endpoint.to_owned(),
        )
        .unwrap();
        OracleRuntimeHostConfig::new(
            OracleRuntimeHostInput {
                resource_uid: ResourceUid::new("oracle-resource"),
                replica_id: ReplicaId::new(1),
                pod_uid: PodUid::new("pod-1"),
                pvc_uid: PvcUid::new("pvc-1"),
                data_root: root.join("metadata"),
                application_root: root.join("application"),
                control_address: SocketAddr::from(([127, 0, 0, 1], 45101)),
                replication_address: SocketAddr::from(([127, 0, 0, 1], 45102)),
                control_endpoint: "http://127.0.0.1:45101".to_owned(),
                replication_endpoint: replication_endpoint.to_owned(),
                bearer_token: "test-token".to_owned(),
                rpc_deadline: Duration::from_secs(5),
                transport_window_capacity: 256,
                shutdown_deadline: Duration::from_secs(5),
            },
            service,
        )
        .unwrap()
    }

    #[test]
    fn initializes_and_validates_exact_binding() {
        let temporary = tempfile::tempdir().unwrap();
        let store = RuntimeBindingStore::new(&config(temporary.path(), 42)).unwrap();
        assert_eq!(
            store.storage_state().unwrap(),
            ApplicationStorageState::FreshEmpty
        );
        store.initialize().unwrap();
        assert_eq!(
            store.storage_state().unwrap(),
            ApplicationStorageState::Established
        );
        store.validate().unwrap();
    }

    #[test]
    fn rejects_target_drift_and_unknown_application_state() {
        let temporary = tempfile::tempdir().unwrap();
        RuntimeBindingStore::new(&config(temporary.path(), 42))
            .unwrap()
            .initialize()
            .unwrap();
        assert_eq!(
            RuntimeBindingStore::new(&config(temporary.path(), 43))
                .unwrap()
                .validate(),
            Err(RuntimeBindingError::IdentityMismatch)
        );

        fs::write(temporary.path().join("application/foreign"), b"foreign").unwrap();
        assert_eq!(
            RuntimeBindingStore::new(&config(temporary.path(), 42))
                .unwrap()
                .storage_state(),
            Err(RuntimeBindingError::UnsafeState)
        );
    }

    #[test]
    fn rejects_symlinked_application_root() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = temporary.path().join("application");
        symlink(&target, &link).unwrap();
        assert_eq!(canonical_root(&link), Err(RuntimeBindingError::InvalidPath));
    }
}

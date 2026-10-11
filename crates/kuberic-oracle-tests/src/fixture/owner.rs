use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::poc::{PocCategory, PocFailure, PocStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    device: u64,
    inode: u64,
    length: u64,
}

impl FileIdentity {
    pub fn from_path(path: &Path) -> Result<Self, PocFailure> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|_| fixture_failure("owned file identity is unavailable"))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(fixture_failure("owned file identity is invalid"));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
        })
    }

    pub fn matches_path(self, path: &Path) -> bool {
        Self::from_path(path).is_ok_and(|current| current == self)
    }
}

pub struct OwnerClaim {
    root: PathBuf,
    lock_path: PathBuf,
    lock_identity: FileIdentity,
    pub token: String,
}

impl OwnerClaim {
    pub fn acquire(root: &Path) -> Result<Self, PocFailure> {
        match std::fs::symlink_metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(root)
                    .map_err(|_| fixture_failure("fixture root could not be created"))?;
            }
            Err(_) => return Err(fixture_failure("fixture root could not be inspected")),
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || !metadata.is_dir()
                    || metadata.uid() != unsafe { libc::geteuid() } =>
            {
                return Err(fixture_failure("fixture root identity is invalid"));
            }
            Ok(_) => {}
        }
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| fixture_failure("fixture root permissions could not be set"))?;
        let metadata = std::fs::symlink_metadata(root)
            .map_err(|_| fixture_failure("fixture root could not be inspected"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(fixture_failure("fixture root identity is invalid"));
        }
        if root
            .read_dir()
            .map_err(|_| fixture_failure("fixture root could not be inspected"))?
            .next()
            .is_some()
        {
            return Err(fixture_failure(
                "fixture root contains an existing ownership record",
            ));
        }
        let lock_path = root.join("owner.lock");
        let token = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| fixture_failure("fixture owner token could not be created"))?
                .as_nanos()
        );
        let mut lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|_| fixture_failure("another Oracle fixture owner is active"))?;
        lock.write_all(token.as_bytes())
            .and_then(|()| lock.sync_all())
            .map_err(|_| fixture_failure("fixture owner claim could not be persisted"))?;
        let lock_identity = FileIdentity::from_path(&lock_path)?;
        Ok(Self {
            root: root.to_path_buf(),
            lock_path,
            lock_identity,
            token,
        })
    }

    pub fn release(self) -> Result<(), PocFailure> {
        if !self.lock_identity.matches_path(&self.lock_path) {
            return Err(cleanup_failure("fixture owner claim identity changed"));
        }
        for entry in self
            .root
            .read_dir()
            .map_err(|_| cleanup_failure("fixture root could not be inspected"))?
        {
            let entry =
                entry.map_err(|_| cleanup_failure("fixture root could not be inspected"))?;
            if entry.path() != self.lock_path {
                return Err(cleanup_failure("fixture root still contains owned files"));
            }
        }
        std::fs::remove_file(&self.lock_path)
            .map_err(|_| cleanup_failure("fixture owner claim could not be removed"))?;
        std::fs::remove_dir(&self.root)
            .map_err(|_| cleanup_failure("fixture root could not be removed"))?;
        Ok(())
    }

    pub fn recover(root: &Path, expected_token: &str) -> Result<Self, PocFailure> {
        let lock_path = root.join("owner.lock");
        let lock_identity = FileIdentity::from_path(&lock_path)?;
        let mut token = String::new();
        std::fs::File::open(&lock_path)
            .and_then(|file| file.take(1024).read_to_string(&mut token))
            .map_err(|_| cleanup_failure("fixture owner claim could not be read"))?;
        if token != expected_token {
            return Err(cleanup_failure("fixture owner claim token changed"));
        }
        Ok(Self {
            root: root.to_path_buf(),
            lock_path,
            lock_identity,
            token,
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OwnershipJournal {
    pub token: String,
    #[serde(default)]
    pub docker_host: String,
    #[serde(default)]
    pub mutation_ambiguous: bool,
    pub network_name: String,
    pub network_expected: bool,
    pub network_id: String,
    pub volume_name: String,
    pub volume_expected: bool,
    pub volume_identity: String,
    pub container_name: String,
    pub container_expected: bool,
    pub container_id: String,
    pub image: String,
    pub host_port: u16,
    pub username_file: Option<(PathBuf, FileIdentity)>,
    pub password_file: Option<(PathBuf, FileIdentity)>,
    pub admin_env_file: Option<(PathBuf, FileIdentity)>,
    pub temporary_files: Vec<(PathBuf, FileIdentity)>,
}

impl OwnershipJournal {
    pub fn persist(&self, root: &Path) -> Result<FileIdentity, PocFailure> {
        let path = root.join("journal.json");
        let temporary = root.join("journal.json.tmp");
        let encoded = serde_json::to_vec_pretty(self)
            .map_err(|_| fixture_failure("fixture journal could not be encoded"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| fixture_failure("fixture journal could not be created"))?;
        file.write_all(&encoded)
            .and_then(|()| file.sync_all())
            .map_err(|_| {
                let _ = std::fs::remove_file(&temporary);
                fixture_failure("fixture journal could not be written")
            })?;
        if std::fs::rename(&temporary, &path).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(fixture_failure("fixture journal could not be published"));
        }
        sync_directory(root, false)?;
        FileIdentity::from_path(&path)
    }

    pub fn load(root: &Path) -> Result<(Self, FileIdentity), PocFailure> {
        let path = root.join("journal.json");
        let temporary = root.join("journal.json.tmp");
        match std::fs::symlink_metadata(&temporary) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(cleanup_failure(
                    "fixture journal temporary file could not be inspected",
                ));
            }
            Ok(_) => {
                let (pending, pending_identity) = read_journal(&temporary)?;
                match std::fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => {
                        return Err(cleanup_failure("fixture journal could not be inspected"));
                    }
                    Ok(_) => {
                        let (current, _) = read_journal(&path)?;
                        if current.token != pending.token {
                            return Err(cleanup_failure("fixture journal temporary owner changed"));
                        }
                    }
                }
                if !pending_identity.matches_path(&temporary) {
                    return Err(cleanup_failure("fixture journal temporary file changed"));
                }
                std::fs::rename(&temporary, &path).map_err(|_| {
                    cleanup_failure("fixture journal temporary file could not be recovered")
                })?;
                sync_directory(root, true)?;
            }
        }
        read_journal(&path)
    }
}

fn read_journal(path: &Path) -> Result<(OwnershipJournal, FileIdentity), PocFailure> {
    let identity = FileIdentity::from_path(path)?;
    let mut encoded = Vec::new();
    File::open(path)
        .and_then(|file| file.take(64 * 1024).read_to_end(&mut encoded))
        .map_err(|_| cleanup_failure("fixture journal could not be read"))?;
    if !identity.matches_path(path) {
        return Err(cleanup_failure("fixture journal changed during read"));
    }
    let journal = serde_json::from_slice(&encoded)
        .map_err(|_| cleanup_failure("fixture journal is invalid"))?;
    Ok((journal, identity))
}

fn sync_directory(root: &Path, cleanup: bool) -> Result<(), PocFailure> {
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| {
            if cleanup {
                cleanup_failure("fixture root could not be synchronized")
            } else {
                fixture_failure("fixture root could not be synchronized")
            }
        })
}

fn fixture_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Fixture, PocCategory::Fixture, message)
}

fn cleanup_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Cleanup, PocCategory::Cleanup, message)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn owner_claim_is_atomic_and_identity_checked() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        assert!(OwnerClaim::acquire(&root).is_err());
        owner.release().unwrap();
        assert!(!root.exists());
    }

    #[test]
    fn owner_claim_rejects_symlinked_fixture_root() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let root = directory.path().join("fixture");
        symlink(&target, &root).unwrap();

        assert!(OwnerClaim::acquire(&root).is_err());
        assert!(target.read_dir().unwrap().next().is_none());
    }

    #[test]
    fn journal_identity_detects_replacement() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: owner.token.clone(),
            ..OwnershipJournal::default()
        };
        let identity = journal.persist(&root).unwrap();
        let path = root.join("journal.json");
        std::fs::rename(&path, root.join("journal.old")).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        assert!(!identity.matches_path(&path));
    }

    #[test]
    fn persisted_journal_and_owner_can_be_recovered() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let token = owner.token.clone();
        let journal = OwnershipJournal {
            token: token.clone(),
            ..OwnershipJournal::default()
        };
        let identity = journal.persist(&root).unwrap();
        std::mem::forget(owner);

        let (loaded, loaded_identity) = OwnershipJournal::load(&root).unwrap();
        assert_eq!(loaded.token, token);
        assert_eq!(loaded_identity, identity);
        let recovered = OwnerClaim::recover(&root, &token).unwrap();
        std::fs::remove_file(root.join("journal.json")).unwrap();
        recovered.release().unwrap();
    }

    #[test]
    fn cleared_and_adopted_resource_identities_round_trip() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let mut journal = OwnershipJournal {
            token: owner.token.clone(),
            container_expected: true,
            volume_expected: true,
            network_expected: true,
            ..OwnershipJournal::default()
        };
        journal.persist(&root).unwrap();

        let (cleared, _) = OwnershipJournal::load(&root).unwrap();
        assert!(cleared.container_id.is_empty());
        assert!(cleared.volume_identity.is_empty());
        assert!(cleared.network_id.is_empty());

        journal.container_id = "container-2".to_owned();
        journal.volume_identity = "created-2|/mount-2|local".to_owned();
        journal.network_id = "network-2".to_owned();
        journal.persist(&root).unwrap();

        let (adopted, _) = OwnershipJournal::load(&root).unwrap();
        assert_eq!(adopted.container_id, "container-2");
        assert_eq!(adopted.volume_identity, "created-2|/mount-2|local");
        assert_eq!(adopted.network_id, "network-2");
    }

    #[test]
    fn load_promotes_complete_owned_temporary_journal() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let current = OwnershipJournal {
            token: owner.token.clone(),
            container_id: "container-1".to_owned(),
            ..OwnershipJournal::default()
        };
        current.persist(&root).unwrap();
        let pending = OwnershipJournal {
            token: owner.token.clone(),
            container_id: "container-2".to_owned(),
            ..OwnershipJournal::default()
        };
        let temporary = root.join("journal.json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&pending).unwrap()).unwrap();
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).unwrap();

        let (loaded, _) = OwnershipJournal::load(&root).unwrap();

        assert_eq!(loaded.container_id, "container-2");
        assert!(!temporary.exists());
    }

    #[test]
    fn load_recovers_first_complete_temporary_journal() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let pending = OwnershipJournal {
            token: owner.token.clone(),
            volume_identity: "volume-1".to_owned(),
            ..OwnershipJournal::default()
        };
        let temporary = root.join("journal.json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&pending).unwrap()).unwrap();
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).unwrap();

        let (loaded, _) = OwnershipJournal::load(&root).unwrap();

        assert_eq!(loaded.volume_identity, "volume-1");
        assert!(root.join("journal.json").exists());
        assert!(!temporary.exists());
    }

    #[test]
    fn load_retains_malformed_temporary_journal_fail_closed() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let _owner = OwnerClaim::acquire(&root).unwrap();
        let temporary = root.join("journal.json.tmp");
        std::fs::write(&temporary, b"not-json").unwrap();
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).unwrap();

        assert!(OwnershipJournal::load(&root).is_err());
        assert!(temporary.exists());
    }
}

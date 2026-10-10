use std::{
    fs::OpenOptions,
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
        if root.exists()
            && root
                .read_dir()
                .map_err(|_| fixture_failure("fixture root could not be inspected"))?
                .next()
                .is_some()
        {
            return Err(fixture_failure(
                "fixture root contains an existing ownership record",
            ));
        }
        std::fs::create_dir_all(root)
            .map_err(|_| fixture_failure("fixture root could not be created"))?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| fixture_failure("fixture root permissions could not be set"))?;
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
        FileIdentity::from_path(&path)
    }

    pub fn load(root: &Path) -> Result<(Self, FileIdentity), PocFailure> {
        let path = root.join("journal.json");
        let identity = FileIdentity::from_path(&path)?;
        let mut encoded = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| file.take(64 * 1024).read_to_end(&mut encoded))
            .map_err(|_| cleanup_failure("fixture journal could not be read"))?;
        if !identity.matches_path(&path) {
            return Err(cleanup_failure("fixture journal changed during read"));
        }
        let journal = serde_json::from_slice(&encoded)
            .map_err(|_| cleanup_failure("fixture journal is invalid"))?;
        Ok((journal, identity))
    }
}

fn fixture_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Fixture, PocCategory::Fixture, message)
}

fn cleanup_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Cleanup, PocCategory::Cleanup, message)
}

#[cfg(test)]
mod tests {
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
}

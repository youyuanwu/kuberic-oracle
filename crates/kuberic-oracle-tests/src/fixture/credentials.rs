use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use zeroize::Zeroizing;

use crate::poc::{PocCategory, PocFailure, PocStage};

use super::owner::FileIdentity;

pub struct FixtureCredentials {
    pub username: String,
    pub password: Zeroizing<String>,
    _admin_password: Zeroizing<String>,
    pub username_file: PathBuf,
    pub password_file: PathBuf,
    pub admin_env_file: PathBuf,
    pub username_identity: FileIdentity,
    pub password_identity: FileIdentity,
    pub admin_env_identity: FileIdentity,
}

impl FixtureCredentials {
    pub fn create(root: &Path) -> Result<Self, PocFailure> {
        let username = "KUBERIC_OBSERVER".to_owned();
        let password = random_hex(24)?;
        let admin_password = random_hex(24)?;
        let username_file = root.join("observer-username");
        let password_file = root.join("observer-password");
        let admin_env_file = root.join("oracle-admin.env");
        let admin_environment = Zeroizing::new(format!("ORACLE_PWD={}\n", admin_password.as_str()));
        let mut created = Vec::new();
        let result = (|| {
            write_owner_file(&username_file, username.as_bytes())?;
            created.push(username_file.clone());
            write_owner_file(&password_file, password.as_bytes())?;
            created.push(password_file.clone());
            write_owner_file(&admin_env_file, admin_environment.as_bytes())?;
            created.push(admin_env_file.clone());
            Ok(Self {
                username,
                password,
                _admin_password: admin_password,
                username_identity: FileIdentity::from_path(&username_file)?,
                password_identity: FileIdentity::from_path(&password_file)?,
                admin_env_identity: FileIdentity::from_path(&admin_env_file)?,
                username_file,
                password_file,
                admin_env_file,
            })
        })();
        if result.is_err() {
            for path in created.into_iter().rev() {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }
}

fn random_hex(bytes: usize) -> Result<Zeroizing<String>, PocFailure> {
    let mut random = Zeroizing::new(vec![0u8; bytes]);
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| fixture_failure("random credential generation failed"))?;
    let mut encoded = Zeroizing::new(String::with_capacity(bytes * 2));
    for byte in random.iter() {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| fixture_failure("random credential encoding failed"))?;
    }
    Ok(encoded)
}

fn write_owner_file(path: &Path, contents: &[u8]) -> Result<(), PocFailure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| fixture_failure("fixture credential file could not be created"))?;
    if file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(fixture_failure(
            "fixture credential file could not be written",
        ));
    }
    Ok(())
}

fn fixture_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Fixture, PocCategory::Fixture, message)
}

#[cfg(test)]
mod tests {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn partial_creation_removes_files_created_by_the_attempt() {
        let directory = tempdir().unwrap();
        let password = directory.path().join("observer-password");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&password)
            .unwrap();

        assert!(FixtureCredentials::create(directory.path()).is_err());
        assert!(!directory.path().join("observer-username").exists());
        assert!(password.exists());
        assert!(!directory.path().join("oracle-admin.env").exists());
    }
}

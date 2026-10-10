use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use zeroize::Zeroizing;

use super::error::{PocCategory, PocFailure, PocStage};

const MAX_SECRET_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub length: u64,
}

#[derive(Debug)]
pub struct SecretValue {
    value: Zeroizing<String>,
    pub identity: FileIdentity,
}

impl SecretValue {
    pub fn expose(&self) -> &str {
        &self.value
    }
}

#[derive(Debug)]
pub struct CredentialPair {
    pub username: SecretValue,
    pub password: SecretValue,
}

pub fn read_credentials(
    username_path: &Path,
    password_path: &Path,
) -> Result<CredentialPair, PocFailure> {
    let username = read_secret(username_path)?;
    let password = read_secret(password_path)?;
    if username.identity.device == password.identity.device
        && username.identity.inode == password.identity.inode
    {
        return Err(secret_failure("credential files must not alias"));
    }
    verify_path_identity(username_path, username.identity)?;
    verify_path_identity(password_path, password.identity)?;
    Ok(CredentialPair { username, password })
}

pub fn read_secret(path: &Path) -> Result<SecretValue, PocFailure> {
    let link_metadata = std::fs::symlink_metadata(path)
        .map_err(|_| secret_failure("credential file is unavailable"))?;
    if link_metadata.file_type().is_symlink() {
        return Err(secret_failure(
            "credential file must not be a symbolic link",
        ));
    }

    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| secret_failure("credential file could not be opened safely"))?;
    let before = checked_metadata(&file)?;
    if link_metadata.dev() != before.device
        || link_metadata.ino() != before.inode
        || link_metadata.len() != before.length
    {
        return Err(secret_failure(
            "credential path changed before the file was opened",
        ));
    }
    let bytes = read_bounded(&mut file, before.length)?;
    let after = checked_metadata(&file)?;
    if before != after {
        return Err(secret_failure("credential file changed during read"));
    }
    verify_path_identity(path, after)?;

    let value = normalize_secret(bytes)?;
    Ok(SecretValue {
        value,
        identity: before,
    })
}

fn verify_path_identity(path: &Path, expected: FileIdentity) -> Result<(), PocFailure> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| secret_failure("credential path changed during read"))?;
    if !metadata.is_file()
        || metadata.dev() != expected.device
        || metadata.ino() != expected.inode
        || metadata.len() != expected.length
    {
        return Err(secret_failure("credential path changed during read"));
    }
    Ok(())
}

fn checked_metadata(file: &File) -> Result<FileIdentity, PocFailure> {
    let metadata = file
        .metadata()
        .map_err(|_| secret_failure("credential metadata is unavailable"))?;
    if !metadata.is_file() {
        return Err(secret_failure("credential path must be a regular file"));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(secret_failure(
            "credential file permissions must be owner-only",
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_SECRET_BYTES as u64 {
        return Err(secret_failure("credential file size is invalid"));
    }
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
    })
}

fn read_bounded(file: &mut File, expected_length: u64) -> Result<Zeroizing<Vec<u8>>, PocFailure> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| secret_failure("credential file could not be read"))?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(expected_length as usize));
    file.by_ref()
        .take(MAX_SECRET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| secret_failure("credential file could not be read"))?;
    if bytes.len() as u64 != expected_length || bytes.len() > MAX_SECRET_BYTES {
        return Err(secret_failure("credential file read was incomplete"));
    }
    Ok(bytes)
}

fn normalize_secret(mut bytes: Zeroizing<Vec<u8>>) -> Result<Zeroizing<String>, PocFailure> {
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    if bytes.is_empty() || bytes.iter().any(|byte| byte.is_ascii_control()) {
        return Err(secret_failure("credential value is invalid"));
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| secret_failure("credential value must be UTF-8"))?;
    Ok(Zeroizing::new(value.to_owned()))
}

fn secret_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::SecretRead, PocCategory::Configuration, message)
}

#[cfg(test)]
mod tests {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn path_replacement_and_resize_invalidate_identity() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("secret");
        write_file(&path, b"secret");
        let secret = read_secret(&path).unwrap();

        std::fs::rename(&path, directory.path().join("old")).unwrap();
        write_file(&path, b"other");
        assert!(verify_path_identity(&path, secret.identity).is_err());

        let resized = directory.path().join("resized");
        write_file(&resized, b"secret");
        let resized_secret = read_secret(&resized).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&resized)
            .unwrap()
            .write_all(b"!")
            .unwrap();
        assert!(verify_path_identity(&resized, resized_secret.identity).is_err());
    }

    fn write_file(path: &Path, contents: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(contents).unwrap();
    }
}

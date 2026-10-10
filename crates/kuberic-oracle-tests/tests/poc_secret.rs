use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{OpenOptionsExt, symlink},
};

use kuberic_oracle_tests::poc::{
    PocCategory, PocStage,
    secret::{read_credentials, read_secret},
};
use tempfile::tempdir;

#[test]
fn reads_owner_only_secret() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("secret");
    write_file(&path, b"observer", 0o600);

    let secret = read_secret(&path).unwrap();

    assert_eq!(secret.expose(), "observer");
    assert_ne!(secret.identity.inode, 0);
}

#[test]
fn rejects_symbolic_link_secret() {
    let directory = tempdir().unwrap();
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    write_file(&target, b"secret", 0o600);
    symlink(&target, &link).unwrap();

    let failure = read_secret(&link).unwrap_err();

    assert_eq!(failure.stage(), PocStage::SecretRead);
    assert_eq!(failure.category(), PocCategory::Configuration);
}

#[test]
fn rejects_hard_link_aliases_between_credentials() {
    let directory = tempdir().unwrap();
    let username = directory.path().join("username");
    let password = directory.path().join("password");
    write_file(&username, b"observer", 0o600);
    std::fs::hard_link(&username, &password).unwrap();

    let failure = read_credentials(&username, &password).unwrap_err();

    assert_eq!(failure.stage(), PocStage::SecretRead);
}

#[test]
fn rejects_permissive_secret_permissions() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("secret");
    write_file(&path, b"secret", 0o644);

    assert!(read_secret(&path).is_err());
}

#[test]
fn rejects_empty_oversized_and_control_values() {
    let directory = tempdir().unwrap();
    let empty = directory.path().join("empty");
    let oversized = directory.path().join("oversized");
    let control = directory.path().join("control");
    let newline = directory.path().join("newline");
    let unicode_control = directory.path().join("unicode-control");
    write_file(&empty, b"", 0o600);
    write_file(&oversized, &vec![b'a'; 4097], 0o600);
    write_file(&control, b"bad\tvalue", 0o600);
    write_file(&newline, b"bad\n", 0o600);
    write_file(&unicode_control, "bad\u{0085}value".as_bytes(), 0o600);

    assert!(read_secret(&empty).is_err());
    assert!(read_secret(&oversized).is_err());
    assert!(read_secret(&control).is_err());
    assert!(read_secret(&newline).is_err());
    assert!(read_secret(&unicode_control).is_err());
}

fn write_file(path: &std::path::Path, contents: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(contents).unwrap();
}

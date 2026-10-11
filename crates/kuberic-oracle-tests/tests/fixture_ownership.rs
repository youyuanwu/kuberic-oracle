use std::sync::Mutex;

use kuberic_oracle_tests::fixture::{FixtureConfig, OracleFixture};
use tempfile::tempdir;

static ENVIRONMENT: Mutex<()> = Mutex::new(());

#[test]
fn preflight_failure_rolls_back_owner_claim() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let directory = tempdir().unwrap();
    let root = directory.path().join("fixture");
    unsafe {
        std::env::set_var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS", "YES");
        std::env::set_var(
            "KUBERIC_ORACLE_DOCKER_HOST",
            "unix:///tmp/kuberic-oracle-missing-docker.sock",
        );
        std::env::set_var("KUBERIC_ORACLE_FIXTURE_ROOT", &root);
    }

    let result = OracleFixture::start(FixtureConfig::from_environment().unwrap());

    unsafe {
        std::env::remove_var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS");
        std::env::remove_var("KUBERIC_ORACLE_DOCKER_HOST");
        std::env::remove_var("KUBERIC_ORACLE_FIXTURE_ROOT");
    }
    assert!(result.is_err());
    assert!(!root.exists());
}

#[test]
fn remote_docker_endpoint_is_rejected_before_fixture_creation() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let directory = tempdir().unwrap();
    let root = directory.path().join("fixture");
    unsafe {
        std::env::set_var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS", "YES");
        std::env::set_var("KUBERIC_ORACLE_DOCKER_HOST", "tcp://127.0.0.1:2375");
        std::env::set_var("KUBERIC_ORACLE_FIXTURE_ROOT", &root);
    }

    let result = FixtureConfig::from_environment();

    unsafe {
        std::env::remove_var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS");
        std::env::remove_var("KUBERIC_ORACLE_DOCKER_HOST");
        std::env::remove_var("KUBERIC_ORACLE_FIXTURE_ROOT");
    }
    assert!(result.is_err());
    assert!(!root.exists());
}

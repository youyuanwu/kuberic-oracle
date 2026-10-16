#![cfg(feature = "kuberic")]

use std::fs;
use std::path::{Path, PathBuf};

use kuberic_oracle::kuberic::{
    KubericAdapterError, ObservationFailureKind, ObservationSourceError, OracleServiceConfig,
    UnsupportedOperation,
};

const SELECTED_KUBERIC_COMMIT: &str = "28b181bb1846d057cce5913513a46ed72de4c977";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(workspace_root().join(path)).unwrap()
}

#[test]
fn public_adapter_types_are_available_only_through_the_kuberic_feature() {
    let _config_type = std::any::type_name::<OracleServiceConfig>();
    let source_error = ObservationSourceError::new(ObservationFailureKind::Deadline);
    assert_eq!(
        source_error.to_string(),
        "Oracle observation failed: deadline exceeded"
    );
    assert!(
        KubericAdapterError::UnsupportedOperation(UnsupportedOperation::BuildReplica)
            .to_string()
            .contains("replica build")
    );
}

#[test]
fn selected_public_v1_dependency_is_exact_and_optional() {
    let workspace = read("Cargo.toml");
    let production = read("crates/kuberic-oracle/Cargo.toml");

    assert!(workspace.contains(SELECTED_KUBERIC_COMMIT));
    assert!(workspace.contains("default-features = false"));
    assert!(workspace.contains("features = [\"host\"]"));
    assert!(production.contains("default = []"));
    assert!(production.contains("kuberic = ["));
    assert!(production.contains("kuberic-runtime = { workspace = true, optional = true }"));
}

#[test]
fn production_adapter_has_no_oracle_driver_or_runtime_executable() {
    let production = read("crates/kuberic-oracle/Cargo.toml");
    let adapter = read("crates/kuberic-oracle/src/kuberic.rs");
    let host = read("crates/kuberic-oracle/src/runtime_host/process.rs");
    let bin_directory = workspace_root().join("crates/kuberic-oracle/src/bin");

    assert!(!production.contains("oracledb"));
    assert!(!adapter.contains("oracledb"));
    assert!(!host.contains("oracledb"));
    assert!(!adapter.contains("docker"));
    assert!(!host.contains("docker"));
    assert!(!bin_directory.exists());
}

#[test]
fn runtime_host_binds_identity_without_adding_a_live_oracle_source() {
    let config = read("crates/kuberic-oracle/src/runtime_host/config.rs");
    let binding = read("crates/kuberic-oracle/src/runtime_host/binding.rs");
    let process = read("crates/kuberic-oracle/src/runtime_host/process.rs");

    for required in [
        "ReplicaProcessConfig",
        "APPLICATION_STORAGE_NAME",
        "RuntimeBindingStore",
        "OpenMode::New",
        "OpenMode::Existing",
        "start_with_shutdown",
        "with_application_storage_paths",
    ] {
        assert!(
            config.contains(required) || binding.contains(required) || process.contains(required),
            "{required}"
        );
    }
    assert!(!process.contains("OracleObservationSource for"));
}

#[test]
fn adapter_history_never_uses_oracle_native_progress() {
    let adapter = read("crates/kuberic-oracle/src/kuberic.rs");

    assert!(adapter.contains("async fn current_progress"));
    assert!(adapter.contains("Ok(0)"));
    assert!(adapter.contains("Ok(INVALID_LSN)"));
    for forbidden in [
        "current_scn.value()",
        "resetlogs_change.value()",
        "activation_number as i64",
        "incarnation_number as i64",
        "impl StateProvider for Oracle",
        "impl StateReplicator for Oracle",
    ] {
        assert!(!adapter.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn unsupported_replication_and_authority_callbacks_remain_explicit() {
    let adapter = read("crates/kuberic-oracle/src/kuberic.rs");
    for required in [
        "UnsupportedOperation::ActiveSecondaryRole",
        "UnsupportedOperation::IdleSecondaryRole",
        "UnsupportedOperation::DataLoss",
        "UnsupportedOperation::CatchUpConfiguration",
        "UnsupportedOperation::CatchUpQuorum",
        "UnsupportedOperation::BuildReplica",
        "UnsupportedOperation::RemoveReplica",
        "service_address: None",
        "ReplicatorCapabilities::conservative()",
    ] {
        assert!(adapter.contains(required), "{required}");
    }
}

use std::path::PathBuf;

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, ContractError, DatabaseId, DatabaseRole,
    EditionProfile, NativeEdition, NativeName, ObservationScope, ObservationTarget,
    ObserverFileConfig, ObserverTimeouts, OracleObserverConfig, OracleSupportConfig, PinnedImage,
    PrincipalPlacement, RawDatabaseRow, RawInstanceRow, RawObservationIdentity, RawObservationRows,
    RawProductRow, ReplicationProfile, admit_observation, parse_observation_rows,
};

const DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn name(value: &str) -> NativeName {
    NativeName::new("test name", value).unwrap()
}

fn support(edition: EditionProfile, replication: ReplicationProfile) -> OracleSupportConfig {
    OracleSupportConfig {
        edition,
        replication,
        replica_count: match replication {
            ReplicationProfile::Standalone => 1,
            ReplicationProfile::DataGuardPhysical => 3,
        },
        expected_native_major_version: 23,
        image: PinnedImage::parse(format!(
            "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
        ))
        .unwrap(),
    }
}

fn identity() -> RawObservationIdentity {
    RawObservationIdentity {
        database_id: "42".to_owned(),
        database_unique_name: "FREE".to_owned(),
        instance_name: "FREE".to_owned(),
        startup_time: "2026-10-10T00:00:00Z".to_owned(),
    }
}

fn rows() -> RawObservationRows {
    RawObservationRows {
        before: identity(),
        database: RawDatabaseRow {
            database_id: "42".to_owned(),
            database_name: "FREE".to_owned(),
            database_unique_name: "FREE".to_owned(),
            database_role: "PRIMARY".to_owned(),
            open_mode: "READ WRITE".to_owned(),
            current_scn: Some("123456789".to_owned()),
            resetlogs_change: "100".to_owned(),
            activation_number: "1".to_owned(),
            log_mode: "ARCHIVELOG".to_owned(),
            force_logging: "YES".to_owned(),
            cdb: "YES".to_owned(),
        },
        instance: RawInstanceRow {
            instance_name: "FREE".to_owned(),
            host_name: "oracle-free-1".to_owned(),
            version_full: "23.26.3.0.0".to_owned(),
            edition: "FREE".to_owned(),
            startup_time: "2026-10-10T00:00:00Z".to_owned(),
            status: "OPEN".to_owned(),
            database_status: "ACTIVE".to_owned(),
        },
        product: RawProductRow {
            product: "Oracle AI Database 26ai Free".to_owned(),
            version: "23.26.0.0.0".to_owned(),
            version_full: "23.26.3.0.0".to_owned(),
            release_status: "Develop, Learn, and Run for Free".to_owned(),
        },
        after: identity(),
    }
}

fn files() -> ObserverFileConfig {
    ObserverFileConfig {
        username_file: PathBuf::from("/run/secrets/oracle-username"),
        password_file: PathBuf::from("/run/secrets/oracle-password"),
        tls_ca_bundle_file: PathBuf::from("/run/secrets/oracle-ca.pem"),
    }
}

fn scope() -> ObservationScope {
    ObservationScope {
        principal_placement: PrincipalPlacement::CommonUserInCdbRoot,
        connection_container: ConnectionContainer::CdbRoot,
        container_data: ContainerDataPolicy::CurrentContainerOnly,
        expected_database_rows: 1,
        expected_instance_rows: 1,
        expected_product_rows: 1,
    }
}

fn timeouts() -> ObserverTimeouts {
    ObserverTimeouts {
        connect_millis: 1_000,
        query_millis: 2_000,
        snapshot_millis: 5_000,
        shutdown_millis: 1_000,
    }
}

#[test]
fn parses_bracketed_standalone_observation() {
    let observation = parse_observation_rows(rows(), 9_900).unwrap();

    assert_eq!(observation.database.database_id.value(), 42);
    assert_eq!(observation.instance.native_edition, NativeEdition::Free);
    assert_eq!(observation.product.native_major_version, 23);
    assert_eq!(
        observation.database.current_scn.unwrap().value(),
        123_456_789
    );
    assert_eq!(observation.product.product, "Oracle AI Database 26ai Free");
}

#[test]
fn mounted_standby_preserves_null_current_scn() {
    let mut raw = rows();
    raw.database.database_role = "PHYSICAL STANDBY".to_owned();
    raw.database.open_mode = "MOUNTED".to_owned();
    raw.database.current_scn = None;
    raw.instance.edition = "EE".to_owned();
    raw.product.product = "Oracle AI Database 26ai Enterprise Edition".to_owned();

    let observation = parse_observation_rows(raw, 9_900).unwrap();
    let admission = admit_observation(
        &support(
            EditionProfile::EnterpriseDeveloper,
            ReplicationProfile::DataGuardPhysical,
        ),
        &ObservationTarget {
            database_id: DatabaseId::new(42).unwrap(),
            database_unique_name: name("FREE"),
        },
        &observation,
        10_000,
        500,
    )
    .unwrap();

    assert_eq!(admission.role, DatabaseRole::PhysicalStandby);
    assert_eq!(admission.current_scn, None);
}

#[test]
fn primary_requires_current_scn_for_admission() {
    let mut raw = rows();
    raw.database.current_scn = None;
    let observation = parse_observation_rows(raw, 9_900).unwrap();

    assert_eq!(
        admit_observation(
            &support(EditionProfile::Free, ReplicationProfile::Standalone),
            &ObservationTarget {
                database_id: DatabaseId::new(42).unwrap(),
                database_unique_name: name("FREE"),
            },
            &observation,
            10_000,
            500,
        ),
        Err(ContractError::CurrentScnRequired)
    );
}

#[test]
fn manual_log_mode_is_rejected() {
    let mut raw = rows();
    raw.database.log_mode = "MANUAL".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "log_mode",
            value: "MANUAL".to_owned(),
        })
    );
}

#[test]
fn standby_nologging_force_state_is_rejected() {
    let mut raw = rows();
    raw.database.force_logging = "NOLOGGING STANDBY DATA AVAILABILITY".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "force_logging",
            value: "NOLOGGING STANDBY DATA AVAILABILITY".to_owned(),
        })
    );
}

#[test]
fn bracket_identity_change_is_rejected() {
    let mut raw = rows();
    raw.after.startup_time = "2026-10-10T00:01:00Z".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::ObservationIdentityChanged)
    );
}

#[test]
fn product_and_instance_major_versions_must_match() {
    let mut raw = rows();
    raw.instance.version_full = "24.1.0.0.0".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::InvalidProductVersion)
    );
}

#[test]
fn malformed_product_version_is_rejected() {
    let mut raw = rows();
    raw.product.version_full = "23.invalid".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::InvalidProductVersion)
    );
}

#[test]
fn empty_product_release_status_is_rejected() {
    let mut raw = rows();
    raw.product.release_status.clear();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::MissingField {
            field: "product_release_status",
        })
    );
}

#[test]
fn unsupported_native_edition_is_rejected() {
    let mut raw = rows();
    raw.instance.edition = "XE".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "edition",
            value: "XE".to_owned(),
        })
    );
}

#[test]
fn unrelated_product_family_is_rejected() {
    let mut raw = rows();
    raw.product.product = "Unrelated Database".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "product",
            value: "Unrelated Database".to_owned(),
        })
    );
}

#[test]
fn configured_and_live_editions_must_match() {
    let observation = parse_observation_rows(rows(), 9_900).unwrap();

    assert_eq!(
        admit_observation(
            &support(
                EditionProfile::EnterpriseDeveloper,
                ReplicationProfile::Standalone,
            ),
            &ObservationTarget {
                database_id: DatabaseId::new(42).unwrap(),
                database_unique_name: name("FREE"),
            },
            &observation,
            10_000,
            500,
        ),
        Err(ContractError::EditionMismatch)
    );
}

#[test]
fn observer_configuration_accepts_independent_polling_cadence() {
    OracleObserverConfig {
        support: support(EditionProfile::Free, ReplicationProfile::Standalone),
        files: files(),
        scope: scope(),
        timeouts: timeouts(),
        poll_interval_millis: 60_000,
    }
    .validate()
    .unwrap();
}

#[test]
fn connect_deadline_must_fit_within_snapshot_deadline() {
    let mut invalid = timeouts();
    invalid.connect_millis = invalid.snapshot_millis + 1;

    assert_eq!(
        invalid.validate(),
        Err(ContractError::DurationExceedsSnapshot {
            field: "connect_millis",
        })
    );
}

#[test]
fn observer_files_must_use_normalized_absolute_paths() {
    let mut invalid = files();
    invalid.password_file = PathBuf::from("../oracle-password");

    assert_eq!(
        invalid.validate(),
        Err(ContractError::InvalidPath {
            field: "password_file",
        })
    );
}

#[test]
fn observer_files_must_be_distinct() {
    let mut invalid = files();
    invalid.password_file = invalid.username_file.clone();

    assert_eq!(
        invalid.validate(),
        Err(ContractError::ObserverFilesMustBeDistinct)
    );
}

#[test]
fn local_pdb_principal_must_connect_to_the_same_pdb() {
    let invalid = ObservationScope {
        principal_placement: PrincipalPlacement::LocalUserInPluggableDatabase {
            name: name("FREEPDB1"),
        },
        connection_container: ConnectionContainer::PluggableDatabase {
            name: name("OTHERPDB"),
        },
        container_data: ContainerDataPolicy::NotApplicable,
        expected_database_rows: 1,
        expected_instance_rows: 1,
        expected_product_rows: 1,
    };

    assert_eq!(
        invalid.validate(),
        Err(ContractError::InvalidContainerScope)
    );
}

#[test]
fn expected_row_counts_must_be_nonzero() {
    let mut invalid = scope();
    invalid.expected_product_rows = 0;

    assert_eq!(
        invalid.validate(),
        Err(ContractError::InvalidRowCount {
            field: "expected_product_rows",
        })
    );
}

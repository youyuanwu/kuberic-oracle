use std::path::PathBuf;

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, ContractError, DatabaseId,
    DatabaseIncarnationIdentity, DatabaseRole, DecimalScn, EditionProfile, NativeEdition,
    NativeName, ObservationScope, ObservationTarget, ObserverFileConfig, ObserverTimeouts,
    OracleObserverConfig, OracleSupportConfig, PinnedImage, PrincipalPlacement,
    RawDatabaseIncarnationRow, RawDatabaseRow, RawInstanceRow, RawObservationIdentity,
    RawObservationRows, RawProductRow, ReplicationProfile, admit_observation,
    parse_observation_rows,
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
        incarnation_number: "3".to_owned(),
        resetlogs_id: "123456789".to_owned(),
        resetlogs_change: "100".to_owned(),
    }
}

fn incarnation() -> RawDatabaseIncarnationRow {
    RawDatabaseIncarnationRow {
        incarnation_number: "3".to_owned(),
        resetlogs_change: "100".to_owned(),
        resetlogs_time: "2026-10-09T23:59:59".to_owned(),
        prior_resetlogs_change: "1".to_owned(),
        prior_resetlogs_time: Some("2026-01-01T00:00:00".to_owned()),
        status: "CURRENT".to_owned(),
        resetlogs_id: "123456789".to_owned(),
        prior_incarnation_number: "2".to_owned(),
        container_id: "0".to_owned(),
    }
}

fn rows() -> RawObservationRows {
    RawObservationRows {
        before: identity(),
        incarnation_before: incarnation(),
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
        incarnation_after: incarnation(),
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
        expected_incarnation_rows: 1,
        expected_instance_rows: 1,
        expected_product_rows: 1,
    }
}

fn target() -> ObservationTarget {
    ObservationTarget {
        database_id: DatabaseId::new(42).unwrap(),
        database_unique_name: name("FREE"),
        incarnation: DatabaseIncarnationIdentity {
            incarnation_number: 3,
            resetlogs_id: 123_456_789,
            resetlogs_change: DecimalScn::parse("100").unwrap(),
        },
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
    assert_eq!(observation.incarnation.identity.incarnation_number, 3);
    assert_eq!(observation.incarnation.identity.resetlogs_id, 123_456_789);
    assert_eq!(
        observation.incarnation.identity.resetlogs_change.value(),
        100
    );
    assert_eq!(observation.incarnation.container_id, 0);
    assert_eq!(observation.instance.native_edition, NativeEdition::Free);
    assert_eq!(observation.product.native_major_version, 23);
    assert_eq!(
        observation.database.current_scn.unwrap().value(),
        123_456_789
    );
    assert_eq!(observation.product.product, "Oracle AI Database 26ai Free");
}

#[test]
fn lineage_preserves_38_digit_resetlogs_changes() {
    let value = "99999999999999999999999999999999999999";
    let mut raw = rows();
    raw.database.resetlogs_change = value.to_owned();
    raw.incarnation_before.resetlogs_change = value.to_owned();
    raw.incarnation_after.resetlogs_change = value.to_owned();
    raw.before.resetlogs_change = value.to_owned();
    raw.after.resetlogs_change = value.to_owned();

    let observation = parse_observation_rows(raw, 9_900).unwrap();

    assert_eq!(
        observation
            .incarnation
            .identity
            .resetlogs_change
            .to_string(),
        value
    );
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
        &target(),
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
            &target(),
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
fn bracket_incarnation_change_is_rejected() {
    let mut raw = rows();
    raw.incarnation_after.resetlogs_id = "123456790".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::ObservationIdentityChanged)
    );
}

#[test]
fn database_and_incarnation_resetlogs_change_must_match() {
    let mut raw = rows();
    raw.database.resetlogs_change = "101".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::ObservationIdentityChanged)
    );
}

#[test]
fn incarnation_scope_status_and_time_are_fail_closed() {
    let mut wrong_scope = rows();
    wrong_scope.incarnation_before.container_id = "3".to_owned();
    wrong_scope.incarnation_after.container_id = "3".to_owned();
    assert_eq!(
        parse_observation_rows(wrong_scope, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "container_id",
            value: "3".to_owned(),
        })
    );

    let mut wrong_status = rows();
    wrong_status.incarnation_before.status = "PARENT".to_owned();
    wrong_status.incarnation_after.status = "PARENT".to_owned();
    assert_eq!(
        parse_observation_rows(wrong_status, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "incarnation_status",
            value: "PARENT".to_owned(),
        })
    );

    let mut malformed_time = rows();
    malformed_time.incarnation_before.resetlogs_time = "2026-02-30T00:00:00".to_owned();
    malformed_time.incarnation_after.resetlogs_time = "2026-02-30T00:00:00".to_owned();
    assert_eq!(
        parse_observation_rows(malformed_time, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "resetlogs_time",
            value: "2026-02-30T00:00:00".to_owned(),
        })
    );
}

#[test]
fn prior_resetlogs_time_is_required_for_a_parent_incarnation() {
    let mut raw = rows();
    raw.incarnation_before.prior_resetlogs_time = None;
    raw.incarnation_after.prior_resetlogs_time = None;

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::MissingField {
            field: "prior_resetlogs_time",
        })
    );
}

#[test]
fn restart_evidence_changes_without_changing_database_lineage() {
    let before = parse_observation_rows(rows(), 9_900).unwrap();
    let mut restarted = rows();
    restarted.before.startup_time = "2026-10-10T00:01:00Z".to_owned();
    restarted.instance.startup_time = "2026-10-10T00:01:00Z".to_owned();
    restarted.after.startup_time = "2026-10-10T00:01:00Z".to_owned();
    let after = parse_observation_rows(restarted, 10_000).unwrap();

    assert_eq!(before.database.database_id, after.database.database_id);
    assert_eq!(before.database.database_name, after.database.database_name);
    assert_eq!(
        before.database.database_unique_name,
        after.database.database_unique_name
    );
    assert_eq!(before.incarnation, after.incarnation);
    assert_ne!(before.instance.startup_time, after.instance.startup_time);
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
            &target(),
            &observation,
            10_000,
            500,
        ),
        Err(ContractError::EditionMismatch)
    );
}

#[test]
fn product_and_live_editions_must_match() {
    let mut raw = rows();
    raw.product.product = "Oracle AI Database 26ai Enterprise Edition".to_owned();

    assert_eq!(
        parse_observation_rows(raw, 9_900),
        Err(ContractError::UnsupportedNativeValue {
            field: "product_edition",
            value: "Oracle AI Database 26ai Enterprise Edition".to_owned(),
        })
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
        expected_incarnation_rows: 1,
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

#[test]
fn incarnation_row_count_must_be_nonzero() {
    let mut invalid = scope();
    invalid.expected_incarnation_rows = 0;

    assert_eq!(
        invalid.validate(),
        Err(ContractError::InvalidRowCount {
            field: "expected_incarnation_rows",
        })
    );
}

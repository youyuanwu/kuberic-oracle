use kuberic_oracle::{
    ContractError, DatabaseId, DatabaseIncarnationIdentity, DatabaseIncarnationSnapshot,
    DatabaseIncarnationStatus, DatabaseRole, DatabaseSnapshot, DecimalScn, EditionProfile,
    ForceLogging, InstanceSnapshot, LogMode, NativeEdition, NativeName, ObservationTarget,
    OpenMode, OracleObservation, OracleSupportConfig, PinnedImage, ProductSnapshot,
    ReplicationProfile, admit_observation,
};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn image() -> PinnedImage {
    PinnedImage::parse(format!(
        "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
    ))
    .unwrap()
}

fn config(
    edition: EditionProfile,
    replication: ReplicationProfile,
    replica_count: u8,
) -> OracleSupportConfig {
    OracleSupportConfig {
        edition,
        replication,
        replica_count,
        expected_native_major_version: 23,
        image: image(),
    }
}

fn name(value: &str) -> NativeName {
    NativeName::new("test name", value).unwrap()
}

fn observation(
    edition: EditionProfile,
    role: DatabaseRole,
    open_mode: OpenMode,
) -> OracleObservation {
    OracleObservation {
        observed_at_unix_millis: 9_900,
        database: DatabaseSnapshot {
            database_id: DatabaseId::new(42).unwrap(),
            database_name: name("FREE"),
            database_unique_name: name("FREE"),
            role,
            open_mode,
            current_scn: Some(DecimalScn::parse("123456789").unwrap()),
            resetlogs_change: DecimalScn::parse("100").unwrap(),
            activation_number: 1,
            log_mode: LogMode::ArchiveLog,
            force_logging: ForceLogging::Enabled,
            is_container_database: true,
        },
        incarnation: DatabaseIncarnationSnapshot {
            identity: DatabaseIncarnationIdentity {
                incarnation_number: 3,
                resetlogs_id: 123_456_789,
                resetlogs_change: DecimalScn::parse("100").unwrap(),
            },
            resetlogs_time: "2026-10-09T23:59:59".to_owned(),
            prior_resetlogs_change: DecimalScn::parse("1").unwrap(),
            prior_resetlogs_time: Some("2026-01-01T00:00:00".to_owned()),
            prior_incarnation_number: 2,
            status: DatabaseIncarnationStatus::Current,
            container_id: 0,
        },
        instance: InstanceSnapshot {
            instance_name: name("FREE"),
            host_name: name("oracle-free-1"),
            version_full: "23.26.3.0.0".to_owned(),
            native_edition: match edition {
                EditionProfile::Free => NativeEdition::Free,
                EditionProfile::EnterpriseDeveloper | EditionProfile::Enterprise => {
                    NativeEdition::Enterprise
                }
            },
            startup_time: "2026-10-10T00:00:00Z".to_owned(),
            status: "OPEN".to_owned(),
            database_status: "ACTIVE".to_owned(),
        },
        product: ProductSnapshot {
            product: match edition {
                EditionProfile::Free => "Oracle AI Database 26ai Free".to_owned(),
                EditionProfile::EnterpriseDeveloper | EditionProfile::Enterprise => {
                    "Oracle AI Database 26ai Enterprise Edition".to_owned()
                }
            },
            version: "23.26.0.0.0".to_owned(),
            version_full: "23.26.3.0.0".to_owned(),
            native_major_version: 23,
            release_status: "Develop, Learn, and Run for Free".to_owned(),
        },
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

#[test]
fn image_requires_digest_pin() {
    assert_eq!(
        PinnedImage::parse("container-registry.oracle.com/database/free:latest"),
        Err(ContractError::InvalidImageDigest)
    );
    assert!(image().as_str().ends_with(DIGEST));
}

#[test]
fn image_round_trips_through_json() {
    let encoded = serde_json::to_string(&image()).unwrap();
    let decoded: PinnedImage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, image());
}

#[test]
fn free_standalone_profile_is_supported() {
    config(EditionProfile::Free, ReplicationProfile::Standalone, 1)
        .validate()
        .unwrap();
}

#[test]
fn free_standalone_profile_requires_one_replica() {
    for actual in [0, 2] {
        assert_eq!(
            config(EditionProfile::Free, ReplicationProfile::Standalone, actual,).validate(),
            Err(ContractError::UnsupportedReplicaCount {
                profile: "standalone",
                expected: 1,
                actual,
            }),
            "{actual}"
        );
    }
}

#[test]
fn free_data_guard_profile_is_rejected() {
    assert_eq!(
        config(
            EditionProfile::Free,
            ReplicationProfile::DataGuardPhysical,
            3,
        )
        .validate(),
        Err(ContractError::DataGuardRequiresEnterprise)
    );
}

#[test]
fn data_guard_profile_requires_three_replicas() {
    assert_eq!(
        config(
            EditionProfile::EnterpriseDeveloper,
            ReplicationProfile::DataGuardPhysical,
            2,
        )
        .validate(),
        Err(ContractError::UnsupportedReplicaCount {
            profile: "data_guard_physical",
            expected: 3,
            actual: 2,
        })
    );
}

#[test]
fn fresh_free_primary_observation_is_admitted() {
    let admission = admit_observation(
        &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
        &target(),
        &observation(
            EditionProfile::Free,
            DatabaseRole::Primary,
            OpenMode::ReadWrite,
        ),
        10_000,
        500,
    )
    .unwrap();

    assert_eq!(admission.role, DatabaseRole::Primary);
    assert_eq!(admission.current_scn.unwrap().value(), 123_456_789);
}

#[test]
fn free_standalone_profile_rejects_standby_role() {
    for role in [
        DatabaseRole::PhysicalStandby,
        DatabaseRole::LogicalStandby,
        DatabaseRole::SnapshotStandby,
        DatabaseRole::FarSync,
    ] {
        assert_eq!(
            admit_observation(
                &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
                &target(),
                &observation(EditionProfile::Free, role, OpenMode::Mounted),
                10_000,
                500,
            ),
            Err(ContractError::UnsupportedDatabaseRole),
            "{role:?}"
        );
    }
}

#[test]
fn stale_observation_is_rejected() {
    assert_eq!(
        admit_observation(
            &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
            &target(),
            &observation(
                EditionProfile::Free,
                DatabaseRole::Primary,
                OpenMode::ReadWrite,
            ),
            20_000,
            500,
        ),
        Err(ContractError::StaleObservation)
    );
}

#[test]
fn future_observation_is_rejected() {
    assert_eq!(
        admit_observation(
            &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
            &target(),
            &observation(
                EditionProfile::Free,
                DatabaseRole::Primary,
                OpenMode::ReadWrite,
            ),
            9_000,
            500,
        ),
        Err(ContractError::FutureObservation)
    );
}

#[test]
fn target_identity_must_match() {
    let mismatched = ObservationTarget {
        database_id: DatabaseId::new(43).unwrap(),
        database_unique_name: name("FREE"),
        incarnation: target().incarnation,
    };

    assert_eq!(
        admit_observation(
            &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
            &mismatched,
            &observation(
                EditionProfile::Free,
                DatabaseRole::Primary,
                OpenMode::ReadWrite,
            ),
            10_000,
            500,
        ),
        Err(ContractError::DatabaseIdentityMismatch)
    );
}

#[test]
fn target_incarnation_must_match_even_when_dbid_matches() {
    for field in ["incarnation_number", "resetlogs_id", "resetlogs_change"] {
        let mut mismatched = target();
        match field {
            "incarnation_number" => mismatched.incarnation.incarnation_number += 1,
            "resetlogs_id" => mismatched.incarnation.resetlogs_id += 1,
            "resetlogs_change" => {
                mismatched.incarnation.resetlogs_change = DecimalScn::parse("101").unwrap()
            }
            _ => unreachable!(),
        }

        assert_eq!(
            admit_observation(
                &config(EditionProfile::Free, ReplicationProfile::Standalone, 1),
                &mismatched,
                &observation(
                    EditionProfile::Free,
                    DatabaseRole::Primary,
                    OpenMode::ReadWrite,
                ),
                10_000,
                500,
            ),
            Err(ContractError::DatabaseIncarnationMismatch),
            "{field}"
        );
    }
}

#[test]
fn admission_revalidates_directly_constructed_lineage_evidence() {
    let support = config(EditionProfile::Free, ReplicationProfile::Standalone, 1);

    let mut mismatched = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    mismatched.database.resetlogs_change = DecimalScn::parse("101").unwrap();
    assert_eq!(
        admit_observation(&support, &target(), &mismatched, 10_000, 500),
        Err(ContractError::ObservationIdentityChanged)
    );

    let mut wrong_scope = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    wrong_scope.incarnation.container_id = 3;
    assert_eq!(
        admit_observation(&support, &target(), &wrong_scope, 10_000, 500),
        Err(ContractError::UnsupportedNativeValue {
            field: "container_id",
            value: "3".to_owned(),
        })
    );

    let mut zero_identity = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    zero_identity.incarnation.identity.incarnation_number = 0;
    assert_eq!(
        admit_observation(&support, &target(), &zero_identity, 10_000, 500),
        Err(ContractError::InvalidUnsignedInteger {
            field: "incarnation_number",
        })
    );

    let mut zero_resetlogs_id = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    zero_resetlogs_id.incarnation.identity.resetlogs_id = 0;
    assert_eq!(
        admit_observation(&support, &target(), &zero_resetlogs_id, 10_000, 500),
        Err(ContractError::InvalidUnsignedInteger {
            field: "resetlogs_id",
        })
    );

    let mut malformed_time = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    malformed_time.incarnation.resetlogs_time = "not-a-date".to_owned();
    assert!(matches!(
        admit_observation(&support, &target(), &malformed_time, 10_000, 500),
        Err(ContractError::UnsupportedNativeValue {
            field: "resetlogs_time",
            ..
        })
    ));

    let mut missing_parent_time = observation(
        EditionProfile::Free,
        DatabaseRole::Primary,
        OpenMode::ReadWrite,
    );
    missing_parent_time.incarnation.prior_resetlogs_time = None;
    assert_eq!(
        admit_observation(&support, &target(), &missing_parent_time, 10_000, 500),
        Err(ContractError::MissingField {
            field: "prior_resetlogs_time",
        })
    );
}

#[test]
fn physical_standby_requires_data_guard_profile() {
    let standby = observation(
        EditionProfile::EnterpriseDeveloper,
        DatabaseRole::PhysicalStandby,
        OpenMode::ReadOnlyWithApply,
    );
    let admission = admit_observation(
        &config(
            EditionProfile::EnterpriseDeveloper,
            ReplicationProfile::DataGuardPhysical,
            3,
        ),
        &target(),
        &standby,
        10_000,
        500,
    )
    .unwrap();

    assert_eq!(admission.role, DatabaseRole::PhysicalStandby);
}

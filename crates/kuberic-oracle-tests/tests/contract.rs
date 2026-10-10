use kuberic_oracle::{
    ContractError, DatabaseId, DatabaseRole, DatabaseSnapshot, DecimalScn, EditionProfile,
    ForceLogging, InstanceSnapshot, LogMode, NativeName, ObservationTarget, OpenMode,
    OracleObservation, OracleSupportConfig, PinnedImage, ProductSnapshot, ReplicationProfile,
    admit_observation,
};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn image() -> PinnedImage {
    PinnedImage::parse(format!(
        "container-registry.oracle.com/database/free:26.0.0@sha256:{DIGEST}"
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
        expected_major_version: 26,
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
            product_major_version: 26,
            edition,
            role,
            open_mode,
            current_scn: Some(DecimalScn::parse("123456789").unwrap()),
            resetlogs_change: DecimalScn::parse("100").unwrap(),
            activation_number: 1,
            log_mode: LogMode::ArchiveLog,
            force_logging: ForceLogging::Enabled,
            is_container_database: true,
        },
        instance: InstanceSnapshot {
            instance_name: name("FREE"),
            host_name: name("oracle-free-1"),
            version_full: "26.1.0.0.0".to_owned(),
            startup_time: "2026-10-10T00:00:00Z".to_owned(),
            status: "OPEN".to_owned(),
            database_status: "ACTIVE".to_owned(),
        },
        product: ProductSnapshot {
            product: "Oracle Database Free".to_owned(),
            version: "26.0.0.0.0".to_owned(),
            version_full: "26.1.0.0.0".to_owned(),
            status: "VALID".to_owned(),
        },
    }
}

fn target() -> ObservationTarget {
    ObservationTarget {
        database_id: DatabaseId::new(42).unwrap(),
        database_unique_name: name("FREE"),
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
fn target_identity_must_match() {
    let mismatched = ObservationTarget {
        database_id: DatabaseId::new(43).unwrap(),
        database_unique_name: name("FREE"),
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

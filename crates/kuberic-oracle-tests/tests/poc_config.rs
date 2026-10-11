use std::path::PathBuf;

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, DatabaseId, EditionProfile, NativeName,
    ObservationScope, ObserverTimeouts, OracleSupportConfig, PinnedImage, PrincipalPlacement,
    ReplicationProfile,
};
use kuberic_oracle_tests::poc::{
    PocAttemptConfig, PocCategory, PocEndpoint, protocol::ObservationRequest,
};

const DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn request() -> ObservationRequest {
    ObservationRequest {
        endpoint: PocEndpoint::loopback(15_221, "FREEPDB1"),
        username_file: PathBuf::from("/run/kuberic-oracle/username"),
        password_file: PathBuf::from("/run/kuberic-oracle/password"),
        support: OracleSupportConfig {
            edition: EditionProfile::Free,
            replication: ReplicationProfile::Standalone,
            replica_count: 1,
            expected_native_major_version: 23,
            image: PinnedImage::parse(format!(
                "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
            ))
            .unwrap(),
        },
        scope: ObservationScope {
            principal_placement: PrincipalPlacement::LocalUserInPluggableDatabase {
                name: NativeName::new("PDB", "FREEPDB1").unwrap(),
            },
            connection_container: ConnectionContainer::PluggableDatabase {
                name: NativeName::new("PDB", "FREEPDB1").unwrap(),
            },
            container_data: ContainerDataPolicy::NotApplicable,
            expected_database_rows: 1,
            expected_incarnation_rows: 1,
            expected_instance_rows: 1,
            expected_product_rows: 1,
        },
        timeouts: ObserverTimeouts {
            connect_millis: 2_000,
            query_millis: 2_000,
            snapshot_millis: 8_000,
            shutdown_millis: 500,
        },
        target_database_id: DatabaseId::new(42).unwrap().value(),
        target_database_unique_name: "FREE".to_owned(),
        target_incarnation_number: 3,
        target_resetlogs_id: 123_456_789,
        target_resetlogs_change: "100".to_owned(),
        maximum_age_millis: 500,
    }
}

#[test]
fn accepts_exact_free_pdb_poc_profile() {
    PocAttemptConfig { request: request() }.validate().unwrap();
}

#[test]
fn rejects_nonloopback_endpoint() {
    let mut request = request();
    request.endpoint.host = "192.0.2.1".parse().unwrap();

    let failure = PocAttemptConfig { request }.validate().unwrap_err();

    assert_eq!(failure.category(), PocCategory::Configuration);
}

#[test]
fn rejects_non_singleton_row_scope() {
    let mut request = request();
    request.scope.expected_product_rows = 2;

    assert!(PocAttemptConfig { request }.validate().is_err());
}

#[test]
fn rejects_deadline_without_shutdown_reserve() {
    let mut request = request();
    request.timeouts.connect_millis = request.timeouts.shutdown_millis;

    assert!(PocAttemptConfig { request }.validate().is_err());
}

#[test]
fn rejects_nonfree_or_wrong_pdb_profile() {
    let mut enterprise = request();
    enterprise.support.edition = EditionProfile::EnterpriseDeveloper;
    assert!(
        PocAttemptConfig {
            request: enterprise
        }
        .validate()
        .is_err()
    );

    let mut wrong_pdb = request();
    wrong_pdb.endpoint.service = "OTHERPDB".to_owned();
    assert!(PocAttemptConfig { request: wrong_pdb }.validate().is_err());
}

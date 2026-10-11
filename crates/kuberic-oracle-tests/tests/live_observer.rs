use std::{os::unix::fs::PermissionsExt, path::Path};

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, EditionProfile, NativeName, ObservationScope,
    ObserverTimeouts, OracleSupportConfig, PinnedImage, PrincipalPlacement, ReplicationProfile,
};
use kuberic_oracle_tests::{
    fixture::{FixtureConfig, OracleFixture},
    poc::{
        Cancellation, PocCategory, PocFailure, WorkerRequest, protocol::ObservationRequest,
        run_worker, run_worker_traced,
    },
};

const IMAGE: &str = "container-registry.oracle.com/database/free@sha256:c4d9b15a1b2240ce90730c23ce298fcdd34f6793792c1b3849181a080009c322";

#[test]
fn observes_owned_oracle_free_member() {
    if std::env::var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS").as_deref() != Ok("YES") {
        eprintln!(
            "Oracle live proof skipped: set KUBERIC_ORACLE_ACCEPT_FREE_TERMS=YES to authorize it"
        );
        return;
    }
    let worker = Path::new(env!("CARGO_BIN_EXE_oracle-poc-worker"));
    let worker_metadata = worker.metadata().unwrap();
    assert!(worker_metadata.is_file());
    assert_ne!(worker_metadata.permissions().mode() & 0o111, 0);
    let config = FixtureConfig::from_environment().unwrap();
    let first_identity = run_first_generation(config.clone()).unwrap();
    let second = OracleFixture::start(config).unwrap();
    let second_identity = (second.database_id(), second.database_incarnation());
    let second_cleanup = second.cleanup();
    if let Err(cleanup) = second_cleanup {
        panic!("second-generation cleanup failed: {cleanup}");
    }
    if first_identity != second_identity {
        panic!("pinned Oracle Free fresh-volume identity behavior changed");
    }
}

fn run_first_generation(
    config: FixtureConfig,
) -> Result<(u64, kuberic_oracle::DatabaseIncarnationIdentity), PocFailure> {
    let mut fixture = OracleFixture::start(config)?;
    let operation = run_cases(&mut fixture);
    let identity = (fixture.database_id(), fixture.database_incarnation());
    let cleanup = fixture.cleanup();
    match (operation, cleanup) {
        (Ok(()), Ok(())) => Ok(identity),
        (Err(operation), Ok(())) => Err(operation),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(operation), Err(cleanup)) => Err(operation.with_cleanup(cleanup)),
    }
}

fn run_cases(fixture: &mut OracleFixture) -> Result<(), PocFailure> {
    let worker = Path::new(env!("CARGO_BIN_EXE_oracle-poc-worker"));
    let observation_request = build_request(fixture, fixture.password_file().to_path_buf())?;
    let trace_path = fixture.connection_trace_path();
    let observation_result = run_worker_traced(
        worker,
        WorkerRequest::Observe(Box::new(observation_request.clone())),
        observation_request.timeouts,
        &Cancellation::default(),
        &trace_path,
    );
    let trace_result = fixture.verify_loopback_trace(&trace_path);
    let trace_cleanup = fixture.remove_connection_trace(&trace_path);
    let observation = observation_result?;
    trace_result?;
    trace_cleanup?;
    if observation.product.product != "Oracle AI Database 26ai Free"
        || observation.product.native_major_version != 23
    {
        return Err(live_failure("live product evidence was unexpected"));
    }

    let (wrong_password, wrong_identity) = fixture.create_wrong_password_file()?;
    let wrong_request = build_request(fixture, wrong_password.clone())?;
    let wrong_result = run_worker(
        worker,
        WorkerRequest::Observe(Box::new(wrong_request.clone())),
        wrong_request.timeouts,
        &Cancellation::default(),
    );
    fixture.remove_owned_file(&wrong_password, wrong_identity)?;
    let wrong_failure = match wrong_result {
        Ok(_) => return Err(live_failure("wrong password unexpectedly succeeded")),
        Err(failure) => failure,
    };
    if wrong_failure.category() != PocCategory::Authentication {
        return Err(live_failure(
            "wrong password was not classified as authentication",
        ));
    }

    for view in [
        "SYS.V_$DATABASE",
        "SYS.V_$INSTANCE",
        "SYS.V_$DATABASE_INCARNATION",
    ] {
        fixture.revoke_view(view)?;
        let denied_request = build_request(fixture, fixture.password_file().to_path_buf())?;
        let denied_result = run_worker(
            worker,
            WorkerRequest::Observe(Box::new(denied_request.clone())),
            denied_request.timeouts,
            &Cancellation::default(),
        );
        fixture.grant_view(view)?;
        fixture.verify_privileges()?;
        let denied = match denied_result {
            Ok(_) => return Err(live_failure("missing grant unexpectedly succeeded")),
            Err(failure) => failure,
        };
        if denied.category() != PocCategory::Authorization {
            return Err(live_failure(
                "missing fixed-view grant was not classified as authorization",
            ));
        }
    }

    Ok(())
}

fn build_request(
    fixture: &OracleFixture,
    password_file: std::path::PathBuf,
) -> Result<ObservationRequest, PocFailure> {
    Ok(ObservationRequest {
        endpoint: fixture.endpoint(),
        username_file: fixture.username_file().to_path_buf(),
        password_file,
        support: OracleSupportConfig {
            edition: EditionProfile::Free,
            replication: ReplicationProfile::Standalone,
            replica_count: 1,
            expected_native_major_version: 23,
            image: PinnedImage::parse(IMAGE).map_err(|_| live_failure("image pin is invalid"))?,
        },
        scope: ObservationScope {
            principal_placement: PrincipalPlacement::LocalUserInPluggableDatabase {
                name: NativeName::new("PDB", "FREEPDB1")
                    .map_err(|_| live_failure("PDB name is invalid"))?,
            },
            connection_container: ConnectionContainer::PluggableDatabase {
                name: NativeName::new("PDB", "FREEPDB1")
                    .map_err(|_| live_failure("PDB name is invalid"))?,
            },
            container_data: ContainerDataPolicy::NotApplicable,
            expected_database_rows: 1,
            expected_incarnation_rows: 1,
            expected_instance_rows: 1,
            expected_product_rows: 1,
        },
        timeouts: ObserverTimeouts {
            connect_millis: 60_000,
            query_millis: 30_000,
            snapshot_millis: 180_000,
            shutdown_millis: 5_000,
        },
        target_database_id: fixture.database_id(),
        target_database_unique_name: fixture.database_unique_name().to_owned(),
        target_incarnation_number: fixture.database_incarnation().incarnation_number,
        target_resetlogs_id: fixture.database_incarnation().resetlogs_id,
        target_resetlogs_change: fixture.database_incarnation().resetlogs_change.to_string(),
        maximum_age_millis: 5_000,
    })
}

fn live_failure(message: &'static str) -> PocFailure {
    PocFailure::fixture_assertion(message)
}

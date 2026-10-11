use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, EditionProfile, NativeName, ObservationScope,
    ObserverTimeouts, OracleObservation, OracleSupportConfig, PinnedImage, PrincipalPlacement,
    ReplicationProfile,
};
use kuberic_oracle_tests::{
    fixture::{FixtureConfig, FixtureResourceEvidence, OracleFixture},
    poc::{
        Cancellation, PocCategory, PocFailure, WorkerRequest, protocol::ObservationRequest,
        run_worker, run_worker_traced,
    },
};

const IMAGE: &str = "container-registry.oracle.com/database/free@sha256:c4d9b15a1b2240ce90730c23ce298fcdd34f6793792c1b3849181a080009c322";
const OPERATION_BUDGET: Duration = Duration::from_secs(25 * 60);
const CLEANUP_RESERVE: Duration = Duration::from_secs(3 * 60);

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
    let started = Instant::now();
    let operation_deadline = started + OPERATION_BUDGET;
    let cleanup_deadline = operation_deadline + CLEANUP_RESERVE;
    let config = FixtureConfig::from_environment().unwrap();
    let mut fixture = OracleFixture::start_until(config, operation_deadline).unwrap();
    let operation = run_cases(&mut fixture, operation_deadline);
    let cleanup = fixture.cleanup_until(cleanup_deadline);
    match (operation, cleanup) {
        (Ok(()), Ok(())) => {}
        (Err(operation), Ok(())) => panic!("operation failed: {operation}"),
        (Ok(()), Err(cleanup)) => panic!("cleanup failed: {cleanup}"),
        (Err(operation), Err(cleanup)) => {
            panic!("operation failed: {operation}; cleanup failed: {cleanup}")
        }
    }
}

fn run_cases(fixture: &mut OracleFixture, operation_deadline: Instant) -> Result<(), PocFailure> {
    let worker = Path::new(env!("CARGO_BIN_EXE_oracle-poc-worker"));
    let observation_request = build_request(
        fixture,
        fixture.password_file().to_path_buf(),
        operation_deadline,
    )?;
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
    let initial_resources = fixture.resource_evidence()?;
    let repeated = observe(fixture, operation_deadline)?;
    require_stable_identity(&observation, &repeated)?;
    if observation.instance.startup_time != repeated.instance.startup_time {
        return Err(live_failure(
            "observer replacement changed instance restart evidence",
        ));
    }

    let (wrong_password, wrong_identity) = fixture.create_wrong_password_file()?;
    let wrong_request = build_request(fixture, wrong_password.clone(), operation_deadline)?;
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
        let denied_request = build_request(
            fixture,
            fixture.password_file().to_path_buf(),
            operation_deadline,
        )?;
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

    let restarted_resources = fixture.restart_container(operation_deadline)?;
    require_same_resources(&initial_resources, &restarted_resources, true)?;
    let restarted = observe(fixture, operation_deadline)?;
    require_stable_identity(&observation, &restarted)?;
    require_restart_changed(&repeated, &restarted)?;

    let recreated_resources = fixture.recreate_container_same_volume(operation_deadline)?;
    require_same_resources(&restarted_resources, &recreated_resources, false)?;
    let recreated = observe(fixture, operation_deadline)?;
    require_stable_identity(&observation, &recreated)?;
    require_restart_changed(&restarted, &recreated)?;

    let original_database_id = fixture.database_id();
    let original_incarnation = fixture.database_incarnation();
    let fresh_resources = fixture.replace_with_fresh_volume(operation_deadline)?;
    if fresh_resources.container_id == recreated_resources.container_id
        || fresh_resources.volume_identity == recreated_resources.volume_identity
    {
        return Err(live_failure(
            "fresh fixture generation did not change container and volume evidence",
        ));
    }
    if fresh_resources.network_id != recreated_resources.network_id
        || fresh_resources.network_owner_label != recreated_resources.network_owner_label
        || fresh_resources.attached_network_id != recreated_resources.network_id
    {
        return Err(live_failure(
            "fresh fixture generation changed retained network evidence",
        ));
    }
    if fixture.database_id() != original_database_id
        || fixture.database_incarnation() != original_incarnation
    {
        return Err(live_failure(
            "pinned Oracle Free fresh-volume identity behavior changed",
        ));
    }

    Ok(())
}

fn observe(
    fixture: &OracleFixture,
    operation_deadline: Instant,
) -> Result<OracleObservation, PocFailure> {
    let worker = Path::new(env!("CARGO_BIN_EXE_oracle-poc-worker"));
    let request = build_request(
        fixture,
        fixture.password_file().to_path_buf(),
        operation_deadline,
    )?;
    run_worker(
        worker,
        WorkerRequest::Observe(Box::new(request.clone())),
        request.timeouts,
        &Cancellation::default(),
    )
}

fn require_stable_identity(
    expected: &OracleObservation,
    observed: &OracleObservation,
) -> Result<(), PocFailure> {
    let expected_database = &expected.database;
    let observed_database = &observed.database;
    if expected_database.database_id != observed_database.database_id
        || expected_database.database_name != observed_database.database_name
        || expected_database.database_unique_name != observed_database.database_unique_name
        || expected_database.resetlogs_change != observed_database.resetlogs_change
        || expected_database.activation_number != observed_database.activation_number
        || expected.incarnation != observed.incarnation
        || expected.instance.instance_name != observed.instance.instance_name
    {
        return Err(live_failure(
            "stable Oracle database identity or lineage evidence changed",
        ));
    }
    Ok(())
}

fn require_restart_changed(
    before: &OracleObservation,
    after: &OracleObservation,
) -> Result<(), PocFailure> {
    if before.instance.startup_time == after.instance.startup_time {
        return Err(live_failure(
            "Oracle instance restart evidence did not change",
        ));
    }
    Ok(())
}

fn require_same_resources(
    before: &FixtureResourceEvidence,
    after: &FixtureResourceEvidence,
    same_container: bool,
) -> Result<(), PocFailure> {
    if (same_container && before.container_id != after.container_id)
        || (!same_container && before.container_id == after.container_id)
        || before.volume_identity != after.volume_identity
        || before.volume_name != after.volume_name
        || before.volume_owner_label != after.volume_owner_label
        || before.network_id != after.network_id
        || before.network_owner_label != after.network_owner_label
        || after.attached_network_id != after.network_id
        || before.container_owner_label != after.container_owner_label
        || before.image != after.image
    {
        return Err(live_failure(
            "fixture restart or recreation evidence was inconsistent",
        ));
    }
    Ok(())
}

fn build_request(
    fixture: &OracleFixture,
    password_file: std::path::PathBuf,
    operation_deadline: Instant,
) -> Result<ObservationRequest, PocFailure> {
    let remaining = operation_deadline.saturating_duration_since(Instant::now());
    if remaining <= Duration::from_secs(6) {
        return Err(PocFailure::fixture_assertion(
            "live proof operation deadline was exhausted",
        ));
    }
    let snapshot_millis = remaining
        .min(Duration::from_secs(180))
        .as_millis()
        .try_into()
        .map_err(|_| live_failure("snapshot deadline was invalid"))?;
    let shutdown_millis = 5_000;
    let operation_millis = snapshot_millis - shutdown_millis;
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
            connect_millis: 60_000u64.min(operation_millis),
            query_millis: 30_000u64.min(operation_millis),
            snapshot_millis,
            shutdown_millis,
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

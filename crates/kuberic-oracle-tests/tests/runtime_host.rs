#![cfg(feature = "kuberic")]

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kuberic_oracle::kuberic::{
    ObservationClock, ObservationSourceError, OracleObservationSource, OracleServiceConfig,
};
use kuberic_oracle::runtime_host::{
    OracleRuntimeHostConfig, OracleRuntimeHostInput, RuntimeProcessError, prepare_runtime_host,
    run_runtime_with_clock_and_shutdown,
};
use kuberic_oracle::{
    DatabaseId, DatabaseIncarnationIdentity, DatabaseIncarnationSnapshot,
    DatabaseIncarnationStatus, DatabaseRole, DatabaseSnapshot, DecimalScn, EditionProfile,
    ForceLogging, InstanceSnapshot, LogMode, NativeEdition, NativeName, ObservationTarget,
    OpenMode, OracleObservation, OracleSupportConfig, PinnedImage, ProductSnapshot,
    ReplicationProfile,
};
use kuberic_runtime::control::{
    normalize_public_v1_agent_status_report, normalize_uninitialized_agent_status_report, proto,
    public_v1_operation_command_to_proto,
};
use kuberic_runtime::host::{ApplicationStorageState, RunningReplica};
use kuberic_runtime::protocol::command::{ProtocolCommand, PublicV1OperationCommand};
use kuberic_runtime::protocol::public_operations::{
    PublicConfiguration, PublicOperationClass, PublicOperationIntent, PublicOperationProgram,
    PublicProgressRole, PublicV1AgentReport, PublicV1Identity,
};
use kuberic_runtime::protocol::types::{
    AccessStatus, ConfigurationDescriptor, ConfigurationMember, Epoch, OperationId, PodUid,
    ProcessSessionId, PvcUid, ReplicaId, ReplicaIdentity, ReplicaInstanceId, ReplicaRole,
    ReplicaStorageIdentity, ResourceUid, derive_agent_generation, derive_initialization_id,
};
use tonic::{Request, transport::Channel};

const RESOURCE_UID: &str = "oracle-runtime-host-test";
const POD_UID: &str = "oracle-runtime-host-pod";
const PVC_UID: &str = "oracle-runtime-host-pvc";
const TOKEN: &str = "oracle-runtime-host-token";
const OBSERVED_AT: u64 = 10_000;
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DEADLINE: Duration = Duration::from_secs(10);

struct RepeatingSource {
    observation: OracleObservation,
}

#[async_trait]
impl OracleObservationSource for RepeatingSource {
    async fn observe(&self) -> Result<OracleObservation, ObservationSourceError> {
        Ok(self.observation.clone())
    }
}

struct FixedClock;

impl ObservationClock for FixedClock {
    fn now_unix_millis(&self) -> Result<u64, ObservationSourceError> {
        Ok(OBSERVED_AT)
    }
}

fn name(value: &str) -> NativeName {
    NativeName::new("test name", value).unwrap()
}

fn support(expected_native_major_version: u16) -> OracleSupportConfig {
    OracleSupportConfig {
        edition: EditionProfile::Free,
        replication: ReplicationProfile::Standalone,
        replica_count: 1,
        expected_native_major_version,
        image: PinnedImage::parse(format!(
            "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
        ))
        .unwrap(),
    }
}

fn target(database_id: u64) -> ObservationTarget {
    ObservationTarget {
        database_id: DatabaseId::new(database_id).unwrap(),
        database_unique_name: name("FREE"),
        incarnation: DatabaseIncarnationIdentity {
            incarnation_number: 3,
            resetlogs_id: 123_456_789,
            resetlogs_change: DecimalScn::parse("100").unwrap(),
        },
    }
}

fn observation() -> OracleObservation {
    OracleObservation {
        observed_at_unix_millis: OBSERVED_AT,
        database: DatabaseSnapshot {
            database_id: DatabaseId::new(42).unwrap(),
            database_name: name("FREE"),
            database_unique_name: name("FREE"),
            role: DatabaseRole::Primary,
            open_mode: OpenMode::ReadWrite,
            current_scn: Some(DecimalScn::parse("123456789").unwrap()),
            resetlogs_change: DecimalScn::parse("100").unwrap(),
            activation_number: 1,
            log_mode: LogMode::ArchiveLog,
            force_logging: ForceLogging::Enabled,
            is_container_database: true,
        },
        incarnation: DatabaseIncarnationSnapshot {
            identity: target(42).incarnation,
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
            native_edition: NativeEdition::Free,
            startup_time: "2026-10-10T00:00:00Z".to_owned(),
            status: "OPEN".to_owned(),
            database_status: "ACTIVE".to_owned(),
        },
        product: ProductSnapshot {
            product: "Oracle AI Database 26ai Free".to_owned(),
            version: "23.26.0.0.0".to_owned(),
            version_full: "23.26.3.0.0".to_owned(),
            native_major_version: 23,
            release_status: "Develop, Learn, and Run for Free".to_owned(),
        },
    }
}

fn free_address() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    address
}

fn runtime_config(
    root: &Path,
    control_address: SocketAddr,
    replication_address: SocketAddr,
    database_id: u64,
    expected_native_major_version: u16,
    application_root: Option<PathBuf>,
) -> OracleRuntimeHostConfig {
    let replication_endpoint = format!("http://{replication_address}");
    let service = OracleServiceConfig::new(
        ResourceUid::new(RESOURCE_UID),
        support(expected_native_major_version),
        target(database_id),
        Duration::from_secs(2),
        1_000,
        replication_endpoint.clone(),
    )
    .unwrap();
    OracleRuntimeHostConfig::new(
        OracleRuntimeHostInput {
            resource_uid: ResourceUid::new(RESOURCE_UID),
            replica_id: ReplicaId::new(1),
            pod_uid: PodUid::new(POD_UID),
            pvc_uid: PvcUid::new(PVC_UID),
            data_root: root.join("metadata"),
            application_root: application_root.unwrap_or_else(|| root.join("application")),
            control_address,
            replication_address,
            control_endpoint: format!("http://{control_address}"),
            replication_endpoint,
            bearer_token: TOKEN.to_owned(),
            rpc_deadline: Duration::from_secs(2),
            transport_window_capacity: 64,
            shutdown_deadline: DEADLINE,
        },
        service,
    )
    .unwrap()
}

fn replica_identity() -> ReplicaIdentity {
    let resource_uid = ResourceUid::new(RESOURCE_UID);
    let replica_id = ReplicaId::new(1);
    let pod_uid = PodUid::new(POD_UID);
    let pvc_uid = PvcUid::new(PVC_UID);
    let initialization = derive_initialization_id(&resource_uid, replica_id, &pod_uid, &pvc_uid);
    ReplicaIdentity {
        replica_id,
        instance_id: ReplicaInstanceId::new(POD_UID),
        agent_generation: derive_agent_generation(&initialization),
    }
}

fn singleton_configuration() -> ConfigurationDescriptor {
    let identity = replica_identity();
    ConfigurationDescriptor::new(
        Epoch::new(0, 1),
        identity.replica_id,
        vec![ConfigurationMember {
            identity,
            role: ReplicaRole::Primary,
        }],
        1,
    )
}

fn public_configuration() -> PublicConfiguration {
    PublicConfiguration {
        current: singleton_configuration(),
        previous: None,
        replicas: Vec::new(),
    }
}

fn authorized<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    request
}

async fn status(
    control_address: SocketAddr,
) -> (
    proto::agent_control_client::AgentControlClient<Channel>,
    proto::AgentStatusReport,
) {
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        let last_error = match proto::agent_control_client::AgentControlClient::connect(format!(
            "http://{control_address}"
        ))
        .await
        {
            Ok(mut client) => {
                match client
                    .get_status(authorized(proto::GetAgentStatusRequest {
                        protocol_version: kuberic_runtime::protocol::PROTOCOL_VERSION,
                        resource_uid: RESOURCE_UID.to_owned(),
                        replica_id: 1,
                        expected_instance_id: POD_UID.to_owned(),
                    }))
                    .await
                {
                    Ok(report) => return (client, report.into_inner()),
                    Err(error) => error.to_string(),
                }
            }
            Err(error) => error.to_string(),
        };
        assert!(
            tokio::time::Instant::now() < deadline,
            "public agent endpoint did not become ready: {last_error}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn initialization_request(process_session_id: String) -> Request<proto::ExecuteCommandRequest> {
    let resource_uid = ResourceUid::new(RESOURCE_UID);
    let replica_id = ReplicaId::new(1);
    let pod_uid = PodUid::new(POD_UID);
    let pvc_uid = PvcUid::new(PVC_UID);
    let initialization = derive_initialization_id(&resource_uid, replica_id, &pod_uid, &pvc_uid);
    let identity = replica_identity();
    authorized(proto::ExecuteCommandRequest {
        protocol_version: kuberic_runtime::protocol::PROTOCOL_VERSION,
        resource_uid: RESOURCE_UID.to_owned(),
        target: Some(identity.clone().into()),
        expected_process_session_id: process_session_id,
        command: Some(
            proto::execute_command_request::Command::InitializeAgentStore(
                proto::InitializeAgentStoreCommand {
                    initialization_id: initialization.to_string(),
                    resource_uid: RESOURCE_UID.to_owned(),
                    local_replica_id: 1,
                    expected_instance_id: POD_UID.to_owned(),
                    expected_pod_uid: POD_UID.to_owned(),
                    expected_pvc_uid: PVC_UID.to_owned(),
                    assigned_agent_generation: identity.agent_generation.to_string(),
                    effective_policy: Some(proto::EffectivePolicy {
                        replica_set_size: 1,
                        write_quorum: 1,
                        read_quorum: 1,
                        failover_delay_seconds: 30,
                    }),
                    bootstrap_configuration: Some(singleton_configuration().into()),
                    provisioning: None,
                },
            ),
        ),
    })
}

fn operation_intent(
    process_session_id: ProcessSessionId,
    revision: u64,
    operation_id: &str,
    program: PublicOperationProgram,
) -> PublicOperationIntent {
    let class = match program {
        PublicOperationProgram::Close => PublicOperationClass::Close,
        PublicOperationProgram::Abort => PublicOperationClass::Abort,
        _ => PublicOperationClass::Authority,
    };
    PublicOperationIntent {
        identity: PublicV1Identity::new(1),
        operation_id: OperationId::new(operation_id),
        revision,
        process_session_id,
        class,
        input_digest: program.digest(),
        replay_disposition: program.replay_disposition(),
        lifecycle: None,
        program: Some(program),
    }
}

fn operation_command(
    process_session_id: ProcessSessionId,
    revision: u64,
    operation_id: &str,
    program: PublicOperationProgram,
) -> PublicV1OperationCommand {
    let identity = replica_identity();
    PublicV1OperationCommand {
        target: identity.clone(),
        intent: operation_intent(process_session_id, revision, operation_id, program),
    }
}

fn operation_request(
    process_session_id: ProcessSessionId,
    revision: u64,
    operation_id: &str,
    program: PublicOperationProgram,
) -> Request<proto::ExecuteCommandRequest> {
    let identity = replica_identity();
    let command = operation_command(process_session_id.clone(), revision, operation_id, program);
    authorized(proto::ExecuteCommandRequest {
        protocol_version: kuberic_runtime::protocol::PROTOCOL_VERSION,
        resource_uid: RESOURCE_UID.to_owned(),
        target: Some(identity.into()),
        expected_process_session_id: process_session_id.to_string(),
        command: Some(proto::execute_command_request::Command::PublicV1Operation(
            public_v1_operation_command_to_proto(&command).unwrap(),
        )),
    })
}

async fn execute_program(
    client: &mut proto::agent_control_client::AgentControlClient<Channel>,
    process_session_id: &ProcessSessionId,
    revision: u64,
    operation_id: &str,
    program: PublicOperationProgram,
) -> PublicV1AgentReport {
    let response = client
        .execute(operation_request(
            process_session_id.clone(),
            revision,
            operation_id,
            program,
        ))
        .await
        .unwrap_or_else(|error| panic!("{operation_id} failed: {error}"))
        .into_inner();
    normalize_public_v1_agent_status_report(
        response
            .observation
            .expect("public operation response observation"),
    )
    .unwrap()
}

async fn initialize_fresh_host(
    host: kuberic_oracle::runtime_host::OracleReplicaHost,
    control_address: SocketAddr,
) -> RunningReplica {
    let mut startup = tokio::spawn(host.start());
    let (mut client, report) = status(control_address).await;
    assert_eq!(
        report.storage_state,
        proto::AgentStorageState::Uninitialized as i32
    );
    assert_eq!(report.resource_uid, RESOURCE_UID);
    assert_eq!(report.pod_uid, POD_UID);
    assert_eq!(report.pvc_uid, PVC_UID);
    let response = client
        .execute(initialization_request(report.process_session_id))
        .await
        .unwrap()
        .into_inner();
    normalize_uninitialized_agent_status_report(
        response
            .observation
            .expect("initialization response observation"),
    )
    .unwrap();
    tokio::time::timeout(DEADLINE, &mut startup)
        .await
        .expect("fresh host initialization timed out")
        .expect("fresh host startup task failed")
        .expect("fresh host failed")
}

async fn stop(mut running: RunningReplica) {
    running.shutdown();
    tokio::time::timeout(DEADLINE, running.wait())
        .await
        .expect("runtime host shutdown timed out")
        .expect("runtime host shutdown failed");
}

fn assert_primary_report(report: &PublicV1AgentReport, session: &ProcessSessionId) {
    let identity = replica_identity();
    assert_eq!(report.identity, PublicV1Identity::new(1));
    assert_eq!(report.resource_uid, ResourceUid::new(RESOURCE_UID));
    assert_eq!(report.replica, identity);
    assert_eq!(&report.process_session_id, session);
    assert_eq!(report.role, ReplicaRole::Primary);
    assert_eq!(report.read_status, AccessStatus::Granted);
    assert_eq!(report.write_status, AccessStatus::Granted);
    assert_eq!(report.progress.role, PublicProgressRole::Primary);
    assert_eq!(report.progress.first, 0);
    assert_eq!(report.progress.last, 0);
    assert!(report.service_location.is_none());
    assert_eq!(report.storage_incarnation.replica, replica_identity());
    assert_eq!(
        report.storage_incarnation.storage,
        ReplicaStorageIdentity::Persistent {
            pod_uid: PodUid::new(POD_UID),
            pvc_uid: PvcUid::new(PVC_UID),
        }
    );
    assert_eq!(report.history.resource_uid, ResourceUid::new(RESOURCE_UID));
}

#[tokio::test(flavor = "current_thread")]
async fn real_replica_host_proves_oracle_singleton_lifecycle_restart_and_binding() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temporary = tempfile::tempdir().unwrap();
        let control_address = free_address();
        let replication_address = free_address();
        let config = runtime_config(
            temporary.path(),
            control_address,
            replication_address,
            42,
            23,
            None,
        );
        let source: Arc<dyn OracleObservationSource> = Arc::new(RepeatingSource {
            observation: observation(),
        });
        let clock: Arc<dyn ObservationClock> = Arc::new(FixedClock);

        let (application, host) =
            prepare_runtime_host(&config, source.clone(), clock.clone()).unwrap();
        assert_eq!(
            application.storage_state().unwrap(),
            ApplicationStorageState::FreshEmpty
        );
        let running = initialize_fresh_host(host, control_address).await;
        let (mut client, initial_wire) = status(control_address).await;
        let initial = normalize_public_v1_agent_status_report(initial_wire).unwrap();
        let first_session = initial.process_session_id;

        execute_program(
            &mut client,
            &first_session,
            1,
            "open-new",
            PublicOperationProgram::Open {
                replica: replica_identity(),
                existing: false,
            },
        )
        .await;
        assert!(application.service().replicator().is_some());
        execute_program(
            &mut client,
            &first_session,
            2,
            "primary-new",
            PublicOperationProgram::Role {
                epoch: Epoch::new(0, 1),
                role: ReplicaRole::Primary,
            },
        )
        .await;
        execute_program(
            &mut client,
            &first_session,
            3,
            "epoch-new",
            PublicOperationProgram::Epoch {
                epoch: Epoch::new(0, 1),
            },
        )
        .await;
        execute_program(
            &mut client,
            &first_session,
            4,
            "singleton-new",
            PublicOperationProgram::Configuration(public_configuration()),
        )
        .await;
        let first_report = execute_program(
            &mut client,
            &first_session,
            5,
            "progress-new",
            PublicOperationProgram::Progress { capability: false },
        )
        .await;
        assert_primary_report(&first_report, &first_session);
        stop(running).await;

        let (restarted_application, restarted_host) =
            prepare_runtime_host(&config, source.clone(), clock.clone()).unwrap();
        assert_eq!(
            restarted_application.storage_state().unwrap(),
            ApplicationStorageState::Established
        );
        let mut restarted = tokio::time::timeout(DEADLINE, restarted_host.start())
            .await
            .expect("established host restart timed out")
            .expect("established host restart failed");
        let (mut restarted_client, restarted_wire) = status(control_address).await;
        let restarted_report = normalize_public_v1_agent_status_report(restarted_wire).unwrap();
        let restarted_session = restarted_report.process_session_id;
        assert_ne!(restarted_session, first_session);

        let stale = restarted_client
            .execute(operation_request(
                first_session,
                6,
                "stale-progress",
                PublicOperationProgram::Progress { capability: false },
            ))
            .await
            .unwrap_err();
        assert_eq!(stale.code(), tonic::Code::FailedPrecondition);
        assert!(stale.message().contains("stale agent process session"));

        execute_program(
            &mut restarted_client,
            &restarted_session,
            6,
            "open-existing",
            PublicOperationProgram::Open {
                replica: replica_identity(),
                existing: true,
            },
        )
        .await;
        execute_program(
            &mut restarted_client,
            &restarted_session,
            7,
            "primary-existing",
            PublicOperationProgram::Role {
                epoch: Epoch::new(0, 1),
                role: ReplicaRole::Primary,
            },
        )
        .await;
        execute_program(
            &mut restarted_client,
            &restarted_session,
            8,
            "epoch-existing",
            PublicOperationProgram::Epoch {
                epoch: Epoch::new(0, 1),
            },
        )
        .await;
        execute_program(
            &mut restarted_client,
            &restarted_session,
            9,
            "singleton-existing",
            PublicOperationProgram::Configuration(public_configuration()),
        )
        .await;
        let restarted_report = execute_program(
            &mut restarted_client,
            &restarted_session,
            10,
            "progress-existing",
            PublicOperationProgram::Progress { capability: false },
        )
        .await;
        assert_primary_report(&restarted_report, &restarted_session);
        restarted
            .handle()
            .execute_public_v1_command(ProtocolCommand::PublicV1Operation(Box::new(
                operation_command(
                    restarted_session.clone(),
                    11,
                    "close-existing",
                    PublicOperationProgram::Close,
                ),
            )))
            .await
            .unwrap();
        assert!(restarted_application.service().replicator().is_none());
        restarted.shutdown();
        tokio::time::timeout(DEADLINE, restarted.wait())
            .await
            .expect("closed host shutdown timed out")
            .expect("closed host shutdown failed");

        let changed_target = runtime_config(
            temporary.path(),
            control_address,
            replication_address,
            43,
            23,
            None,
        );
        let target_error =
            match prepare_runtime_host(&changed_target, source.clone(), clock.clone()) {
                Err(error) => error,
                Ok(_) => panic!("changed Oracle target unexpectedly prepared"),
            };
        assert!(matches!(target_error, RuntimeProcessError::Binding(_)));
        assert!(
            target_error
                .to_string()
                .contains("binding identity differs")
        );

        let changed_support = runtime_config(
            temporary.path(),
            control_address,
            replication_address,
            42,
            24,
            None,
        );
        let support_error =
            match prepare_runtime_host(&changed_support, source.clone(), clock.clone()) {
                Err(error) => error,
                Ok(_) => panic!("changed Oracle support unexpectedly prepared"),
            };
        assert!(matches!(support_error, RuntimeProcessError::Binding(_)));
        assert!(
            support_error
                .to_string()
                .contains("binding identity differs")
        );

        let changed_root = runtime_config(
            temporary.path(),
            control_address,
            replication_address,
            42,
            23,
            Some(temporary.path().join("moved-application")),
        );
        let (moved_application, moved_host) =
            prepare_runtime_host(&changed_root, source, clock).unwrap();
        assert_eq!(
            moved_application.storage_state().unwrap(),
            ApplicationStorageState::FreshEmpty
        );
        let moved_result = tokio::time::timeout(DEADLINE, moved_host.start())
            .await
            .expect("changed application root rejection timed out");
        let moved_error = match moved_result {
            Err(error) => error,
            Ok(_) => panic!("changed application root unexpectedly started"),
        };
        assert!(
            moved_error
                .to_string()
                .contains("application storage paths differ")
        );
    })
    .await
    .expect("server-free real Oracle ReplicaHost proof timed out");
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_orchestration_cancels_uninitialized_startup() {
    let temporary = tempfile::tempdir().unwrap();
    let config = runtime_config(
        temporary.path(),
        free_address(),
        free_address(),
        42,
        23,
        None,
    );
    let source: Arc<dyn OracleObservationSource> = Arc::new(RepeatingSource {
        observation: observation(),
    });
    let clock: Arc<dyn ObservationClock> = Arc::new(FixedClock);

    tokio::time::timeout(
        DEADLINE,
        run_runtime_with_clock_and_shutdown(config, source, clock, async { Ok(()) }),
    )
    .await
    .expect("startup cancellation exceeded its outer deadline")
    .expect("startup cancellation failed");
}

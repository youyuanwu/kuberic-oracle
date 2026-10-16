use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kuberic_oracle::kuberic::{
    ObservationSourceError, OracleObservationSource, OracleServiceConfig, SystemObservationClock,
};
use kuberic_oracle::runtime_host::{
    OracleRuntimeHostConfig, OracleRuntimeHostInput, prepare_runtime_host,
};
use kuberic_oracle::{ObservationTarget, OracleObservation};
use kuberic_runtime::protocol::command::{ProtocolCommand, PublicV1OperationCommand};
use kuberic_runtime::protocol::public_operations::{
    PublicConfiguration, PublicOperationClass, PublicOperationIntent, PublicOperationProgram,
    PublicV1Identity,
};
use kuberic_runtime::protocol::types::{
    ConfigurationDescriptor, ConfigurationMember, EffectivePolicy, Epoch, OperationId, PodUid,
    ProcessSessionId, PublicProtocolGeneration, PvcUid, ReplicaId, ReplicaIdentity,
    ReplicaInstanceId, ReplicaRole, ReplicaStorageIdentity, ReplicaStorageIncarnationContext,
    ResourceUid, StorageGeneration, derive_agent_generation, derive_bootstrap_history,
    derive_history_initialization_id, derive_initialization_id,
};
use kuberic_runtime::testing::sqlite_store::SqliteStore;
use kuberic_runtime::testing::state::{SCHEMA_VERSION, StorageIdentity};

use super::{PocCategory, PocFailure};

const RUNTIME_RESOURCE_UID: &str = "oracle-live-runtime";
const RUNTIME_POD_UID: &str = "oracle-live-runtime-pod";
const RUNTIME_PVC_UID: &str = "oracle-live-runtime-pvc";
const RUNTIME_TOKEN: &str = "******";
const RUNTIME_DEADLINE: Duration = Duration::from_secs(60);
const RUNTIME_ATTEMPTS: usize = 3;

#[derive(Clone)]
pub struct WorkerObservationSource {
    observation: OracleObservation,
    support: kuberic_oracle::OracleSupportConfig,
    observation_timeout: Duration,
    maximum_age_millis: u64,
}

impl WorkerObservationSource {
    pub fn new(
        observation: OracleObservation,
        support: kuberic_oracle::OracleSupportConfig,
        observation_timeout: Duration,
        maximum_age_millis: u64,
    ) -> Self {
        Self {
            observation,
            support,
            observation_timeout,
            maximum_age_millis,
        }
    }

    pub async fn exercise_runtime_host(&self, root: &Path) -> Result<(), PocFailure> {
        let mut last_failure = None;
        for attempt in 1..=RUNTIME_ATTEMPTS {
            let attempt_root = root.join(format!("attempt-{attempt}"));
            std::fs::DirBuilder::new()
                .recursive(false)
                .mode(0o700)
                .create(&attempt_root)
                .map_err(|_| runtime_failure("live runtime attempt root could not be created"))?;
            match self.exercise_runtime_host_once(&attempt_root).await {
                Ok(()) => return Ok(()),
                Err(error) => last_failure = Some(error),
            }
        }
        Err(last_failure
            .unwrap_or_else(|| runtime_failure("live runtime attempts did not execute")))
    }

    async fn exercise_runtime_host_once(&self, root: &Path) -> Result<(), PocFailure> {
        let control_address = free_address()?;
        let replication_address = free_address()?;
        let replication_endpoint = format!("http://{replication_address}");
        let target = ObservationTarget {
            database_id: self.observation.database.database_id,
            database_unique_name: self.observation.database.database_unique_name.clone(),
            incarnation: self.observation.incarnation.identity,
        };
        let service = OracleServiceConfig::new(
            ResourceUid::new(RUNTIME_RESOURCE_UID),
            self.support.clone(),
            target,
            self.observation_timeout,
            self.maximum_age_millis,
            replication_endpoint.clone(),
        )
        .map_err(|_| runtime_failure("live runtime service configuration is invalid"))?;
        let config = OracleRuntimeHostConfig::new(
            OracleRuntimeHostInput {
                resource_uid: ResourceUid::new(RUNTIME_RESOURCE_UID),
                replica_id: ReplicaId::new(1),
                pod_uid: PodUid::new(RUNTIME_POD_UID),
                pvc_uid: PvcUid::new(RUNTIME_PVC_UID),
                data_root: root.join("metadata"),
                application_root: root.join("application"),
                control_address,
                replication_address,
                control_endpoint: format!("http://{control_address}"),
                replication_endpoint,
                bearer_token: RUNTIME_TOKEN.to_owned(),
                rpc_deadline: Duration::from_secs(5),
                transport_window_capacity: 64,
                shutdown_deadline: RUNTIME_DEADLINE,
            },
            service,
        )
        .map_err(|_| runtime_failure("live runtime host configuration is invalid"))?;
        let source: Arc<dyn OracleObservationSource> = Arc::new(self.clone());
        let (application, host) =
            prepare_runtime_host(&config, source, Arc::new(SystemObservationClock))
                .map_err(|_| runtime_failure("live runtime host preparation failed"))?;
        let identity = runtime_identity();
        let initialization_id = derive_initialization_id(
            &ResourceUid::new(RUNTIME_RESOURCE_UID),
            ReplicaId::new(1),
            &PodUid::new(RUNTIME_POD_UID),
            &PvcUid::new(RUNTIME_PVC_UID),
        );
        let store = SqliteStore::create_public_v1_with_application_paths(
            SqliteStore::metadata_database_path(config.data_root()),
            PublicV1Identity::new(1),
            StorageIdentity {
                schema_version: SCHEMA_VERSION,
                resource_uid: ResourceUid::new(RUNTIME_RESOURCE_UID),
                pod_uid: PodUid::new(RUNTIME_POD_UID),
                pvc_uid: PvcUid::new(RUNTIME_PVC_UID),
                initialization_id,
                local_identity: identity,
                effective_policy: EffectivePolicy::fixed(1, 30)
                    .ok_or_else(|| runtime_failure("live runtime effective policy is invalid"))?,
            },
            config.application_storage_paths(),
        )
        .map_err(|_| runtime_failure("live runtime metadata initialization failed"))?;
        let generation = PublicProtocolGeneration::new(1);
        let history_initialization =
            derive_history_initialization_id(&ResourceUid::new(RUNTIME_RESOURCE_UID), generation);
        let history = derive_bootstrap_history(
            &ResourceUid::new(RUNTIME_RESOURCE_UID),
            generation,
            &history_initialization,
        );
        store
            .initialize_public_v1_history(
                &history_initialization,
                &history,
                &ReplicaStorageIncarnationContext {
                    replica: runtime_identity(),
                    storage: ReplicaStorageIdentity::Persistent {
                        pod_uid: PodUid::new(RUNTIME_POD_UID),
                        pvc_uid: PvcUid::new(RUNTIME_PVC_UID),
                    },
                    storage_generation: StorageGeneration::new(
                        "oracle-live-runtime-storage-generation",
                    ),
                },
            )
            .await
            .map_err(|error| {
                PocFailure::fixture_command(format!(
                    "live runtime history initialization failed: {error}"
                ))
            })?;
        drop(store);
        let (startup_shutdown, receiver) = tokio::sync::watch::channel(false);
        let mut startup = tokio::spawn(host.start_with_shutdown(receiver));
        let mut running = match tokio::time::timeout(RUNTIME_DEADLINE, &mut startup).await {
            Ok(Ok(Ok(Some(running)))) => running,
            Ok(Ok(Ok(None))) | Ok(Ok(Err(_))) | Ok(Err(_)) => {
                return Err(runtime_failure("live runtime startup failed"));
            }
            Err(_) => {
                let _ = cancel_startup(&startup_shutdown, &mut startup).await;
                return Err(runtime_failure("live runtime startup failed"));
            }
        };
        let handle = running.handle();
        let result = exercise_supported_lifecycle(&handle, &application).await;
        let close = if result.is_ok() {
            let session = running.handle().process_session_id();
            running
                .handle()
                .execute_public_v1_command(ProtocolCommand::PublicV1Operation(Box::new(
                    operation_command(session, 6, "live-close", PublicOperationProgram::Close),
                )))
                .await
                .map_err(|_| runtime_failure("live runtime Close failed"))
        } else {
            Ok(())
        };
        running.shutdown();
        let shutdown = tokio::time::timeout(RUNTIME_DEADLINE, running.wait())
            .await
            .map_err(|_| runtime_failure("live runtime shutdown timed out"))?
            .map_err(|_| runtime_failure("live runtime shutdown failed"));
        result.and(close).and(shutdown)
    }
}

#[async_trait]
impl OracleObservationSource for WorkerObservationSource {
    async fn observe(&self) -> Result<OracleObservation, ObservationSourceError> {
        Ok(self.observation.clone())
    }
}

async fn cancel_startup(
    shutdown: &tokio::sync::watch::Sender<bool>,
    startup: &mut tokio::task::JoinHandle<
        kuberic_runtime::host::Result<Option<kuberic_runtime::host::RunningReplica>>,
    >,
) -> Result<(), PocFailure> {
    shutdown.send_replace(true);
    match tokio::time::timeout(RUNTIME_DEADLINE, &mut *startup).await {
        Ok(Ok(Ok(Some(mut running)))) => {
            running.shutdown();
            tokio::time::timeout(RUNTIME_DEADLINE, running.wait())
                .await
                .map_err(|_| runtime_failure("live runtime readiness-race shutdown timed out"))?
                .map_err(|_| runtime_failure("live runtime readiness-race shutdown failed"))
        }
        Ok(Ok(Ok(None))) | Ok(Ok(Err(_))) | Ok(Err(_)) => Ok(()),
        Err(_) => {
            startup.abort();
            let _ = tokio::time::timeout(RUNTIME_DEADLINE, &mut *startup).await;
            Err(runtime_failure(
                "live runtime startup cancellation timed out",
            ))
        }
    }
}

async fn exercise_supported_lifecycle(
    handle: &kuberic_runtime::host::ReplicaHandle,
    application: &Arc<kuberic_oracle::runtime_host::OracleRuntimeHostApplication>,
) -> Result<(), PocFailure> {
    let session = handle.process_session_id();
    for (revision, operation_id, program) in [
        (
            1,
            "live-open",
            PublicOperationProgram::Open {
                replica: runtime_identity(),
                existing: false,
            },
        ),
        (
            2,
            "live-primary",
            PublicOperationProgram::Role {
                epoch: Epoch::new(0, 1),
                role: ReplicaRole::Primary,
            },
        ),
        (
            3,
            "live-epoch",
            PublicOperationProgram::Epoch {
                epoch: Epoch::new(0, 1),
            },
        ),
        (
            4,
            "live-configuration",
            PublicOperationProgram::Configuration(PublicConfiguration {
                current: singleton_configuration(),
                previous: None,
                replicas: Vec::new(),
            }),
        ),
        (
            5,
            "live-progress",
            PublicOperationProgram::Progress { capability: false },
        ),
    ] {
        handle
            .execute_public_v1_command(ProtocolCommand::PublicV1Operation(Box::new(
                operation_command(session.clone(), revision, operation_id, program),
            )))
            .await
            .map_err(|error| {
                PocFailure::fixture_command(format!("live runtime {operation_id} failed: {error}"))
            })?;
    }
    let progress = handle
        .public_v1_report()
        .await
        .map_err(|_| runtime_failure("live runtime report failed"))?;
    if progress.progress.first != 0
        || progress.progress.last != 0
        || progress.role != ReplicaRole::Primary
        || progress.service_location.is_some()
        || application.service().replicator().is_none()
    {
        return Err(runtime_failure(
            "live runtime report violated singleton Oracle contract",
        ));
    }
    Ok(())
}

fn operation_command(
    session: ProcessSessionId,
    revision: u64,
    operation_id: &str,
    program: PublicOperationProgram,
) -> PublicV1OperationCommand {
    let class = if matches!(program, PublicOperationProgram::Close) {
        PublicOperationClass::Close
    } else {
        PublicOperationClass::Authority
    };
    PublicV1OperationCommand {
        target: runtime_identity(),
        intent: PublicOperationIntent {
            identity: PublicV1Identity::new(1),
            operation_id: OperationId::new(operation_id),
            revision,
            process_session_id: session,
            class,
            input_digest: program.digest(),
            replay_disposition: program.replay_disposition(),
            lifecycle: None,
            program: Some(program),
        },
    }
}

fn singleton_configuration() -> ConfigurationDescriptor {
    let identity = runtime_identity();
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

fn runtime_identity() -> ReplicaIdentity {
    let resource_uid = ResourceUid::new(RUNTIME_RESOURCE_UID);
    let replica_id = ReplicaId::new(1);
    let pod_uid = PodUid::new(RUNTIME_POD_UID);
    let pvc_uid = PvcUid::new(RUNTIME_PVC_UID);
    let initialization = derive_initialization_id(&resource_uid, replica_id, &pod_uid, &pvc_uid);
    ReplicaIdentity {
        replica_id,
        instance_id: ReplicaInstanceId::new(RUNTIME_POD_UID),
        agent_generation: derive_agent_generation(&initialization),
    }
}

fn free_address() -> Result<SocketAddr, PocFailure> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|_| runtime_failure("live runtime port allocation failed"))?;
    listener
        .local_addr()
        .map_err(|_| runtime_failure("live runtime port inspection failed"))
}

fn runtime_failure(message: &'static str) -> PocFailure {
    PocFailure::new(super::PocStage::Fixture, PocCategory::Protocol, message)
}

use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kuberic_oracle::kuberic::{
    ObservationFailureKind, ObservationSourceError, OracleObservationSource, OracleServiceConfig,
    SystemObservationClock,
};
use kuberic_oracle::runtime_host::{
    OracleRuntimeHostConfig, OracleRuntimeHostInput, prepare_runtime_host,
};
use kuberic_oracle::{
    DatabaseId, DatabaseIncarnationIdentity, DecimalScn, NativeName, ObservationTarget,
    OracleObservation,
};
use kuberic_runtime::protocol::command::{ProtocolCommand, PublicV1OperationCommand};
use kuberic_runtime::protocol::public_operations::{
    PublicConfiguration, PublicOperationClass, PublicOperationIntent, PublicOperationProgram,
    PublicV1Identity,
};
use kuberic_runtime::protocol::types::{
    ConfigurationDescriptor, ConfigurationMember, EffectivePolicy, Epoch, OperationId, PodUid,
    ProcessSessionId, PvcUid, ReplicaId, ReplicaIdentity, ReplicaInstanceId, ReplicaRole,
    ResourceUid, derive_agent_generation, derive_initialization_id,
};
use kuberic_runtime::testing::sqlite_store::SqliteStore;
use kuberic_runtime::testing::state::{SCHEMA_VERSION, StorageIdentity};
use tokio::sync::Semaphore;

use super::{
    Cancellation, PocCategory, PocFailure, WorkerRequest, protocol::ObservationRequest, run_worker,
};

const MAX_CONCURRENT_WORKERS: usize = 1;
const RUNTIME_RESOURCE_UID: &str = "oracle-live-runtime";
const RUNTIME_POD_UID: &str = "oracle-live-runtime-pod";
const RUNTIME_PVC_UID: &str = "oracle-live-runtime-pvc";
const RUNTIME_TOKEN: &str = "******";
const RUNTIME_DEADLINE: Duration = Duration::from_secs(60);
const RUNTIME_ATTEMPTS: usize = 3;

#[derive(Clone)]
pub struct WorkerObservationSource {
    worker_path: PathBuf,
    request: ObservationRequest,
    worker_slots: Arc<Semaphore>,
}

impl WorkerObservationSource {
    pub fn new(worker_path: &Path, request: ObservationRequest) -> Self {
        Self {
            worker_path: worker_path.to_path_buf(),
            request,
            worker_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_WORKERS)),
        }
    }

    pub async fn wait_for_idle(&self) {
        let permits = self
            .worker_slots
            .clone()
            .acquire_many_owned(MAX_CONCURRENT_WORKERS as u32)
            .await
            .expect("worker semaphore remains open");
        drop(permits);
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
            database_id: DatabaseId::new(self.request.target_database_id)
                .map_err(|_| runtime_failure("live runtime target DBID is invalid"))?,
            database_unique_name: NativeName::new(
                "target database unique name",
                self.request.target_database_unique_name.clone(),
            )
            .map_err(|_| runtime_failure("live runtime target unique name is invalid"))?,
            incarnation: DatabaseIncarnationIdentity {
                incarnation_number: self.request.target_incarnation_number,
                resetlogs_id: self.request.target_resetlogs_id,
                resetlogs_change: DecimalScn::parse(&self.request.target_resetlogs_change)
                    .map_err(|_| runtime_failure("live runtime resetlogs change is invalid"))?,
            },
        };
        let observation_timeout = Duration::from_millis(
            self.request
                .timeouts
                .snapshot_millis
                .saturating_add(self.request.timeouts.shutdown_millis)
                .saturating_add(5_000),
        );
        let service = OracleServiceConfig::new(
            ResourceUid::new(RUNTIME_RESOURCE_UID),
            self.request.support.clone(),
            target,
            observation_timeout,
            self.request.maximum_age_millis,
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
        drop(
            SqliteStore::create_public_v1_with_application_paths(
                SqliteStore::metadata_database_path(config.data_root()),
                PublicV1Identity::new(1),
                StorageIdentity {
                    schema_version: SCHEMA_VERSION,
                    resource_uid: ResourceUid::new(RUNTIME_RESOURCE_UID),
                    pod_uid: PodUid::new(RUNTIME_POD_UID),
                    pvc_uid: PvcUid::new(RUNTIME_PVC_UID),
                    initialization_id,
                    local_identity: identity,
                    effective_policy: EffectivePolicy::fixed(1, 30).ok_or_else(|| {
                        runtime_failure("live runtime effective policy is invalid")
                    })?,
                },
                config.application_storage_paths(),
            )
            .map_err(|_| runtime_failure("live runtime metadata initialization failed"))?,
        );
        let (startup_shutdown, receiver) = tokio::sync::watch::channel(false);
        let mut startup = tokio::spawn(host.start_with_shutdown(receiver));
        let mut running = match tokio::time::timeout(RUNTIME_DEADLINE, &mut startup).await {
            Ok(Ok(Ok(Some(running)))) => running,
            Ok(Ok(Ok(None))) | Ok(Ok(Err(_))) | Ok(Err(_)) => {
                self.wait_for_idle().await;
                return Err(runtime_failure("live runtime startup failed"));
            }
            Err(_) => {
                let _ = cancel_startup(&startup_shutdown, &mut startup).await;
                self.wait_for_idle().await;
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
        self.wait_for_idle().await;
        result.and(close).and(shutdown)
    }
}

#[async_trait]
impl OracleObservationSource for WorkerObservationSource {
    async fn observe(&self) -> Result<OracleObservation, ObservationSourceError> {
        let permit = self
            .worker_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ObservationSourceError::new(ObservationFailureKind::Cancelled))?;
        let worker_path = self.worker_path.clone();
        let request = self.request.clone();
        let timeouts = request.timeouts;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            run_worker(
                &worker_path,
                WorkerRequest::Observe(Box::new(request)),
                timeouts,
                &Cancellation::default(),
            )
        })
        .await
        .map_err(|_| ObservationSourceError::new(ObservationFailureKind::Response))?
        .map_err(map_failure)
    }
}

fn map_failure(failure: PocFailure) -> ObservationSourceError {
    let kind = match failure.category() {
        PocCategory::Cancelled => ObservationFailureKind::Cancelled,
        PocCategory::Timeout => ObservationFailureKind::Deadline,
        PocCategory::Authentication | PocCategory::Transport => ObservationFailureKind::Connection,
        PocCategory::Authorization => ObservationFailureKind::Query,
        PocCategory::Configuration
        | PocCategory::Capability
        | PocCategory::Protocol
        | PocCategory::RowCount
        | PocCategory::NativeValue
        | PocCategory::IdentityChanged
        | PocCategory::Shutdown
        | PocCategory::Fixture
        | PocCategory::Cleanup => ObservationFailureKind::Response,
    };
    ObservationSourceError::new(kind)
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
            .map_err(|_| runtime_failure("live runtime operation failed"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poc::PocStage;

    #[test]
    fn maps_sanitized_worker_categories_to_coarse_public_failures() {
        for (category, expected) in [
            (PocCategory::Cancelled, ObservationFailureKind::Cancelled),
            (PocCategory::Timeout, ObservationFailureKind::Deadline),
            (
                PocCategory::Authentication,
                ObservationFailureKind::Connection,
            ),
            (PocCategory::Transport, ObservationFailureKind::Connection),
            (PocCategory::Authorization, ObservationFailureKind::Query),
            (PocCategory::Configuration, ObservationFailureKind::Response),
            (PocCategory::Capability, ObservationFailureKind::Response),
            (PocCategory::Protocol, ObservationFailureKind::Response),
            (PocCategory::RowCount, ObservationFailureKind::Response),
            (PocCategory::NativeValue, ObservationFailureKind::Response),
            (
                PocCategory::IdentityChanged,
                ObservationFailureKind::Response,
            ),
            (PocCategory::Shutdown, ObservationFailureKind::Response),
            (PocCategory::Fixture, ObservationFailureKind::Response),
            (PocCategory::Cleanup, ObservationFailureKind::Response),
        ] {
            let failure = PocFailure::new(PocStage::Fixture, category, "private detail");
            assert_eq!(map_failure(failure).kind, expected);
        }
    }
}

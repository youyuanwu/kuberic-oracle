use std::fmt;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use kuberic_runtime::application::{
    INVALID_LSN, OpenContext, RoleChange, StateProvider, StatefulServiceReplica,
};
use kuberic_runtime::protocol::types::{
    Epoch, PartitionId, ProcessSessionId, ReplicaId, ReplicaIdentity, ReplicaRole, ResourceUid,
};
use kuberic_runtime::replicator::{
    PrimaryReplicator, ReplicaInformation, ReplicaSetConfiguration, ReplicaSetQuorumMode,
    Replicator, ReplicatorCapabilities, ReplicatorFactory, ReplicatorFactoryContext,
    ReplicatorInterfaces, ReplicatorSettings,
};
use kuberic_runtime::{Result as KubericResult, RuntimeError as KubericRuntimeError};
use thiserror::Error;

use crate::{
    Admission, ContractError, EditionProfile, ObservationTarget, OracleObservation,
    OracleSupportConfig, ReplicationProfile, STANDALONE_REPLICA_COUNT, admit_observation,
};

const CREATED: u8 = 0;
const OPEN: u8 = 1;
const CLOSED: u8 = 2;
const ABORTED: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationFailureKind {
    Cancelled,
    Clock,
    Connection,
    Deadline,
    Query,
    Response,
}

impl fmt::Display for ObservationFailureKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "cancelled",
            Self::Clock => "clock unavailable",
            Self::Connection => "connection failed",
            Self::Deadline => "deadline exceeded",
            Self::Query => "query failed",
            Self::Response => "response was invalid",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Oracle observation failed: {kind}")]
pub struct ObservationSourceError {
    pub kind: ObservationFailureKind,
}

impl ObservationSourceError {
    pub const fn new(kind: ObservationFailureKind) -> Self {
        Self { kind }
    }
}

#[async_trait]
pub trait OracleObservationSource: Send + Sync {
    async fn observe(&self) -> Result<OracleObservation, ObservationSourceError>;
}

pub trait ObservationClock: Send + Sync {
    fn now_unix_millis(&self) -> Result<u64, ObservationSourceError>;
}

#[derive(Debug, Default)]
pub struct SystemObservationClock;

impl ObservationClock for SystemObservationClock {
    fn now_unix_millis(&self) -> Result<u64, ObservationSourceError> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ObservationSourceError::new(ObservationFailureKind::Clock))?;
        u64::try_from(duration.as_millis())
            .map_err(|_| ObservationSourceError::new(ObservationFailureKind::Clock))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedOperation {
    ActiveSecondaryRole,
    IdleSecondaryRole,
    DataLoss,
    CatchUpConfiguration,
    CatchUpQuorum,
    BuildReplica,
    RemoveReplica,
}

impl fmt::Display for UnsupportedOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActiveSecondaryRole => "active-secondary role",
            Self::IdleSecondaryRole => "idle-secondary role",
            Self::DataLoss => "data-loss recovery",
            Self::CatchUpConfiguration => "catch-up configuration",
            Self::CatchUpQuorum => "catch-up quorum",
            Self::BuildReplica => "replica build",
            Self::RemoveReplica => "replica removal",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum KubericAdapterError {
    #[error("invalid Oracle Kuberic adapter configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Oracle support configuration is invalid: {0}")]
    InvalidOracleContract(ContractError),
    #[error("Oracle resource identity does not match the Kuberic partition")]
    ResourceIdentityMismatch,
    #[error("Oracle runtime replica identity does not match the Open identity")]
    RuntimeIdentityMismatch,
    #[error("Oracle runtime process session or Replicator settings changed")]
    RuntimeBindingMismatch,
    #[error("Oracle observation is unavailable: {0}")]
    ObservationUnavailable(ObservationFailureKind),
    #[error("Oracle observation was not admitted: {0}")]
    ObservationRejected(ContractError),
    #[error("Oracle Free does not support Kuberic {0}")]
    UnsupportedOperation(UnsupportedOperation),
    #[error("Kuberic epoch change would grant unsupported Oracle authority")]
    EpochChangeUnsupported,
    #[error("Kuberic replica-set configuration is not the exact Oracle Free singleton")]
    InvalidSingletonConfiguration,
    #[error("Kuberic replica-set configuration replay differs from the admitted value")]
    ConfigurationReplayMismatch,
    #[error("Oracle Kuberic adapter is not open")]
    NotOpen,
    #[error("Oracle Kuberic adapter is closed")]
    Closed,
}

impl From<KubericAdapterError> for KubericRuntimeError {
    fn from(error: KubericAdapterError) -> Self {
        match error {
            KubericAdapterError::NotOpen => Self::NotOpen,
            KubericAdapterError::Closed => Self::Closed,
            other => Self::Application(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleServiceConfig {
    resource_uid: ResourceUid,
    support: OracleSupportConfig,
    target: ObservationTarget,
    observation_timeout: Duration,
    maximum_age_millis: u64,
    replication_address: String,
}

impl OracleServiceConfig {
    pub fn new(
        resource_uid: ResourceUid,
        support: OracleSupportConfig,
        target: ObservationTarget,
        observation_timeout: Duration,
        maximum_age_millis: u64,
        replication_address: String,
    ) -> Result<Self, KubericAdapterError> {
        support
            .validate()
            .map_err(KubericAdapterError::InvalidOracleContract)?;
        if support.edition != EditionProfile::Free
            || support.replication != ReplicationProfile::Standalone
            || support.replica_count != STANDALONE_REPLICA_COUNT
        {
            return Err(KubericAdapterError::InvalidConfiguration(
                "the custom Replicator supports only one standalone Oracle Free database",
            ));
        }
        if resource_uid.as_str().is_empty() {
            return Err(KubericAdapterError::InvalidConfiguration(
                "resource UID must not be empty",
            ));
        }
        if maximum_age_millis == 0 {
            return Err(KubericAdapterError::InvalidConfiguration(
                "maximum observation age must be nonzero",
            ));
        }
        if observation_timeout.is_zero() {
            return Err(KubericAdapterError::InvalidConfiguration(
                "observation timeout must be nonzero",
            ));
        }
        if replication_address.is_empty()
            || replication_address.len() > 2048
            || replication_address.chars().any(char::is_control)
        {
            return Err(KubericAdapterError::InvalidConfiguration(
                "Replicator address must be nonempty, bounded, and contain no control characters",
            ));
        }
        Ok(Self {
            resource_uid,
            support,
            target,
            observation_timeout,
            maximum_age_millis,
            replication_address,
        })
    }

    pub fn resource_uid(&self) -> &ResourceUid {
        &self.resource_uid
    }

    pub fn support(&self) -> &OracleSupportConfig {
        &self.support
    }

    pub fn target(&self) -> &ObservationTarget {
        &self.target
    }

    pub const fn maximum_age_millis(&self) -> u64 {
        self.maximum_age_millis
    }

    pub const fn observation_timeout(&self) -> Duration {
        self.observation_timeout
    }

    pub fn replication_address(&self) -> &str {
        &self.replication_address
    }
}

enum ServiceLifecycle {
    Created,
    Open(Arc<OracleReplicator>),
    Closed,
    Aborted,
}

pub struct OracleService {
    config: OracleServiceConfig,
    source: Arc<dyn OracleObservationSource>,
    clock: Arc<dyn ObservationClock>,
    open_attempt: tokio::sync::Mutex<()>,
    lifecycle: Mutex<ServiceLifecycle>,
}

impl OracleService {
    pub fn new(
        config: OracleServiceConfig,
        source: Arc<dyn OracleObservationSource>,
        clock: Arc<dyn ObservationClock>,
    ) -> Self {
        Self {
            config,
            source,
            clock,
            open_attempt: tokio::sync::Mutex::new(()),
            lifecycle: Mutex::new(ServiceLifecycle::Created),
        }
    }

    pub fn validate_resource_identity(
        &self,
        partition_id: &PartitionId,
    ) -> Result<(), KubericAdapterError> {
        if partition_id.as_str() == self.config.resource_uid.as_str() {
            Ok(())
        } else {
            Err(KubericAdapterError::ResourceIdentityMismatch)
        }
    }

    pub fn replicator(&self) -> Option<Arc<OracleReplicator>> {
        match &*lock(&self.lifecycle) {
            ServiceLifecycle::Open(replicator) => Some(replicator.clone()),
            _ => None,
        }
    }
}

#[async_trait]
impl StatefulServiceReplica for OracleService {
    async fn open(self: Arc<Self>, context: OpenContext) -> KubericResult<Arc<dyn Replicator>> {
        self.validate_resource_identity(
            &context.partition.get_partition_information().partition_id,
        )?;
        let _attempt = self.open_attempt.lock().await;
        if !matches!(*lock(&self.lifecycle), ServiceLifecycle::Created) {
            return Err(KubericRuntimeError::Closed);
        }

        let replicator = Arc::new(OracleReplicator::new(
            self.config.clone(),
            self.source.clone(),
            self.clock.clone(),
            context.identity.clone(),
        ));
        let interfaces = context
            .partition
            .with_factory(Arc::new(OracleReplicatorFactory::new(replicator.clone())))
            .create_replicator(
                None,
                Some(ReplicatorSettings {
                    replication_address: self.config.replication_address.clone(),
                }),
            )
            .await?;
        debug_assert!(interfaces.state_replicator().is_none());
        debug_assert!(interfaces.primary_replicator().is_some());

        let mut lifecycle = lock(&self.lifecycle);
        if !matches!(*lifecycle, ServiceLifecycle::Created) {
            replicator.abort();
            return Err(KubericRuntimeError::Closed);
        }
        *lifecycle = ServiceLifecycle::Open(replicator);
        Ok(interfaces.replicator())
    }

    async fn change_role(&self, role: ReplicaRole) -> KubericResult<RoleChange> {
        let replicator = match &*lock(&self.lifecycle) {
            ServiceLifecycle::Created => return Err(KubericRuntimeError::NotOpen),
            ServiceLifecycle::Open(replicator) => replicator.clone(),
            ServiceLifecycle::Closed | ServiceLifecycle::Aborted => {
                return Err(KubericRuntimeError::Closed);
            }
        };
        replicator.publish_application_role(role).await?;
        Ok(RoleChange {
            service_address: None,
        })
    }

    async fn close(&self) -> KubericResult<()> {
        let replicator = {
            let mut lifecycle = lock(&self.lifecycle);
            let replicator = match &*lifecycle {
                ServiceLifecycle::Open(replicator) => Some(replicator.clone()),
                _ => None,
            };
            *lifecycle = ServiceLifecycle::Closed;
            replicator
        };
        let _attempt = self.open_attempt.lock().await;
        if let Some(replicator) = replicator {
            replicator.close().await?;
        }
        Ok(())
    }

    fn abort(&self) {
        let replicator = {
            let mut lifecycle = lock(&self.lifecycle);
            let replicator = match &*lifecycle {
                ServiceLifecycle::Open(replicator) => Some(replicator.clone()),
                _ => None,
            };
            *lifecycle = ServiceLifecycle::Aborted;
            replicator
        };
        if let Some(replicator) = replicator {
            replicator.abort();
        }
    }
}

pub struct OracleReplicatorFactory {
    replicator: Arc<OracleReplicator>,
}

impl OracleReplicatorFactory {
    pub fn new(replicator: Arc<OracleReplicator>) -> Self {
        Self { replicator }
    }

    pub fn interfaces(&self) -> ReplicatorInterfaces {
        ReplicatorInterfaces::primary(
            self.replicator.clone(),
            None,
            ReplicatorCapabilities::conservative(),
        )
    }
}

#[async_trait]
impl ReplicatorFactory for OracleReplicatorFactory {
    async fn create_replicator(
        &self,
        context: ReplicatorFactoryContext,
        state_provider: Option<Arc<dyn StateProvider>>,
        settings: ReplicatorSettings,
    ) -> KubericResult<ReplicatorInterfaces> {
        if state_provider.is_some() {
            return Err(KubericAdapterError::InvalidConfiguration(
                "Oracle owns no Kuberic StateProvider",
            )
            .into());
        }
        self.replicator.bind_runtime(
            context.identity()?,
            context.process_session_id()?,
            &settings,
        )?;
        Ok(self.interfaces())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeBinding {
    identity: ReplicaIdentity,
    process_session_id: ProcessSessionId,
    replication_address: String,
}

#[derive(Debug)]
struct ReplicatorLifecycle {
    state: u8,
    generation: u64,
}

pub struct OracleReplicator {
    config: OracleServiceConfig,
    source: Arc<dyn OracleObservationSource>,
    clock: Arc<dyn ObservationClock>,
    open_identity: ReplicaIdentity,
    runtime_binding: Mutex<Option<RuntimeBinding>>,
    lifecycle: RwLock<ReplicatorLifecycle>,
    role: Mutex<Option<ReplicaRole>>,
    epoch: Mutex<Option<Epoch>>,
    configuration: Mutex<Option<ReplicaSetConfiguration>>,
}

impl OracleReplicator {
    pub fn new(
        config: OracleServiceConfig,
        source: Arc<dyn OracleObservationSource>,
        clock: Arc<dyn ObservationClock>,
        open_identity: ReplicaIdentity,
    ) -> Self {
        Self {
            config,
            source,
            clock,
            open_identity,
            runtime_binding: Mutex::new(None),
            lifecycle: RwLock::new(ReplicatorLifecycle {
                state: CREATED,
                generation: 0,
            }),
            role: Mutex::new(None),
            epoch: Mutex::new(None),
            configuration: Mutex::new(None),
        }
    }

    fn bind_runtime(
        &self,
        identity: ReplicaIdentity,
        process_session_id: ProcessSessionId,
        settings: &ReplicatorSettings,
    ) -> Result<(), KubericAdapterError> {
        if identity != self.open_identity
            || identity.replica_id.value() <= 0
            || identity.instance_id.is_empty()
            || identity.agent_generation.is_empty()
        {
            return Err(KubericAdapterError::RuntimeIdentityMismatch);
        }
        if process_session_id.is_empty()
            || settings.replication_address != self.config.replication_address
        {
            return Err(KubericAdapterError::RuntimeBindingMismatch);
        }
        let candidate = RuntimeBinding {
            identity,
            process_session_id,
            replication_address: settings.replication_address.clone(),
        };
        let mut binding = lock(&self.runtime_binding);
        match &*binding {
            Some(existing) if existing == &candidate => Ok(()),
            Some(_) => Err(KubericAdapterError::RuntimeBindingMismatch),
            None => {
                *binding = Some(candidate);
                Ok(())
            }
        }
    }

    fn begin_operation(&self) -> Result<u64, KubericAdapterError> {
        let lifecycle = read(&self.lifecycle);
        match lifecycle.state {
            OPEN => Ok(lifecycle.generation),
            CREATED => Err(KubericAdapterError::NotOpen),
            CLOSED | ABORTED => Err(KubericAdapterError::Closed),
            _ => unreachable!("known Oracle Replicator lifecycle state"),
        }
    }

    fn finish_operation(
        &self,
        generation: u64,
    ) -> Result<RwLockReadGuard<'_, ReplicatorLifecycle>, KubericAdapterError> {
        let lifecycle = read(&self.lifecycle);
        if lifecycle.state != OPEN || lifecycle.generation != generation {
            return Err(KubericAdapterError::Closed);
        }
        Ok(lifecycle)
    }

    async fn observe_admitted(&self) -> Result<Admission, KubericAdapterError> {
        let observation =
            tokio::time::timeout(self.config.observation_timeout, self.source.observe())
                .await
                .map_err(|_| {
                    KubericAdapterError::ObservationUnavailable(ObservationFailureKind::Deadline)
                })?
                .map_err(|error| KubericAdapterError::ObservationUnavailable(error.kind))?;
        let now = self
            .clock
            .now_unix_millis()
            .map_err(|error| KubericAdapterError::ObservationUnavailable(error.kind))?;
        admit_observation(
            &self.config.support,
            &self.config.target,
            &observation,
            now,
            self.config.maximum_age_millis,
        )
        .map_err(KubericAdapterError::ObservationRejected)
    }

    async fn set_role(&self, role: ReplicaRole, epoch: Option<Epoch>) -> KubericResult<()> {
        let generation = self.begin_operation()?;
        match role {
            ReplicaRole::None => {
                let _lifecycle = self.finish_operation(generation)?;
                *lock(&self.role) = Some(ReplicaRole::None);
                return Ok(());
            }
            ReplicaRole::Primary => {}
            ReplicaRole::ActiveSecondary => {
                return Err(KubericAdapterError::UnsupportedOperation(
                    UnsupportedOperation::ActiveSecondaryRole,
                )
                .into());
            }
            ReplicaRole::IdleSecondary => {
                return Err(KubericAdapterError::UnsupportedOperation(
                    UnsupportedOperation::IdleSecondaryRole,
                )
                .into());
            }
        }

        self.observe_admitted().await?;
        let _lifecycle = self.finish_operation(generation)?;
        if let Some(epoch) = epoch {
            let mut admitted_epoch = lock(&self.epoch);
            if admitted_epoch.is_some_and(|current| current != epoch) {
                return Err(KubericAdapterError::EpochChangeUnsupported.into());
            }
            *admitted_epoch = Some(epoch);
        }
        *lock(&self.role) = Some(ReplicaRole::Primary);
        Ok(())
    }

    async fn publish_application_role(&self, role: ReplicaRole) -> KubericResult<()> {
        self.set_role(role, None).await
    }

    fn validate_singleton_configuration(
        &self,
        current: &ReplicaSetConfiguration,
    ) -> Result<(), KubericAdapterError> {
        let configuration = &current.configuration;
        let Some(binding) = lock(&self.runtime_binding).clone() else {
            return Err(KubericAdapterError::RuntimeBindingMismatch);
        };
        if configuration.members.len() != 1
            || configuration.write_quorum != 1
            || configuration.epoch.data_loss_number < 0
            || configuration.epoch.configuration_number < 0
            || configuration.configuration_id != configuration.expected_id()
            || configuration.primary_id != binding.identity.replica_id
            || !current.replicas.is_empty()
        {
            return Err(KubericAdapterError::InvalidSingletonConfiguration);
        }
        let member = &configuration.members[0];
        if member.identity != binding.identity || member.role != ReplicaRole::Primary {
            return Err(KubericAdapterError::InvalidSingletonConfiguration);
        }
        Ok(())
    }

    fn unsupported<T>(&self, operation: UnsupportedOperation) -> KubericResult<T> {
        Err(KubericAdapterError::UnsupportedOperation(operation).into())
    }
}

#[async_trait]
impl Replicator for OracleReplicator {
    async fn open(&self) -> KubericResult<String> {
        if lock(&self.runtime_binding).is_none() {
            return Err(KubericAdapterError::RuntimeBindingMismatch.into());
        }
        let mut lifecycle = write(&self.lifecycle);
        if lifecycle.state != CREATED {
            return Err(KubericRuntimeError::Closed);
        }
        lifecycle.state = OPEN;
        lifecycle.generation = lifecycle.generation.wrapping_add(1);
        Ok(self.config.replication_address.clone())
    }

    async fn change_role(&self, epoch: Epoch, role: ReplicaRole) -> KubericResult<()> {
        self.set_role(role, Some(epoch)).await
    }

    async fn update_epoch(&self, epoch: Epoch) -> KubericResult<()> {
        let generation = self.begin_operation()?;
        self.observe_admitted().await?;
        let _lifecycle = self.finish_operation(generation)?;
        let mut admitted_epoch = lock(&self.epoch);
        if admitted_epoch.is_some_and(|current| current != epoch) {
            return Err(KubericAdapterError::EpochChangeUnsupported.into());
        }
        *admitted_epoch = Some(epoch);
        Ok(())
    }

    async fn close(&self) -> KubericResult<()> {
        let mut lifecycle = write(&self.lifecycle);
        if lifecycle.state != ABORTED {
            lifecycle.state = CLOSED;
            lifecycle.generation = lifecycle.generation.wrapping_add(1);
        }
        Ok(())
    }

    fn abort(&self) {
        let mut lifecycle = write(&self.lifecycle);
        lifecycle.state = ABORTED;
        lifecycle.generation = lifecycle.generation.wrapping_add(1);
    }

    async fn current_progress(&self) -> KubericResult<i64> {
        let generation = self.begin_operation()?;
        self.observe_admitted().await?;
        let _lifecycle = self.finish_operation(generation)?;
        Ok(0)
    }

    async fn catch_up_capability(&self) -> KubericResult<i64> {
        let generation = self.begin_operation()?;
        self.observe_admitted().await?;
        let _lifecycle = self.finish_operation(generation)?;
        Ok(INVALID_LSN)
    }
}

#[async_trait]
impl PrimaryReplicator for OracleReplicator {
    async fn on_data_loss(&self) -> KubericResult<bool> {
        self.unsupported(UnsupportedOperation::DataLoss)
    }

    async fn update_catch_up_replica_set_configuration(
        &self,
        _current: ReplicaSetConfiguration,
        _previous: ReplicaSetConfiguration,
    ) -> KubericResult<()> {
        self.unsupported(UnsupportedOperation::CatchUpConfiguration)
    }

    async fn wait_for_catch_up_quorum(&self, _mode: ReplicaSetQuorumMode) -> KubericResult<()> {
        self.unsupported(UnsupportedOperation::CatchUpQuorum)
    }

    async fn update_current_replica_set_configuration(
        &self,
        current: ReplicaSetConfiguration,
    ) -> KubericResult<()> {
        let generation = self.begin_operation()?;
        self.validate_singleton_configuration(&current)?;
        self.observe_admitted().await?;
        let _lifecycle = self.finish_operation(generation)?;
        let mut admitted = lock(&self.configuration);
        match &*admitted {
            Some(existing) if existing != &current => {
                Err(KubericAdapterError::ConfigurationReplayMismatch.into())
            }
            Some(_) => Ok(()),
            None => {
                let admitted_epoch = *lock(&self.epoch);
                if admitted_epoch.is_some_and(|epoch| epoch != current.configuration.epoch) {
                    return Err(KubericAdapterError::EpochChangeUnsupported.into());
                }
                *admitted = Some(current);
                Ok(())
            }
        }
    }

    async fn build_replica(&self, _replica: ReplicaInformation) -> KubericResult<()> {
        self.unsupported(UnsupportedOperation::BuildReplica)
    }

    async fn remove_replica(&self, _replica_id: ReplicaId) -> KubericResult<()> {
        self.unsupported(UnsupportedOperation::RemoveReplica)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use kuberic_runtime::protocol::types::{
        AgentGeneration, ConfigurationMember, ReplicaInstanceId,
    };
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        DatabaseId, DatabaseIncarnationIdentity, DatabaseIncarnationSnapshot,
        DatabaseIncarnationStatus, DatabaseRole, DatabaseSnapshot, DecimalScn, ForceLogging,
        InstanceSnapshot, LogMode, NativeEdition, NativeName, OpenMode, PinnedImage,
        ProductSnapshot,
    };

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Clone)]
    struct FixedClock(u64);

    impl ObservationClock for FixedClock {
        fn now_unix_millis(&self) -> Result<u64, ObservationSourceError> {
            Ok(self.0)
        }
    }

    struct RepeatSource {
        result: Mutex<Result<OracleObservation, ObservationSourceError>>,
        calls: AtomicUsize,
    }

    impl RepeatSource {
        fn observation(observation: OracleObservation) -> Self {
            Self {
                result: Mutex::new(Ok(observation)),
                calls: AtomicUsize::new(0),
            }
        }

        fn failure(kind: ObservationFailureKind) -> Self {
            Self {
                result: Mutex::new(Err(ObservationSourceError::new(kind))),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl OracleObservationSource for RepeatSource {
        async fn observe(&self) -> Result<OracleObservation, ObservationSourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            lock(&self.result).clone()
        }
    }

    struct BlockingSource {
        observation: OracleObservation,
        entered: Notify,
        release: Notify,
    }

    #[async_trait]
    impl OracleObservationSource for BlockingSource {
        async fn observe(&self) -> Result<OracleObservation, ObservationSourceError> {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(self.observation.clone())
        }
    }

    fn name(value: &str) -> NativeName {
        NativeName::new("test name", value).unwrap()
    }

    fn support() -> OracleSupportConfig {
        OracleSupportConfig {
            edition: EditionProfile::Free,
            replication: ReplicationProfile::Standalone,
            replica_count: 1,
            expected_native_major_version: 23,
            image: PinnedImage::parse(format!(
                "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
            ))
            .unwrap(),
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

    fn observation(observed_at_unix_millis: u64) -> OracleObservation {
        OracleObservation {
            observed_at_unix_millis,
            database: DatabaseSnapshot {
                database_id: DatabaseId::new(42).unwrap(),
                database_name: name("FREE"),
                database_unique_name: name("FREE"),
                role: DatabaseRole::Primary,
                open_mode: OpenMode::ReadWrite,
                current_scn: Some(
                    DecimalScn::parse("99999999999999999999999999999999999999").unwrap(),
                ),
                resetlogs_change: DecimalScn::parse("100").unwrap(),
                activation_number: 1,
                log_mode: LogMode::ArchiveLog,
                force_logging: ForceLogging::Enabled,
                is_container_database: true,
            },
            incarnation: DatabaseIncarnationSnapshot {
                identity: target().incarnation,
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

    fn identity(replica_id: i64) -> ReplicaIdentity {
        ReplicaIdentity {
            replica_id: ReplicaId::new(replica_id),
            instance_id: ReplicaInstanceId::new(format!("instance-{replica_id}")),
            agent_generation: AgentGeneration::new(format!("generation-{replica_id}")),
        }
    }

    fn service_config() -> OracleServiceConfig {
        OracleServiceConfig::new(
            ResourceUid::new("oracle-resource"),
            support(),
            target(),
            Duration::from_secs(1),
            100,
            "127.0.0.1:45001".to_owned(),
        )
        .unwrap()
    }

    async fn opened_replicator(
        source: Arc<dyn OracleObservationSource>,
        now: u64,
    ) -> Arc<OracleReplicator> {
        let local = identity(1);
        let replicator = Arc::new(OracleReplicator::new(
            service_config(),
            source,
            Arc::new(FixedClock(now)),
            local.clone(),
        ));
        replicator
            .bind_runtime(
                local,
                ProcessSessionId::new("session-1"),
                &ReplicatorSettings {
                    replication_address: "127.0.0.1:45001".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(
            Replicator::open(replicator.as_ref()).await.unwrap(),
            "127.0.0.1:45001"
        );
        replicator
    }

    fn singleton(epoch: Epoch) -> ReplicaSetConfiguration {
        let local = identity(1);
        ReplicaSetConfiguration {
            configuration: kuberic_runtime::protocol::types::ConfigurationDescriptor::new(
                epoch,
                local.replica_id,
                vec![ConfigurationMember {
                    identity: local,
                    role: ReplicaRole::Primary,
                }],
                1,
            ),
            replicas: Vec::new(),
        }
    }

    #[test]
    fn service_config_accepts_only_singleton_oracle_free() {
        let mut enterprise = support();
        enterprise.edition = EditionProfile::Enterprise;
        assert!(matches!(
            OracleServiceConfig::new(
                ResourceUid::new("oracle-resource"),
                enterprise,
                target(),
                Duration::from_secs(1),
                100,
                "127.0.0.1:45001".to_owned(),
            ),
            Err(KubericAdapterError::InvalidConfiguration(_))
        ));
        assert!(matches!(
            OracleServiceConfig::new(
                ResourceUid::new("oracle-resource"),
                support(),
                target(),
                Duration::ZERO,
                100,
                "127.0.0.1:45001".to_owned(),
            ),
            Err(KubericAdapterError::InvalidConfiguration(
                "observation timeout must be nonzero"
            ))
        ));
    }

    #[test]
    fn factory_exposes_primary_control_without_state_interfaces() {
        let replicator = Arc::new(OracleReplicator::new(
            service_config(),
            Arc::new(RepeatSource::observation(observation(1_000))),
            Arc::new(FixedClock(1_000)),
            identity(1),
        ));
        let interfaces = OracleReplicatorFactory::new(replicator).interfaces();
        assert!(interfaces.primary_replicator().is_some());
        assert!(interfaces.state_replicator().is_none());
        assert_eq!(
            interfaces.capabilities(),
            ReplicatorCapabilities::conservative()
        );
    }

    #[tokio::test]
    async fn progress_is_empty_adapter_history_not_oracle_scn() {
        let source = Arc::new(RepeatSource::observation(observation(1_000)));
        let replicator = opened_replicator(source.clone(), 1_010).await;
        assert_eq!(replicator.current_progress().await.unwrap(), 0);
        assert_eq!(replicator.catch_up_capability().await.unwrap(), INVALID_LSN);
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn primary_role_and_exact_singleton_configuration_are_admitted() {
        let replicator = opened_replicator(
            Arc::new(RepeatSource::observation(observation(1_000))),
            1_010,
        )
        .await;
        let epoch = Epoch::new(0, 1);
        replicator
            .change_role(epoch, ReplicaRole::Primary)
            .await
            .unwrap();
        replicator
            .update_current_replica_set_configuration(singleton(epoch))
            .await
            .unwrap();
        replicator
            .update_current_replica_set_configuration(singleton(epoch))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn secondary_peer_and_failover_operations_are_rejected() {
        let replicator = opened_replicator(
            Arc::new(RepeatSource::observation(observation(1_000))),
            1_010,
        )
        .await;
        let role_error = replicator
            .change_role(Epoch::new(0, 1), ReplicaRole::ActiveSecondary)
            .await
            .unwrap_err();
        assert!(role_error.to_string().contains("active-secondary"));

        let mut peer_configuration = singleton(Epoch::new(0, 1));
        peer_configuration
            .configuration
            .members
            .push(ConfigurationMember {
                identity: identity(2),
                role: ReplicaRole::ActiveSecondary,
            });
        assert!(
            replicator
                .update_current_replica_set_configuration(peer_configuration)
                .await
                .unwrap_err()
                .to_string()
                .contains("exact Oracle Free singleton")
        );
        assert!(
            replicator
                .update_current_replica_set_configuration(singleton(Epoch::new(-1, 1)))
                .await
                .unwrap_err()
                .to_string()
                .contains("exact Oracle Free singleton")
        );
        assert!(
            replicator
                .on_data_loss()
                .await
                .unwrap_err()
                .to_string()
                .contains("data-loss recovery")
        );
        assert!(
            replicator
                .wait_for_catch_up_quorum(ReplicaSetQuorumMode::WriteQuorum)
                .await
                .unwrap_err()
                .to_string()
                .contains("catch-up quorum")
        );
    }

    #[tokio::test]
    async fn epoch_and_configuration_replacement_are_rejected() {
        let replicator = opened_replicator(
            Arc::new(RepeatSource::observation(observation(1_000))),
            1_010,
        )
        .await;
        let epoch = Epoch::new(0, 1);
        replicator
            .change_role(epoch, ReplicaRole::Primary)
            .await
            .unwrap();
        replicator
            .update_current_replica_set_configuration(singleton(epoch))
            .await
            .unwrap();

        assert!(
            replicator
                .update_epoch(Epoch::new(0, 2))
                .await
                .unwrap_err()
                .to_string()
                .contains("unsupported Oracle authority")
        );
        assert!(
            replicator
                .update_current_replica_set_configuration(singleton(Epoch::new(0, 2)))
                .await
                .unwrap_err()
                .to_string()
                .contains("replay differs")
        );
    }

    #[tokio::test]
    async fn stale_future_and_failed_observations_fail_closed() {
        let stale = opened_replicator(
            Arc::new(RepeatSource::observation(observation(1_000))),
            1_101,
        )
        .await;
        assert!(
            stale
                .current_progress()
                .await
                .unwrap_err()
                .to_string()
                .contains("freshness bound")
        );

        let future = opened_replicator(
            Arc::new(RepeatSource::observation(observation(1_100))),
            1_000,
        )
        .await;
        assert!(
            future
                .current_progress()
                .await
                .unwrap_err()
                .to_string()
                .contains("future")
        );

        let failed = opened_replicator(
            Arc::new(RepeatSource::failure(ObservationFailureKind::Deadline)),
            1_000,
        )
        .await;
        assert!(
            failed
                .current_progress()
                .await
                .unwrap_err()
                .to_string()
                .contains("deadline exceeded")
        );
    }

    #[tokio::test]
    async fn abort_fences_an_in_flight_observation_completion() {
        let source = Arc::new(BlockingSource {
            observation: observation(1_000),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let replicator = opened_replicator(source.clone(), 1_010).await;
        let callback = tokio::spawn({
            let replicator = replicator.clone();
            async move { replicator.current_progress().await }
        });
        source.entered.notified().await;
        replicator.abort();
        source.release.notify_one();

        assert!(matches!(
            callback.await.unwrap(),
            Err(KubericRuntimeError::Closed)
        ));
    }

    #[tokio::test]
    async fn observation_timeout_bounds_a_stalled_source() {
        let source = Arc::new(BlockingSource {
            observation: observation(1_000),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let local = identity(1);
        let replicator = Arc::new(OracleReplicator::new(
            OracleServiceConfig::new(
                ResourceUid::new("oracle-resource"),
                support(),
                target(),
                Duration::from_millis(1),
                100,
                "127.0.0.1:45001".to_owned(),
            )
            .unwrap(),
            source,
            Arc::new(FixedClock(1_010)),
            local.clone(),
        ));
        replicator
            .bind_runtime(
                local,
                ProcessSessionId::new("session-1"),
                &ReplicatorSettings {
                    replication_address: "127.0.0.1:45001".to_owned(),
                },
            )
            .unwrap();
        replicator.open().await.unwrap();

        assert!(
            replicator
                .current_progress()
                .await
                .unwrap_err()
                .to_string()
                .contains("deadline exceeded")
        );
    }
}

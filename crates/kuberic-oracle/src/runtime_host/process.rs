use std::collections::BTreeMap;
use std::future::Future;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use kuberic_runtime::application::{OpenContext, OpenMode, RoleChange, StatefulServiceReplica};
use kuberic_runtime::host::{ApplicationStorageState, ReplicaHost, RunningReplica};
use kuberic_runtime::protocol::types::ReplicaRole;
use kuberic_runtime::{Replicator, Result as KubericResult, RuntimeError as KubericRuntimeError};
use thiserror::Error;

use super::binding::{RuntimeBindingError, RuntimeBindingStore};
use super::config::{OracleRuntimeEndpointResolver, OracleRuntimeHostConfig};
use crate::kuberic::{
    ObservationClock, OracleObservationSource, OracleService, SystemObservationClock,
};

pub type OracleReplicaHost =
    ReplicaHost<OracleRuntimeHostApplication, OracleRuntimeEndpointResolver>;

pub struct OracleRuntimeHostApplication {
    inner: Arc<OracleService>,
    binding: RuntimeBindingStore,
}

impl OracleRuntimeHostApplication {
    pub fn new(
        config: &OracleRuntimeHostConfig,
        source: Arc<dyn OracleObservationSource>,
        clock: Arc<dyn ObservationClock>,
    ) -> Result<Self, RuntimeProcessError> {
        Ok(Self {
            inner: Arc::new(OracleService::new(config.service().clone(), source, clock)),
            binding: RuntimeBindingStore::new(config)?,
        })
    }

    pub fn storage_state(&self) -> Result<ApplicationStorageState, RuntimeProcessError> {
        self.binding.storage_state().map_err(Into::into)
    }

    pub fn storage_paths(&self) -> BTreeMap<String, PathBuf> {
        BTreeMap::from([(
            super::config::APPLICATION_STORAGE_NAME.to_owned(),
            self.binding.root().to_owned(),
        )])
    }

    pub fn service(&self) -> &Arc<OracleService> {
        &self.inner
    }

    pub fn binding(&self) -> &RuntimeBindingStore {
        &self.binding
    }

    fn prepare_binding(&self, mode: OpenMode) -> Result<(), RuntimeBindingError> {
        match (mode, self.binding.storage_state()?) {
            (OpenMode::New, ApplicationStorageState::FreshEmpty) => self.binding.initialize(),
            (OpenMode::New, ApplicationStorageState::Established)
            | (OpenMode::Existing, ApplicationStorageState::Established) => self.binding.validate(),
            (OpenMode::Existing, ApplicationStorageState::FreshEmpty) => {
                Err(RuntimeBindingError::NotInitialized)
            }
        }
    }
}

#[async_trait]
impl StatefulServiceReplica for OracleRuntimeHostApplication {
    async fn open(self: Arc<Self>, context: OpenContext) -> KubericResult<Arc<dyn Replicator>> {
        self.prepare_binding(context.mode)
            .map_err(binding_application_error)?;
        self.inner.clone().open(context).await
    }

    async fn change_role(&self, role: ReplicaRole) -> KubericResult<RoleChange> {
        self.inner.change_role(role).await
    }

    async fn close(&self) -> KubericResult<()> {
        self.inner.close().await
    }

    fn abort(&self) {
        self.inner.abort();
    }
}

pub fn prepare_runtime_host(
    config: &OracleRuntimeHostConfig,
    source: Arc<dyn OracleObservationSource>,
    clock: Arc<dyn ObservationClock>,
) -> Result<(Arc<OracleRuntimeHostApplication>, OracleReplicaHost), RuntimeProcessError> {
    prepare_data_root(config.data_root())?;
    let application = Arc::new(OracleRuntimeHostApplication::new(config, source, clock)?);
    let storage = application.storage_state()?;
    let host = ReplicaHost::new(
        config.replica_process_config(),
        application.clone(),
        storage,
        Arc::new(config.resolver()),
    )
    .with_application_storage_paths(application.storage_paths());
    Ok((application, host))
}

pub async fn run_runtime_with_shutdown(
    config: OracleRuntimeHostConfig,
    source: Arc<dyn OracleObservationSource>,
    shutdown: impl Future<Output = Result<(), RuntimeProcessError>> + Send + 'static,
) -> Result<(), RuntimeProcessError> {
    run_runtime_with_clock_and_shutdown(config, source, Arc::new(SystemObservationClock), shutdown)
        .await
}

pub async fn run_runtime_with_clock_and_shutdown(
    config: OracleRuntimeHostConfig,
    source: Arc<dyn OracleObservationSource>,
    clock: Arc<dyn ObservationClock>,
    shutdown: impl Future<Output = Result<(), RuntimeProcessError>> + Send + 'static,
) -> Result<(), RuntimeProcessError> {
    let (application, host) = prepare_runtime_host(&config, source, clock)?;
    let shutdown_deadline = config.shutdown_deadline();

    tokio::pin!(shutdown);
    let (startup_shutdown, receiver) = tokio::sync::watch::channel(false);
    let mut startup = tokio::spawn(host.start_with_shutdown(receiver));
    let (started, trigger) = tokio::select! {
        result = &mut startup => (result, None),
        result = &mut shutdown => {
            startup_shutdown.send_replace(true);
            match tokio::time::timeout(shutdown_deadline, &mut startup).await {
                Ok(started) => (started, Some(result)),
                Err(_) => {
                    application.abort();
                    let trigger = combine(
                        result,
                        Err(RuntimeProcessError::Timeout("runtime startup cancellation")),
                        "startup cancellation",
                    );
                    let termination = abort_startup_task(&mut startup, shutdown_deadline).await;
                    let result = combine(trigger, termination, "startup task termination");
                    return finish_application(application.as_ref(), result, shutdown_deadline)
                        .await;
                }
            }
        }
    };

    let mut replica = match started {
        Ok(Ok(Some(replica))) => {
            if let Some(trigger) = trigger {
                return finish_shutdown(
                    replica,
                    application.as_ref(),
                    None,
                    trigger,
                    shutdown_deadline,
                )
                .await;
            }
            replica
        }
        result => {
            let startup_result = match result {
                Ok(Ok(None)) => Ok(()),
                Ok(Err(error)) => Err(RuntimeProcessError::Host(error.to_string())),
                Err(error) => Err(RuntimeProcessError::Task(error.to_string())),
                Ok(Ok(Some(_))) => unreachable!(),
            };
            let result = combine(
                startup_result,
                trigger.unwrap_or(Ok(())),
                "shutdown trigger",
            );
            return finish_application(application.as_ref(), result, shutdown_deadline).await;
        }
    };

    let (completion, trigger) = tokio::select! {
        result = replica.wait() => (Some(result), Ok(())),
        result = &mut shutdown => (None, result),
    };
    finish_shutdown(
        replica,
        application.as_ref(),
        completion,
        trigger,
        shutdown_deadline,
    )
    .await
}

async fn finish_shutdown(
    mut replica: RunningReplica,
    application: &impl StatefulServiceReplica,
    completion: Option<kuberic_runtime::host::Result<()>>,
    trigger: Result<(), RuntimeProcessError>,
    deadline: std::time::Duration,
) -> Result<(), RuntimeProcessError> {
    replica.shutdown();
    let completion = match completion {
        Some(result) => result.map_err(|error| RuntimeProcessError::Host(error.to_string())),
        None => match tokio::time::timeout(deadline, replica.wait()).await {
            Ok(result) => result.map_err(|error| RuntimeProcessError::Host(error.to_string())),
            Err(_) => {
                application.abort();
                let joined = match tokio::time::timeout(deadline, replica.wait()).await {
                    Ok(result) => {
                        result.map_err(|error| RuntimeProcessError::Host(error.to_string()))
                    }
                    Err(_) => Err(RuntimeProcessError::Timeout(
                        "runtime completion after abort",
                    )),
                };
                combine(
                    Err(RuntimeProcessError::Timeout(
                        "runtime shutdown acknowledgement",
                    )),
                    joined,
                    "runtime completion after timeout",
                )
            }
        },
    };
    let result = combine(completion, trigger, "shutdown trigger");
    finish_application(application, result, deadline).await
}

async fn abort_startup_task(
    startup: &mut tokio::task::JoinHandle<kuberic_runtime::host::Result<Option<RunningReplica>>>,
    deadline: std::time::Duration,
) -> Result<(), RuntimeProcessError> {
    startup.abort();
    match tokio::time::timeout(deadline, startup).await {
        Ok(Err(error)) if error.is_cancelled() => Ok(()),
        Ok(Err(error)) => Err(RuntimeProcessError::Task(error.to_string())),
        Ok(Ok(Err(error))) => Err(RuntimeProcessError::Host(error.to_string())),
        Ok(Ok(Ok(None))) => Ok(()),
        Ok(Ok(Ok(Some(mut replica)))) => {
            replica.shutdown();
            match tokio::time::timeout(deadline, replica.wait()).await {
                Ok(result) => result.map_err(|error| RuntimeProcessError::Host(error.to_string())),
                Err(_) => Err(RuntimeProcessError::Timeout(
                    "runtime startup task completion after abort",
                )),
            }
        }
        Err(_) => Err(RuntimeProcessError::Timeout("runtime startup task abort")),
    }
}

async fn finish_application(
    application: &impl StatefulServiceReplica,
    result: Result<(), RuntimeProcessError>,
    deadline: std::time::Duration,
) -> Result<(), RuntimeProcessError> {
    let cleanup = match tokio::time::timeout(deadline, application.close()).await {
        Ok(result) => result.map_err(|error| RuntimeProcessError::Application(error.to_string())),
        Err(_) => Err(RuntimeProcessError::Timeout("runtime application shutdown")),
    };
    if cleanup.is_err() {
        application.abort();
    }
    combine(result, cleanup, "application shutdown")
}

fn combine(
    primary: Result<(), RuntimeProcessError>,
    cleanup: Result<(), RuntimeProcessError>,
    context: &'static str,
) -> Result<(), RuntimeProcessError> {
    match (primary, cleanup) {
        (Err(primary), Err(cleanup)) => Err(RuntimeProcessError::Combined {
            context,
            primary: primary.to_string(),
            cleanup: cleanup.to_string(),
        }),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn prepare_data_root(path: &Path) -> Result<(), RuntimeProcessError> {
    if !path.exists() {
        std::fs::DirBuilder::new()
            .recursive(false)
            .mode(0o700)
            .create(path)
            .map_err(|_| RuntimeProcessError::UnsafeDataRoot)?;
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| RuntimeProcessError::UnsafeDataRoot)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(RuntimeProcessError::UnsafeDataRoot);
    }
    Ok(())
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn binding_application_error(error: RuntimeBindingError) -> KubericRuntimeError {
    KubericRuntimeError::Application(error.to_string())
}

#[derive(Debug, Error)]
pub enum RuntimeProcessError {
    #[error(transparent)]
    Binding(#[from] RuntimeBindingError),
    #[error("runtime data root is unsafe")]
    UnsafeDataRoot,
    #[error("Kuberic runtime host failed: {0}")]
    Host(String),
    #[error("Kuberic runtime application failed: {0}")]
    Application(String),
    #[error("runtime task failed: {0}")]
    Task(String),
    #[error("{0} timed out")]
    Timeout(&'static str),
    #[error("{context} failed: {primary}; cleanup failed: {cleanup}")]
    Combined {
        context: &'static str,
        primary: String,
        cleanup: String,
    },
}

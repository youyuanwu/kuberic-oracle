use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kuberic_runtime::host::{ReplicaEndpointResolver, ReplicaProcessConfig};
use kuberic_runtime::protocol::types::{PodUid, PvcUid, ReplicaId, ReplicaIdentity, ResourceUid};
use thiserror::Error;

use crate::kuberic::OracleServiceConfig;

pub const APPLICATION_STORAGE_NAME: &str = "oracle-runtime-binding";
const MAX_BEARER_TOKEN_BYTES: usize = 4_096;
const MAX_ENDPOINT_BYTES: usize = 2_048;
const MAX_DEADLINE: Duration = Duration::from_secs(300);
const MAX_TRANSPORT_WINDOW_CAPACITY: usize = 65_536;

#[derive(Clone)]
pub struct OracleRuntimeHostInput {
    pub resource_uid: ResourceUid,
    pub replica_id: ReplicaId,
    pub pod_uid: PodUid,
    pub pvc_uid: PvcUid,
    pub data_root: PathBuf,
    pub application_root: PathBuf,
    pub control_address: SocketAddr,
    pub replication_address: SocketAddr,
    pub control_endpoint: String,
    pub replication_endpoint: String,
    pub bearer_token: String,
    pub rpc_deadline: Duration,
    pub transport_window_capacity: usize,
    pub shutdown_deadline: Duration,
}

impl fmt::Debug for OracleRuntimeHostInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OracleRuntimeHostInput")
            .field("resource_uid", &self.resource_uid)
            .field("replica_id", &self.replica_id)
            .field("pod_uid", &self.pod_uid)
            .field("pvc_uid", &self.pvc_uid)
            .field("data_root", &self.data_root)
            .field("application_root", &self.application_root)
            .field("control_address", &self.control_address)
            .field("replication_address", &self.replication_address)
            .field("control_endpoint", &self.control_endpoint)
            .field("replication_endpoint", &self.replication_endpoint)
            .field("bearer_token", &"<redacted>")
            .field("rpc_deadline", &self.rpc_deadline)
            .field("transport_window_capacity", &self.transport_window_capacity)
            .field("shutdown_deadline", &self.shutdown_deadline)
            .finish()
    }
}

#[derive(Clone)]
pub struct OracleRuntimeHostConfig {
    input: OracleRuntimeHostInput,
    service: OracleServiceConfig,
}

impl fmt::Debug for OracleRuntimeHostConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OracleRuntimeHostConfig")
            .field("input", &self.input)
            .field("service", &self.service)
            .finish()
    }
}

impl OracleRuntimeHostConfig {
    pub fn new(
        mut input: OracleRuntimeHostInput,
        service: OracleServiceConfig,
    ) -> Result<Self, RuntimeHostConfigError> {
        validate_identity_component(input.resource_uid.as_str())?;
        if input.replica_id.value() <= 0 {
            return Err(RuntimeHostConfigError::InvalidIdentity);
        }
        validate_identity_component(input.pod_uid.as_str())?;
        validate_identity_component(input.pvc_uid.as_str())?;
        if input.resource_uid != *service.resource_uid() {
            return Err(RuntimeHostConfigError::ServiceIdentityMismatch);
        }

        input.data_root = canonical_target_path(&input.data_root)?;
        input.application_root = canonical_target_path(&input.application_root)?;
        if input.data_root == input.application_root
            || input.data_root.starts_with(&input.application_root)
        {
            return Err(RuntimeHostConfigError::InvalidPath);
        }
        if input.control_address.port() == 0
            || input.replication_address.port() == 0
            || input.control_address == input.replication_address
        {
            return Err(RuntimeHostConfigError::InvalidEndpoint);
        }
        validate_http_endpoint(&input.control_endpoint)?;
        validate_http_endpoint(&input.replication_endpoint)?;
        if input.replication_endpoint != service.replication_address() {
            return Err(RuntimeHostConfigError::ServiceEndpointMismatch);
        }
        if input.bearer_token.is_empty()
            || input.bearer_token.len() > MAX_BEARER_TOKEN_BYTES
            || input.bearer_token.trim() != input.bearer_token
            || input
                .bearer_token
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(RuntimeHostConfigError::InvalidBearerToken);
        }
        for deadline in [input.rpc_deadline, input.shutdown_deadline] {
            if deadline.is_zero() || deadline > MAX_DEADLINE {
                return Err(RuntimeHostConfigError::InvalidDeadline);
            }
        }
        if input.transport_window_capacity == 0
            || input.transport_window_capacity > MAX_TRANSPORT_WINDOW_CAPACITY
        {
            return Err(RuntimeHostConfigError::InvalidTransportWindow);
        }
        Ok(Self { input, service })
    }

    pub fn resource_uid(&self) -> &ResourceUid {
        &self.input.resource_uid
    }

    pub const fn replica_id(&self) -> ReplicaId {
        self.input.replica_id
    }

    pub fn pod_uid(&self) -> &PodUid {
        &self.input.pod_uid
    }

    pub fn pvc_uid(&self) -> &PvcUid {
        &self.input.pvc_uid
    }

    pub fn data_root(&self) -> &Path {
        &self.input.data_root
    }

    pub fn application_root(&self) -> &Path {
        &self.input.application_root
    }

    pub fn control_endpoint(&self) -> &str {
        &self.input.control_endpoint
    }

    pub fn replication_endpoint(&self) -> &str {
        &self.input.replication_endpoint
    }

    pub fn service(&self) -> &OracleServiceConfig {
        &self.service
    }

    pub fn shutdown_deadline(&self) -> Duration {
        self.input.shutdown_deadline
    }

    pub fn replica_process_config(&self) -> ReplicaProcessConfig {
        ReplicaProcessConfig {
            resource_uid: self.input.resource_uid.clone(),
            replica_id: self.input.replica_id,
            pod_uid: self.input.pod_uid.clone(),
            pvc_uid: self.input.pvc_uid.clone(),
            data_root: self.input.data_root.clone(),
            control_address: self.input.control_address,
            replication_address: self.input.replication_address,
            bearer_token: self.input.bearer_token.clone(),
            rpc_deadline: self.input.rpc_deadline,
            transport_window_capacity: self.input.transport_window_capacity,
        }
    }

    pub fn application_storage_paths(&self) -> BTreeMap<String, PathBuf> {
        BTreeMap::from([(
            APPLICATION_STORAGE_NAME.to_owned(),
            self.input.application_root.clone(),
        )])
    }

    pub fn resolver(&self) -> OracleRuntimeEndpointResolver {
        OracleRuntimeEndpointResolver {
            replica_id: self.input.replica_id,
            control_endpoint: self.input.control_endpoint.clone(),
            replication_endpoint: self.input.replication_endpoint.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OracleRuntimeEndpointResolver {
    replica_id: ReplicaId,
    control_endpoint: String,
    replication_endpoint: String,
}

impl ReplicaEndpointResolver for OracleRuntimeEndpointResolver {
    fn control_endpoint(&self, identity: &ReplicaIdentity) -> String {
        if identity.replica_id == self.replica_id {
            self.control_endpoint.clone()
        } else {
            String::new()
        }
    }

    fn replication_endpoint(&self, identity: &ReplicaIdentity) -> String {
        if identity.replica_id == self.replica_id {
            self.replication_endpoint.clone()
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RuntimeHostConfigError {
    #[error("runtime host identity is invalid")]
    InvalidIdentity,
    #[error("runtime host path is invalid")]
    InvalidPath,
    #[error("runtime host endpoint is invalid")]
    InvalidEndpoint,
    #[error("runtime host bearer token is invalid")]
    InvalidBearerToken,
    #[error("runtime host deadline is invalid")]
    InvalidDeadline,
    #[error("runtime host transport window is invalid")]
    InvalidTransportWindow,
    #[error("runtime host resource differs from the Oracle service resource")]
    ServiceIdentityMismatch,
    #[error("runtime host Replicator endpoint differs from the Oracle service")]
    ServiceEndpointMismatch,
}

fn validate_identity_component(value: &str) -> Result<(), RuntimeHostConfigError> {
    if value.is_empty()
        || value.len() > 1_024
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(RuntimeHostConfigError::InvalidIdentity)
    } else {
        Ok(())
    }
}

fn validate_http_endpoint(value: &str) -> Result<(), RuntimeHostConfigError> {
    if value.is_empty() || value.len() > MAX_ENDPOINT_BYTES || value.chars().any(char::is_control) {
        return Err(RuntimeHostConfigError::InvalidEndpoint);
    }
    let authority = value
        .strip_prefix("http://")
        .ok_or(RuntimeHostConfigError::InvalidEndpoint)?;
    if authority.is_empty() || authority.contains('/') || authority.contains('@') {
        return Err(RuntimeHostConfigError::InvalidEndpoint);
    }
    let (_, port) = authority
        .rsplit_once(':')
        .ok_or(RuntimeHostConfigError::InvalidEndpoint)?;
    if port.parse::<u16>().ok().filter(|port| *port != 0).is_none() {
        return Err(RuntimeHostConfigError::InvalidEndpoint);
    }
    Ok(())
}

fn canonical_target_path(path: &Path) -> Result<PathBuf, RuntimeHostConfigError> {
    if !path.is_absolute() {
        return Err(RuntimeHostConfigError::InvalidPath);
    }
    if path.exists() {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| RuntimeHostConfigError::InvalidPath)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(RuntimeHostConfigError::InvalidPath);
        }
        return path
            .canonicalize()
            .map_err(|_| RuntimeHostConfigError::InvalidPath);
    }
    let parent = path.parent().ok_or(RuntimeHostConfigError::InvalidPath)?;
    let parent = parent
        .canonicalize()
        .map_err(|_| RuntimeHostConfigError::InvalidPath)?;
    let name = path
        .file_name()
        .ok_or(RuntimeHostConfigError::InvalidPath)?;
    Ok(parent.join(name))
}

#[cfg(test)]
mod tests {
    use kuberic_runtime::protocol::types::{AgentGeneration, ReplicaInstanceId};

    use super::*;
    use crate::{
        DatabaseId, DatabaseIncarnationIdentity, DecimalScn, EditionProfile, NativeName,
        ObservationTarget, OracleSupportConfig, PinnedImage, ReplicationProfile,
    };

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn service(resource: &str, replication_endpoint: &str) -> OracleServiceConfig {
        OracleServiceConfig::new(
            ResourceUid::new(resource),
            OracleSupportConfig {
                edition: EditionProfile::Free,
                replication: ReplicationProfile::Standalone,
                replica_count: 1,
                expected_native_major_version: 23,
                image: PinnedImage::parse(format!(
                    "container-registry.oracle.com/database/free:23.26.3.0@sha256:{DIGEST}"
                ))
                .unwrap(),
            },
            ObservationTarget {
                database_id: DatabaseId::new(42).unwrap(),
                database_unique_name: NativeName::new("unique name", "FREE").unwrap(),
                incarnation: DatabaseIncarnationIdentity {
                    incarnation_number: 3,
                    resetlogs_id: 4,
                    resetlogs_change: DecimalScn::parse("5").unwrap(),
                },
            },
            Duration::from_secs(5),
            1_000,
            replication_endpoint.to_owned(),
        )
        .unwrap()
    }

    fn input(root: &Path) -> OracleRuntimeHostInput {
        OracleRuntimeHostInput {
            resource_uid: ResourceUid::new("oracle-resource"),
            replica_id: ReplicaId::new(1),
            pod_uid: PodUid::new("pod-1"),
            pvc_uid: PvcUid::new("pvc-1"),
            data_root: root.join("metadata"),
            application_root: root.join("application"),
            control_address: "127.0.0.1:45001".parse().unwrap(),
            replication_address: "127.0.0.1:45002".parse().unwrap(),
            control_endpoint: "http://127.0.0.1:45001".to_owned(),
            replication_endpoint: "http://127.0.0.1:45002".to_owned(),
            bearer_token: "test-token".to_owned(),
            rpc_deadline: Duration::from_secs(5),
            transport_window_capacity: 256,
            shutdown_deadline: Duration::from_secs(5),
        }
    }

    #[test]
    fn validates_and_maps_exact_process_configuration() {
        let temporary = tempfile::tempdir().unwrap();
        let config = OracleRuntimeHostConfig::new(
            input(temporary.path()),
            service("oracle-resource", "http://127.0.0.1:45002"),
        )
        .unwrap();
        let process = config.replica_process_config();
        assert_eq!(process.resource_uid.as_str(), "oracle-resource");
        assert_eq!(process.replica_id, ReplicaId::new(1));
        assert_eq!(process.control_address.port(), 45001);
        assert_eq!(config.application_storage_paths().len(), 1);
    }

    #[test]
    fn rejects_service_identity_endpoint_and_secret_mismatch() {
        let temporary = tempfile::tempdir().unwrap();
        assert_eq!(
            OracleRuntimeHostConfig::new(
                input(temporary.path()),
                service("other-resource", "http://127.0.0.1:45002"),
            )
            .unwrap_err(),
            RuntimeHostConfigError::ServiceIdentityMismatch
        );

        assert_eq!(
            OracleRuntimeHostConfig::new(
                input(temporary.path()),
                service("oracle-resource", "http://127.0.0.1:45003"),
            )
            .unwrap_err(),
            RuntimeHostConfigError::ServiceEndpointMismatch
        );

        let mut invalid = input(temporary.path());
        invalid.bearer_token = "secret token".to_owned();
        assert_eq!(
            OracleRuntimeHostConfig::new(
                invalid,
                service("oracle-resource", "http://127.0.0.1:45002"),
            )
            .unwrap_err(),
            RuntimeHostConfigError::InvalidBearerToken
        );

        let mut overlapping = input(temporary.path());
        overlapping.application_root = temporary.path().join("application");
        overlapping.data_root = overlapping.application_root.join("metadata");
        assert_eq!(
            OracleRuntimeHostConfig::new(
                overlapping,
                service("oracle-resource", "http://127.0.0.1:45002"),
            )
            .unwrap_err(),
            RuntimeHostConfigError::InvalidPath
        );
    }

    #[test]
    fn resolver_exposes_only_the_local_logical_replica() {
        let temporary = tempfile::tempdir().unwrap();
        let config = OracleRuntimeHostConfig::new(
            input(temporary.path()),
            service("oracle-resource", "http://127.0.0.1:45002"),
        )
        .unwrap();
        let resolver = config.resolver();
        let identity = |replica_id| ReplicaIdentity {
            replica_id: ReplicaId::new(replica_id),
            instance_id: ReplicaInstanceId::new(format!("instance-{replica_id}")),
            agent_generation: AgentGeneration::new(format!("generation-{replica_id}")),
        };
        assert_eq!(
            resolver.control_endpoint(&identity(1)),
            "http://127.0.0.1:45001"
        );
        assert!(resolver.replication_endpoint(&identity(2)).is_empty());
    }
}

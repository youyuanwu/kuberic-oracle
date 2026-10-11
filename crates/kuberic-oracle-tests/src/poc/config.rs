use std::{
    net::{IpAddr, Ipv4Addr},
    path::{Component, Path},
};

use kuberic_oracle::{
    ConnectionContainer, ContainerDataPolicy, ContractError, DecimalScn, EditionProfile,
    ObservationScope, ObserverTimeouts, PrincipalPlacement, ReplicationProfile,
};
use serde::{Deserialize, Serialize};

use super::{
    error::{PocCategory, PocFailure, PocStage},
    protocol::ObservationRequest,
};

const MAX_SERVICE_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PocEndpoint {
    pub host: IpAddr,
    pub port: u16,
    pub service: String,
}

impl PocEndpoint {
    pub fn validate(&self) -> Result<(), PocFailure> {
        if !self.host.is_loopback() {
            return Err(config_failure("PoC endpoint must be loopback"));
        }
        if self.port == 0 {
            return Err(config_failure("PoC endpoint port must be nonzero"));
        }
        if self.service.is_empty()
            || self.service.len() > MAX_SERVICE_BYTES
            || self.service.trim() != self.service
            || self.service.chars().any(char::is_control)
        {
            return Err(config_failure("PoC service name is invalid"));
        }
        Ok(())
    }

    pub fn loopback(port: u16, service: impl Into<String>) -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            service: service.into(),
        }
    }

    pub fn connect_string(&self) -> String {
        format!("tcp://{}:{}/{}", self.host, self.port, self.service)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PocAttemptConfig {
    pub request: ObservationRequest,
}

impl PocAttemptConfig {
    pub fn validate(&self) -> Result<(), PocFailure> {
        self.request.endpoint.validate()?;
        if self.request.endpoint.service != "FREEPDB1" {
            return Err(config_failure("live PoC service must be FREEPDB1"));
        }
        self.request.support.validate().map_err(contract_failure)?;
        if self.request.support.edition != EditionProfile::Free
            || self.request.support.replication != ReplicationProfile::Standalone
            || self.request.support.replica_count != 1
            || self.request.support.expected_native_major_version != 23
        {
            return Err(config_failure(
                "live PoC requires the Oracle Free standalone native-23 profile",
            ));
        }
        self.request.scope.validate().map_err(contract_failure)?;
        self.request.timeouts.validate().map_err(contract_failure)?;
        validate_path("username file", &self.request.username_file)?;
        validate_path("password file", &self.request.password_file)?;
        if self.request.username_file == self.request.password_file {
            return Err(config_failure("PoC credential files must be distinct"));
        }
        if self.request.target_database_id == 0 {
            return Err(config_failure("target database ID must be nonzero"));
        }
        if self.request.target_incarnation_number == 0 || self.request.target_resetlogs_id == 0 {
            return Err(config_failure(
                "target database incarnation identifiers must be nonzero",
            ));
        }
        DecimalScn::parse(&self.request.target_resetlogs_change)
            .map_err(|_| config_failure("target resetlogs change must be an exact Oracle SCN"))?;
        if self.request.target_database_unique_name.is_empty()
            || self.request.target_database_unique_name.len() > MAX_SERVICE_BYTES
            || self.request.target_database_unique_name.trim()
                != self.request.target_database_unique_name
            || self
                .request
                .target_database_unique_name
                .chars()
                .any(char::is_control)
        {
            return Err(config_failure(
                "target database unique name must not be empty",
            ));
        }
        validate_single_row_scope(&self.request.scope)?;
        validate_deadline_reserve(&self.request.timeouts)?;
        Ok(())
    }
}

fn validate_path(label: &'static str, path: &Path) -> Result<(), PocFailure> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || !path
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(config_failure(label));
    }
    Ok(())
}

fn validate_single_row_scope(scope: &ObservationScope) -> Result<(), PocFailure> {
    if scope.expected_database_rows != 1
        || scope.expected_incarnation_rows != 1
        || scope.expected_instance_rows != 1
        || scope.expected_product_rows != 1
    {
        return Err(config_failure(
            "live PoC requires exactly one row from every fixed query",
        ));
    }
    let is_free_pdb = matches!(
        (&scope.principal_placement, &scope.connection_container),
        (
            PrincipalPlacement::LocalUserInPluggableDatabase {
                name: principal_name
            },
            ConnectionContainer::PluggableDatabase {
                name: connection_name
            }
        ) if principal_name.as_str() == "FREEPDB1"
            && connection_name.as_str() == "FREEPDB1"
            && scope.container_data == ContainerDataPolicy::NotApplicable
    );
    if !is_free_pdb {
        return Err(config_failure(
            "live PoC requires one local observer in FREEPDB1",
        ));
    }
    Ok(())
}

fn validate_deadline_reserve(timeouts: &ObserverTimeouts) -> Result<(), PocFailure> {
    if timeouts.shutdown_millis < 20
        || timeouts.connect_millis <= timeouts.shutdown_millis
        || timeouts.query_millis <= timeouts.shutdown_millis
        || timeouts.snapshot_millis <= timeouts.shutdown_millis
    {
        return Err(config_failure(
            "operation deadlines must reserve the shutdown deadline",
        ));
    }
    Ok(())
}

fn contract_failure(error: ContractError) -> PocFailure {
    PocFailure::new(
        PocStage::Configuration,
        PocCategory::Configuration,
        match error {
            ContractError::DataGuardRequiresEnterprise => "support profile is invalid",
            ContractError::UnsupportedReplicaCount { .. } => "replica count is invalid",
            ContractError::UnsupportedMajorVersion { .. } => "native major version is invalid",
            ContractError::InvalidContainerScope => "observation scope is invalid",
            ContractError::InvalidRowCount { .. } => "expected row count is invalid",
            ContractError::InvalidDuration { .. }
            | ContractError::DurationExceedsSnapshot { .. } => "deadline configuration is invalid",
            _ => "PoC configuration is invalid",
        },
    )
}

fn config_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Configuration, PocCategory::Configuration, message)
}

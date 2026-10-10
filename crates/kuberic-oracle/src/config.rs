use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ContractError, NativeName, PinnedImage};

pub const STANDALONE_REPLICA_COUNT: u8 = 1;
pub const DATA_GUARD_REPLICA_COUNT: u8 = 3;
const MAX_FILE_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionProfile {
    Free,
    EnterpriseDeveloper,
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicationProfile {
    Standalone,
    DataGuardPhysical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleSupportConfig {
    pub edition: EditionProfile,
    pub replication: ReplicationProfile,
    pub replica_count: u8,
    pub expected_native_major_version: u16,
    pub image: PinnedImage,
}

impl OracleSupportConfig {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.expected_native_major_version < 23 {
            return Err(ContractError::UnsupportedMajorVersion {
                minimum: 23,
                actual: self.expected_native_major_version,
            });
        }

        match self.replication {
            ReplicationProfile::Standalone => {
                if self.replica_count != STANDALONE_REPLICA_COUNT {
                    return Err(ContractError::UnsupportedReplicaCount {
                        profile: "standalone",
                        expected: STANDALONE_REPLICA_COUNT,
                        actual: self.replica_count,
                    });
                }
            }
            ReplicationProfile::DataGuardPhysical => {
                if self.edition == EditionProfile::Free {
                    return Err(ContractError::DataGuardRequiresEnterprise);
                }
                if self.replica_count != DATA_GUARD_REPLICA_COUNT {
                    return Err(ContractError::UnsupportedReplicaCount {
                        profile: "data_guard_physical",
                        expected: DATA_GUARD_REPLICA_COUNT,
                        actual: self.replica_count,
                    });
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverFileConfig {
    pub username_file: PathBuf,
    pub password_file: PathBuf,
    pub tls_ca_bundle_file: PathBuf,
}

impl ObserverFileConfig {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_file_path("username_file", &self.username_file)?;
        validate_file_path("password_file", &self.password_file)?;
        validate_file_path("tls_ca_bundle_file", &self.tls_ca_bundle_file)?;
        if self.username_file == self.password_file
            || self.username_file == self.tls_ca_bundle_file
            || self.password_file == self.tls_ca_bundle_file
        {
            return Err(ContractError::ObserverFilesMustBeDistinct);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalPlacement {
    CommonUserInCdbRoot,
    LocalUserInPluggableDatabase { name: NativeName },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionContainer {
    CdbRoot,
    PluggableDatabase { name: NativeName },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerDataPolicy {
    NotApplicable,
    CurrentContainerOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationScope {
    pub principal_placement: PrincipalPlacement,
    pub connection_container: ConnectionContainer,
    pub container_data: ContainerDataPolicy,
    pub expected_database_rows: u16,
    pub expected_instance_rows: u16,
    pub expected_product_rows: u16,
}

impl ObservationScope {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_row_count("expected_database_rows", self.expected_database_rows)?;
        validate_row_count("expected_instance_rows", self.expected_instance_rows)?;
        validate_row_count("expected_product_rows", self.expected_product_rows)?;

        match (&self.principal_placement, &self.connection_container) {
            (
                PrincipalPlacement::LocalUserInPluggableDatabase {
                    name: principal_name,
                },
                ConnectionContainer::PluggableDatabase {
                    name: connection_name,
                },
            ) if principal_name == connection_name
                && self.container_data == ContainerDataPolicy::NotApplicable => {}
            (PrincipalPlacement::CommonUserInCdbRoot, _)
                if self.container_data == ContainerDataPolicy::CurrentContainerOnly => {}
            _ => return Err(ContractError::InvalidContainerScope),
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverTimeouts {
    pub connect_millis: u64,
    pub query_millis: u64,
    pub snapshot_millis: u64,
    pub shutdown_millis: u64,
}

impl ObserverTimeouts {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_duration("connect_millis", self.connect_millis)?;
        validate_duration("query_millis", self.query_millis)?;
        validate_duration("snapshot_millis", self.snapshot_millis)?;
        validate_duration("shutdown_millis", self.shutdown_millis)?;

        if self.connect_millis > self.snapshot_millis {
            return Err(ContractError::DurationExceedsSnapshot {
                field: "connect_millis",
            });
        }
        if self.query_millis > self.snapshot_millis {
            return Err(ContractError::DurationExceedsSnapshot {
                field: "query_millis",
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleObserverConfig {
    pub support: OracleSupportConfig,
    pub files: ObserverFileConfig,
    pub scope: ObservationScope,
    pub timeouts: ObserverTimeouts,
    pub poll_interval_millis: u64,
}

impl OracleObserverConfig {
    pub fn validate(&self) -> Result<(), ContractError> {
        self.support.validate()?;
        self.files.validate()?;
        self.scope.validate()?;
        self.timeouts.validate()?;
        validate_duration("poll_interval_millis", self.poll_interval_millis)?;
        Ok(())
    }
}

fn validate_file_path(field: &'static str, path: &Path) -> Result<(), ContractError> {
    let path_text = path
        .to_str()
        .filter(|value| !value.is_empty() && value.len() <= MAX_FILE_PATH_BYTES);
    let has_root = path.is_absolute();
    let has_file_name = path
        .components()
        .any(|component| matches!(component, Component::Normal(_)));
    let is_normalized = !path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir));

    if path_text.is_none() || !has_root || !has_file_name || !is_normalized {
        return Err(ContractError::InvalidPath { field });
    }

    Ok(())
}

fn validate_duration(field: &'static str, value: u64) -> Result<(), ContractError> {
    if value == 0 {
        return Err(ContractError::InvalidDuration { field });
    }
    Ok(())
}

fn validate_row_count(field: &'static str, value: u16) -> Result<(), ContractError> {
    if value == 0 {
        return Err(ContractError::InvalidRowCount { field });
    }
    Ok(())
}

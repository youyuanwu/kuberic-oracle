use serde::{Deserialize, Serialize};

use crate::{ContractError, PinnedImage};

pub const STANDALONE_REPLICA_COUNT: u8 = 1;
pub const DATA_GUARD_REPLICA_COUNT: u8 = 3;

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
    pub expected_major_version: u16,
    pub image: PinnedImage,
}

impl OracleSupportConfig {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.expected_major_version < 23 {
            return Err(ContractError::UnsupportedMajorVersion {
                minimum: 23,
                actual: self.expected_major_version,
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

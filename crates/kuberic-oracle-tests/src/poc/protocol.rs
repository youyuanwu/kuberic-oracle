use std::path::PathBuf;

use kuberic_oracle::{ObservationScope, ObserverTimeouts, OracleObservation, OracleSupportConfig};
use serde::{Deserialize, Serialize};

use super::{
    config::PocEndpoint,
    error::{PocFailure, PocStage},
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const BUILD_ID: &str = env!("CARGO_PKG_VERSION");
pub const MAX_PROTOCOL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationRequest {
    pub endpoint: PocEndpoint,
    pub username_file: PathBuf,
    pub password_file: PathBuf,
    pub support: OracleSupportConfig,
    pub scope: ObservationScope,
    pub timeouts: ObserverTimeouts,
    pub target_database_id: u64,
    pub target_database_unique_name: String,
    pub observed_at_unix_millis: u64,
    pub now_unix_millis: u64,
    pub maximum_age_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperStep {
    pub stage: PocStage,
    pub millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum HelperMode {
    Success,
    Sleep { stage: PocStage, millis: u64 },
    Sequence { steps: Vec<HelperStep> },
    Panic { stage: PocStage },
    MalformedOutput,
    IgnoreTerm { stage: PocStage, millis: u64 },
    SpawnDescendant { stage: PocStage, millis: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request")]
pub enum WorkerRequest {
    Observe(Box<ObservationRequest>),
    Helper(HelperMode),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "command")]
pub enum ParentCommand {
    Start {
        protocol_version: u16,
        request: Box<WorkerRequest>,
    },
    Arm {
        stage: PocStage,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum WorkerEvent {
    Hello {
        protocol_version: u16,
        build_id: String,
    },
    StageReady {
        stage: PocStage,
    },
    Success {
        observation: Box<OracleObservation>,
    },
    Failure {
        failure: PocFailure,
    },
}

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocStage {
    Configuration,
    SecretRead,
    Spawn,
    Handshake,
    Connect,
    QueryIdentityBefore,
    QueryDatabase,
    QueryInstance,
    QueryProduct,
    QueryIdentityAfter,
    ParseAdmission,
    Close,
    Cancellation,
    Termination,
    Reap,
    Fixture,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocCategory {
    Configuration,
    Capability,
    Authentication,
    Authorization,
    Transport,
    Protocol,
    Timeout,
    Cancelled,
    RowCount,
    NativeValue,
    IdentityChanged,
    Shutdown,
    Fixture,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PocFailure {
    stage: PocStage,
    category: PocCategory,
    oracle_code: Option<usize>,
    message: String,
    cleanup: Option<Box<PocFailure>>,
}

impl PocFailure {
    pub(crate) fn new(stage: PocStage, category: PocCategory, message: &'static str) -> Self {
        Self {
            stage,
            category,
            oracle_code: None,
            message: message.to_owned(),
            cleanup: None,
        }
    }

    pub(crate) fn with_oracle_code(mut self, oracle_code: usize) -> Self {
        self.oracle_code = Some(oracle_code);
        self
    }

    pub(crate) fn with_cleanup(mut self, cleanup: PocFailure) -> Self {
        self.cleanup = Some(Box::new(cleanup));
        self
    }

    pub const fn stage(&self) -> PocStage {
        self.stage
    }

    pub const fn category(&self) -> PocCategory {
        self.category
    }

    pub const fn oracle_code(&self) -> Option<usize> {
        self.oracle_code
    }

    pub fn cleanup(&self) -> Option<&PocFailure> {
        self.cleanup.as_deref()
    }

    pub fn fixture_assertion(message: &'static str) -> Self {
        Self::new(PocStage::Fixture, PocCategory::Fixture, message)
    }
}

impl fmt::Display for PocFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?}/{:?}: {}",
            self.stage, self.category, self.message
        )?;
        if let Some(code) = self.oracle_code {
            write!(formatter, " (ORA-{code:05})")?;
        }
        if let Some(cleanup) = &self.cleanup {
            write!(formatter, "; cleanup: {cleanup}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PocFailure {}

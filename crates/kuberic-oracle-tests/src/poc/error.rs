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
    QueryIncarnationBefore,
    QueryDatabase,
    QueryInstance,
    QueryProduct,
    QueryIncarnationAfter,
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

    pub(crate) fn fixture_command(message: String) -> Self {
        Self {
            stage: PocStage::Fixture,
            category: PocCategory::Fixture,
            oracle_code: None,
            message,
            cleanup: None,
        }
    }

    pub fn with_cleanup(mut self, cleanup: PocFailure) -> Self {
        self.cleanup = Some(Box::new(cleanup));
        self
    }

    pub(crate) fn append_cleanup(mut self, cleanup: PocFailure) -> Self {
        self.cleanup = Some(match self.cleanup.take() {
            Some(existing) => Box::new(existing.append_cleanup(cleanup)),
            None => Box::new(cleanup),
        });
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

    pub fn fixture_timeout() -> Self {
        Self::new(
            PocStage::Fixture,
            PocCategory::Timeout,
            "fixture operation timed out",
        )
    }

    pub(crate) fn sanitized_from_worker(self) -> Self {
        let message = match self.category {
            PocCategory::Configuration => "worker configuration failed",
            PocCategory::Capability => "worker capability is unavailable",
            PocCategory::Authentication => "Oracle authentication failed",
            PocCategory::Authorization => "Oracle authorization failed",
            PocCategory::Transport => "Oracle transport failed",
            PocCategory::Protocol => "Oracle protocol operation failed",
            PocCategory::Timeout => "Oracle operation timed out",
            PocCategory::Cancelled => "Oracle operation was cancelled",
            PocCategory::RowCount => "fixed query row count was invalid",
            PocCategory::NativeValue => "Oracle evidence was invalid",
            PocCategory::IdentityChanged => "Oracle identity changed",
            PocCategory::Shutdown => "worker shutdown failed",
            PocCategory::Fixture => "fixture operation failed",
            PocCategory::Cleanup => "fixture cleanup failed",
        };
        Self {
            stage: self.stage,
            category: self.category,
            oracle_code: self.oracle_code,
            message: message.to_owned(),
            cleanup: None,
        }
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

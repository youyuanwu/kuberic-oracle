use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ContractError {
    #[error("{field} must not be empty")]
    MissingField { field: &'static str },
    #[error("{field} exceeds the maximum length of {max}")]
    InvalidLength { field: &'static str, max: usize },
    #[error("{field} contains an unsupported character")]
    InvalidCharacter { field: &'static str },
    #[error("container image must be pinned as a repository reference with a sha256 digest")]
    InvalidImageDigest,
    #[error("Oracle SCN must be a 1-38 digit unsigned decimal")]
    InvalidScn,
    #[error("{field} must be an unsigned decimal integer")]
    InvalidUnsignedInteger { field: &'static str },
    #[error("Oracle DBID must be nonzero")]
    InvalidDatabaseId,
    #[error("unsupported Oracle value for {field}: {value}")]
    UnsupportedNativeValue { field: &'static str, value: String },
    #[error("Oracle product version is malformed")]
    InvalidProductVersion,
    #[error("database or instance identity changed during observation")]
    ObservationIdentityChanged,
    #[error("{field} duration must be nonzero")]
    InvalidDuration { field: &'static str },
    #[error("{field} duration must not exceed the whole-snapshot deadline")]
    DurationExceedsSnapshot { field: &'static str },
    #[error("{field} path must be absolute, normalized, UTF-8, and at most 4096 bytes")]
    InvalidPath { field: &'static str },
    #[error("username, password, and TLS trust material must use distinct files")]
    ObserverFilesMustBeDistinct,
    #[error("{field} expected row count must be nonzero")]
    InvalidRowCount { field: &'static str },
    #[error("principal placement and connection container are incompatible")]
    InvalidContainerScope,
    #[error("Oracle Data Guard requires Enterprise Developer or Enterprise edition")]
    DataGuardRequiresEnterprise,
    #[error("unsupported replica count for {profile}: expected {expected}, observed {actual}")]
    UnsupportedReplicaCount {
        profile: &'static str,
        expected: u8,
        actual: u8,
    },
    #[error(
        "unsupported Oracle native major version: expected at least {minimum}, observed {actual}"
    )]
    UnsupportedMajorVersion { minimum: u16, actual: u16 },
    #[error("observation timestamp is in the future")]
    FutureObservation,
    #[error("observation is older than the configured freshness bound")]
    StaleObservation,
    #[error("observed database identity does not match the configured target")]
    DatabaseIdentityMismatch,
    #[error("observed native edition does not match the configured profile")]
    EditionMismatch,
    #[error("observed Oracle native major version does not match the configured profile")]
    MajorVersionMismatch,
    #[error("unsupported native database role for the configured profile")]
    UnsupportedDatabaseRole,
    #[error("unsupported open mode for the observed native database role")]
    UnsupportedOpenMode,
    #[error("Data Guard admission requires ARCHIVELOG mode")]
    ArchiveLogRequired,
    #[error("Data Guard admission requires FORCE LOGGING")]
    ForceLoggingRequired,
    #[error("CURRENT_SCN is required for the observed role and open mode")]
    CurrentScnRequired,
}

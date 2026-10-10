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
    #[error("Oracle DBID must be nonzero")]
    InvalidDatabaseId,
    #[error("Oracle Data Guard requires Enterprise Developer or Enterprise edition")]
    DataGuardRequiresEnterprise,
    #[error("unsupported replica count for {profile}: expected {expected}, observed {actual}")]
    UnsupportedReplicaCount {
        profile: &'static str,
        expected: u8,
        actual: u8,
    },
    #[error("unsupported Oracle major version: expected at least {minimum}, observed {actual}")]
    UnsupportedMajorVersion { minimum: u16, actual: u16 },
    #[error("observation timestamp is in the future")]
    FutureObservation,
    #[error("observation is older than the configured freshness bound")]
    StaleObservation,
    #[error("observed database identity does not match the configured target")]
    DatabaseIdentityMismatch,
    #[error("observed edition does not match the configured profile")]
    EditionMismatch,
    #[error("observed Oracle major version does not match the configured artifact")]
    MajorVersionMismatch,
    #[error("unsupported native database role for the configured profile")]
    UnsupportedDatabaseRole,
    #[error("unsupported open mode for the observed native database role")]
    UnsupportedOpenMode,
    #[error("Data Guard admission requires ARCHIVELOG mode")]
    ArchiveLogRequired,
    #[error("Data Guard admission requires FORCE LOGGING")]
    ForceLoggingRequired,
}

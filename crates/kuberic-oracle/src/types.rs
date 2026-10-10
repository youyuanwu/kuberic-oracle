use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{ContractError, EditionProfile};

const MAX_NATIVE_NAME_BYTES: usize = 128;
const MAX_SCN_DIGITS: usize = 38;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PinnedImage(String);

impl PinnedImage {
    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        let (repository, digest) = value
            .rsplit_once('@')
            .ok_or(ContractError::InvalidImageDigest)?;
        let hash = digest
            .strip_prefix("sha256:")
            .ok_or(ContractError::InvalidImageDigest)?;

        if repository.is_empty()
            || repository.chars().any(char::is_whitespace)
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ContractError::InvalidImageDigest);
        }

        Ok(Self(format!(
            "{repository}@sha256:{}",
            hash.to_ascii_lowercase()
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PinnedImage {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<PinnedImage> for String {
    fn from(value: PinnedImage) -> Self {
        value.0
    }
}

impl fmt::Display for PinnedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NativeName(String);

impl NativeName {
    pub fn new(field: &'static str, value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractError::MissingField { field });
        }
        if value.len() > MAX_NATIVE_NAME_BYTES {
            return Err(ContractError::InvalidLength {
                field,
                max: MAX_NATIVE_NAME_BYTES,
            });
        }
        if value.trim() != value || value.chars().any(char::is_control) {
            return Err(ContractError::InvalidCharacter { field });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NativeName {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new("Oracle native name", value)
    }
}

impl From<NativeName> for String {
    fn from(value: NativeName) -> Self {
        value.0
    }
}

impl fmt::Display for NativeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct DatabaseId(u64);

impl DatabaseId {
    pub fn new(value: u64) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::InvalidDatabaseId);
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for DatabaseId {
    type Error = ContractError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DatabaseId> for u64 {
    fn from(value: DatabaseId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecimalScn(u128);

impl DecimalScn {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        if value.is_empty()
            || value.len() > MAX_SCN_DIGITS
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ContractError::InvalidScn);
        }
        value
            .parse::<u128>()
            .map(Self)
            .map_err(|_| ContractError::InvalidScn)
    }

    pub const fn value(self) -> u128 {
        self.0
    }
}

impl fmt::Display for DecimalScn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for DecimalScn {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DecimalScn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseRole {
    Primary,
    PhysicalStandby,
    LogicalStandby,
    SnapshotStandby,
    FarSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenMode {
    Mounted,
    ReadOnly,
    ReadOnlyWithApply,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogMode {
    ArchiveLog,
    NoArchiveLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSnapshot {
    pub database_id: DatabaseId,
    pub database_name: NativeName,
    pub database_unique_name: NativeName,
    pub product_major_version: u16,
    pub edition: EditionProfile,
    pub role: DatabaseRole,
    pub open_mode: OpenMode,
    pub current_scn: DecimalScn,
    pub resetlogs_change: DecimalScn,
    pub activation_number: u64,
    pub log_mode: LogMode,
    pub force_logging: bool,
    pub is_container_database: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceSnapshot {
    pub instance_name: NativeName,
    pub host_name: NativeName,
    pub version_full: String,
    pub startup_time: String,
    pub status: String,
    pub database_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleObservation {
    pub observed_at_unix_millis: u64,
    pub database: DatabaseSnapshot,
    pub instance: InstanceSnapshot,
}

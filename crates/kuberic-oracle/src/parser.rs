use serde::{Deserialize, Serialize};

use crate::{
    ContractError, DatabaseId, DatabaseRole, DatabaseSnapshot, DecimalScn, ForceLogging,
    InstanceSnapshot, LogMode, NativeEdition, NativeName, OpenMode, OracleObservation,
    ProductSnapshot,
};

const MAX_OBSERVED_TEXT_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawObservationIdentity {
    pub database_id: String,
    pub database_unique_name: String,
    pub instance_name: String,
    pub startup_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDatabaseRow {
    pub database_id: String,
    pub database_name: String,
    pub database_unique_name: String,
    pub database_role: String,
    pub open_mode: String,
    pub current_scn: Option<String>,
    pub resetlogs_change: String,
    pub activation_number: String,
    pub log_mode: String,
    pub force_logging: String,
    pub cdb: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawInstanceRow {
    pub instance_name: String,
    pub host_name: String,
    pub version_full: String,
    pub edition: String,
    pub startup_time: String,
    pub status: String,
    pub database_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProductRow {
    pub product: String,
    pub version: String,
    pub version_full: String,
    pub release_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawObservationRows {
    pub before: RawObservationIdentity,
    pub database: RawDatabaseRow,
    pub instance: RawInstanceRow,
    pub product: RawProductRow,
    pub after: RawObservationIdentity,
}

pub fn parse_observation_rows(
    rows: RawObservationRows,
    observed_at_unix_millis: u64,
) -> Result<OracleObservation, ContractError> {
    let before = parse_identity(rows.before)?;
    let after = parse_identity(rows.after)?;

    if before != after {
        return Err(ContractError::ObservationIdentityChanged);
    }

    let product = parse_product(rows.product)?;
    let database = parse_database(rows.database)?;
    let instance = parse_instance(rows.instance)?;

    if database.database_id != before.database_id
        || database.database_unique_name != before.database_unique_name
        || instance.instance_name != before.instance_name
        || instance.startup_time != before.startup_time
    {
        return Err(ContractError::ObservationIdentityChanged);
    }

    if parse_major_version(&instance.version_full)? != product.native_major_version {
        return Err(ContractError::InvalidProductVersion);
    }

    Ok(OracleObservation {
        observed_at_unix_millis,
        database,
        instance,
        product,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedIdentity {
    database_id: DatabaseId,
    database_unique_name: NativeName,
    instance_name: NativeName,
    startup_time: String,
}

fn parse_identity(row: RawObservationIdentity) -> Result<ParsedIdentity, ContractError> {
    Ok(ParsedIdentity {
        database_id: parse_database_id(&row.database_id)?,
        database_unique_name: NativeName::new("database_unique_name", row.database_unique_name)?,
        instance_name: NativeName::new("instance_name", row.instance_name)?,
        startup_time: parse_text("startup_time", row.startup_time)?,
    })
}

fn parse_database(row: RawDatabaseRow) -> Result<DatabaseSnapshot, ContractError> {
    Ok(DatabaseSnapshot {
        database_id: parse_database_id(&row.database_id)?,
        database_name: NativeName::new("database_name", row.database_name)?,
        database_unique_name: NativeName::new("database_unique_name", row.database_unique_name)?,
        role: parse_database_role(&row.database_role)?,
        open_mode: parse_open_mode(&row.open_mode)?,
        current_scn: row
            .current_scn
            .as_deref()
            .map(DecimalScn::parse)
            .transpose()?,
        resetlogs_change: DecimalScn::parse(&row.resetlogs_change)?,
        activation_number: parse_u64("activation_number", &row.activation_number)?,
        log_mode: parse_log_mode(&row.log_mode)?,
        force_logging: parse_force_logging(&row.force_logging)?,
        is_container_database: parse_yes_no("cdb", &row.cdb)?,
    })
}

fn parse_instance(row: RawInstanceRow) -> Result<InstanceSnapshot, ContractError> {
    Ok(InstanceSnapshot {
        instance_name: NativeName::new("instance_name", row.instance_name)?,
        host_name: NativeName::new("host_name", row.host_name)?,
        version_full: parse_text("instance_version_full", row.version_full)?,
        native_edition: parse_native_edition(&row.edition)?,
        startup_time: parse_text("startup_time", row.startup_time)?,
        status: parse_text("instance_status", row.status)?,
        database_status: parse_text("database_status", row.database_status)?,
    })
}

fn parse_product(row: RawProductRow) -> Result<ProductSnapshot, ContractError> {
    let product = parse_text("product", row.product)?;
    if !product.starts_with("Oracle AI Database") {
        return Err(unsupported("product", &product));
    }
    let version = parse_text("product_version", row.version)?;
    let version_full = parse_text("product_version_full", row.version_full)?;
    let release_status = parse_text("product_release_status", row.release_status)?;
    let native_major_version = parse_major_version(&version_full)?;
    if parse_major_version(&version)? != native_major_version {
        return Err(ContractError::InvalidProductVersion);
    }
    Ok(ProductSnapshot {
        product,
        version,
        version_full,
        native_major_version,
        release_status,
    })
}

fn parse_database_id(value: &str) -> Result<DatabaseId, ContractError> {
    DatabaseId::new(parse_u64("database_id", value)?)
}

fn parse_u64(field: &'static str, value: &str) -> Result<u64, ContractError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ContractError::InvalidUnsignedInteger { field });
    }
    value
        .parse()
        .map_err(|_| ContractError::InvalidUnsignedInteger { field })
}

fn parse_major_version(value: &str) -> Result<u16, ContractError> {
    let mut components = value.split('.');
    let major = components
        .next()
        .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(ContractError::InvalidProductVersion)?
        .parse::<u16>()
        .map_err(|_| ContractError::InvalidProductVersion)?;

    if major == 0
        || components.any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.parse::<u16>().is_err()
        })
    {
        return Err(ContractError::InvalidProductVersion);
    }
    Ok(major)
}

fn parse_database_role(value: &str) -> Result<DatabaseRole, ContractError> {
    match value {
        "PRIMARY" => Ok(DatabaseRole::Primary),
        "PHYSICAL STANDBY" => Ok(DatabaseRole::PhysicalStandby),
        "LOGICAL STANDBY" => Ok(DatabaseRole::LogicalStandby),
        "SNAPSHOT STANDBY" => Ok(DatabaseRole::SnapshotStandby),
        "FAR SYNC" => Ok(DatabaseRole::FarSync),
        _ => Err(unsupported("database_role", value)),
    }
}

fn parse_native_edition(value: &str) -> Result<NativeEdition, ContractError> {
    match value {
        "FREE" => Ok(NativeEdition::Free),
        "EE" => Ok(NativeEdition::Enterprise),
        _ => Err(unsupported("edition", value)),
    }
}

fn parse_open_mode(value: &str) -> Result<OpenMode, ContractError> {
    match value {
        "MOUNTED" => Ok(OpenMode::Mounted),
        "READ ONLY" => Ok(OpenMode::ReadOnly),
        "READ ONLY WITH APPLY" => Ok(OpenMode::ReadOnlyWithApply),
        "READ WRITE" => Ok(OpenMode::ReadWrite),
        _ => Err(unsupported("open_mode", value)),
    }
}

fn parse_log_mode(value: &str) -> Result<LogMode, ContractError> {
    match value {
        "ARCHIVELOG" => Ok(LogMode::ArchiveLog),
        "NOARCHIVELOG" => Ok(LogMode::NoArchiveLog),
        _ => Err(unsupported("log_mode", value)),
    }
}

fn parse_force_logging(value: &str) -> Result<ForceLogging, ContractError> {
    match value {
        "YES" => Ok(ForceLogging::Enabled),
        "NO" => Ok(ForceLogging::Disabled),
        _ => Err(unsupported("force_logging", value)),
    }
}

fn parse_yes_no(field: &'static str, value: &str) -> Result<bool, ContractError> {
    match value {
        "YES" => Ok(true),
        "NO" => Ok(false),
        _ => Err(unsupported(field, value)),
    }
}

fn parse_text(field: &'static str, value: String) -> Result<String, ContractError> {
    if value.is_empty() {
        return Err(ContractError::MissingField { field });
    }
    if value.len() > MAX_OBSERVED_TEXT_BYTES {
        return Err(ContractError::InvalidLength {
            field,
            max: MAX_OBSERVED_TEXT_BYTES,
        });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(ContractError::InvalidCharacter { field });
    }
    Ok(value)
}

fn unsupported(field: &'static str, value: &str) -> ContractError {
    ContractError::UnsupportedNativeValue {
        field,
        value: value.to_owned(),
    }
}

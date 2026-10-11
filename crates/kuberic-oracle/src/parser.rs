use serde::{Deserialize, Serialize};

use crate::{
    ContractError, DatabaseId, DatabaseIncarnationIdentity, DatabaseIncarnationSnapshot,
    DatabaseIncarnationStatus, DatabaseRole, DatabaseSnapshot, DecimalScn, ForceLogging,
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
    pub incarnation_number: String,
    pub resetlogs_id: String,
    pub resetlogs_change: String,
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
pub struct RawDatabaseIncarnationRow {
    pub incarnation_number: String,
    pub resetlogs_change: String,
    pub resetlogs_time: String,
    pub prior_resetlogs_change: String,
    pub prior_resetlogs_time: Option<String>,
    pub status: String,
    pub resetlogs_id: String,
    pub prior_incarnation_number: String,
    pub container_id: String,
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
    pub incarnation_before: RawDatabaseIncarnationRow,
    pub database: RawDatabaseRow,
    pub instance: RawInstanceRow,
    pub product: RawProductRow,
    pub incarnation_after: RawDatabaseIncarnationRow,
    pub after: RawObservationIdentity,
}

pub fn parse_observation_rows(
    rows: RawObservationRows,
    observed_at_unix_millis: u64,
) -> Result<OracleObservation, ContractError> {
    let before = parse_identity(rows.before)?;
    let after = parse_identity(rows.after)?;
    let incarnation_before = parse_incarnation(rows.incarnation_before)?;
    let incarnation_after = parse_incarnation(rows.incarnation_after)?;

    if before != after
        || incarnation_before != incarnation_after
        || before.incarnation != incarnation_before.identity
    {
        return Err(ContractError::ObservationIdentityChanged);
    }

    let product = parse_product(rows.product)?;
    let database = parse_database(rows.database)?;
    let instance = parse_instance(rows.instance)?;

    if database.database_id != before.database_id
        || database.database_unique_name != before.database_unique_name
        || instance.instance_name != before.instance_name
        || instance.startup_time != before.startup_time
        || database.resetlogs_change != incarnation_before.identity.resetlogs_change
    {
        return Err(ContractError::ObservationIdentityChanged);
    }

    if parse_major_version(&instance.version_full)? != product.native_major_version {
        return Err(ContractError::InvalidProductVersion);
    }
    validate_product_edition(&product.product, instance.native_edition)?;

    Ok(OracleObservation {
        observed_at_unix_millis,
        database,
        incarnation: incarnation_before,
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
    incarnation: DatabaseIncarnationIdentity,
}

fn parse_identity(row: RawObservationIdentity) -> Result<ParsedIdentity, ContractError> {
    Ok(ParsedIdentity {
        database_id: parse_database_id(&row.database_id)?,
        database_unique_name: NativeName::new("database_unique_name", row.database_unique_name)?,
        instance_name: NativeName::new("instance_name", row.instance_name)?,
        startup_time: parse_text("startup_time", row.startup_time)?,
        incarnation: DatabaseIncarnationIdentity {
            incarnation_number: parse_positive_u64("incarnation_number", &row.incarnation_number)?,
            resetlogs_id: parse_positive_u64("resetlogs_id", &row.resetlogs_id)?,
            resetlogs_change: DecimalScn::parse(&row.resetlogs_change)?,
        },
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

fn parse_incarnation(
    row: RawDatabaseIncarnationRow,
) -> Result<DatabaseIncarnationSnapshot, ContractError> {
    let incarnation_number = parse_positive_u64("incarnation_number", &row.incarnation_number)?;
    let resetlogs_id = parse_positive_u64("resetlogs_id", &row.resetlogs_id)?;
    let prior_incarnation_number =
        parse_u64("prior_incarnation_number", &row.prior_incarnation_number)?;
    let container_id = parse_u64("container_id", &row.container_id)?;
    if container_id != 0 {
        return Err(unsupported("container_id", &row.container_id));
    }
    let status = match row.status.as_str() {
        "CURRENT" => DatabaseIncarnationStatus::Current,
        _ => return Err(unsupported("incarnation_status", &row.status)),
    };
    let resetlogs_time = parse_oracle_date_text("resetlogs_time", row.resetlogs_time)?;
    let prior_resetlogs_time = row
        .prior_resetlogs_time
        .map(|value| parse_oracle_date_text("prior_resetlogs_time", value))
        .transpose()?;
    if prior_incarnation_number != 0 && prior_resetlogs_time.is_none() {
        return Err(ContractError::MissingField {
            field: "prior_resetlogs_time",
        });
    }

    let snapshot = DatabaseIncarnationSnapshot {
        identity: DatabaseIncarnationIdentity {
            incarnation_number,
            resetlogs_id,
            resetlogs_change: DecimalScn::parse(&row.resetlogs_change)?,
        },
        resetlogs_time,
        prior_resetlogs_change: DecimalScn::parse(&row.prior_resetlogs_change)?,
        prior_resetlogs_time,
        prior_incarnation_number,
        status,
        container_id,
    };
    validate_incarnation_snapshot(&snapshot)?;
    Ok(snapshot)
}

pub(crate) fn validate_observation_evidence(
    observation: &OracleObservation,
) -> Result<(), ContractError> {
    validate_incarnation_snapshot(&observation.incarnation)?;
    if observation.database.resetlogs_change != observation.incarnation.identity.resetlogs_change {
        return Err(ContractError::ObservationIdentityChanged);
    }
    Ok(())
}

fn validate_incarnation_snapshot(
    incarnation: &DatabaseIncarnationSnapshot,
) -> Result<(), ContractError> {
    if incarnation.identity.incarnation_number == 0 {
        return Err(ContractError::InvalidUnsignedInteger {
            field: "incarnation_number",
        });
    }
    if incarnation.identity.resetlogs_id == 0 {
        return Err(ContractError::InvalidUnsignedInteger {
            field: "resetlogs_id",
        });
    }
    if incarnation.container_id != 0 {
        return Err(unsupported(
            "container_id",
            &incarnation.container_id.to_string(),
        ));
    }
    parse_oracle_date_text("resetlogs_time", incarnation.resetlogs_time.clone())?;
    if let Some(prior_time) = &incarnation.prior_resetlogs_time {
        parse_oracle_date_text("prior_resetlogs_time", prior_time.clone())?;
    }
    if incarnation.prior_incarnation_number != 0 && incarnation.prior_resetlogs_time.is_none() {
        return Err(ContractError::MissingField {
            field: "prior_resetlogs_time",
        });
    }
    Ok(())
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

fn parse_positive_u64(field: &'static str, value: &str) -> Result<u64, ContractError> {
    let parsed = parse_u64(field, value)?;
    if parsed == 0 {
        return Err(ContractError::InvalidUnsignedInteger { field });
    }
    Ok(parsed)
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

fn validate_product_edition(
    product: &str,
    native_edition: NativeEdition,
) -> Result<(), ContractError> {
    let matches = match native_edition {
        NativeEdition::Free => product.ends_with(" Free"),
        NativeEdition::Enterprise => product.ends_with(" Enterprise Edition"),
    };
    if !matches {
        return Err(unsupported("product_edition", product));
    }
    Ok(())
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

fn parse_oracle_date_text(field: &'static str, value: String) -> Result<String, ContractError> {
    let value = parse_text(field, value)?;
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(unsupported(field, &value));
    }

    let year = parse_date_component(field, bytes, 0, 4)?;
    let month = parse_date_component(field, bytes, 5, 7)?;
    let day = parse_date_component(field, bytes, 8, 10)?;
    let hour = parse_date_component(field, bytes, 11, 13)?;
    let minute = parse_date_component(field, bytes, 14, 16)?;
    let second = parse_date_component(field, bytes, 17, 19)?;
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return Err(unsupported(field, &value)),
    };
    if year == 0 || day == 0 || day > maximum_day || hour > 23 || minute > 59 || second > 59 {
        return Err(unsupported(field, &value));
    }
    Ok(value)
}

fn parse_date_component(
    field: &'static str,
    value: &[u8],
    start: usize,
    end: usize,
) -> Result<u32, ContractError> {
    let component = &value[start..end];
    if !component.iter().all(u8::is_ascii_digit) {
        return Err(ContractError::UnsupportedNativeValue {
            field,
            value: String::from_utf8_lossy(value).into_owned(),
        });
    }
    std::str::from_utf8(component)
        .ok()
        .and_then(|component| component.parse().ok())
        .ok_or_else(|| ContractError::UnsupportedNativeValue {
            field,
            value: String::from_utf8_lossy(value).into_owned(),
        })
}

fn unsupported(field: &'static str, value: &str) -> ContractError {
    ContractError::UnsupportedNativeValue {
        field,
        value: value.to_owned(),
    }
}

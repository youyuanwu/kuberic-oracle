use crate::{
    ContractError, DatabaseId, DatabaseRole, DecimalScn, EditionProfile, ForceLogging, LogMode,
    NativeEdition, NativeName, OpenMode, OracleObservation, OracleSupportConfig,
    ReplicationProfile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationTarget {
    pub database_id: DatabaseId,
    pub database_unique_name: NativeName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub role: DatabaseRole,
    pub current_scn: Option<DecimalScn>,
    pub resetlogs_change: DecimalScn,
    pub activation_number: u64,
}

pub fn admit_observation(
    config: &OracleSupportConfig,
    target: &ObservationTarget,
    observation: &OracleObservation,
    now_unix_millis: u64,
    maximum_age_millis: u64,
) -> Result<Admission, ContractError> {
    config.validate()?;

    if observation.observed_at_unix_millis > now_unix_millis {
        return Err(ContractError::FutureObservation);
    }
    if now_unix_millis - observation.observed_at_unix_millis > maximum_age_millis {
        return Err(ContractError::StaleObservation);
    }

    let database = &observation.database;
    if database.database_id != target.database_id
        || database.database_unique_name != target.database_unique_name
    {
        return Err(ContractError::DatabaseIdentityMismatch);
    }
    if observation.instance.native_edition != expected_native_edition(config.edition) {
        return Err(ContractError::EditionMismatch);
    }
    if observation.product.native_major_version != config.expected_native_major_version {
        return Err(ContractError::MajorVersionMismatch);
    }

    match config.replication {
        ReplicationProfile::Standalone => admit_standalone(database)?,
        ReplicationProfile::DataGuardPhysical => admit_data_guard(database)?,
    }

    Ok(Admission {
        role: database.role,
        current_scn: database.current_scn,
        resetlogs_change: database.resetlogs_change,
        activation_number: database.activation_number,
    })
}

fn admit_standalone(database: &crate::DatabaseSnapshot) -> Result<(), ContractError> {
    if database.role != DatabaseRole::Primary {
        return Err(ContractError::UnsupportedDatabaseRole);
    }
    if database.open_mode != OpenMode::ReadWrite {
        return Err(ContractError::UnsupportedOpenMode);
    }
    if database.current_scn.is_none() {
        return Err(ContractError::CurrentScnRequired);
    }
    Ok(())
}

fn admit_data_guard(database: &crate::DatabaseSnapshot) -> Result<(), ContractError> {
    if database.log_mode != LogMode::ArchiveLog {
        return Err(ContractError::ArchiveLogRequired);
    }
    if database.force_logging != ForceLogging::Enabled {
        return Err(ContractError::ForceLoggingRequired);
    }

    let valid_open_mode = match database.role {
        DatabaseRole::Primary => database.open_mode == OpenMode::ReadWrite,
        DatabaseRole::PhysicalStandby => matches!(
            database.open_mode,
            OpenMode::Mounted | OpenMode::ReadOnly | OpenMode::ReadOnlyWithApply
        ),
        DatabaseRole::LogicalStandby | DatabaseRole::SnapshotStandby | DatabaseRole::FarSync => {
            false
        }
    };

    if !matches!(
        database.role,
        DatabaseRole::Primary | DatabaseRole::PhysicalStandby
    ) {
        return Err(ContractError::UnsupportedDatabaseRole);
    }
    if !valid_open_mode {
        return Err(ContractError::UnsupportedOpenMode);
    }
    if database.open_mode != OpenMode::Mounted && database.current_scn.is_none() {
        return Err(ContractError::CurrentScnRequired);
    }

    Ok(())
}

const fn expected_native_edition(edition: EditionProfile) -> NativeEdition {
    match edition {
        EditionProfile::Free => NativeEdition::Free,
        EditionProfile::EnterpriseDeveloper | EditionProfile::Enterprise => {
            NativeEdition::Enterprise
        }
    }
}

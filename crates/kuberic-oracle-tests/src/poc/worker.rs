use std::{
    io::{BufRead, BufReader, Read, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use kuberic_oracle::{
    DatabaseId, DatabaseIncarnationIdentity, DecimalScn, NativeName, ObservationTarget,
    RawDatabaseIncarnationRow, RawDatabaseRow, RawInstanceRow, RawObservationIdentity,
    RawObservationRows, RawProductRow, admit_observation, parse_observation_rows,
    query::{
        DATABASE_INCARNATION_SQL, DATABASE_SNAPSHOT_SQL, INSTANCE_SNAPSHOT_SQL,
        OBSERVATION_IDENTITY_SQL, PRODUCT_COMPONENT_SQL,
    },
};
use oracledb::{Config, Error, ErrorKind, OracleNumber, OracleTimestamp, Row};

use super::{
    config::PocAttemptConfig,
    error::{PocCategory, PocFailure, PocStage},
    protocol::{
        BUILD_ID, ObservationRequest, PROTOCOL_VERSION, ParentCommand, WorkerEvent, WorkerRequest,
    },
    secret::read_credentials,
};

#[derive(Debug)]
pub struct WorkerMainError;

pub fn run_observation_worker() -> Result<(), WorkerMainError> {
    let stdin = std::io::stdin();
    let mut input = BufReader::new(
        stdin
            .lock()
            .take(super::protocol::MAX_PROTOCOL_BYTES as u64 + 1),
    );
    let mut output = std::io::stdout().lock();
    write_event(
        &mut output,
        &WorkerEvent::Hello {
            protocol_version: PROTOCOL_VERSION,
            build_id: BUILD_ID.to_owned(),
        },
    )
    .map_err(|_| WorkerMainError)?;

    let request = match read_command(&mut input).map_err(|_| WorkerMainError)? {
        ParentCommand::Start {
            protocol_version,
            request,
        } if protocol_version == PROTOCOL_VERSION => match *request {
            WorkerRequest::Observe(request) => *request,
            WorkerRequest::Helper(_) => return Err(WorkerMainError),
        },
        _ => return Err(WorkerMainError),
    };

    match observe(&mut input, &mut output, request) {
        Ok(observation) => write_event(
            &mut output,
            &WorkerEvent::Success {
                observation: Box::new(observation),
            },
        )
        .map_err(|_| WorkerMainError),
        Err(failure) => {
            let _ = write_event(&mut output, &WorkerEvent::Failure { failure });
            Err(WorkerMainError)
        }
    }
}

fn observe(
    input: &mut impl BufRead,
    output: &mut impl Write,
    request: ObservationRequest,
) -> Result<kuberic_oracle::OracleObservation, PocFailure> {
    PocAttemptConfig {
        request: request.clone(),
    }
    .validate()?;

    arm(input, output, PocStage::SecretRead)?;
    let credentials = read_credentials(&request.username_file, &request.password_file)?;

    arm(input, output, PocStage::Connect)?;
    let config = Config::default()
        .set_credentials(credentials.username.expose(), credentials.password.expose())
        .set_connect_string(&request.endpoint.connect_string())
        .map_err(|error| classify_driver_error(PocStage::Connect, &error))?;
    let mut connection = oracledb::connect(config)
        .map_err(|error| classify_driver_error(PocStage::Connect, &error))?;
    connection
        .set_call_timeout(Some(query_call_timeout(&request)))
        .map_err(|error| classify_driver_error(PocStage::Connect, &error))?;

    let (before, observed_at_unix_millis) = query_identity(
        input,
        output,
        &connection,
        &request,
        PocStage::QueryIdentityBefore,
    )?;
    let incarnation_before = query_incarnation(
        input,
        output,
        &connection,
        &request,
        PocStage::QueryIncarnationBefore,
    )?;
    let database = query_database(input, output, &connection, &request)?;
    let instance = query_instance(input, output, &connection, &request)?;
    let product = query_product(input, output, &connection, &request)?;
    let incarnation_after = query_incarnation(
        input,
        output,
        &connection,
        &request,
        PocStage::QueryIncarnationAfter,
    )?;
    let (after, _) = query_identity(
        input,
        output,
        &connection,
        &request,
        PocStage::QueryIdentityAfter,
    )?;

    arm(input, output, PocStage::ParseAdmission)?;
    let observation = parse_observation_rows(
        RawObservationRows {
            before,
            incarnation_before,
            database,
            instance,
            product,
            incarnation_after,
            after,
        },
        observed_at_unix_millis,
    )
    .map_err(classify_contract_error)?;
    let target = ObservationTarget {
        database_id: DatabaseId::new(request.target_database_id)
            .map_err(classify_contract_error)?,
        database_unique_name: NativeName::new(
            "target_database_unique_name",
            request.target_database_unique_name,
        )
        .map_err(classify_contract_error)?,
        incarnation: DatabaseIncarnationIdentity {
            incarnation_number: request.target_incarnation_number,
            resetlogs_id: request.target_resetlogs_id,
            resetlogs_change: DecimalScn::parse(&request.target_resetlogs_change)
                .map_err(classify_contract_error)?,
        },
    };
    admit_observation(
        &request.support,
        &target,
        &observation,
        unix_millis()?,
        request.maximum_age_millis,
    )
    .map_err(classify_contract_error)?;

    arm(input, output, PocStage::Close)?;
    connection
        .close()
        .map_err(|error| classify_driver_error(PocStage::Close, &error))?;
    Ok(observation)
}

fn query_incarnation(
    input: &mut impl BufRead,
    output: &mut impl Write,
    connection: &oracledb::Connection,
    request: &ObservationRequest,
    stage: PocStage,
) -> Result<RawDatabaseIncarnationRow, PocFailure> {
    arm(input, output, stage)?;
    connection
        .set_call_timeout(Some(query_call_timeout(request)))
        .map_err(|error| classify_driver_error(stage, &error))?;
    let row = query_exactly_one(connection, DATABASE_INCARNATION_SQL, stage)?;
    Ok(RawDatabaseIncarnationRow {
        incarnation_number: number_string(&row, "INCARNATION_NUMBER", stage)?,
        resetlogs_change: number_string(&row, "RESETLOGS_CHANGE", stage)?,
        resetlogs_time: text(&row, "RESETLOGS_TIME", stage)?,
        prior_resetlogs_change: number_string(&row, "PRIOR_RESETLOGS_CHANGE", stage)?,
        prior_resetlogs_time: optional_text(&row, "PRIOR_RESETLOGS_TIME", stage)?,
        status: text(&row, "STATUS", stage)?,
        resetlogs_id: number_string(&row, "RESETLOGS_ID", stage)?,
        prior_incarnation_number: number_string(&row, "PRIOR_INCARNATION_NUMBER", stage)?,
        container_id: number_string(&row, "CONTAINER_ID", stage)?,
    })
}

fn query_identity(
    input: &mut impl BufRead,
    output: &mut impl Write,
    connection: &oracledb::Connection,
    request: &ObservationRequest,
    stage: PocStage,
) -> Result<(RawObservationIdentity, u64), PocFailure> {
    arm(input, output, stage)?;
    let observed_at = unix_millis()?;
    connection
        .set_call_timeout(Some(query_call_timeout(request)))
        .map_err(|error| classify_driver_error(stage, &error))?;
    let row = query_exactly_one(connection, OBSERVATION_IDENTITY_SQL, stage)?;
    Ok((
        RawObservationIdentity {
            database_id: number_string(&row, "DATABASE_ID", stage)?,
            database_unique_name: text(&row, "DATABASE_UNIQUE_NAME", stage)?,
            instance_name: text(&row, "INSTANCE_NAME", stage)?,
            startup_time: timestamp_string(&row, "STARTUP_TIME", stage)?,
        },
        observed_at,
    ))
}

fn query_database(
    input: &mut impl BufRead,
    output: &mut impl Write,
    connection: &oracledb::Connection,
    request: &ObservationRequest,
) -> Result<RawDatabaseRow, PocFailure> {
    let stage = PocStage::QueryDatabase;
    arm(input, output, stage)?;
    connection
        .set_call_timeout(Some(query_call_timeout(request)))
        .map_err(|error| classify_driver_error(stage, &error))?;
    let row = query_exactly_one(connection, DATABASE_SNAPSHOT_SQL, stage)?;
    Ok(RawDatabaseRow {
        database_id: number_string(&row, "DATABASE_ID", stage)?,
        database_name: text(&row, "DATABASE_NAME", stage)?,
        database_unique_name: text(&row, "DATABASE_UNIQUE_NAME", stage)?,
        database_role: text(&row, "DATABASE_ROLE", stage)?,
        open_mode: text(&row, "OPEN_MODE", stage)?,
        current_scn: optional_number_string(&row, "CURRENT_SCN", stage)?,
        resetlogs_change: number_string(&row, "RESETLOGS_CHANGE", stage)?,
        activation_number: number_string(&row, "ACTIVATION_NUMBER", stage)?,
        log_mode: text(&row, "LOG_MODE", stage)?,
        force_logging: text(&row, "FORCE_LOGGING", stage)?,
        cdb: text(&row, "CDB", stage)?,
    })
}

fn query_instance(
    input: &mut impl BufRead,
    output: &mut impl Write,
    connection: &oracledb::Connection,
    request: &ObservationRequest,
) -> Result<RawInstanceRow, PocFailure> {
    let stage = PocStage::QueryInstance;
    arm(input, output, stage)?;
    connection
        .set_call_timeout(Some(query_call_timeout(request)))
        .map_err(|error| classify_driver_error(stage, &error))?;
    let row = query_exactly_one(connection, INSTANCE_SNAPSHOT_SQL, stage)?;
    Ok(RawInstanceRow {
        instance_name: text(&row, "INSTANCE_NAME", stage)?,
        host_name: text(&row, "HOST_NAME", stage)?,
        version_full: text(&row, "VERSION_FULL", stage)?,
        edition: text(&row, "EDITION", stage)?,
        startup_time: timestamp_string(&row, "STARTUP_TIME", stage)?,
        status: text(&row, "STATUS", stage)?,
        database_status: text(&row, "DATABASE_STATUS", stage)?,
    })
}

fn query_product(
    input: &mut impl BufRead,
    output: &mut impl Write,
    connection: &oracledb::Connection,
    request: &ObservationRequest,
) -> Result<RawProductRow, PocFailure> {
    let stage = PocStage::QueryProduct;
    arm(input, output, stage)?;
    connection
        .set_call_timeout(Some(query_call_timeout(request)))
        .map_err(|error| classify_driver_error(stage, &error))?;
    let row = query_exactly_one(connection, PRODUCT_COMPONENT_SQL, stage)?;
    Ok(RawProductRow {
        product: text(&row, "PRODUCT", stage)?,
        version: text(&row, "VERSION", stage)?,
        version_full: text(&row, "VERSION_FULL", stage)?,
        release_status: text(&row, "RELEASE_STATUS", stage)?,
    })
}

fn query_exactly_one(
    connection: &oracledb::Connection,
    sql: &str,
    stage: PocStage,
) -> Result<Row, PocFailure> {
    let cursor = connection
        .query(sql, &[])
        .map_err(|error| classify_driver_error(stage, &error))?;
    exactly_one(
        cursor.map(|row| row.map_err(|error| classify_driver_error(stage, &error))),
        stage,
    )
}

fn exactly_one<T>(
    mut rows: impl Iterator<Item = Result<T, PocFailure>>,
    stage: PocStage,
) -> Result<T, PocFailure> {
    let row = rows
        .next()
        .transpose()?
        .ok_or_else(|| row_count_failure(stage))?;
    if rows.next().transpose()?.is_some() {
        return Err(row_count_failure(stage));
    }
    Ok(row)
}

fn text(row: &Row, column: &str, stage: PocStage) -> Result<String, PocFailure> {
    row.get(column)
        .map_err(|error| classify_driver_error(stage, &error))
}

fn optional_text(row: &Row, column: &str, stage: PocStage) -> Result<Option<String>, PocFailure> {
    row.get(column)
        .map_err(|error| classify_driver_error(stage, &error))
}

fn number_string(row: &Row, column: &str, stage: PocStage) -> Result<String, PocFailure> {
    row.get::<OracleNumber>(column)
        .map(|value| value.to_string())
        .map_err(|error| classify_driver_error(stage, &error))
}

fn optional_number_string(
    row: &Row,
    column: &str,
    stage: PocStage,
) -> Result<Option<String>, PocFailure> {
    row.get::<Option<OracleNumber>>(column)
        .map(|value| value.map(|number| number.to_string()))
        .map_err(|error| classify_driver_error(stage, &error))
}

fn timestamp_string(row: &Row, column: &str, stage: PocStage) -> Result<String, PocFailure> {
    row.get::<OracleTimestamp>(column)
        .map(|value| value.to_string())
        .map_err(|error| classify_driver_error(stage, &error))
}

fn query_call_timeout(request: &ObservationRequest) -> Duration {
    Duration::from_millis(
        request
            .timeouts
            .query_millis
            .saturating_sub(request.timeouts.shutdown_millis),
    )
}

fn arm(
    input: &mut impl BufRead,
    output: &mut impl Write,
    stage: PocStage,
) -> Result<(), PocFailure> {
    write_event(output, &WorkerEvent::StageReady { stage })
        .map_err(|_| protocol_failure(stage, "worker stage could not be reported"))?;
    match read_command(input).map_err(|_| protocol_failure(stage, "worker command is invalid"))? {
        ParentCommand::Arm { stage: armed } if armed == stage => Ok(()),
        _ => Err(protocol_failure(stage, "worker stage was not armed")),
    }
}

fn read_command(input: &mut impl BufRead) -> Result<ParentCommand, serde_json::Error> {
    let mut line = String::new();
    input.read_line(&mut line).map_err(serde_json::Error::io)?;
    if line.len() >= super::protocol::MAX_PROTOCOL_BYTES {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "worker command exceeds the protocol bound",
        )));
    }
    serde_json::from_str(line.trim_end())
}

fn write_event(output: &mut impl Write, event: &WorkerEvent) -> Result<(), serde_json::Error> {
    serde_json::to_writer(&mut *output, event)?;
    output.write_all(b"\n").map_err(serde_json::Error::io)?;
    output.flush().map_err(serde_json::Error::io)
}

fn classify_driver_error(stage: PocStage, error: &Error) -> PocFailure {
    match error.kind() {
        ErrorKind::DbError(database) => {
            let code = database.code();
            let category = match code {
                1017 | 28000 | 28001 => PocCategory::Authentication,
                942 | 1031 => PocCategory::Authorization,
                _ => PocCategory::Protocol,
            };
            PocFailure::new(stage, category, "Oracle operation failed").with_oracle_code(code)
        }
        ErrorKind::CallTimeoutExceeded => {
            PocFailure::new(stage, PocCategory::Timeout, "Oracle operation timed out")
        }
        ErrorKind::ListenerRefusedConnection(..)
        | ErrorKind::StreamOperation
        | ErrorKind::DeadConnection => {
            PocFailure::new(stage, PocCategory::Transport, "Oracle transport failed")
        }
        ErrorKind::InvalidOracleNumber(_)
        | ErrorKind::UnsupportedConversion(..)
        | ErrorKind::ValueWasNull
        | ErrorKind::OutOfRange(_)
        | ErrorKind::IntegerTooLarge(..)
        | ErrorKind::UnexpectedNegativeInteger => PocFailure::new(
            stage,
            PocCategory::NativeValue,
            "Oracle value conversion failed",
        ),
        ErrorKind::NoDataFound => row_count_failure(stage),
        ErrorKind::InvalidConnectString(..)
        | ErrorKind::NoConnectString
        | ErrorKind::NoCredentials
        | ErrorKind::InvalidServiceName(..) => PocFailure::new(
            PocStage::Configuration,
            PocCategory::Configuration,
            "Oracle connection configuration is invalid",
        ),
        _ => PocFailure::new(
            stage,
            PocCategory::Protocol,
            "Oracle protocol operation failed",
        ),
    }
}

fn classify_contract_error(error: kuberic_oracle::ContractError) -> PocFailure {
    let category = match error {
        kuberic_oracle::ContractError::ObservationIdentityChanged
        | kuberic_oracle::ContractError::DatabaseIdentityMismatch
        | kuberic_oracle::ContractError::DatabaseIncarnationMismatch => {
            PocCategory::IdentityChanged
        }
        _ => PocCategory::NativeValue,
    };
    PocFailure::new(
        PocStage::ParseAdmission,
        category,
        "Oracle evidence was rejected",
    )
}

fn row_count_failure(stage: PocStage) -> PocFailure {
    PocFailure::new(
        stage,
        PocCategory::RowCount,
        "fixed query returned an unexpected row count",
    )
}

fn protocol_failure(stage: PocStage, message: &'static str) -> PocFailure {
    PocFailure::new(stage, PocCategory::Protocol, message)
}

fn unix_millis() -> Result<u64, PocFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|_| {
            PocFailure::new(
                PocStage::ParseAdmission,
                PocCategory::Protocol,
                "system time is invalid",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_rejects_zero_and_multiple_rows() {
        let stage = PocStage::QueryDatabase;
        assert_eq!(
            exactly_one(Vec::<Result<u8, PocFailure>>::new().into_iter(), stage)
                .unwrap_err()
                .category(),
            PocCategory::RowCount
        );
        assert_eq!(
            exactly_one(vec![Ok(1), Ok(2)].into_iter(), stage)
                .unwrap_err()
                .category(),
            PocCategory::RowCount
        );
    }

    #[test]
    fn exactly_one_returns_the_only_row() {
        assert_eq!(
            exactly_one(
                vec![Ok::<u8, PocFailure>(7)].into_iter(),
                PocStage::QueryDatabase,
            )
            .unwrap(),
            7
        );
    }

    #[test]
    fn identity_contract_failures_are_classified_separately() {
        for error in [
            kuberic_oracle::ContractError::ObservationIdentityChanged,
            kuberic_oracle::ContractError::DatabaseIdentityMismatch,
            kuberic_oracle::ContractError::DatabaseIncarnationMismatch,
        ] {
            assert_eq!(
                classify_contract_error(error).category(),
                PocCategory::IdentityChanged
            );
        }
        assert_eq!(
            classify_contract_error(kuberic_oracle::ContractError::InvalidScn).category(),
            PocCategory::NativeValue
        );
    }
}

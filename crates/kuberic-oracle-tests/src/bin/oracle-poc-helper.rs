use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use kuberic_oracle::{
    DatabaseId, DatabaseRole, DatabaseSnapshot, DecimalScn, ForceLogging, InstanceSnapshot,
    LogMode, NativeEdition, NativeName, OpenMode, OracleObservation, ProductSnapshot,
};
use kuberic_oracle_tests::poc::{
    HelperMode, PocStage,
    protocol::{
        BUILD_ID, MAX_PROTOCOL_BYTES, PROTOCOL_VERSION, ParentCommand, WorkerEvent, WorkerRequest,
    },
};

fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct HelperError;

fn run() -> Result<(), HelperError> {
    let stdin = std::io::stdin();
    let mut input = BufReader::new(stdin.lock().take(MAX_PROTOCOL_BYTES as u64 + 1));
    let mut output = std::io::stdout().lock();
    write_event(
        &mut output,
        &WorkerEvent::Hello {
            protocol_version: PROTOCOL_VERSION,
            build_id: BUILD_ID.to_owned(),
        },
    )?;
    let mode = match read_command(&mut input)? {
        ParentCommand::Start {
            protocol_version,
            request,
        } if protocol_version == PROTOCOL_VERSION => match *request {
            WorkerRequest::Helper(mode) => mode,
            WorkerRequest::Observe(_) => return Err(HelperError),
        },
        _ => return Err(HelperError),
    };

    match mode {
        HelperMode::Success => {
            armed_stage(&mut input, &mut output, PocStage::Connect)?;
            armed_stage(&mut input, &mut output, PocStage::Close)?;
            write_event(
                &mut output,
                &WorkerEvent::Success {
                    observation: Box::new(observation()),
                },
            )
        }
        HelperMode::Sleep { stage, millis } => {
            armed_stage(&mut input, &mut output, stage)?;
            thread::sleep(Duration::from_millis(millis));
            Ok(())
        }
        HelperMode::Sequence { steps } => {
            for step in steps {
                armed_stage(&mut input, &mut output, step.stage)?;
                thread::sleep(Duration::from_millis(step.millis));
            }
            write_event(
                &mut output,
                &WorkerEvent::Success {
                    observation: Box::new(observation()),
                },
            )
        }
        HelperMode::Panic { stage } => {
            armed_stage(&mut input, &mut output, stage)?;
            panic!("test helper panic");
        }
        HelperMode::MalformedOutput => {
            output.write_all(b"not-json\n").map_err(|_| HelperError)?;
            output.flush().map_err(|_| HelperError)
        }
        HelperMode::OversizedOutput => {
            output
                .write_all(&vec![b'x'; MAX_PROTOCOL_BYTES + 1])
                .map_err(|_| HelperError)?;
            output.flush().map_err(|_| HelperError)
        }
        HelperMode::RawFailure => {
            output
                .write_all(
                    br#"{"event":"failure","failure":{"stage":"connect","category":"protocol","oracle_code":null,"message":"RAW_SECRET_SENTINEL","cleanup":null}}"#,
                )
                .map_err(|_| HelperError)?;
            output.write_all(b"\n").map_err(|_| HelperError)?;
            output.flush().map_err(|_| HelperError)
        }
        HelperMode::SuccessThenHang { millis } => {
            armed_stage(&mut input, &mut output, PocStage::Connect)?;
            armed_stage(&mut input, &mut output, PocStage::Close)?;
            write_event(
                &mut output,
                &WorkerEvent::Success {
                    observation: Box::new(observation()),
                },
            )?;
            thread::sleep(Duration::from_millis(millis));
            Ok(())
        }
        HelperMode::SuccessWithDescendant {
            millis,
            inherit_stdout,
        } => {
            armed_stage(&mut input, &mut output, PocStage::Connect)?;
            armed_stage(&mut input, &mut output, PocStage::Close)?;
            let mut descendant = Command::new("sh");
            descendant
                .arg("-c")
                .arg(format!("sleep {}", millis.div_ceil(1000)))
                .stdin(Stdio::null())
                .stderr(Stdio::null());
            if inherit_stdout {
                descendant.stdout(Stdio::inherit());
            } else {
                descendant.stdout(Stdio::null());
            }
            let _child = descendant.spawn().map_err(|_| HelperError)?;
            write_event(
                &mut output,
                &WorkerEvent::Success {
                    observation: Box::new(observation()),
                },
            )
        }
        HelperMode::SuccessThenMalformed => {
            armed_stage(&mut input, &mut output, PocStage::Connect)?;
            armed_stage(&mut input, &mut output, PocStage::Close)?;
            write_event(
                &mut output,
                &WorkerEvent::Success {
                    observation: Box::new(observation()),
                },
            )?;
            output.write_all(b"not-json\n").map_err(|_| HelperError)?;
            output.flush().map_err(|_| HelperError)
        }
        HelperMode::IgnoreTerm { stage, millis } => {
            armed_stage(&mut input, &mut output, stage)?;
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
            thread::sleep(Duration::from_millis(millis));
            Ok(())
        }
        HelperMode::SpawnDescendant { stage, millis } => {
            armed_stage(&mut input, &mut output, stage)?;
            let _child = Command::new("sh")
                .arg("-c")
                .arg(format!("sleep {}", millis.div_ceil(1000)))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| HelperError)?;
            thread::sleep(Duration::from_millis(millis));
            Ok(())
        }
        HelperMode::ExitWithDescendant { stage, millis } => {
            armed_stage(&mut input, &mut output, stage)?;
            let _child = Command::new("sh")
                .arg("-c")
                .arg(format!("sleep {}", millis.div_ceil(1000)))
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| HelperError)?;
            Ok(())
        }
    }
}

fn armed_stage(
    input: &mut impl BufRead,
    output: &mut impl Write,
    stage: PocStage,
) -> Result<(), HelperError> {
    write_event(output, &WorkerEvent::StageReady { stage })?;
    match read_command(input)? {
        ParentCommand::Arm { stage: armed } if armed == stage => Ok(()),
        _ => Err(HelperError),
    }
}

fn read_command(input: &mut impl BufRead) -> Result<ParentCommand, HelperError> {
    let mut line = String::new();
    input.read_line(&mut line).map_err(|_| HelperError)?;
    if line.len() >= MAX_PROTOCOL_BYTES {
        return Err(HelperError);
    }
    serde_json::from_str(line.trim_end()).map_err(|_| HelperError)
}

fn write_event(output: &mut impl Write, event: &WorkerEvent) -> Result<(), HelperError> {
    serde_json::to_writer(&mut *output, event).map_err(|_| HelperError)?;
    output.write_all(b"\n").map_err(|_| HelperError)?;
    output.flush().map_err(|_| HelperError)
}

fn observation() -> OracleObservation {
    OracleObservation {
        observed_at_unix_millis: 9_900,
        database: DatabaseSnapshot {
            database_id: DatabaseId::new(42).unwrap(),
            database_name: name("FREE"),
            database_unique_name: name("FREE"),
            role: DatabaseRole::Primary,
            open_mode: OpenMode::ReadWrite,
            current_scn: Some(DecimalScn::parse("123456789").unwrap()),
            resetlogs_change: DecimalScn::parse("100").unwrap(),
            activation_number: 1,
            log_mode: LogMode::ArchiveLog,
            force_logging: ForceLogging::Enabled,
            is_container_database: true,
        },
        instance: InstanceSnapshot {
            instance_name: name("FREE"),
            host_name: name("oracle-free-1"),
            version_full: "23.26.3.0.0".to_owned(),
            native_edition: NativeEdition::Free,
            startup_time: "2026-10-10T00:00:00.000000000Z".to_owned(),
            status: "OPEN".to_owned(),
            database_status: "ACTIVE".to_owned(),
        },
        product: ProductSnapshot {
            product: "Oracle AI Database 26ai Free".to_owned(),
            version: "23.26.0.0.0".to_owned(),
            version_full: "23.26.3.0.0".to_owned(),
            native_major_version: 23,
            release_status: "Develop, Learn, and Run for Free".to_owned(),
        },
    }
}

fn name(value: &str) -> NativeName {
    NativeName::new("test helper name", value).unwrap()
}

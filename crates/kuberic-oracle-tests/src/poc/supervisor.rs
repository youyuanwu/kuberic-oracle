use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        io::AsRawFd,
        process::CommandExt,
    },
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use kuberic_oracle::{ObserverTimeouts, OracleObservation};

use super::{
    config::PocAttemptConfig,
    error::{PocCategory, PocFailure, PocStage},
    protocol::{
        BUILD_ID, MAX_PROTOCOL_BYTES, PROTOCOL_VERSION, ParentCommand, WorkerEvent, WorkerRequest,
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

enum ReaderMessage {
    Event(Box<WorkerEvent>),
    ProtocolFailure,
    Eof,
}

pub fn run_worker(
    worker_path: &Path,
    request: WorkerRequest,
    timeouts: ObserverTimeouts,
    cancellation: &Cancellation,
) -> Result<OracleObservation, PocFailure> {
    run_worker_internal(worker_path, request, timeouts, cancellation, None)
}

pub fn run_worker_traced(
    worker_path: &Path,
    request: WorkerRequest,
    timeouts: ObserverTimeouts,
    cancellation: &Cancellation,
    trace_path: &Path,
) -> Result<OracleObservation, PocFailure> {
    run_worker_internal(
        worker_path,
        request,
        timeouts,
        cancellation,
        Some(trace_path),
    )
}

fn run_worker_internal(
    worker_path: &Path,
    request: WorkerRequest,
    timeouts: ObserverTimeouts,
    cancellation: &Cancellation,
    trace_path: Option<&Path>,
) -> Result<OracleObservation, PocFailure> {
    validate_supervisor_timeouts(&timeouts)?;
    validate_worker_request(worker_path, &request, &timeouts)?;
    let started = Instant::now();
    let snapshot_deadline = started + Duration::from_millis(timeouts.snapshot_millis);
    let mut child = spawn_worker(worker_path, trace_path)?;
    let process_group = child.id() as i32;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| protocol_failure(PocStage::Spawn, "worker input pipe is unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| protocol_failure(PocStage::Spawn, "worker output pipe is unavailable"))?;
    let (receiver, reader) = start_reader(stdout);

    let hello_deadline = operation_deadline(
        Instant::now(),
        timeouts.connect_millis,
        timeouts.shutdown_millis,
        snapshot_deadline,
    )?;
    let hello = match receive_until(&receiver, hello_deadline, cancellation, PocStage::Handshake) {
        Ok(message) => message,
        Err(failure) => {
            return terminate_with_failure(
                &mut child,
                process_group,
                cleanup_deadline(&timeouts, snapshot_deadline),
                failure,
                reader,
            );
        }
    };
    let valid_hello = match hello {
        ReaderMessage::Event(event) => matches!(
            *event,
            WorkerEvent::Hello {
                protocol_version: PROTOCOL_VERSION,
                ref build_id,
            } if build_id == BUILD_ID
        ),
        ReaderMessage::ProtocolFailure | ReaderMessage::Eof => false,
    };
    if !valid_hello {
        let failure = protocol_failure(PocStage::Handshake, "worker handshake is invalid");
        return terminate_with_failure(
            &mut child,
            process_group,
            cleanup_deadline(&timeouts, snapshot_deadline),
            failure,
            reader,
        );
    }

    let expected_stages = match &request {
        WorkerRequest::Observe(_) => Some([
            PocStage::SecretRead,
            PocStage::Connect,
            PocStage::QueryIdentityBefore,
            PocStage::QueryDatabase,
            PocStage::QueryInstance,
            PocStage::QueryProduct,
            PocStage::QueryIdentityAfter,
            PocStage::ParseAdmission,
            PocStage::Close,
        ]),
        WorkerRequest::Helper(_) => None,
    };
    if let Err(failure) = write_command(
        &mut stdin,
        &ParentCommand::Start {
            protocol_version: PROTOCOL_VERSION,
            request: Box::new(request),
        },
        hello_deadline,
        cancellation,
    ) {
        return terminate_with_failure(
            &mut child,
            process_group,
            cleanup_deadline(&timeouts, snapshot_deadline),
            failure,
            reader,
        );
    }

    let mut active_stage = PocStage::Handshake;
    let mut active_deadline = hello_deadline;
    let mut expected_stage_index = 0usize;
    loop {
        let message = match receive_until(&receiver, active_deadline, cancellation, active_stage) {
            Ok(message) => message,
            Err(failure) => {
                return terminate_with_failure(
                    &mut child,
                    process_group,
                    cleanup_deadline(&timeouts, snapshot_deadline),
                    failure,
                    reader,
                );
            }
        };

        match message {
            ReaderMessage::Event(event) => match *event {
                WorkerEvent::StageReady { stage } => {
                    if let Some(expected) = expected_stages {
                        if expected.get(expected_stage_index) != Some(&stage) {
                            let failure = protocol_failure(stage, "worker stage order is invalid");
                            return terminate_with_failure(
                                &mut child,
                                process_group,
                                cleanup_deadline(&timeouts, snapshot_deadline),
                                failure,
                                reader,
                            );
                        }
                        expected_stage_index += 1;
                    }
                    active_stage = stage;
                    active_deadline = match deadline_for_stage(stage, &timeouts, snapshot_deadline)
                    {
                        Ok(deadline) => deadline,
                        Err(failure) => {
                            return terminate_with_failure(
                                &mut child,
                                process_group,
                                cleanup_deadline(&timeouts, snapshot_deadline),
                                failure,
                                reader,
                            );
                        }
                    };
                    if let Err(failure) = write_command(
                        &mut stdin,
                        &ParentCommand::Arm { stage },
                        active_deadline,
                        cancellation,
                    ) {
                        return terminate_with_failure(
                            &mut child,
                            process_group,
                            cleanup_deadline(&timeouts, snapshot_deadline),
                            failure,
                            reader,
                        );
                    }
                }
                WorkerEvent::Success { observation } => {
                    if expected_stages
                        .is_some_and(|expected| expected_stage_index != expected.len())
                    {
                        let failure = protocol_failure(
                            active_stage,
                            "worker reported success before all stages completed",
                        );
                        return terminate_with_failure(
                            &mut child,
                            process_group,
                            cleanup_deadline(&timeouts, snapshot_deadline),
                            failure,
                            reader,
                        );
                    }
                    let status = match wait_for_exit(
                        &mut child,
                        active_deadline,
                        cancellation,
                        PocStage::Close,
                    ) {
                        Ok(status) => status,
                        Err(failure) => {
                            return terminate_with_failure(
                                &mut child,
                                process_group,
                                cleanup_deadline(&timeouts, snapshot_deadline),
                                failure,
                                reader,
                            );
                        }
                    };
                    if !status.success() {
                        let _ = reader.join();
                        return Err(protocol_failure(
                            PocStage::Close,
                            "worker exited unsuccessfully after success",
                        ));
                    }
                    if group_exists(process_group)? {
                        let cleanup = terminate_and_reap(
                            &mut child,
                            process_group,
                            cleanup_deadline(&timeouts, snapshot_deadline),
                        )
                        .err();
                        if cleanup.is_none() {
                            let _ = reader.join();
                        }
                        let failure = protocol_failure(
                            PocStage::Close,
                            "worker left descendants after success",
                        );
                        return Err(match cleanup {
                            Some(cleanup) => failure.with_cleanup(cleanup),
                            None => failure,
                        });
                    }
                    if reader.join().is_err() {
                        return Err(protocol_failure(
                            PocStage::Close,
                            "worker output reader failed",
                        ));
                    }
                    if receiver
                        .try_iter()
                        .any(|message| !matches!(message, ReaderMessage::Eof))
                    {
                        return Err(protocol_failure(
                            PocStage::Close,
                            "worker emitted trailing protocol output",
                        ));
                    }
                    return Ok(*observation);
                }
                WorkerEvent::Failure { failure } => {
                    let failure = failure.sanitized_from_worker();
                    let cleanup = terminate_and_reap(
                        &mut child,
                        process_group,
                        cleanup_deadline(&timeouts, snapshot_deadline),
                    )
                    .err();
                    let _ = reader.join();
                    return Err(match cleanup {
                        Some(cleanup) => failure.with_cleanup(cleanup),
                        None => failure,
                    });
                }
                WorkerEvent::Hello { .. } => {
                    let failure =
                        protocol_failure(active_stage, "worker protocol sequence is invalid");
                    return terminate_with_failure(
                        &mut child,
                        process_group,
                        cleanup_deadline(&timeouts, snapshot_deadline),
                        failure,
                        reader,
                    );
                }
            },
            ReaderMessage::ProtocolFailure => {
                let failure = protocol_failure(active_stage, "worker protocol sequence is invalid");
                return terminate_with_failure(
                    &mut child,
                    process_group,
                    cleanup_deadline(&timeouts, snapshot_deadline),
                    failure,
                    reader,
                );
            }
            ReaderMessage::Eof => {
                let failure = protocol_failure(active_stage, "worker exited without a result");
                return terminate_with_failure(
                    &mut child,
                    process_group,
                    cleanup_deadline(&timeouts, snapshot_deadline),
                    failure,
                    reader,
                );
            }
        }
    }
}

fn spawn_worker(worker_path: &Path, trace_path: Option<&Path>) -> Result<Child, PocFailure> {
    let mut command = match trace_path {
        Some(trace_path) => {
            if !trace_path.is_absolute() || trace_path.exists() {
                return Err(capability_failure("worker trace path is invalid"));
            }
            let mut command = Command::new("strace");
            command
                .args(["-f", "-qq", "-e", "trace=connect", "-o"])
                .arg(trace_path)
                .arg(worker_path);
            command
        }
        None => Command::new(worker_path),
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    command
        .spawn()
        .map_err(|_| protocol_failure(PocStage::Spawn, "worker could not be started"))
}

fn start_reader(
    stdout: impl Read + Send + 'static,
) -> (Receiver<ReaderMessage>, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let limited = stdout.take(MAX_PROTOCOL_BYTES as u64 + 1);
        let mut reader = BufReader::new(limited);
        let mut total = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(ReaderMessage::Eof);
                    return;
                }
                Ok(read) => {
                    total += read;
                    if total > MAX_PROTOCOL_BYTES {
                        let _ = sender.send(ReaderMessage::ProtocolFailure);
                        return;
                    }
                    match serde_json::from_str::<WorkerEvent>(line.trim_end()) {
                        Ok(event) => {
                            if sender.send(ReaderMessage::Event(Box::new(event))).is_err() {
                                return;
                            }
                        }
                        Err(_) => {
                            let _ = sender.send(ReaderMessage::ProtocolFailure);
                            return;
                        }
                    }
                }
                Err(_) => {
                    let _ = sender.send(ReaderMessage::ProtocolFailure);
                    return;
                }
            }
        }
    });
    (receiver, handle)
}

fn receive_until(
    receiver: &Receiver<ReaderMessage>,
    deadline: Instant,
    cancellation: &Cancellation,
    stage: PocStage,
) -> Result<ReaderMessage, PocFailure> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(PocFailure::new(
                stage,
                PocCategory::Timeout,
                "PoC stage exceeded its deadline",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(PocFailure::new(
                PocStage::Cancellation,
                PocCategory::Cancelled,
                "PoC attempt was cancelled",
            ));
        }
        let wait = (deadline - now).min(POLL_INTERVAL);
        match receiver.recv_timeout(wait) {
            Ok(message) => return Ok(message),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(ReaderMessage::Eof),
        }
    }
}

fn write_command(
    stdin: &mut ChildStdin,
    command: &ParentCommand,
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<(), PocFailure> {
    let mut encoded = serde_json::to_vec(command).map_err(|_| {
        protocol_failure(PocStage::Handshake, "worker command could not be encoded")
    })?;
    if encoded.len() >= MAX_PROTOCOL_BYTES {
        return Err(protocol_failure(
            PocStage::Handshake,
            "worker command exceeds the protocol bound",
        ));
    }
    encoded.push(b'\n');
    set_nonblocking(stdin)?;
    let mut offset = 0usize;
    while offset < encoded.len() {
        let now = Instant::now();
        if now >= deadline {
            return Err(PocFailure::new(
                PocStage::Handshake,
                PocCategory::Timeout,
                "worker command write exceeded its deadline",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(PocFailure::new(
                PocStage::Cancellation,
                PocCategory::Cancelled,
                "PoC attempt was cancelled",
            ));
        }
        match stdin.write(&encoded[offset..]) {
            Ok(0) => {
                return Err(protocol_failure(
                    PocStage::Handshake,
                    "worker input pipe closed",
                ));
            }
            Ok(written) => offset += written,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL.min(deadline - now));
            }
            Err(_) => {
                return Err(protocol_failure(
                    PocStage::Handshake,
                    "worker input pipe failed",
                ));
            }
        }
    }
    Ok(())
}

fn set_nonblocking(stdin: &ChildStdin) -> Result<(), PocFailure> {
    let descriptor = stdin.as_raw_fd();
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(protocol_failure(
            PocStage::Handshake,
            "worker input pipe could not be bounded",
        ));
    }
    Ok(())
}

fn deadline_for_stage(
    stage: PocStage,
    timeouts: &ObserverTimeouts,
    snapshot_deadline: Instant,
) -> Result<Instant, PocFailure> {
    let now = Instant::now();
    let budget = match stage {
        PocStage::Connect => timeouts.connect_millis,
        PocStage::QueryIdentityBefore
        | PocStage::QueryDatabase
        | PocStage::QueryInstance
        | PocStage::QueryProduct
        | PocStage::QueryIdentityAfter => timeouts.query_millis,
        PocStage::Close => timeouts.shutdown_millis,
        _ => snapshot_deadline
            .saturating_duration_since(now)
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    };
    operation_deadline(
        now,
        budget,
        if stage == PocStage::Close {
            timeouts.shutdown_millis / 2
        } else {
            timeouts.shutdown_millis
        },
        snapshot_deadline,
    )
}

fn operation_deadline(
    now: Instant,
    budget_millis: u64,
    reserve_millis: u64,
    snapshot_deadline: Instant,
) -> Result<Instant, PocFailure> {
    if budget_millis <= reserve_millis {
        return Err(PocFailure::new(
            PocStage::Configuration,
            PocCategory::Configuration,
            "deadline cannot contain the shutdown reserve",
        ));
    }
    let stage_deadline = now + Duration::from_millis(budget_millis - reserve_millis);
    let snapshot_operation_deadline = snapshot_deadline
        .checked_sub(Duration::from_millis(reserve_millis))
        .ok_or_else(|| {
            PocFailure::new(
                PocStage::Configuration,
                PocCategory::Configuration,
                "snapshot deadline cannot contain the shutdown reserve",
            )
        })?;
    Ok(stage_deadline.min(snapshot_operation_deadline))
}

fn cleanup_deadline(timeouts: &ObserverTimeouts, snapshot_deadline: Instant) -> Instant {
    (Instant::now() + Duration::from_millis(timeouts.shutdown_millis)).min(snapshot_deadline)
}

fn wait_for_exit(
    child: &mut Child,
    deadline: Instant,
    cancellation: &Cancellation,
    stage: PocStage,
) -> Result<ExitStatus, PocFailure> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(PocFailure::new(
                stage,
                PocCategory::Timeout,
                "worker did not exit before the deadline",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(PocFailure::new(
                PocStage::Cancellation,
                PocCategory::Cancelled,
                "PoC attempt was cancelled",
            ));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| protocol_failure(stage, "worker status could not be read"))?
        {
            return Ok(status);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn terminate_with_failure(
    child: &mut Child,
    process_group: i32,
    deadline: Instant,
    failure: PocFailure,
    reader: thread::JoinHandle<()>,
) -> Result<OracleObservation, PocFailure> {
    let cleanup = terminate_and_reap(child, process_group, deadline).err();
    if cleanup.is_none() {
        let _ = reader.join();
    }
    Err(match cleanup {
        Some(cleanup) => failure.with_cleanup(cleanup),
        None => failure,
    })
}

fn terminate_and_reap(
    child: &mut Child,
    process_group: i32,
    deadline: Instant,
) -> Result<(), PocFailure> {
    let mut child_exited = child
        .try_wait()
        .map_err(|_| shutdown_failure(PocStage::Reap, "worker status could not be read"))?
        .is_some();

    if group_exists(process_group)? {
        signal_group(process_group, libc::SIGTERM)?;
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    let kill_at = Instant::now() + remaining / 2;
    loop {
        if !child_exited {
            child_exited = child
                .try_wait()
                .map_err(|_| shutdown_failure(PocStage::Reap, "worker status could not be read"))?
                .is_some();
        }
        if child_exited && !group_exists(process_group)? {
            return Ok(());
        }
        let now = Instant::now();
        if now >= kill_at {
            break;
        }
        thread::sleep(POLL_INTERVAL.min(kill_at - now));
    }

    if group_exists(process_group)? {
        signal_group(process_group, libc::SIGKILL)?;
    }
    loop {
        if !child_exited {
            child_exited = child
                .try_wait()
                .map_err(|_| shutdown_failure(PocStage::Reap, "worker status could not be read"))?
                .is_some();
        }
        if child_exited && !group_exists(process_group)? {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(shutdown_failure(
                PocStage::Reap,
                "worker could not be reaped before the deadline",
            ));
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

fn validate_worker_request(
    worker_path: &Path,
    request: &WorkerRequest,
    timeouts: &ObserverTimeouts,
) -> Result<(), PocFailure> {
    if let WorkerRequest::Observe(request) = request {
        if request.timeouts != *timeouts {
            return Err(capability_failure(
                "worker and supervisor deadlines must match",
            ));
        }
        PocAttemptConfig {
            request: (**request).clone(),
        }
        .validate()?;
    }
    let request_size = serde_json::to_vec(request)
        .map_err(|_| capability_failure("worker request could not be encoded"))?
        .len();
    if request_size >= MAX_PROTOCOL_BYTES / 2 {
        return Err(capability_failure(
            "worker request exceeds the protocol bound",
        ));
    }

    let expected_name = match request {
        WorkerRequest::Observe(_) => "oracle-poc-worker",
        WorkerRequest::Helper(_) => "oracle-poc-helper",
    };
    let link_metadata = std::fs::symlink_metadata(worker_path)
        .map_err(|_| capability_failure("worker executable is unavailable"))?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(capability_failure("worker executable is invalid"));
    }
    let canonical = worker_path
        .canonicalize()
        .map_err(|_| capability_failure("worker executable is unavailable"))?;
    if canonical.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
        return Err(capability_failure("worker executable identity is invalid"));
    }

    let current = std::env::current_exe()
        .map_err(|_| capability_failure("current executable identity is unavailable"))?;
    let current_parent = current
        .parent()
        .ok_or_else(|| capability_failure("current executable directory is unavailable"))?;
    let expected_parent =
        if current_parent.file_name().and_then(|name| name.to_str()) == Some("deps") {
            current_parent.parent().unwrap_or(current_parent)
        } else {
            current_parent
        };
    if canonical.parent() != Some(expected_parent) {
        return Err(capability_failure("worker executable directory is invalid"));
    }

    let metadata = canonical
        .metadata()
        .map_err(|_| capability_failure("worker executable metadata is unavailable"))?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o111 == 0 {
        return Err(capability_failure("worker executable ownership is invalid"));
    }
    Ok(())
}

fn validate_supervisor_timeouts(timeouts: &ObserverTimeouts) -> Result<(), PocFailure> {
    const MAX_MILLIS: u64 = 60 * 60 * 1000;
    if timeouts.shutdown_millis < 20
        || timeouts.connect_millis <= timeouts.shutdown_millis
        || timeouts.query_millis <= timeouts.shutdown_millis
        || timeouts.snapshot_millis <= timeouts.shutdown_millis
        || timeouts.connect_millis > timeouts.snapshot_millis
        || timeouts.query_millis > timeouts.snapshot_millis
        || [
            timeouts.connect_millis,
            timeouts.query_millis,
            timeouts.snapshot_millis,
            timeouts.shutdown_millis,
        ]
        .into_iter()
        .any(|value| value > MAX_MILLIS)
    {
        return Err(PocFailure::new(
            PocStage::Configuration,
            PocCategory::Configuration,
            "supervisor deadline configuration is invalid",
        ));
    }
    Ok(())
}

fn group_exists(process_group: i32) -> Result<bool, PocFailure> {
    let result = unsafe { libc::kill(-process_group, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(shutdown_failure(
            PocStage::Reap,
            "worker process group could not be inspected",
        )),
    }
}

fn signal_group(process_group: i32, signal: i32) -> Result<(), PocFailure> {
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(shutdown_failure(
        PocStage::Termination,
        "worker process group could not be terminated",
    ))
}

fn protocol_failure(stage: PocStage, message: &'static str) -> PocFailure {
    PocFailure::new(stage, PocCategory::Protocol, message)
}

fn shutdown_failure(stage: PocStage, message: &'static str) -> PocFailure {
    PocFailure::new(stage, PocCategory::Shutdown, message)
}

fn capability_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Spawn, PocCategory::Capability, message)
}

use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

use kuberic_oracle::ObserverTimeouts;
use kuberic_oracle_tests::poc::{
    Cancellation, HelperMode, HelperStep, PocCategory, PocStage, WorkerRequest, run_worker,
};

fn helper_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_oracle-poc-helper"))
}

fn timeouts() -> ObserverTimeouts {
    ObserverTimeouts {
        connect_millis: 1_000,
        query_millis: 1_000,
        snapshot_millis: 2_000,
        shutdown_millis: 200,
    }
}

#[test]
fn accepts_success_only_after_helper_exits() {
    let observation = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Success),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap();

    assert_eq!(observation.database.database_id.value(), 42);
}

#[test]
fn times_out_and_reaps_sleeping_worker_within_bound() {
    let timeouts = ObserverTimeouts {
        connect_millis: 500,
        query_millis: 500,
        snapshot_millis: 800,
        shutdown_millis: 200,
    };
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Sleep {
            stage: PocStage::Connect,
            millis: 2_000,
        }),
        timeouts,
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.stage(), PocStage::Connect);
    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(600));
}

#[test]
fn cancellation_terminates_and_reaps_worker() {
    let cancellation = Cancellation::default();
    let trigger = cancellation.clone();
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Sleep {
            stage: PocStage::Connect,
            millis: 5_000,
        }),
        ObserverTimeouts {
            connect_millis: 2_000,
            query_millis: 2_000,
            snapshot_millis: 2_500,
            shutdown_millis: 300,
        },
        &cancellation,
    )
    .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(failure.category(), PocCategory::Cancelled);
    assert!(started.elapsed() <= Duration::from_millis(500));
}

#[test]
fn malformed_worker_output_is_protocol_failure() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::MalformedOutput),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Protocol);
}

#[test]
fn oversized_worker_output_is_protocol_failure() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::OversizedOutput),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Protocol);
}

#[test]
fn worker_failure_message_is_reconstructed_by_parent() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::RawFailure),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Protocol);
    assert!(!failure.to_string().contains("RAW_SECRET_SENTINEL"));
}

#[test]
fn escalates_when_worker_ignores_termination() {
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::IgnoreTerm {
            stage: PocStage::Connect,
            millis: 5_000,
        }),
        ObserverTimeouts {
            connect_millis: 500,
            query_millis: 500,
            snapshot_millis: 800,
            shutdown_millis: 200,
        },
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(600));
}

#[test]
fn snapshot_deadline_is_cumulative_across_stages() {
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Sequence {
            steps: vec![
                HelperStep {
                    stage: PocStage::Connect,
                    millis: 250,
                },
                HelperStep {
                    stage: PocStage::QueryDatabase,
                    millis: 400,
                },
            ],
        }),
        ObserverTimeouts {
            connect_millis: 500,
            query_millis: 500,
            snapshot_millis: 700,
            shutdown_millis: 100,
        },
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(800));
}

#[test]
fn panic_is_reported_as_protocol_failure() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Panic {
            stage: PocStage::Connect,
        }),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Protocol);
}

#[test]
fn success_is_not_committed_until_the_worker_exits() {
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::SuccessThenHang { millis: 5_000 }),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.stage(), PocStage::Close);
    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(300));
}

#[test]
fn success_with_descendant_is_rejected_and_group_is_terminated() {
    for inherit_stdout in [false, true] {
        let started = Instant::now();
        let failure = run_worker(
            helper_path(),
            WorkerRequest::Helper(HelperMode::SuccessWithDescendant {
                millis: 5_000,
                inherit_stdout,
            }),
            timeouts(),
            &Cancellation::default(),
        )
        .unwrap_err();
        assert_eq!(failure.category(), PocCategory::Protocol);
        assert!(started.elapsed() <= Duration::from_millis(300));
    }
}

#[test]
fn trailing_output_after_success_is_rejected() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::SuccessThenMalformed),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Protocol);
}

#[test]
fn cancellation_wins_after_success_envelope_but_before_clean_exit() {
    let cancellation = Cancellation::default();
    let trigger = cancellation.clone();
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        trigger.cancel();
    });
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::SuccessThenHang { millis: 5_000 }),
        timeouts(),
        &cancellation,
    )
    .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(failure.category(), PocCategory::Cancelled);
}

#[test]
fn timeout_wins_over_late_cancellation() {
    let cancellation = Cancellation::default();
    let trigger = cancellation.clone();
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(350));
        trigger.cancel();
    });
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Sleep {
            stage: PocStage::Connect,
            millis: 5_000,
        }),
        ObserverTimeouts {
            connect_millis: 500,
            query_millis: 500,
            snapshot_millis: 800,
            shutdown_millis: 200,
        },
        &cancellation,
    )
    .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(failure.category(), PocCategory::Timeout);
}

#[test]
fn descendant_holding_pipes_is_terminated_with_the_group() {
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::SpawnDescendant {
            stage: PocStage::Connect,
            millis: 5_000,
        }),
        ObserverTimeouts {
            connect_millis: 500,
            query_millis: 500,
            snapshot_millis: 800,
            shutdown_millis: 200,
        },
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(600));
}

#[test]
fn descendant_is_terminated_after_group_leader_exits() {
    let started = Instant::now();
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::ExitWithDescendant {
            stage: PocStage::Connect,
            millis: 5_000,
        }),
        ObserverTimeouts {
            connect_millis: 500,
            query_millis: 500,
            snapshot_millis: 800,
            shutdown_millis: 200,
        },
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Timeout);
    assert!(started.elapsed() <= Duration::from_millis(600));
}

#[test]
fn rejects_arbitrary_worker_before_launch() {
    let failure = run_worker(
        Path::new("/bin/echo"),
        WorkerRequest::Helper(HelperMode::Success),
        timeouts(),
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.category(), PocCategory::Capability);
    assert!(!failure.to_string().contains("/bin/echo"));
}

#[test]
fn rejects_invalid_outer_deadlines_before_launch() {
    let failure = run_worker(
        helper_path(),
        WorkerRequest::Helper(HelperMode::Success),
        ObserverTimeouts {
            connect_millis: 200,
            query_millis: 200,
            snapshot_millis: 500,
            shutdown_millis: 200,
        },
        &Cancellation::default(),
    )
    .unwrap_err();

    assert_eq!(failure.stage(), PocStage::Configuration);
}

//! R2 acceptance: the XMem worker protocol and the frame exporter, both run
//! without PyTorch or weights.
//!
//! The worker test spawns a real child process (a PowerShell fake model) and
//! speaks the JSON-lines protocol over its stdin/stdout. It proves the transport
//! and the event shapes; it does not pretend to run XMem.

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use filmcraft_media::Generator;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{
    ExportedFrames, RotoJobError, RotoJobLimits, RotoJobOutcome, RotoJobSpec, WORKER_PROTOCOL, WorkerCommand, WorkerDirection, WorkerError, WorkerEvent,
    WorkerInfo, XmemWorker, export_frames, run_job,
};

const R: FrameRate = FrameRate::FPS_24;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("frameforge-r2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary root");
    root
}

/// Handshake budget for the fixtures. A PowerShell start is normally under a
/// second, but these tests run in parallel on a small CI runner, so the bound
/// is generous: it only bounds failure, it is not an expected wait.
const READY_DEADLINE: Duration = Duration::from_secs(60);

/// Serializes fixture starts. Nine parallel PowerShell starts starved each
/// other on the CI runner (one handshake missed a 20 s deadline), so at most
/// one interpreter starts at a time; the tests themselves still run in
/// parallel once their worker is ready.
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

fn spawn_fixture(name: &str) -> XmemWorker {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    let script_text = script.to_str().expect("fixture path is UTF-8");
    XmemWorker::spawn(Path::new("powershell"), &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script_text]).expect("spawn the fixture worker")
}

/// Spawn a fixture and complete its `hello`/`ready` handshake while holding the
/// spawn lock, so a slow start is never mistaken for a protocol timeout.
fn spawn_ready(name: &str) -> (XmemWorker, WorkerInfo) {
    let guard = SPAWN_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut worker = spawn_fixture(name);
    let info = worker.handshake_timeout(READY_DEADLINE).expect("ready handshake");
    assert_eq!(info.protocol, WORKER_PROTOCOL, "the fixture speaks this build's protocol");
    assert!(!info.backend.is_empty(), "a ready worker names its backend");
    drop(guard);
    (worker, info)
}

fn spec(first_frame: i64, last_frame: i64) -> RotoJobSpec {
    RotoJobSpec {
        frames_dir: "frames".to_string(),
        first_frame,
        last_frame,
        initial_mask: "mask.png".to_string(),
        output_dir: "masks".to_string(),
        width: 32,
        height: 18,
        model: "XMem.pth".to_string(),
        model_sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        direction: WorkerDirection::Forward,
    }
}

#[test]
fn fake_worker_speaks_the_json_lines_protocol() {
    let (mut worker, ready) = spawn_ready("fake_xmem_worker.ps1");
    assert_eq!(ready.protocol, WORKER_PROTOCOL);
    assert_eq!(ready.backend, "fake-cpu");

    worker
        .send(&WorkerCommand::Propagate {
            frames_dir: "frames".to_string(),
            first_frame: 0,
            last_frame: 2,
            initial_mask: "mask.png".to_string(),
            output_dir: "masks".to_string(),
            width: 32,
            height: 18,
            model: "XMem.pth".to_string(),
            model_sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            direction: WorkerDirection::Forward,
        })
        .expect("send propagate");

    let mut progress = Vec::new();
    loop {
        match worker.recv().expect("worker event") {
            WorkerEvent::Progress { frame, total, .. } => {
                assert_eq!(total, 3, "the fake worker covers the inclusive range");
                progress.push(frame);
            }
            WorkerEvent::Done { frames, .. } => {
                assert_eq!(frames, 3);
                break;
            }
            other => panic!("unexpected worker event: {other:?}"),
        }
    }
    assert_eq!(progress, vec![0, 1, 2], "progress arrives in media order");

    worker.send(&WorkerCommand::Cancel).expect("send cancel");
    match worker.recv().expect("cancelled event") {
        WorkerEvent::Cancelled => {}
        other => panic!("cancel must be acknowledged, got {other:?}"),
    }
    worker.request_shutdown().expect("send shutdown");
}

#[test]
fn a_quiet_worker_reports_a_timeout_instead_of_blocking() {
    let (mut worker, _) = spawn_ready("fake_xmem_worker.ps1");

    let started = Instant::now();
    let error = worker.recv_timeout(Duration::from_millis(250)).expect_err("a quiet worker has no event to give");
    assert!(matches!(error, WorkerError::Timeout(_)), "{error}");
    assert!(started.elapsed() < Duration::from_secs(20), "the deadline is honoured");
    worker.kill().expect("kill the worker");
}

#[test]
fn a_silent_worker_times_out_during_the_handshake() {
    // The silent fixture is spawned under the same lock (its start is the only
    // costly part); the handshake then waits on the deliberately short deadline.
    let guard = SPAWN_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut worker = spawn_fixture("silent_xmem_worker.ps1");
    let started = Instant::now();
    let error = worker.handshake_timeout(Duration::from_millis(400)).expect_err("silence is a timeout, not a ready");
    drop(guard);
    assert!(matches!(error, WorkerError::Timeout(_)), "{error}");
    assert!(started.elapsed() < Duration::from_secs(20), "the handshake deadline is honoured");
    worker.kill().expect("kill the silent worker");
}

#[test]
fn roto_job_drives_a_fake_worker_to_done() {
    let (mut worker, _) = spawn_ready("fake_xmem_worker.ps1");
    let cancel = AtomicBool::new(false);
    let mut seen: Vec<i64> = Vec::new();

    let outcome = run_job(&mut worker, &spec(0, 2), &RotoJobLimits::default(), &cancel, |frame, total, _fps| {
        assert_eq!(total, 3, "the worker reports the requested total");
        seen.push(frame);
    })
    .expect("the fake worker finishes");
    match outcome {
        RotoJobOutcome::Done(report) => {
            assert_eq!(report.frames, 3);
            assert_eq!(report.progress_events, 3);
            assert!(!report.killed, "a clean pass is not a kill");
        }
        other => panic!("expected done, got {other:?}"),
    }
    assert_eq!(seen, vec![0, 1, 2], "progress arrives in media order");
}

#[test]
fn roto_job_refuses_a_bad_request_before_sending_anything() {
    let (mut worker, _) = spawn_ready("fake_xmem_worker.ps1");
    let cancel = AtomicBool::new(false);

    let inverted = run_job(&mut worker, &spec(5, 4), &RotoJobLimits::default(), &cancel, |_, _, _| {}).expect_err("an inverted range is not a job");
    assert!(matches!(inverted, RotoJobError::Refused(_)), "{inverted}");

    let tiny_limit = RotoJobLimits { max_frames: 2, ..RotoJobLimits::default() };
    let too_many = run_job(&mut worker, &spec(0, 9), &tiny_limit, &cancel, |_, _, _| {}).expect_err("more than two frames is refused");
    assert!(matches!(too_many, RotoJobError::Refused(_)), "{too_many}");

    let mut bad_hash = spec(0, 1);
    bad_hash.model_sha256 = "not-a-hash".to_string();
    let refused = run_job(&mut worker, &bad_hash, &RotoJobLimits::default(), &cancel, |_, _, _| {}).expect_err("a bad model hash is refused");
    assert!(matches!(refused, RotoJobError::Refused(_)), "{refused}");

    // Nothing was sent, so the same worker still completes a valid job.
    let outcome = run_job(&mut worker, &spec(0, 1), &RotoJobLimits::default(), &cancel, |_, _, _| {}).expect("the worker was left untouched");
    assert!(matches!(outcome, RotoJobOutcome::Done(ref report) if report.frames == 2), "{outcome:?}");
}

#[test]
fn roto_job_kills_a_stalled_worker_instead_of_waiting() {
    let (mut worker, _) = spawn_ready("stalling_xmem_worker.ps1");
    let cancel = AtomicBool::new(false);
    let limits = RotoJobLimits { stall: Duration::from_millis(500), overall: Duration::from_secs(30), ..RotoJobLimits::default() };

    let started = Instant::now();
    let error = run_job(&mut worker, &spec(0, 0), &limits, &cancel, |_, _, _| {}).expect_err("silence after one event is a stall");
    assert!(matches!(error, RotoJobError::Stalled(_)), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10), "the stall deadline is honoured");
}

#[test]
fn roto_job_cancel_kills_a_worker_that_never_acknowledges() {
    let (mut worker, _) = spawn_ready("stalling_xmem_worker.ps1");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let setter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });

    let limits =
        RotoJobLimits { stall: Duration::from_secs(30), overall: Duration::from_secs(30), cancel_ack: Duration::from_millis(300), ..RotoJobLimits::default() };
    let started = Instant::now();
    let outcome = run_job(&mut worker, &spec(0, 0), &limits, &cancel, |_, _, _| {}).expect("a cancel is an outcome, not an error");
    let _ = setter.join();
    match outcome {
        RotoJobOutcome::Cancelled(report) => assert!(report.killed, "a worker that ignores cancel is killed, and the report says so"),
        other => panic!("expected a cancel, got {other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(10), "cancel does not wait for the stall deadline");
}

#[test]
fn frame_exporter_writes_scaled_pngs_in_media_order() {
    let root = temp_root("exporter");
    let source = GeneratorSource::new(Generator::ColorMatte { color: [0.0, 1.0, 0.0, 1.0] }, 64, 36, R, Tick(TICKS_PER_SECOND));
    let range = TimeRange::new(Tick::ZERO, R.tick_of(3));
    let exported: ExportedFrames = export_frames(&source, range, R, &root, (32, 18)).expect("export frames");

    assert_eq!((exported.first_frame, exported.last_frame), (0, 2));
    assert_eq!((exported.width, exported.height), (32, 18));
    assert_eq!(exported.count, 3);
    for frame in 0..3 {
        let path = root.join(format!("frame_{frame:08}.png"));
        let image = image::open(&path).expect("exported PNG").to_rgba8();
        assert_eq!(image.dimensions(), (32, 18), "frame {frame} is scaled to the working size");
        assert_eq!(image.get_pixel(0, 0).0, [0, 255, 0, 255], "frame {frame} keeps the source colour");
    }
}

#[test]
fn frame_exporter_rejects_an_empty_range() {
    let root = temp_root("empty-range");
    let source = GeneratorSource::new(Generator::ColorMatte { color: [0.0, 1.0, 0.0, 1.0] }, 64, 36, R, Tick(TICKS_PER_SECOND));
    let error = export_frames(&source, TimeRange::new(Tick::ZERO, Tick::ZERO), R, &root, (32, 18)).expect_err("an empty range is not a frame export");
    assert!(error.to_string().contains("empty"), "{error}");
}

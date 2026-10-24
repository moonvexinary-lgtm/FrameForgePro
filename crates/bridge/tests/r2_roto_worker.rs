//! R2 acceptance: the XMem worker protocol and the frame exporter, both run
//! without PyTorch or weights.
//!
//! The worker test spawns a real child process (a PowerShell fake model) and
//! speaks the JSON-lines protocol over its stdin/stdout. It proves the transport
//! and the event shapes; it does not pretend to run XMem.

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{ExportedFrames, WORKER_PROTOCOL, WorkerCommand, WorkerDirection, WorkerEvent, XmemWorker, export_frames};

const R: FrameRate = FrameRate::FPS_24;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("frameforge-r2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary root");
    root
}

#[test]
fn fake_worker_speaks_the_json_lines_protocol() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("fake_xmem_worker.ps1");
    let script_text = script.to_str().expect("fixture path is UTF-8");
    let mut worker =
        XmemWorker::spawn(Path::new("powershell"), &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script_text]).expect("spawn fake worker");

    let ready = worker.handshake().expect("ready handshake");
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

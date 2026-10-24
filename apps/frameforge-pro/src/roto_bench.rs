//! `frameforge-pro roto-bench [frames]` — measures the Rust-side R2 pipeline.
//!
//! It times decode → working-size scale → PNG write on a synthetic Bars & Tone
//! source. XMem inference is **not** measured: the optional runtime and weights
//! are absent unless the user installed them with consent, and this command never
//! invents a number for them.

use std::process::ExitCode;
use std::time::Instant;

use filmcraft_media::Generator;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_time::{FrameRate, Tick, TimeRange};
use frameforge_bridge::export_frames;

const RATE: FrameRate = FrameRate::FPS_25;
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

pub fn run(args: &[String]) -> ExitCode {
    let frames = args.first().and_then(|value| value.parse::<i64>().ok()).unwrap_or(120).clamp(1, 600);
    let duration = RATE.tick_of(frames);
    let output = std::env::temp_dir().join(format!("frameforge-roto-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    let source = GeneratorSource::new(Generator::BarsAndTone, WIDTH, HEIGHT, RATE, duration);
    let range = TimeRange::new(Tick::ZERO, duration);

    let started = Instant::now();
    let result = export_frames(&source, range, RATE, &output, (WIDTH, HEIGHT));
    let elapsed = started.elapsed();
    let _ = std::fs::remove_dir_all(&output);

    match result {
        Ok(exported) => {
            let seconds = elapsed.as_secs_f64();
            let fps = if seconds > 0.0 { exported.count as f64 / seconds } else { 0.0 };
            println!("FrameForge Pro roto-bench");
            println!("source: synthetic Bars & Tone {WIDTH}x{HEIGHT} @ {:.3} fps", RATE.as_f64());
            println!("frame export: {} frames in {seconds:.3} s ({fps:.2} frames/s)", exported.count);
            println!("XMem CPU inference: not measured (optional pack and consented weights are not installed)");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("roto-bench failed: {error}");
            ExitCode::FAILURE
        }
    }
}

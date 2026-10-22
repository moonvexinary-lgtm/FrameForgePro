//! FrameForge Pro — the unified desktop application.
//!
//! One native window over one document: the FilmCraft editing project and the
//! EffectCraft compositing compositions of a `.ffproj`, with the workspaces
//! the milestones add one by one (see `docs/ARCHITECTURE.md`).
//!
//! Command line:
//!
//! * `frameforge-pro` — the application window;
//! * `frameforge-pro catalog` — the machine-readable effects registry (JSON);
//! * `frameforge-pro selftest` — the foundation self-test (CI runs this).

mod app;
mod audio;
mod color;
mod edit;
mod effects;
mod export;
mod mixer;
mod pixels;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("catalog") => print_catalog(),
        Some("selftest") => self_test(),
        _ => run_gui(),
    }
}

/// The window, taskbar and app-switcher icon: the project's own ICO, decoded
/// at startup (Explorer's executable icon is embedded by `build.rs`).
fn app_icon() -> egui::IconData {
    let ico: &[u8] = include_bytes!("../../../assets/frameforgepro-icon.ico");
    let Ok(decoded) = image::load_from_memory_with_format(ico, image::ImageFormat::Ico) else {
        return egui::IconData::default();
    };
    let rgba = decoded.to_rgba8();
    egui::IconData { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
}

fn run_gui() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FrameForge Pro")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 620.0])
            .with_icon(app_icon()),
        ..eframe::NativeOptions::default()
    };
    match eframe::run_native("FrameForge Pro", options, Box::new(|_cc| Ok(Box::new(app::App::new())))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("FrameForge Pro could not open a window: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_catalog() -> ExitCode {
    match serde_json::to_string_pretty(&frameforge_bridge::catalog_report()) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("catalog serialization failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn self_test() -> ExitCode {
    println!("FrameForge Pro {}", env!("CARGO_PKG_VERSION"));
    println!("engines: filmcraft (editing) + effectcraft (compositing)");
    println!("shared time base: {} ticks per second", frameforge_core::TICKS_PER_SECOND);

    match frameforge_core::verify_foundation() {
        Ok(()) => {
            println!("foundation: OK — exact time unifies the editing and compositing engines");
            ExitCode::SUCCESS
        }
        Err(failures) => {
            for f in failures {
                eprintln!("foundation FAILURE: {f}");
            }
            ExitCode::FAILURE
        }
    }
}

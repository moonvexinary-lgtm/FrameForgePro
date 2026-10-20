//! FrameForge Pro.
//!
//! Milestone M3 (see `docs/ARCHITECTURE.md`): the application binary is still
//! its own self-test, and gains its first real tool command:
//!
//! * `frameforge-pro` — runs the foundation self-test (real checks, exit code
//!   reflects them).
//! * `frameforge-pro catalog` — prints the machine-readable effects registry:
//!   both engines' catalogs (id, name, category, parameters, GPU/32-bit
//!   support) and the cross-engine duplicate audit, as JSON.
//!
//! The desktop workspaces (Edit, Effects, Color, Audio, Export) are built on
//! this foundation in the following milestones; nothing in this binary is a
//! placeholder.

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "catalog" => print_catalog(),
        _ => self_test(),
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

//! FrameForge Pro.
//!
//! Milestone M1 (see `docs/ARCHITECTURE.md`): the application binary is its own
//! self-test. It verifies the shared foundation the two integrated engines stand on —
//! the exact time base the whole editor is built over — and reports the result with a
//! real exit code. The desktop workspaces (Edit, Effects, Color, Audio, Export) are
//! built on this foundation in the following milestones; nothing in this binary is a
//! placeholder.

use std::process::ExitCode;

fn main() -> ExitCode {
    println!("FrameForge Pro {}", env!("CARGO_PKG_VERSION"));
    println!("engines: filmcraft (editing) + effectcraft (compositing)");
    println!("shared time base: {} ticks per second", frameforge_core::TICKS_PER_SECOND);

    match frameforge_core::verify_foundation() {
        Ok(()) => {
            println!("foundation: OK — exact time unifies the editing and compositing engines");
            ExitCode::SUCCESS
        }
        Err(failures) => {
            for f in &failures {
                eprintln!("foundation FAILURE: {f}");
            }
            ExitCode::FAILURE
        }
    }
}

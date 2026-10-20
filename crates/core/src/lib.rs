//! Shared foundation of FrameForge Pro.
//!
//! FrameForge Pro is one application built on two engines:
//!
//! * **FilmCraft** (`filmcraft-*`): the nonlinear editing engine — project model, bins,
//!   multitrack timeline, media decoding, audio mixing, colour management, export.
//! * **EffectCraft** (`effectcraft-*`): the compositing and motion-graphics engine —
//!   layers, keyframes, masks, the effects catalog, CPU/GPU compositing.
//!
//! The engines are integrated over a shared foundation, not glued at the UI. That
//! foundation starts here, with the one thing every frame, clip, keyframe and audio
//! sample passes through: **time**.
//!
//! Both engines already measure time identically: an integer [`Tick`] at
//! [`TICKS_PER_SECOND`] = 254 016 000 000 per second — the least common multiple of
//! every broadcast frame rate and every common audio sample rate — so edits, keyframes
//! and audio never drift. This crate re-exports the editing engine's time as the
//! application's canonical time and converts to the compositing engine's identical type
//! at zero cost.
//!
//! As the integration grows, the rest of the shared surface (video frames, colour
//! spaces, media references, transforms, blend modes, keyframes, masks, render requests)
//! is defined and cross-checked here the same way: one representation, tested against
//! both engines.
//!
//! Never-crash rules (inherited from both upstreams): every function is total — no
//! `unwrap`, `expect`, `panic!`, `todo!` or `unsafe`; checked/saturating arithmetic for
//! anything input-derived.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub use effectcraft_time as comp_time;
pub use filmcraft_time as edit_time;

/// The application's canonical time types, from the editing engine.
pub use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};

/// Converts a time from the editing engine to the compositing engine.
///
/// The two `Tick` types are the same integer representation (both engines are built from
/// a shared design at 254 016 000 000 ticks per second), so this is a field move — no
/// arithmetic, no rounding, no drift.
#[inline]
pub const fn to_comp(t: edit_time::Tick) -> comp_time::Tick {
    comp_time::Tick(t.0)
}

/// Converts a time from the compositing engine back to the editing engine.
#[inline]
pub const fn to_edit(t: comp_time::Tick) -> edit_time::Tick {
    edit_time::Tick(t.0)
}

/// Verifies every property the unified application depends on, returning a
/// human-readable list of what failed. `Ok(())` means the shared foundation holds.
///
/// The test suite and the `frameforge-pro` binary both call this, so the shipped
/// executable re-checks its own foundation. Every check below is a real property of the
/// two engines this application integrates; nothing here is decorative.
pub fn verify_foundation() -> Result<(), Vec<&'static str>> {
    let mut failures = Vec::new();

    // 1. One time base. If this fails, no timeline math can be shared at all.
    if edit_time::TICKS_PER_SECOND != comp_time::TICKS_PER_SECOND {
        failures.push("engine time bases differ: filmcraft-time and effectcraft-time must agree on TICKS_PER_SECOND");
    }

    // 2. Tick values survive the crossing in both directions, unchanged.
    for v in [0, 1, -1, 17, 123_456, TICKS_PER_SECOND, -TICKS_PER_SECOND, i64::MAX / 4, i64::MIN / 4] {
        let edit = edit_time::Tick(v);
        let comp = to_comp(edit);
        if comp.0 != v || to_edit(comp).0 != v || edit.seconds() != comp.seconds() {
            failures.push("Tick conversion between the engines is not lossless");
            break;
        }
    }

    // 3. Audio-sample and container-timestamp conversions agree exactly:
    //    48 samples of 48 kHz audio is exactly one millisecond in ticks, and a 90 kHz
    //    container timestamp of 3 600 units is exactly 40 ms in both engines.
    let samples_edit = edit_time::Tick::from_units(48, 48_000);
    let samples_comp = comp_time::Tick::from_units(48, 48_000);
    if samples_edit.0 != samples_comp.0 || samples_edit.0 != TICKS_PER_SECOND / 1_000 {
        failures.push("audio sample timing differs between the engines");
    }
    let pts_edit = edit_time::Tick::from_rational(3_600, 1, 90_000);
    let pts_comp = comp_time::Tick::from_rational(3_600, 1, 90_000);
    if pts_edit.0 != pts_comp.0 || pts_edit.to_units_floor(48_000) != pts_comp.to_units_floor(48_000) {
        failures.push("container timestamp conversion differs between the engines");
    }

    // 4. Frame rates agree on every broadcast rate the application can be set to:
    //    frame duration, frame indexing and snapping must be identical in both engines.
    for rate in edit_time::FrameRate::COMMON {
        let comp_rate = comp_time::FrameRate { num: rate.num, den: rate.den };
        if rate.frame_duration().0 != comp_rate.frame_duration().0 {
            failures.push("frame durations differ between the engines");
            break;
        }
        let t = edit_time::Tick::from_seconds_f64(3.7);
        if rate.frame_at(t) != comp_rate.frame_at(to_comp(t)) {
            failures.push("frame indexing differs between the engines");
            break;
        }
        let snapped = rate.snap_nearest(edit_time::Tick(TICKS_PER_SECOND / 7));
        if snapped.0 != comp_rate.snap_nearest(to_comp(snapped)).0 {
            failures.push("frame snapping differs between the engines");
            break;
        }
    }

    if failures.is_empty() { Ok(()) } else { Err(failures) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_bases_are_identical() {
        assert_eq!(edit_time::TICKS_PER_SECOND, comp_time::TICKS_PER_SECOND);
        assert_eq!(edit_time::TICKS_PER_SECOND, 254_016_000_000);
    }

    #[test]
    fn foundation_holds() {
        let r = verify_foundation();
        assert!(r.is_ok(), "the shared foundation must hold: {r:?}");
    }
}

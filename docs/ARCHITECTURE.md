# FrameForge Pro — Architecture

Last updated: milestone M9 (2026-10-22) · Branch: `frameforge-rust-rewrite`

Planning documents from the M9 audit: [`BASELINE_AUDIT.md`](BASELINE_AUDIT.md) (measured
state and defects), [`ARCHITECTURE_DECISION.md`](ARCHITECTURE_DECISION.md) (Rust-first vs
FFmpeg/MLT/GStreamer; XMem out-of-process), [`FEATURE_PARITY.md`](FEATURE_PARITY.md)
(honest status and priorities), [`ROTO_AI_PLAN.md`](ROTO_AI_PLAN.md) (mandatory XMem
staging).

## Goal

One native Windows application that unifies:

1. nonlinear editing (the FilmCraft engine),
2. compositing, effects and motion graphics (the EffectCraft engine),
3. the FrameForge Pro identity and professional editing workflows.

Premiere Pro is a *behavioural reference for workflows only*. No Adobe code, assets, or
artwork. The legacy C++/Qt application is not part of this architecture; it remains
recoverable on `frameforge-dev` and the `frameforge-cpp-legacy` tag.

## Layered plan

| Layer | Responsibility | Status |
|---|---|---|
| L6 apps | `frameforge-pro` binary; workspaces: Edit / Effects / Color / Audio / Export | M5–M9: real workspaces, session edits, autosave |
| L5 ui-egui | unified shell: workspaces, timeline, monitors, panels (egui — same UI stack as both upstreams) | M5–M8: working; docking/layout persistence still planned |
| L4 engine | unified session: project, commands, undo; timeline items reference compositions | M6: FilmCraft `Session` is the editing authority |
| L3 render/export | FilmCraft track compositor + EffectCraft layer compositor behind adapters; one export pipeline | M2/M4: verified; export formats decode-verified for H.264 |
| L2 edit/effects | edit algebra; effect-bridge: EffectCraft effects on FilmCraft timeline clips, shared keyframes | M2: verified (307 effects swept) |
| L1 media | FilmCraft codecs/media (already shared by both engines upstream) | upstream, pinned |
| L0 foundation | time, geometry, colour | M1: time (verified) |

## M1 — what exists now

- **`crates/core` (`frameforge-core`)**: the shared foundation. The canonical time is
  FilmCraft's `Tick` / `FrameRate` / `TimeRange`; conversions to EffectCraft's identical
  types are zero-cost field moves. `verify_foundation()` cross-checks every shared
  property (time base, lossless conversion, audio-sample and container-timestamp math,
  frame duration/indexing/snapping on all 11 broadcast rates) against both engines; the
  test suite and the shipped binary both run it.
- **`apps/frameforge-pro`**: the application binary. At M1 it is the application's own
  self-test — real checks with a real exit code, not a placeholder.
- **CI** (`.github/workflows/ci.yml`, `windows-2025`): rustfmt check, clippy
  `-D warnings`, `cargo check`, `cargo test`, release build, binary self-test,
  disk-space reports before and after. **All compilation is cloud-only.**

## M2 — the integration layer (✔ CI-verified, run 39371821743)

- **`crates/bridge` (`frameforge-bridge`)**: the layer where the engines meet.
  - [`ComposedSource`] is a `MediaSource` whose frames pass through a chain of
    **real EffectCraft effects** (`ec.blur.gaussian` in the proof) before the
    editing engine's renderer reads them — so preview and export share one
    picture and there is no second renderer.
  - Parameters are animated with the **editing engine's keyframe system**
    (`filmcraft_project::Param`, media time, exact ticks); evaluation is a pure
    function of time, and the acceptance test proves rendering backward
    reproduces identical frames.
  - [`FrameForgeProject`] (`.ffproj` v1): the FilmCraft editing project plus a
    compositing `Composition` per media item; atomic, round-trip-checked JSON.
  - Acceptance (`crates/bridge/tests/m2_poc.rs`, run by CI): registry ≥ 300
    effects → build timeline (synthetic Bars-and-Tone video + audio clips) →
    keyframed Gaussian Blur → monotonic softening → direction-independent
    frames → blur-0 identity → save/reopen byte-identical frames → real H.264 +
    AAC export → reopened and verified (resolution, frame rate, duration, tone
    present, blur measurable in the decoded frames).

## M3 — file media, project import, the effects registry (✔ CI-verified, run 39379740235)

- **File media** (`ProjectSources`): `MediaRef::File` items open through the
  editing engine's codec registry (`filmcraft-codecs` openers) — MP4/MOV/MKV/
  WAV/stills — and wrap in `ComposedSource` like generators do. A missing or
  unreadable file is the **offline slate**, never a failure: the project opens,
  renders, and heals when the file returns.
- **`.fcproj` import** (`FrameForgeProject::import_fcproj`): a FilmCraft
  project file is adopted as the editing half of a `.ffproj` (compositions
  empty). `.ecproj` import (comps → compositions) is M4, with the nested-
  composition model.
- **Effects registry** (`catalog`): both engines' catalogs in one
  machine-readable shape (engine, id, name, category, parameters with
  defaults, GPU/32-bit badges), plus `name_collisions()` — the cross-engine
  duplicate audit. `frameforge-pro catalog` prints it as JSON.
- **Per-effect sweep** (test `m3_effects_registry_audit`, CI): every
  compositing-engine effect runs through the real composed-source path on a
  real frame and is classified (runs / bridge-unsupported); none may panic.
  Counts are reported by the test run, not asserted from documentation.

## M4 — compositions with real layers (✔ CI-verified, run 39542160028)

- **The deep integration**: a [`Composition`] can hold a full compositing-engine
  project whose comp *is* the clip's picture on the timeline. Its source
  footage layer reads this clip's media through the effect chain, served over
  the compositing engine's own `FootageSource` seam — text, shapes, solids,
  masks, blend modes and nested comps render over real footage with the
  compositing engine's layer compositor, at the exact same ticks.
- `.ffproj` schema 2: the compositing project serializes inside the document;
  schema-1 files load unchanged (new fields default).
- **Tone generators** reconstruct (audio-only media items play).
- **Relink**: `FrameForgeProject::relink` repoints file media; the offline slate
  heals the moment the file is back.
- **`.ecproj` import**: a compositing project's first comp lands on the
  timeline as an alpha-carrying media item (`import_ecproj`).
- Verified in CI: keyframed text animates over served footage (mass-centroid
  travels between its keys), direction-independent frames, byte-identical
  save/reopen (the compositing project serializes inside `.ffproj`), the tone
  generator is audible, relink heals a missing file, and an imported `.ecproj`
  paints its comp on the timeline.
- Honest scope: the comp's audio mixing passes the clip's audio through
  unchanged (comp Audio Levels are future work); footage items of an imported
  `.ecproj` other than the source layer stay offline; the moving-text
  investigation is recorded in the test suite (`m4_text_visibility_matrix`) —
  two wrong test-side assumptions (centre-position semantics, white-on-white
  measurement), never an engine fault.

## M5 — the application shell (✔ CI-verified, run 39561711515)

One native window (egui/wgpu, the stack both engines ship with), the user's
own icon in the window and the executable, and real workspaces over the
verified engine — no fake controls:

- **Edit** — the project panel (import: Bars & Tone, Tone, files through the
  codecs), double-click places a real clip on the timeline, the program
  monitor renders through the same compositor export uses, the timeline draws
  real tracks/clips with a scrubbable playhead, clip selection, Delete removes
  a clip, wall-clock playback (silent — the audio clock arrives with the
  engine session, M6).
- **Effects** — the selected clip's composition: the real effect chain with
  enabled toggles, a searchable catalog of every compositing-engine effect,
  and live parameter editing (sliders, checkboxes, popups, colors) writing the
  same document the renderer reads; the comp's layer list, read-only until the
  compositing workspace (M6).
- **Export** — only formats with built-in encoders (H.264, ProRes, DNxHR,
  Motion-JPEG, WAV); a real render on a worker thread with the pipeline's own
  progress bar; the report shows the written file.
- Color and Audio appear as clearly-marked planned entries, never fake panels.
- Trimming, drag edits, undo and keyframing UIs are honestly deferred to M6
  (the engine session), and the UI says so where it matters.

## M6 — the editing engine session (✔ CI-verified, run 39703930594)

The shell no longer edits the project model directly: the FilmCraft `Session`
owns the editing project, and every project-changing action runs through
[`Session::execute`] — the same command registry, validation and undo history
the upstream editor uses. The `.ffproj` document is assembled from the
session's project and the bridge's compositions on save, and disassembled on
open; undo restores items under the same ids, so compositions follow.

- **Commands wired**: `file.newSequence`, `file.newBarsAndTone`,
  `file.newColorMatte`, `file.import`, `timeline.place` (V1/A1 with the
  engine's validation), `edit.clear` (named-clip delete), `sequence.addEditAllTracks`
  (razor at the playhead), `edit.undo`, `edit.redo` — plus a `Session::edit`
  closure for the Tone generator (no command yet) and media removal.
- **Undo/redo**: toolbar buttons with real enable state from the history,
  Ctrl+Z / Ctrl+Shift+Z.
- **The audio clock**: a cpal output stream (48 kHz stereo f32 requested) fed
  by a worker thread that renders the timeline mix through the engine's mixer
  graph in 100 ms chunks; the playhead is exactly the audio the device
  consumed (`Tick::from_units`). No usable device → honest wall-clock fallback
  with the reason in the status bar.

## M7 — editing depth (✔ CI-verified, run 39717619124)

- **Drag-move and trim on the timeline**: dragging a clip body moves it and
  dragging its 8-px edges trims it — both through the engine's own
  `timeline.move` / `timeline.trim` commands (validated, clamped, linked
  audio along, one undo step per gesture). The document only changes on
  release; during the drag a translucent ghost previews the result.
- **The keyframe editor**: every scalar effect parameter carries a stopwatch
  (◆+) that keys it at the playhead's offset into the clip — media time, the
  same keyframe system the timeline uses, so keys follow trims and moves.
  Keys list with their values and delete individually or in one clear; the
  slider shows the parameter evaluated at the playhead while animated.
- **The panic hook** (upstream's rule): a fault in the frame is caught,
  the session survives, the status bar says to save.
- **Downloadable builds**: every green push uploads
  `frameforge-pro.exe` (with the icon embedded, 24 MB) as a 7-day CI
  artifact — the first runnable build you can download from the Actions run
  page without installing anything locally.

## M8 — Color, Audio, comp layers, releases (✔ CI-verified, run 39729926210)

- **The Color workspace**: the program monitor beside the selected clip's
  FilmCraft effect stack — apply from the engine's video-effect catalog
  (`effects.apply`), toggle (`effects.toggleEnabled`), remove
  (`effects.remove`), and set parameters (`effects.setParam`): sliders,
  checkboxes and popups, all undoable engine commands. This is the classic
  grade panel: Lumetri Color, ASC CDL, Brightness & Contrast…
- **The Audio workspace**: the sequence's mixer — a strip per audio track
  plus the Mix, with fader, pan, mute and solo, all through
  `mixer.setStrip` (undoable). The audio clock's mixer graph reads exactly
  this state, so a moved fader is heard immediately.
- **Comp layer editing**: visibility toggles, removal, and new text/solid
  layers on the clip's compositing comp — through bridge `Composition`
  methods over the same EffectCraft project the M4 acceptance test renders.
- **Releases**: a tag push (`v*`) builds and publishes a GitHub Release with
  a ZIP (exe + licenses + README) — `softprops/action-gh-release`.

## M9 — durability and honest planning (✔ CI-verified, run 39878432877)

- **Crash-recovery autosaves**: while a saved project is dirty, the shell writes an
  atomic `<project>.ffproj.autosave` sidecar at most every 20 s (same schema, same
  round-trip check, same atomic write as a normal save). Opening a project with a newer
  autosave offers recovery; if the project file itself is unreadable, the autosave is
  loaded directly and the error is reported. A successful save clears the sidecar.
- **Unsaved-changes guard**: New/Open confirm before discarding work; Ctrl+S and
  Ctrl+Shift+S save/save-as; the status bar shows the last autosave.
- **Keyframe-preserving parameter writes**: editing an animated scalar now adds or
  updates a key at the playhead (the engine's `effects.setParam` semantics) instead of
  replacing the `Param` and destroying the animation. All parameter writes now
  invalidate the program-monitor cache, so a changed effect is actually shown.
- **Selection repair**: removing an effect before the selected one keeps the selection
  pointing at the same effect.
- **Measured audit and plans**: `BASELINE_AUDIT.md`, `ARCHITECTURE_DECISION.md`,
  `FEATURE_PARITY.md`, `ROTO_AI_PLAN.md` — including the mandatory XMem design, its
  verified MIT code licence, the 237.5 MB official weight download, and the rule that
  nothing is claimed until a real worker test runs.
- **Tests**: `crates/bridge/tests/m9_durability.rs` — autosave round-trip, newer-than
  detection, clear, corrupt sidecar error, keyframe-preserving writes, and refusal of a
  newer project schema. The first three passed in run 39878432877.

## Milestones

| # | Milestone | Status |
|---|---|---|
| M1 | shared time foundation + cloud CI | ✔ (CI-verified, run 39371821743) |
| M2 | integration proof: timeline clip + real EffectCraft effect + keyframed parameter; save/reopen; render; export; verified output (synthetic media) | ✔ (CI-verified, run 39371821743) |
| M3 | file media through the codecs, offline slates, `.fcproj` import, machine-readable two-engine effects registry + duplicate audit + per-effect sweep | ✔ (CI-verified, run 39379740235) |
| M4 | compositions with real layers inside the timeline (compositing-engine comps whose footage layer is backed by the editing engine's media); tone generators; relink + healing; `.ecproj` import | ✔ (CI-verified, run 39542160028) |
| M5 | the application shell: an egui window with the user's icon, real workspaces (Edit: project panel, timeline, program monitor; Effects: composition editor with live parameters; Export: real renders with progress), `.ffproj` open/save, media import and clip placement | ✔ (CI-verified, run 39561711515) |
| M6 | the editing engine session: every edit an undoable engine command, the audio clock, razor, undo/redo in the UI | ✔ (CI-verified, run 39703930594) |
| M7 | editing depth: drag-move and trim through engine commands with a ghost preview, the keyframe editor (stopwatch + keys in media time), the panic hook, downloadable Windows dev builds from CI | ✔ (CI-verified, run 39717619124) |
| M8 | Color workspace (FilmCraft's effect stack on the clip: apply/toggle/remove/setParam), Audio workspace (the mixer: faders, pan, mute, solo through mixer.setStrip), comp layer editing, tagged GitHub Releases with a ZIP | ✔ (CI-verified, run 39729926210) |
| M9 | durability: crash-recovery autosaves, unsaved-changes guard, keyframe-preserving parameter writes, preview invalidation, measured baseline audit + architecture decision + feature parity + XMem plan | ✔ (CI-verified, run 39878432877) |

## Dependency policy

- Upstream engines are consumed as **pinned git dependencies** (exact revisions in the
  root `Cargo.toml`), not copied source. FilmCraft is pinned to the *same revision the
  EffectCraft workspace builds against*, so both engines share one copy of each
  `filmcraft-*` crate in the graph (no duplicate media/time/colour engines).
- Licenses: MIT OR Apache-2.0 / BSD / ISC / Zlib / CC0 / Unicode / BSL only — the same
  allowlist both upstreams enforce. No GPL code, ever, in this branch.
- The workspace builds self-contained on GitHub Actions; no absolute local paths, no
  sibling-checkout requirements.

## Constraints honored

- Cloud-only builds; the local machine carries only git and a minimal Rust toolchain
  (formatting only; ~760 MB).
- Disk floor: ≥ 50 GB free on C: monitored before every local step.
- Honest status: a milestone is ✔ only when GitHub Actions proves it.

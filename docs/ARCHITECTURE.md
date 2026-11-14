# FrameForge Pro — Architecture

Last updated: U1 (2026-10-28) · Branch: `frameforge-rust-rewrite`

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
| L6 apps | `frameforge-pro` binary; workspaces: Edit / Effects / Color / Audio / Compositing / Text & Graphics / Masking / Export | M5–M9: real workspaces, session edits, autosave; U1: themed shell and the new screens |
| L5 ui-egui | unified shell: workspaces, timeline, monitors, panels (egui — same UI stack as both upstreams) | M5–M8: working; docking/layout persistence still planned |
| L4 engine | unified session: project, commands, undo; timeline items reference compositions | M6: FilmCraft `Session` is the editing authority; M11: composition edits share its undo order; R2: worker protocol + frame exporter |
| L3 render/export | FilmCraft track compositor + EffectCraft layer compositor behind adapters; one export pipeline | M2/M4: verified; H.264 is decode-verified; M12: overwrite confirmation + real cancel; R1: cached roto alpha in preview/export |
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
  - [`FrameForgeProject`] (`.ffproj`, schema 3): the FilmCraft editing project plus a
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
  schema-1 files load unchanged (new fields default). R1 later adds schema 3 for
  the optional roto mask reference.
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
  `file.newColorMatte`, `file.import`, `timeline.place` (source-patched
  destinations with the engine's validation), `edit.clear` (named-clip delete), `sequence.addEditAllTracks`
  (razor at the playhead), and the `Session::edit` closure for the Tone
  generator (no command yet) and media removal. Undo/redo call `Session::undo` /
  `Session::redo` directly so the shell can pair composition snapshots with the
  engine's steps (M11) instead of using the `edit.undo` / `edit.redo` commands.
- **Undo/redo**: toolbar buttons with real enable state from the combined
  history, Ctrl+Z / Ctrl+Shift+Z.
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

## M10 — source-patched multi-track editing (✔ CI-verified, run 39887395556)

- **Track headers**: the timeline now has a fixed header column per track with
  two real controls. **T** toggles the engine's targeted tracks (navigation,
  paste, mute/solo), and **S** toggles the source patch (`video_dest` /
  `audio_dest`) through `timeline.setTargeting`. Both are editor state, not the
  document — neither is undoable nor marks the project dirty, matching the
  engine.
- **Placement follows the patch**: double-clicking media plans its destination
  from `Session::targeting()`, appends to the patched video/audio tracks and
  calls `timeline.place` with those exact track ids. A linked picture+sound
  clip shares one start; audio-only media names the sound track as `track`;
  an unpatched destination is an actionable status error instead of a silent
  fallback to V1/A1.
- **Tests**: `apps/frameforge-pro/src/edit.rs` — patched V2/A2 placement
  end-to-end through the real engine, the audio-only parameter shape, and the
  unpatched-video error.

## M11 — one chronological undo history (✔ CI-verified, run 40037973347)

Timeline edits are whole-project snapshots owned by FilmCraft's `Session`.
Composition/effect edits live beside that project, so they previously bypassed
the history: Ctrl+Z after an effect change undid an unrelated timeline edit.
M11 gives both kinds of edit one order.

- **`apps/frameforge-pro/src/history.rs` (`EditHistory`)**: a slot per engine
  history step. `None` is a project step the engine already stores; `Some` is
  the before/after snapshot of the affected composition. A composition edit
  adds a project-neutral `Session::edit` step (`"Edit composition"`) so the
  snapshot has a place in the same stack; undo/redo pops one engine step and one
  snapshot together.
- **Commands are tracked, not guessed**: `App::execute` records the identity of
  the newest engine snapshot (`undo_top`) before and after each command, so
  selection/targeting and merged fader moves add no slot. Custom edits
  (`add_tone`, media removal) go through `App::edit_project`, which does the
  same.
- **Gesture coalescing**: pointer presses bump a gesture id; every change in one
  slider/colour drag updates the same composition step's `after` state. A new
  click or a keyboard change after release is a separate step. A new edit after
  undo clears both the engine redo and the composition redo.
- **Limit alignment**: when the engine trims its oldest step at its limit, the
  matching slot is dropped; redo is mirrored against the engine's actual length,
  because the engine's redo can legitimately push past the limit.
- **Undo/redo application**: the engine restores the project; the shell applies
  the composition's `before`/`after`, invalidates the preview sources, clears
  stale selections, and repairs `selected_effect` when it no longer exists.
- **Failure safety**: if the neutral engine step cannot be recorded, the
  composition is rolled back to `before` and the status bar reports the engine
  error, so the document and history cannot disagree.
- **Tests**: seven tests in `history.rs` — composition undo leaves the previous
  timeline edit alone (and redo restores both), one drag is one step, a new edit
  clears redo, non-editing and merged commands add no slot, the history limit
  stays aligned, and a closed gesture does not swallow a later change. An
  App-level test (`app::tests::ctrl_z_after_a_composition_edit_leaves_the_timeline_edit_alone`)
  drives `App::undo`/`redo` and asserts both the project name and the
  composition map.
- **Limitations**: the egui gesture bracket in `effects.rs` is code-inspected,
  not UI-automated; history is session-only (not saved in `.ffproj`); snapshots
  clone only the affected composition, so undo memory grows with composition
  size, not project size.

## M12 — export safety and the close-window guard (✔ CI-verified, run 40045638522)

M11's export workspace could overwrite an existing file without asking and could
not stop a render; closing the window also bypassed the M9 unsaved-changes guard.

- **Overwrite confirmation**: `export.rs::output_path` resolves the exact final
  name — trimmed, with the selected extension added once, and an empty path
  rejected — before anything is written. `start` asks Yes/No when that file
  exists; No keeps the existing file and starts nothing.
- **Real cancel**: the running panel's Cancel button stores FilmCraft's
  `Progress.cancel` atomic, which the exporter checks between frames/batches.
  `ExportError::Cancelled` is mapped to a dedicated sentinel; because FilmCraft
  writes directly to the target, the shell removes the truncated partial file
  and says so. Once requested, the button is replaced by "Cancelling…".
- **Close guard**: `App::ui` reads `viewport().close_requested()` and sends
  `ViewportCommand::CancelClose` unless a dirty document is saved or explicitly
  discarded. The native dialog offers Save / Discard / Cancel (Yes/No/Cancel);
  a cancelled Save As or a failed write keeps the window open. Clean projects
  close silently.
- **Tests**: `export.rs::tests::output_path_adds_the_selected_extension_once`,
  `export.rs::tests::a_cancelled_export_is_reported_as_a_cancellation`, and
  `app::tests::close_guard_blocks_only_a_dirty_project_when_discard_is_denied`.
  At M12 the suite was 28 tests; R1 and R2 bring it to 36 (run `40059782293`), T1 adds one
  (37 in run `40063686209`), T2 adds one (38 in run `40128137965`), T3 adds two (40 in run
  `40133164791`), T4 adds one, T5 adds four, T6 adds one, T7 adds two, T8 adds two, T9 adds one, T10 adds one, T11 adds one and T12 adds one, so the suite was 54 at T12 (run `40260600879`); the media-bin, preview, UI-interaction and export slices bring it to 79 (E2, run `42395055258`).
- **Limitations**: native dialogs and the viewport command are code-inspected,
  not UI-automated. Presets and range selection arrived in E1 and the queue in
  E2, so a second deliverable now waits behind the first instead of being
  blocked; cancel still deletes the partial output rather than resuming.

## T1 — real track visibility, mute, solo and lock (✔ CI-verified, run 40063686209)

The M10 headers exposed target/source-patch only, so the timeline could not hide a
video track, mute or solo audio, or lock a track without leaving the Edit workspace.

- **Headers**: each video track has **T** (target), **S** (source patch), **V**
  (visibility via `enabled`) and **L** (lock); each audio track has **T**, **S**,
  **M** (mute), the solo circle (`solo`) and **L**. T/S remain editor state through
  `timeline.setTargeting` (not undoable, not dirtying). V/M/solo/L are real
  `timeline.setTrack` document edits run through `App::execute`, so they join the
  combined undo history and invalidate the program monitor.
- **Test**: `edit::tests::track_header_visibility_mute_solo_and_lock_are_undoable_set_track_edits`
  builds the command from the same `track_action_command` helper the UI uses,
  asserts the dynamic JSON key and the engine state, then undoes the lock and
  confirms the earlier settings remain. The renderer's use of `enabled`, `muted`
  and `solo` is upstream-tested.
- **Limitations**: no scripted UI click or hidden/muted/solo render-difference
  test yet; the header column is fixed at 140 px; sync-lock toggles are not exposed.

## T2 — sequence markers on the ruler (✔ CI-verified, run 40128137965)

The engine had marker commands and the demo project ships markers, but the Edit
workspace drew none and could not add, clear or navigate them.

- **Ruler markers**: every `Sequence::markers` entry is drawn in the ruler as a
  label-coloured stem and head; clicking within a few pixels jumps the playhead to
  it and names it in the status bar. Markers are the engine's real, persisted,
  `Label`-coloured sequence markers (the demo project's four markers show up).
- **Commands**: **Add Marker (M)** runs `markers.add` at the frame-snapped
  playhead through `App::execute` (undoable, dirtying); **Clear Marker** runs
  `markers.clearCurrent` after putting the engine's own playhead in step;
  **Previous / Next Marker** are pure editor navigation over the sorted marker
  list. The **M** and **Shift+M** keys are Edit-workspace-only and stand down while
  a text field has focus.
- **Test**: `edit::tests::marker_helpers_add_clear_and_navigate_through_the_real_engine`
  drives `add_marker`, `go_marker` and `clear_marker` on a fresh project, asserts
  the frame-snapped placement, navigation order and clearing, then undoes the
  clear and checks the marker returns.
- **Also fixed**: `Player::new` no longer opens the audio device at construction.
  The device is opened on the first real `play`, so app construction (and the app
  tests) never touch WASAPI; this removed an intermittent
  `STATUS_ACCESS_VIOLATION` in the test binary.
- **Limitations at T2**: no marker rename/comment editor or marker drag on the ruler;
  marker colours were display-only. T10/T13 add the name/comment/length/colour editor
  and T12 adds CSV export; marker drag and CSV/EDL import remain absent.

## T3 — snapping moves and trims (✔ CI-verified, run 40133164791)

- **Toggle**: a **Snap** checkbox in the timeline toolbar writes the engine's own
  `sequence.snap` editor state (not an undoable document edit, matching the
  engine).
- **Targets**: zero, the playhead and every clip start/end except the clip being
  dragged. A moved clip snaps by whichever of its start or end is closer to a
  target; a trimmed edge snaps to the target and the delta is adjusted to land
  there. The radius is 8 screen pixels at the current zoom, so snapping stays
  visually constant as the timeline zooms.
- **Test**: `edit::tests::snap_tick_uses_the_nearest_target_inside_the_tolerance`
  pins the radius and boundary behaviour, and
  `edit::tests::snap_targets_include_the_playhead_and_skip_the_dragged_clip`
  checks the target set against the real demo sequence.
- **Limitations**: markers are not snap targets yet; only the regular trim mode
  snaps; snapping is not linked-pair aware beyond the single dragged clip.

## T4 — insert / overwrite at the playhead (✔ CI-verified, run 40138769007)

Double-clicking bin media still appends to the patched tracks; the new path is
the professional source edit that appends cannot express.

- **Selection**: clicking a project-panel media item selects it (highlighted) as
  the Insert/Overwrite source. Removing that media clears the selection.
- **Commands**: **Insert** and **Overwrite** in the timeline header place the
  selected item at the playhead on the patched V/A destinations through
  `timeline.place` with the engine's `insert` flag. Insert ripples existing clips
  right; overwrite trims or removes whatever is under the new clip. Both go
  through `App::execute`, so they are one undo step each.
- **Test**: `edit::tests::insert_and_overwrite_at_the_playhead_use_the_real_engine_insert_flag`
  builds a two-clip sequence, drives `place_at_playhead` through a real `App`,
  checks that insert shifted the first clip and that overwrite trimmed it, with
  an undo between.
- **Limitations (resolved by T5)**: T4 itself placed the whole media item because
  the shell had no source monitor or source marks. T5 adds both, and the Source
  panel now places exactly the marked range.

## T5 — Source monitor, source In/Out and marker ripple (CI-verified, run 40156687200)

The engine already owns a real Source monitor (`source.open`/`source.inspect`/
`source.setPlayhead`, media-item `mark_in`/`mark_out`, `source.insert` /
`source.overwrite` with three-point editing and marker ripple). T5 wires it in:
the editor no longer has to guess a duration for Insert/Overwrite.

- **Panel**: the Edit workspace's monitor area is split into Program and Source
  with a draggable gutter. The Source panel renders the open item through the
  engine's own `filmcraft_render::render_item` path (same decode/colour path the
  renderer uses), with source timecode, frame-step, a seek slider, Play/Pause and
  In/Out controls. Playback is a **wall clock**: T5 left it video-only (the audio
  clock was bound to the Program sequence); T14 below gives the Source monitor its
  own sound through the same shared stream.
- **Independence**: source seeking and marking act on `state.source_playhead` and
  never move the Program monitor's playhead; the two previews are separate
  textures. `I`/`O` set the source marks, `Shift+I`/`Shift+O` go to them, and
  `,`/`.` are the engine's Insert/Overwrite shortcuts (Edit workspace only, and
  they stand down while a text field has focus).
- **Marks**: marks live on the media item (`mark_in`/`mark_out`), so they persist
  through save/reopen with the project file (no schema change). The effective
  range clamps one-sided marks to the source edges, collapses reversed marks to an
  empty selection, and the panel says "No In/Out marks - using full source" when
  neither is set.
- **Placement**: the Source panel's **Insert (,)** / **Overwrite (.)** use
  `source.insert` / `source.overwrite`, so they place exactly the marked range and
  honor the engine's three-point edit. The T4 bin buttons still place the whole
  item; double-click still appends.
- **Marker ripple**: a **Ripple Markers** checkbox drives the engine's
  `markers.rippleSequenceMarkers` state. FrameForge defaults it **on**
  (Premiere-like). On insert, markers at or after the insert point move by the
  inserted range; earlier markers stay. Overwrite does not ripple.
- **Tests**: `source_open_previews_real_frames_and_its_marks_stay_off_the_program_playhead`,
  `source_insert_places_the_marked_range_and_ripples_markers_with_undo` (with
  before/at/after markers, undo, and ripple-off), `source_mark_edges_are_normalized_and_one_sided_marks_use_the_source_edges`
  and `source_marks_survive_project_save_and_reopen`.
- **Limitations**: source playback was silent in T5 (T14 adds its own sound);
  there is no source-marker panel or source range drag; the Program monitor still
  owns In/Out for the sequence (sequence in/out UI is still open).

## T14 - the Source monitor's own sound (CI-verified, run 42429090124)

T5's Source transport was a video-only wall clock, so auditioning a clip was
silent. The Program and Source monitors now share one output stream and one
clock design.

- **Item audio** (`audio.rs`): `item_audio(sources, item, start, frames)` renders
  the open item's own decoded sound, folded to interleaved stereo — silence when
  the item has no source or the decode fails, so auditioning never edits the
  project and never panics on a missing or unsupported file. `item_has_audio` is
  the transport's pure decision. A source with more than two channels is
  monitored through its first pair; the Program monitor keeps the full downmix.
- **One stream, two mixes**: `Player` gains `play_item` over a shared
  `start(mix)`, so the sequence mix (`play`) and the item mix (`play_item`) use
  the same device, queue, resampler, device-frame clock and silent fallback.
  `render_into` extracts the worker's chunk/resample/channel loop.
- **Clock and exclusivity** (`edit.rs`): `source_toggle_play` hands the audio to
  the player and follows the audio clock (`source_clock_start + Player::position`);
  otherwise it keeps the wall clock and names why it is silent ("this source has
  no audio", the device error). `source_tick` uses the shared `advance_source`
  boundary rule for both clocks. Both monitors share one stream, so starting one
  stops the other, and a seek re-anchors the audio clock.
- **Tests** (six): a real Tone source renders non-silent stereo of the requested
  length; a missing source plays silence; `item_has_audio` follows the media info
  (Tone audible, Black Video not); `render_into` streams the whole range in
  100 ms chunks and resamples 48 kHz stereo to 24 kHz mono; and
  `advance_source` stops on the last frame for both clocks. No test opens an
  audio device.
- **Limitations**: the cpal output path and the Program/Source exclusivity are
  code-inspected, not automated (a test would open WASAPI); scrubbing while the
  source plays with sound restarts the worker from the new position; the
  source-marker panel and source range drag are still open.
- **CI**: run `42429090124` — fmt, clippy `-D warnings`, check, **95 tests**,
  release build and the binary self-test.

## T15 - the Source range bar and Source markers (CI-verified, run 42499750102)

T5's Source range was two marks set only from the I/O keys, and the panel's
transport had a plain seek slider. The panel now shows the marked span as an
editable bar and the source's markers as rows.

- **Geometry** (`edit.rs`): `selected_range(start, end, mark_in, mark_out, fd)`
  is the one "Out is inclusive, clamp to the source" rule shared by the
  transport, the bar and the drag; `source_range_rects` maps that range onto the
  bar and returns the two handle rects plus the body between them, so the draw
  loop, the conversion and the tests share one geometry.
- **Gestures**: clicking the track (or the band) seeks; dragging a handle sets
  that mark; dragging the band slides both. `drag_source_range` asks the
  engine's `source_monitor::adjust_range` for the clamped, frame-snapped range,
  then writes it with `App::edit_project_merged("Mark", "source-range-drag", …)`
  so every frame of one drag joins the same undo step. Moving a mark clears the
  matching split points, exactly as `project.setMarks` does. A subclip's range
  is its definition, so its handles are neither drawn nor draggable.
- **Markers**: a `+ Marker` chip calls the engine's `markers.add` with
  `target: "source"`; the strip below lists the open source's markers, seeks on
  a row click and seeks then `clearCurrent` on the row's ×. Every command is the
  engine's Source-targeted one, so the list cannot drift from the document and
  the delete is undoable. `M`/`Shift+M` now follow the open monitor: source
  markers while a source is open, program markers otherwise.
- **Tests** (five): the handles land on 2 s/6 s of a 10 s bar; `selected_range`
  with no marks, an inclusive Out, a past-the-end Out and an inverted pair; and
  three headless gestures — an In-handle drag moving the mark one second with
  one Ctrl+Z restoring it, a body drag moving both marks one second, and the
  marker strip adding, seeking and deleting with the delete undone.
- **Limitations**: the Source panel has no marker drag or import; a second drag
  with no other edit between reuses the engine's merge key, so two such drags
  share an undo step; and the `M`/`Shift+M` Source routing is code-inspected (the
  program-marker key test runs with no source open).
- **CI**: run `42499750102` — fmt, clippy `-D warnings`, check, **100 tests**,
  release build and the binary self-test.

## T16 - the sequence In/Out ruler drag (CI-verified, run 42506032070)

T9 could set the Program marks only from the I/O keys, and the ruler painted
neither mark. The timeline now shows the marked span and lets a person drag
either end.

- **Geometry** (`edit.rs`): `sequence_mark_rects` maps the sequence marks onto
  the ruler with the ruler's own linear time-to-pixels mapping, so the handles,
  the playhead and `timeline_clip_rect` share one geometry. Each handle is
  `SEQUENCE_MARK_HANDLE_W` wide and the ruler tall, which is the rect the UI
  tests grab.
- **Paint**: a faint accent band spans the ruler between In and Out; the two
  handles are painted after the marker stems, so a marker at the same frame
  cannot hide one. In uses the accent colour and Out amber, distinct from the
  red playhead.
- **Gestures**: dragging a handle asks `drag_sequence_mark` for the frame-snapped
  mark; In is clamped at Out and Out at In (a drag stops rather than clearing
  the other end, unlike `markers.markIn`/`markOut`), and both are clamped to the
  sequence. Moving a mark clears its split points exactly as those commands do
  (code-inspected; the UI tests assert the marks, the clamp and the undo). The
  write is `App::edit_project_merged("Mark", "sequence-range-drag", ...)`,
  so every frame of one drag joins the same undo step.
- **Tests** (three `ui_tests`): the handles appear on 2 s and 6 s only once the
  marks exist; dragging In by one second moves In and leaves Out, and one
  Ctrl+Z restores it; dragging Out by one second mirrors that; and an Out
  dragged five seconds left stops on In instead of clearing it.
- **Limitations**: split (video/audio) In/Out are still key-only; the split-point
  clearing the drag performs has no UI test; the ruler band is painted but not
  draggable, so scrubbing inside the marked range keeps working; and a second
  handle drag with no other edit between reuses the engine's merge key, so two
  such drags share an undo step.
- **CI**: run `42506032070` - fmt, clippy `-D warnings`, check, **103 tests**,
  release build and the binary self-test.

## T17 - split video/audio In/Out (CI-verified, run 42527534314)

T16 moved the ordinary Program marks, but the engine's video-only and
audio-only split points had no UI. Both now have one.

- **Chips** (`edit.rs`): the timeline's second header row carries four painted
  chips (V In/V Out/A In/A Out). `program_split_mark` snaps the playhead and
  runs `markers.markSplitVideoIn`/`...VideoOut`/`...AudioIn`/`...AudioOut`, so
  each is one undo step. `source_chip` is renamed `chip` and reused.
- **Geometry**: `sequence_split_rects` puts video points in the ruler's top
  half and audio points in the bottom half, so points at the same frame stay
  separately grabbable; it shares the ruler's linear time-to-pixels mapping.
- **Paint**: the tabs are painted last, in channel colours (`CLIP_VIDEO_HI`,
  `CLIP_AUDIO_HI`), over the Program handles.
- **Gestures**: `drag_split_mark` frame-snaps a tab and clamps it at the
  channel's *effective* opposite - its split counterpart, or the ordinary
  Program mark it inherits - instead of clearing it the way the engine's
  `Side::set` would. The write is one merged project edit ("Mark Split",
  "sequence-split-drag"), so a whole drag is one undo step.
- **Tests** (three `ui_tests`): the chips set a video-only In and an audio-only
  Out and undo the latter; the video In handle drags one second and leaves the
  other three points alone, with one-step undo; and the audio Out dragged five
  seconds left stops on the audio In.
- **Limitations**: the engine's `sequence.lift`/`extract` and the export
  `in_out_range` ignore split marks, so they are display/navigation only today;
  source-side split marks (the engine's `target: "source"`) still have no UI;
  and at a shared frame a split tab wins the grab over the Program handle
  beneath it.
- **CI**: run `42527534314` - fmt, clippy `-D warnings`, check, **106 tests**,
  release build and the binary self-test.

## T18 - source marker drag and marker CSV import (CI-verified, run 42537235054)

Row 39's two remaining UI gaps, closed without an engine change.

- **Source marker drag** (`edit.rs`): the Source range bar paints each marker
  as a warm stem with a small tab in its top band. `source_marker_rects` maps
  marker starts with the bar's own time-to-pixels rule, and `drag_source_marker`
  snaps the new start to the source frame grid, clamps it to the source bounds
  and writes one merged project edit ("Move Source Marker",
  "source-marker-drag"), so a whole drag is one Ctrl+Z. The edit targets the
  view's root `media` item, so a subclip's inherited markers move on the master
  clip.
- **CSV import** (`edit.rs`): `csv_records` is an RFC 4180 state machine
  (quoted commas, doubled quotes, embedded line breaks, dropped blank lines);
  `parse_markers_csv` reads the export's own header, prefers `start_ticks` and
  falls back to `start_timecode` at the sequence rate, and rejects bad numbers
  and labels with the spreadsheet row number. `import_markers_csv_text`
  validates every row first, then one "Import Markers" project edit adds them
  all, so a bad row imports nothing. The Markers menu gains "Import Markers
  CSV...".
- **Tests**: `edit::tests::marker_csv_import_parses_quoted_text_and_is_one_undo`
  (quoted text, ticks/timecode precedence, one-step undo, row-numbered error)
  and
  `ui_tests::dragging_the_source_marker_handle_moves_only_that_marker_and_one_undo_restores_it`
  (handle geometry, an isolated one-second drag, one-step undo).
- **Limitations**: Program (timeline) markers still do not drag; EDL import and
  marker-list selection remain; imported markers are always `Comment` kind.
- **CI**: run `42537235054` - fmt, clippy `-D warnings`, check, **108 tests**,
  release build and the binary self-test.

## T30 - snapping for caption cue gestures (CI-verified, run 43544459466)

T29 moved and trimmed cues freely, so a cue dragged to "about" a beat missed
it by a few frames while the clip gestures landed exactly on markers.

- **Targets**: a caption gesture snaps to the same family the clip gestures
  use - zero, the App playhead, every sequence marker and every other cue's
  start and end. `caption_snap_targets` builds that list and leaves the
  dragged cue out, so a cue can never snap to its own edges and a move never
  collapses onto itself.
- **Move**: `snap_caption_move` tries the candidate start and the candidate end
  against the targets, takes whichever actually snapped through `choose_snap`
  (a definitely-unchanged edge loses), and returns the start the engine should
  move to. The release converts it back to a delta for `captions.move`, so the
  undo step is still one engine edit.
- **Trim**: `snap_caption_trim` pushes the dragged edge to the nearest target
  inside the 8 px radius and adjusts the delta so `captions.trim` lands the
  edge there. The other edge is pinned by the engine's trim, not by the snap.
- **Radius**: the same `SNAP_PX = 8.0` and `snap_tolerance(zoom)` as the clips,
  so at 40 px/s the radius is 0.2 s; snapping honours the editor's snapping
  toggle and does nothing when it is off.
- **Status**: the release reads "Moved caption (snapped)" or "Trimmed caption
  (snapped)" only when the snap changed the delta, matching the clip wording.
- **Tests**:
  `ui_tests::a_caption_move_and_trim_snap_to_a_marker_and_one_undo_restores_each`
  marks 2.5 s and 8.5 s, drags the 1 s to 4 s cue 1.45 s later so the raw start
  is 2.45 s and the cue lands 2.5 s to 5.5 s, then drags the 6 s to 8 s cue's
  out edge 0.45 s later so the raw end is 8.45 s and the tail lands on 8.5 s,
  with one Ctrl+Z restoring each gesture. The unit test
  `caption_snap_targets_include_markers_the_playhead_and_other_cue_edges`
  proves the target list keeps markers, the playhead and another cue's edges
  while dropping the dragged cue's own two. The demo sequence's 23.976 rate
  puts a cue asked for at 5 s on 5.005 s, so the test reads the cues' snapped
  edges rather than raw seconds; the first green attempt (run `43541963090`)
  failed only that assumption and `9939c81` corrected it.
- **Limitations**: caption gestures snap to sequence markers, the playhead and
  other cues only - not to clip edges, sequence In/Out or the grid - and a
  multi-cue selection still cannot be dragged as a group.
- **Red evidence**: red commit `f19baef` added the UI test against unsnapped
  gestures and failed exactly that path: **144 passed; 1 failed**, with fmt,
  clippy `-D warnings`, check and the release build all green.
- **CI**: run 43544459466 - fmt, clippy `-D warnings`, check, **146 tests**, release
  build and the binary self-test.

## T29 - caption cue drag and trim on the timeline (CI-verified, run 43537808604)

T28 painted the cues but they were only click targets: a cue out of place had
to be retyped in the Caption window, and one that ran too long could only be
shortened through numeric Start / Length fields.

- **Gestures**: the timeline body owns presses on a caption row. A press on a
  cue's body starts a move; a press within 8 px of either end starts a trim of
  that end. The pressed cue becomes the engine's caption selection through the
  same journal-free `captions.select`, so the bar lights up while it drags.
  The gesture is read from the first waypoint past egui's drag threshold,
  exactly like a clip drag, and a ghost bar previews the geometry release will
  commit.
- **Commit**: release runs `captions.move` with the one cue and the drag's tick
  delta, or `captions.trim` with the cue, the edge and that delta, through
  `App::execute`, so each gesture is one undo step and dirties the project
  once. The engine clamps a move at zero and at its neighbours, and a trim at
  the engine's minimum cue duration; a zero-delta drag is the engine's
  `Nothing`, so it changes nothing. An open Caption window refreshes from the
  new times, and the status line reads "Moved caption" or "Trimmed caption".
- **Cue widgets**: the painted bar still registers its rectangle under
  `("ff-timeline-caption", id)` for tests and hover, but it now senses only
  hover: the body's press starts the gesture, so a cue drag and a clip drag
  read the same way.
- **Test**:
  `ui_tests::dragging_a_caption_cue_moves_it_and_its_out_edge_trims_it_each_as_one_undo`
  adds a 1 s to 4 s cue on a `Dialogue` track, drags the body 40 px right at
  40 px/s and gets 2 s to 5 s with "Moved caption" and one Ctrl+Z back to
  1 s to 4 s, then drags the out edge 40 px left from inside the 8 px band and
  gets 1 s to 3 s with "Trimmed caption" and one Ctrl+Z back to the full cue.
- **Limitations**: an in-edge trim moves the start without a ripple, and a
  multi-cue selection cannot be dragged as a group from the timeline. Snapping
  arrived with T30.
- **Red evidence**: red commit `22f363d` added the UI test against cue bars
  that only clicked and failed exactly that path: **143 passed; 1 failed**.
- **CI**: run 43537808604 - fmt, clippy `-D warnings`, check, **144 tests**, release
  build and the binary self-test.

## T28 - caption tracks on the timeline (CI-verified, run 43495417982)

T26 and T27 edited captions in a floating window, but the timeline painted
only video and audio rows: a cue was invisible where it actually plays.

- **Rows**: every caption track paints as a row under the audio rows, in the
  sequence's own track order (`C1` is the topmost track, because
  `captions.newTrack` inserts at the top). The header names the engine's slot
  and the track's name, dimmed when the track's output (`enabled`) is off; the
  cues stay editable either way. Caption rows are appended after V/A rows, so
  every existing row index and clip geometry is unchanged.
- **Cues**: each caption paints as a bar at its own start and duration through
  the same `timeline_clip_rect` the clips use, filled in the accent's teal,
  stroked in the accent when it is the engine's caption selection, and
  labelled with its first line.
- **Click**: a cue is its own click target (`ff-timeline-caption-<id>`); the
  click runs `captions.select` on the session - selection is editor state, so
  it adds no undo step and never dirties the project - refreshes an open
  Caption window and reports "Selected caption".
- **Tests**:
  `ui_tests::a_caption_cue_paints_on_its_own_timeline_row_and_a_click_selects_it`
  adds a `Dialogue` track and a 1 s to 4 s cue through the engine, finds the
  cue's bar on the row after V2/A2, checks its left edge, width and top
  against `timeline_clip_rect`, clears the engine's caption selection, clicks
  the bar and gets the cue back as the only selection with the status line.
- **Limitations**: cue drag and trim arrived with T29; there is no per-track
  show/hide control (the engine only has `captions.showAll` /
  `captions.hideAll`), and the bar has no speaker or cue-id badge.
- **Red evidence**: red commit `b612df0` added the UI test against the
  unpainted timeline and failed exactly that path: **142 passed; 1 failed**.
- **CI**: run 43495417982 - fmt, clippy `-D warnings`, check, **143 tests**, release
  build and the binary self-test.

## T27 - caption import/export, split and merge-up (CI-verified, run 43439895270)

T26 could type a cue it had just added, but the sequence's caption files had no
way in or out, and a cue that needed cutting or joining had to be deleted and
retyped.

- **Import** (`captions.rs`): Import opens a caption-file picker (`.srt`,
  `.vtt`, `.scc`, `.mcc`, `.stl`, `.ttml`, `.dfxp`) and runs `captions.import`
  on the chosen path. The engine reads the file through its services, detects
  the format from the bytes and the extension, snaps every cue to the
  sequence's frame grid and inserts the parsed track at the top as one
  undoable edit. The window re-reads `captions.list`, targets the imported
  track and follows the engine's cleared caption selection.
- **Export**: the format choice (SubRip or WebVTT) sets both the save filter
  and the `format` handed to `captions.export`, which writes the window's
  target track in that format. The export runs on the session directly, so it
  adds no undo step and never dirties the project, and the reply's cue count
  becomes the status line.
- **Split**: Split at Playhead runs `captions.split` with the selected cue and
  the App playhead as an explicit tick, then selects the new right-hand cue
  from the engine's selection. The engine refuses a cut that would leave
  either half under its minimum duration.
- **Merge Up**: the window keeps its rows in `(start, id)` order, so the row
  before the selected one on the same track is the track's previous cue.
  Merge Up runs `captions.merge` with that pair; the engine spans the kept cue
  to the last end and joins the trimmed texts with a newline.
- **Tests**:
  `captions::tests::importing_an_srt_adds_a_track_named_for_the_file_and_one_undo_removes_it`
  writes a two-cue SRT to the temp directory, imports it and gets frames
  24..60 and 72..96 with their own text, a track named for the file stem, one
  undo step, a cleared engine selection and the imported track as the window's
  target; one undo removes the track and its cues.
  `...::exporting_webvtt_writes_the_cues_without_dirtying_the_project_and_reimports`
  exports a 3 s cue as WebVTT and reads back the `WEBVTT` signature,
  `00:00:00.000 --> 00:00:03.000` and the text, with the undo length and the
  clean flag unchanged, then re-imports the file into a fresh sequence and
  gets the same cue back.
  `...::a_file_that_is_not_a_caption_errors_with_its_name_and_leaves_the_sequence_alone`
  imports a `.txt` and gets an error naming the file, no track and no undo
  step. `...::splitting_the_selected_caption_at_the_playhead_is_one_undo` cuts
  a 3 s cue at 1 s into 0..24 and 24..72 with the right half selected, and one
  undo restores the whole cue.
  `...::merging_the_selected_caption_up_folds_it_into_the_earlier_cue_as_one_undo`
  merges 0..36 "one" with 36..72 "two" into 0..72 "one\ntwo" and one undo
  restores both.
  `ui_tests::splitting_and_merging_the_caption_from_the_editor_are_undoable_steps`
  clicks the chip, Add, then Split at Playhead at the 2 s playhead on the 1 s
  to 4 s cue and gets two cues, clicks Merge Up and gets the single cue back,
  and one Ctrl+Z restores the split.
- **Limitations**: caption tracks paint on the timeline from T28 and their cues
  drag from T29; there is no style editor and no burned-in preview. Import
  always adds a new track rather than folding a file into an existing one,
  Split only cuts the selected cue (the engine's listen-under-the-playhead
  variant is not exposed), and Merge Up only joins the previous cue on the
  same track, not an arbitrary selection.
- **Red evidence**: red commit `4fc8173` (run `43372950488`) stubbed the four
  writes and failed exactly the six new paths: **136 passed; 6 failed**, with
  fmt, clippy `-D warnings`, check and the release build all green.
- **CI**: run 43439895270 - fmt, clippy `-D warnings`, check, **142 tests**, release
  build and the binary self-test.

## T26 - caption editor (CI-verified, run 43369355615)

Row 87 said FilmCraft has captions and the shell does not expose them: the
sequence's caption tracks and cues were real in the document, but nothing
listed them, nothing could add one at the playhead and no cue's text could be
typed.

- **List** (`captions.rs`): the window reads `captions.list` - journal-free, so
  opening or refreshing it never dirties the project and never adds an undo
  step - and keeps the engine's own `C1`/`C2` slot labels, the engine's
  timecodes at the sequence rate, its duration timecodes and its caption order.
  Each track is a chip; its hover text carries the engine's format and count.
- **Add**: Add Track runs `captions.newTrack` with the Subtitle format. Add runs
  `captions.add` with the App playhead passed as an explicit `time` and the
  length draft (3 s by default), so the cue snaps to the sequence's frame grid.
  The engine would create a missing track inside that same command; the app
  creates it first instead, so the track is its own undo step and the first Add
  is one Ctrl+Z for the cue and a second for the track rather than an untracked
  side effect.
- **Select**: a row click runs `captions.select` - the engine's caption
  selection, which clears the clip selection the way the engine does - and
  seeds the text, start and length drafts from that cue's own start and end
  ticks, so Apply edits exactly the cue that was clicked.
- **Edit**: Apply writes the text (`captions.setText`) and then the timing
  (`captions.setTimes`, start plus length in seconds, frame-snapped by the
  engine) as their own undo steps; Delete runs `captions.delete` on the
  selected cue and refreshes through the engine's cleared selection; Prev and
  Next step the playhead to a real cue start through `captions.previous` /
  `captions.next`.
- **Tests**:
  `captions::tests::the_editor_lists_the_engines_own_tracks_and_captions_in_time_order`
  (two tracks and three cues added out of order: the engine's `C1`/`C2` labels
  top-first because `newTrack` inserts at the top, the per-track counts, the
  timeline order, and a 1.5 s cue reported as `00:00:01:12` = 36 frames),
  `...::adding_a_track_and_a_caption_at_the_playhead_snaps_to_frames_and_undoes`
  (a 1 s playhead and a 2.5 s draft land on frames 24..84, one undo removes the
  cue and a second the track),
  `...::editing_the_selected_captions_text_and_times_is_one_undo_each` (text,
  then a 2 s start and 2.5 s length = frames 48..108, one undo each, and the
  second undo restores the text), and
  `...::deleting_the_selected_caption_removes_it_and_undo_restores_it`.
  `ui_tests::the_captions_chip_opens_the_editor_and_adds_a_caption_at_the_playhead`
  clicks the toolbar chip, then Add, and gets the sequence's caption track plus
  a 3 s "New caption" at the 2 s playhead, selected in the engine; Ctrl+Z
  removes the cue and the next one the track.
- **Limitations**: caption tracks paint on the timeline from T28 and their cues
  drag from T29; there is no style editor and no burned-in preview.
  Import/export, split and merge-up arrived with T27.
- **Red evidence**: the original red commit (`6487605`) never compiled - the
  window's row loop borrowed the editor immutably while the row handler called
  `select` with `&mut editor` - so run `43360963172` reported E0502 instead of
  failing tests. A diagnostic commit (`c4b9478`, run `43367130015`) re-stubbed
  the five writes on the compiling tree and failed exactly the five behavioural
  paths: **131 passed; 5 failed**, with the list test passing. The writes were
  then restored (`5b9b294`).
- **CI**: run `43369355615` - fmt, clippy `-D warnings`, check, **136 tests**, release
  build and the binary self-test.

## T25 - transitions on the timeline (CI-verified, run 43138585328)

T24's browser could name a transition and set its length from a field, but the
timeline painted nothing between the two clips a transition joins: it could not
be seen, clicked or dragged.

- **Drawing** (`edit.rs`): every transition paints as a band over the cut it
  joins - the clips' own `timeline_clip_rect` mapping, a dark fill, the accent
  stroke and a bowtie - and the band of the engine's `transition_selection`
  paints selected. Bands are hit before clips, because a transition sits on top
  of what it joins, which is where a person looks for it.
- **Click**: selecting a band runs `timeline.select` with the transition and the
  clip it lands on. That is a query, so there is no history step and no
  dirtying: it makes the transition the engine's `transition_selection` (Effect
  Controls, Clear Transition) and selects the incoming clip, so the browser's
  heading agrees with what was clicked.
- **Drag**: an edge (within 8 pt) resizes it and the body slides it over the
  cut; either is written on release as one `sequence.setTransition`, so a whole
  drag is one Ctrl+Z. The span comes from
  `filmcraft_edit::transitions::drag_span`, the engine's own clamp, so it cannot
  leave the two clips, cannot go below one frame and, between two clips, cannot
  leave their cut; the app takes a direct `filmcraft-edit` dependency at the
  same pinned revision (already in the graph) for that one function rather than
  re-deriving the rules. The ghost preview calls the same function, so a drag
  shows exactly what it commits, and a drag the clamp leaves where it was writes
  nothing at all.
- **Tests**:
  `edit::tests::dragging_a_transition_edge_sets_its_length_and_one_undo_restores_it`
  (at 40 px/s one second of drag extends the out edge from 24 to 48 frames while
  the start holds, a 10 000 px drag clamps at the clips' 2.5 s, and two undos
  return to the applied dissolve) and
  `ui_tests::clicking_a_timeline_transition_selects_it_and_dragging_its_edge_sets_its_length`
  (a 24-frame dissolve at a cut the test makes with Ctrl+K: the click selects
  the transition and the clip it lands on, the right-edge drag makes it 48
  frames, and one Ctrl+Z restores 24).
- **Limitations**: changing an applied transition's effect still means removing
  and re-applying it (`sequence.setTransition` edits settings and the span, not
  the effect id), and a transition cannot be dragged to another cut.
- **CI**: run `43138585328` - fmt, clippy `-D warnings`, check, **130 tests**,
  release build and the binary self-test.

## T24 - transition browser (CI-verified, run 43090989519)

The transitions row could apply the default dissolve / crossfade and edit the
one at the selected clip's In edge, but nothing listed the engine's other
transitions, the audio variants were unreachable and there was no way to pick
one.

- **Catalog** (`transitions.rs`): the browser loads `effects.list` with
  `kind: "VideoTransition"` / `"AudioTransition"` - a query, so opening it
  neither dirties the project nor adds a history step - and keeps each row's
  engine id, name and folder path. The window is a fixed 400x540 box: an
  auto-sized anchored window takes its size from its contents, and the
  contents-filling catalog handed that size straight back, so the window
  resized by the same 34.7 pt every other frame and no click could land on it.
  It offers the two tabs, labels every row with the engine's leaf folder and
  narrows the list with a name/folder filter.
- **Apply**: the chosen id, the browser's length (1-600 frames) and the
  selected clip's In edge go to `sequence.applyVideoTransition` /
  `sequence.applyAudioTransition`; a linked pair is resolved first, so an
  audio variant can be applied from the picture (and the other way round).
  Apply only takes a length, so a Start / End at cut alignment is a second
  `sequence.setTransition`; Center at cut is one edit and one undo.
- **Selection and removal**: the browser names the transition already at the
  selected edit (effect, frames, alignment) and Select makes it the engine's
  `transition_selection` through `timeline.select`; Remove deletes that one
  transition in one undo. Clicking a row arms Apply through `choose`, which
  ignores an id the reloaded catalog no longer lists.
- **Tests**:
  `transitions::tests::the_catalog_is_the_engines_own_transition_registry`
  (engine folders and the filter),
  `...::the_browser_chooses_a_catalog_row_and_ignores_an_unknown_effect`,
  `...::applying_a_wipe_uses_the_chosen_effect_alignment_and_length`
  (Linear Wipe, 10 frames, Start at cut on the cut),
  `...::an_audio_transition_variant_applies_to_the_linked_sound` (Constant
  Gain on A1 only) and
  `...::the_browser_selects_and_removes_the_transition_at_the_selected_edit`
  (selection, removal, one-undo restore);
  `ui_tests::the_transition_chip_opens_the_browser_and_applies_the_chosen_effect`
  clicks the clip, opens the browser from the chip, checks the engine catalog
  and a filtered row, requires the Apply button's rect to be identical across
  three frames (the window must stop moving before a click can land), applies
  Linear Wipe and restores with one Ctrl+Z.
- **Limitations**: a non-Center alignment is two engine edits (two undos);
  a transition cannot be dragged or clicked in the Timeline yet, so the
  browser is the only per-transition selection, and changing an applied
  transition's effect means removing and re-applying it.
- **CI**: run `43090989519` - fmt, clippy `-D warnings`, check, **128 tests**,
  release build and the binary self-test.

## T23 - speed, reverse and freeze (CI-verified, run 42949817569)

The engine could change a clip's rate (`clip.speedDuration`) and hold a source
frame (`clip.frameHoldOptions`), but the shell called neither, so the parity row
for speed and freeze was empty.

- **Dialog** (`speed.rs`): the Clip menu's "Speed/Duration..." and a Speed chip
  in the timeline toolbar (shown when a clip is selected) open a floating
  window seeded from the selected clip's own rate, direction and time
  interpolation. It carries a percentage field, 25/50/100/200/400 presets,
  Reverse, Ripple and the Frame Sampling / Frame Blending / Optical Flow
  choice, and its width is pinned so an anchored window cannot take the whole
  screen on its first layout pass.
- **Apply**: runs `clip.speedDuration` with the clip named explicitly, so the
  engine recomputes the timeline length from the source span (200% halves a
  10 s clip), moves a linked pair together and keeps the change as one undo.
  The command is gated on the engine's own selection, which the timeline keeps
  on the app, so `with_engine_selection` mirrors the clip into the engine
  state for the call and restores the previous selection afterwards.
- **Freeze**: Freeze Frame runs `clip.frameHoldOptions` on the in point, the
  out point or the Program playhead, which holds that source frame for the
  clip's whole length without changing it; Remove Hold clears it. The
  playhead is sent as an explicit `sequenceTime` because the engine's own
  playhead is never mirrored from the monitor - a "playhead" hold would
  otherwise freeze frame 0. A freeze is a held frame in the document, not a
  rendered still file.
- **Tests**: `speed::tests::speed_dialog_doubles_the_rate_and_halves_the_duration_as_one_undo`,
  `speed::tests::speed_dialog_slows_down_reverses_and_ripples_the_clips_after_it`
  and `speed::tests::freeze_frame_holds_a_source_frame_and_can_be_removed`
  assert the exact durations, the reverse flag, the ripple shift, the held
  source frame and the undo; `ui_tests::the_speed_chip_opens_the_dialog_and_a_preset_halves_the_selected_clip`
  clicks the clip, opens the dialog from the chip, clicks the 200% preset and
  Apply, then restores the length with one Ctrl+Z.
- **Limitations**: no duration field (the rate drives the length), the
  interpolation mode is stored and handed to the renderer but no rendered
  optical-flow difference is measured, and a freeze cannot yet be given its
  own length inside the dialog.
- **CI**: run `42949817569` - fmt, clippy `-D warnings`, check, **122 tests**,
  release build and the binary self-test.

## T22 - sequence marker EDL import (CI-verified, run 42834979022)

Markers could round-trip through the CSV report but not through the EDLs
editors already exchange, where locators ride as `* LOC:` comment lines.

- **Format** (`edit.rs`): `parse_markers_edl` reads
  `* LOC: <record tc> [COLOR] <name>` from any CMX 3600 list (a title, FCM
  and cut events are ignored, so a full EDL works). The record timecodes
  carry the hour the list starts at, so the hour of the first locator is the
  base and every marker lands relative to it; a locator before that hour is
  refused. The Avid colour words map onto the label palette (`BLUE` ->
  `Blue`, `CYAN` -> `Teal`, ...) and a locator without one is green. Names
  are the remaining words; locators have no comment or length.
- **One edit** (`edit.rs`): the CSV and EDL importers share
  `apply_imported_markers`, so both add every marker through one undoable
  "Import Markers" edit and a bad row aborts the whole import. Errors name
  the line (EDL) or the row (CSV).
- **UI**: the marker panel gains "Import Markers EDL..." beside the CSV
  button; the parse runs before the edit, so a bad file leaves the sequence
  untouched.
- **Tests**: `marker_edl_import_reads_loc_lines_and_is_one_undo` imports a
  24 fps cut list with `BLUE Intro` at `01:00:02:00` and a plain
  `Wider shot` at `01:00:05:00`, asserting the 2 s/5 s starts, the colour
  mapping, the green default, the empty comment, the status line and a
  single undo; `marker_edl_import_refuses_a_bad_locator_and_a_file_without_any`
  covers the no-locator and bad-timecode refusals and that nothing is added.
- **Limitations**: marker EDL export remains CSV only, and drop-frame EDL
  timecodes are read as non-drop.
- **CI**: run `42834979022` - fmt, clippy `-D warnings`, check, **118 tests**,
  release build and the binary self-test.

## T21 - snapping to markers and across linked pairs (CI-verified, run 42761241822)

T3's snapper knew only zero, the playhead and clip edges, and it treated the
other half of a linked A/V pair as an ordinary neighbour, so a pair always
tugged itself back onto its own edges.

- **Marker targets** (`edit.rs`): `snap_targets` seeds every
  `Sequence.markers` start alongside zero and the playhead, so a move or a
  trim can land on a cue.
- **Linked pairs**: when the dragged clip carries a `link`, every item sharing
  it is left out of the target list. The engine moves or trims a pair as one,
  so its own other start and end are not neighbours. The exclusion is derived
  from the sequence alone, so `snap_move` and `snap_trim` both get it through
  the one helper.
- **Edge choice** (`edit.rs`): the move's "nearest of start or end" comparison
  measured both candidates against the raw start, so an edge that did not snap
  (distance zero) always beat one that did - a drag only snapped when both ends
  found targets. `choose_snap` now prefers a moved edge, then the nearer of two
  moved edges, and leaves the clip alone when neither snaps.
- **Tests**: `edit::tests::snap_targets_include_sequence_markers` checks a
  marker becomes a target; `edit::tests::snap_move_does_not_snap_a_linked_pair_to_its_own_other_half`
  places a 10 s pair at 2 s and shows the picture stays at its raw 2.1 s
  instead of snapping back to its sound's 2 s; the demo-fixture target test
  now counts four excluded edges (both halves) instead of two; and
  `ui_tests::dragging_a_clip_snaps_its_start_and_linked_sound_to_a_marker`
  drags 1.9 s to a 2 s marker and asserts both halves land on 2 s with
  `Moved clip (snapped)`; `edit::tests::a_snap_prefers_the_edge_that_actually_moved`
  pins the edge choice on its own.
- **Limitations**: ripple-aware snapping (so the material a ripple shifts also
  lands on a target) remains open, and a marker snaps only by its start.
- **CI**: run `42761241822` - fmt, clippy `-D warnings`, check, **116 tests**,
  release build and the binary self-test.

## T20 - timeline marker list (CI-verified, run 42749131608)

T2's markers were only reachable by clicking their ruler stem; there was no
place to read, choose or delete one by name.

- **Window** (`edit.rs`): `MarkerList { selected: Option<u64> }` is editor
  state on `App`, opened by the timeline header's `ff-marker-list-open` chip or
  the Markers menu's "Marker List..." row, and drawn by `marker_list_window`
  from `App::ui_frame` the way the relink dialog floats. The content width is
  pinned to 360 points, so an anchored window never lays itself out at full
  screen on its first pass. Rows carry the marker's colour dot, timecode, name
  and comment, with explicit ids (`ff-marker-list-row`, `ff-marker-list-del`)
  so scripted clicks reach them.
- **Selection**: `select_marker` looks the id up on the sequence, moves the
  Program and engine playheads to its start, and reports
  `Marker "<name>" at <timecode>`; the window highlights the chosen row.
- **Delete**: `delete_marker` removes exactly that id through one undoable
  "Delete Marker" project edit (the engine only offers clear-at-playhead and
  clear-all), clears the row's selection, and leaves the other markers alone.
- **Tests** (two `ui_tests`): opening the list and clicking the 2 s then the
  6 s row seeks both playheads and reports each marker, and selecting then
  deleting the 2 s row removes only it, with one Ctrl+Z restoring both.
- **Limitations**: the list is not a docked panel and has no inline rename
  (the header's Update Marker still edits at the playhead); EDL import remains.
- **CI**: run `42749131608` - fmt, clippy `-D warnings`, check,
  **112 tests**, release build and the binary self-test.

## T19 - sequence marker ruler drag (CI-verified, run 42731129937)

T18 made Source markers draggable; the timeline's own markers still only
jumped on click.

- **Geometry** (`edit.rs`): `sequence_marker_rects` gives every marker a
  `SEQUENCE_MARKER_HANDLE_W`-wide grab target in the ruler's top band, using
  the ruler's own time-to-pixels mapping. The targets are registered before
  the Program In/Out and split handles, so paint precedence (a mark draws over
  a marker) and grab precedence agree: a mark at the same frame wins.
- **Gesture**: `drag_sequence_marker` snaps the new start to the sequence
  frame grid and clamps it to `[0, max(sequence_end, current)]` - a marker
  already past the end keeps its own start as the upper bound instead of being
  pulled inward. The marker keeps its id, name, comment, length and colour, and
  the write is one merged project edit ("Move Marker",
  "sequence-marker-drag"), so a whole drag is one Ctrl+Z.
- **Tests** (two `ui_tests`): a marker at 2 s among a second at 6 s is dragged
  by one ruler second, only it moves, and one Ctrl+Z restores both; a 2 s
  marker dragged five seconds left stops at zero.
- **Limitations**: markers may share a frame (the later one wins the grab);
  EDL import and marker-list selection remain.
- **CI**: run `42731129937` - fmt, clippy `-D warnings`, check, **110 tests**,
  release build and the binary self-test.

## T6 - Ripple delete with linked A/V and marker ripple (CI-verified, run 40163011141)

`Shift+Delete` and the timeline **Ripple Delete** button call the engine's real
`edit.rippleDelete`. The engine expands the selection to linked A/V, deletes the
clips, then closes the gap on every track that lost material and on sync-locked
tracks; tracks that lost nothing are not shifted.

- **Locked tracks**: the shell refuses the whole edit when the selected clip's
  track is locked, instead of letting the linked pair desynchronise. The engine
  would skip a locked track, so the pre-check gives the reason in the status
  line. The pre-check only inspects the selected clip's track; a locked linked
  partner is left to the engine and is a known edge case.
- **Markers**: with Ripple Markers on (default), markers strictly inside the
  removed span are removed; markers at the span's end or later move left by the
  removed duration; earlier markers stay. Off leaves markers untouched. Undo
  restores both clips and markers.
- **Scope**: the gap closes on the tracks that lost clips plus sync-locked tracks
  (engine `ripple_delete_spans` / `shift_track_from`); other tracks are untouched.
- **Test**: `ripple_delete_closes_the_gap_across_linked_av_and_markers_and_refuses_locks`
  drives a real linked Bars & Tone pair, asserts the picture and sound move
  together, the before/inside/at/after marker policy, one-step undo, and the
  locked-track refusal.
- **Limitations**: no targeting-only ripple (it follows the engine's affected +
  sync-locked policy); a transition selected alone cannot be ripple-deleted.

## T7 - Roll, slip and slide (CI-verified, run 40163011141)

The timeline header's **Trim** row and `Alt+Left/Right` (slip), `Alt+Shift+Left/Right`
(slide) and `Alt+Ctrl+Left/Right` (roll) drive the engine's separate
`timeline.roll`, `timeline.slip` and `timeline.slide` commands with
`deltaFrames`, so the arithmetic uses the sequence rate's exact tick count and
the engine clamps to the media handles.

- **Roll**: `roll_pair` finds the adjacent pair on the selected clip's track -
  its out edge with the next clip, or the previous clip's out edge with its in -
  and rolls that shared edit point. The surrounding clip positions and the
  sequence duration do not change.
- **Slip**: only the selected clip's source In/Out move; its timeline span and
  both neighbours stay put.
- **Slide**: the selected clip moves; the left neighbour's out and the right
  neighbour's source In absorb the move, so the sequence's outer boundaries stay
  fixed.
- **Tests**: `roll_moves_only_the_shared_edit_point_and_undoes` and
  `slip_and_slide_keep_the_right_things_fixed` build 2 s clips with 2 s handles
  through the Source monitor's three-point edit, then assert the exact 12-frame
  results and one-step undo for each operation.
- **Limitations**: the buttons and keyboard nudge one frame at a time (no drag
  gesture or arbitrary edit-point picking yet); roll uses the nearest adjacent
  pair only; the engine's clamped delta is reported in the status line.

## T8 - Real video cross-dissolve and audio crossfade (CI-verified, run 40163011141)

The **Dissolve** and **Crossfade** buttons (and Ctrl/Cmd+D,
Ctrl/Cmd+Shift+D) apply a 12-frame transition at the selected clip's In edge
through `sequence.applyVideoTransition` / `sequence.applyAudioTransition`.
Selecting either half of a linked pair finds the other half, so picture and
sound can be treated separately.

- **Video**: the engine applies `cross_dissolve` centred on the cut
  (start = cut - 6 frames, duration = 12 frames). `render_sequence` is the same
  path the Program monitor and export use. The test renders colour mattes at
  1.5 s (red), 2.0 s (a real red/blue mix) and 2.5 s (blue), then undoes the
  transition and confirms the cut is a hard blue frame.
- **Audio**: the engine applies `constant_power` gains over the same centred
  range and `audio::mix_sequence` is the real mixer path. The test selects the
  incoming picture, resolves its linked sound, and measures stereo RMS around
  the cut. Both 1 kHz sides are phase-aligned in this fixture, so the overlap is
  louder than either side, proving both sample streams are mixed.
- **Undo**: each transition is one engine edit; applying both is two steps.
- **Limitations**: only the default cross-dissolve/constant-power transition and
  a fixed 12-frame duration are exposed; there is no effect browser, custom
  duration, alignment control or transition handle in the UI yet.

## T9 - Sequence In/Out marks with Lift and Extract (CI-verified, run 40236814552)

The timeline header's **Mark In**, **Mark Out**, **Clear I/O**, **Lift (;)** and
**Extract (')** buttons drive the engine's real Program marks and range edits.
`I`/`O` mark the open source clip when one is open; with no source clip they mark
the sequence's Program In/Out instead, so the keys are never a no-op. Marking
snaps the playhead to the sequence frame grid first, and every action is one
undo step.

- **Marks**: `markers.markIn` / `markers.markOut` write `q.mark_in` / `q.mark_out`
  on the active sequence (Out is inclusive, so the effective range ends one frame
  past Out). `markers.clearInOut` clears both; media-item source marks are
  untouched. The engine also drops a previously set opposite mark when the new
  one would invert the range.
- **Lift**: `sequence.lift` removes the In/Out range on the targeted tracks
  (default: all tracks) and leaves the gap, so later clips keep their time. The
  marks stay set for a follow-up edit.
- **Extract**: `sequence.extract` removes the range on targeted **and
  sync-locked** tracks and closes the gap, so later clips move left; it also
  ripples sequence markers when Ripple Markers is on, clears the marks, and parks
  the engine playhead at the range start, which the shell follows.
- **Test**: `sequence_in_out_marks_lift_leaves_a_gap_and_extract_closes_it_and_undoes`
  drives a real three-clip linked Bars & Tone sequence: it marks In/Out through
  the helpers, asserts lift removes the middle picture and sound while leaving the
  last clip at 4 s, that extract closes the gap to 2 s, clears the marks and moves
  the shell playhead, and that undo restores the clips and marks.
- **Limitations**: no sequence In/Out drag in the ruler, no split (video-only or
  audio-only) In/Out, and no keyboard shortcut for Program marking while a source
  clip is open (use the buttons); Lift/Extract follow the engine's targeting, so
  untargeted tracks are untouched.

## T10 - Marker rename and comment editing (CI-verified, run 40252471033; extended by T13)

The timeline header's marker row now has **marker name**, **comment**, **length** and **colour** fields plus an **Update Marker** button (T13 extends the T10 rename editor). The button edits the sequence marker at the playhead, so a normal click-to-jump (or `M` to add) selects the work target.

- **Engine boundary**: the engine has no marker-edit command, so the shell uses
  the existing `Session::edit` custom-edit path (`App::edit_project`) and mutates
  the active sequence's marker list. The edit is a whole-project snapshot, so one
  **Update Marker** click is exactly one undo step and redo restores all fields.
- **Identity**: the helper finds the marker by its snapped start and updates it
  by `MarkerId`, not by index, so markers keep their identity across ripples and
  marker-list reordering.
- **Guard rails**: an empty name is refused (the engine's `markers.add` default
  is an empty string, so blanks are real); with no marker at the playhead the
  status bar says so rather than creating one. The helper snaps the playhead to
  the sequence frame grid before matching.
- **Test**: the T10 test was extended and renamed in T13 to
  `marker_fields_update_is_an_undoable_project_edit`; it adds a real marker through
  `markers.add`, sets name, comment, a 24-frame length and the Blue colour, asserts
  all four fields, then undoes and asserts the original empty name/comment,
  zero-length point marker and Green colour with the marker still present; finally
  it moves the playhead away and asserts the no-marker status.
- **Limitations**: only the marker at the playhead is editable (no marker-list
  selection), marker drag and CSV/EDL import are still absent, and the length
  field is capped at 6000 frames in the UI.


## T11 - Transition length and alignment (CI-verified, run 40256569182)

The trim row's **Transition:** controls set the frames (1-600) and alignment
(Center/Start/End at cut) used by **Dissolve** and **Crossfade**, and **Update
Transition** edits the transition already at the selected clip's In edge.

- **Apply**: Dissolve/Crossfade pass the chosen `frames` to
  `sequence.applyVideoTransition` / `sequence.applyAudioTransition` (which take
  length and edge but not alignment). A fresh transition is centred on the cut
  by the engine, and the status bar reports the length.
- **Edit**: Update Transition finds the transition whose `to` is the selected
  clip on that clip's own track kind (select the sound for an audio crossfade)
  and runs `sequence.setTransition` with `frames` and `align` as one undo step.
  The engine keeps a two-clip transition over the cut and inside the clips it
  joins, so an over-long duration or an alignment that would push it off the cut
  is refused in the status bar, not truncated silently.
- **Test**: `transition_duration_and_alignment_are_editable_through_the_engine`
  applies a real 24-frame dissolve at a 24 fps cut (start = cut - 12 frames,
  centred), updates it to 8 frames end-at-cut (start = cut - 8 frames), checks
  the same transition id and `TransitionAlign::EndAtCut`, undoes back to 24
  centred, and confirms the no-transition guard on the outgoing clip.
- **Limitations**: alignment applies through Update Transition (application is
  always centred), the UI caps length at 600 frames, and there is still no
  transition browser, drag handle or per-transition list selection.

## T12 - Marker CSV export (CI-verified, run 40260600879)

The marker row's **Markers CSV** button exports the active sequence's markers to
a CSV file chosen through the normal save dialog. The button is disabled when the
sequence has no markers, and the helper refuses an empty list rather than writing
a header-only file that looks like a successful export.

- **Columns**: `name,comment,start_ticks,duration_ticks,start_timecode,label`,
  one row per marker in the engine's order (sorted by start). Ticks are exact;
  `start_timecode` is a people-readable `HH:MM:SS:FF` at the sequence rate, and
  `label` is the marker colour's engine name.
- **Escaping**: name and comment are always quoted as RFC 4180 fields with
  embedded quotes doubled, so a comma, quote or newline in a marker cannot shift
  the columns. Numbers, timecode and label are written unquoted.
- **Boundary**: `markers_csv` is a pure function tested byte-for-byte, and
  `write_markers_csv(app, path)` does the file write (returning the marker count
  and setting the status) without the dialog, so the test exercises the real
  bytes. The dialog path reports an I/O failure in the status bar.
- **Test**: `marker_csv_export_escapes_text_and_writes_the_file` adds one marker
  named `A, "B"` with a two-line comment, a 12-frame duration and the Blue label,
  asserts the exact CSV text (including `00:00:01:00` at 24 fps), writes it to a
  temp file, reads the same bytes back and checks the status line.
- **Limitations**: export only (no CSV import), no EDL/XML/FCPXML, no column or
  delimiter choice, and only the active sequence's markers; marker drag is still
  absent.

## T13 - Marker length and colour editing (CI-verified, run 40513248065)

The marker row's **length** field is a frame count (0 = a point marker) and its
**colour** combo exposes the engine's full label set. **Update Marker** writes
name, comment, length and colour together through the same custom project edit
as T10; no engine command exists for marker edits.

- **Length**: the UI permits 0-6000 frames; the helper converts frames to exact
  ticks at the sequence rate with `tick_of`, so the stored duration is the same
  value the CSV export reports. Zero restores a point marker.
- **Colour**: the combo is populated from `Label::ALL`, and the helper stores the
  selected `Label` on the `MarkerId`; undo restores the previous colour.
- **Status**: the status line reports the new length and colour, and still
  refuses an empty name or a playhead with no marker.
- **Test**: `marker_fields_update_is_an_undoable_project_edit` covers all four
  fields and undo; it is the T10 test extended rather than a second, weaker
  path.
- **Limitations**: no marker drag, no CSV/EDL import and no marker-list
  selection; a marker's start cannot be moved through this editor.

## U1 - the TSX workspace port and its new screens (CI-verified, run 40794727835)

The supplied TSX mock-up (one `EditingWorkspace` component; a static visual
specification with no state and no engine calls) was rebuilt as native egui, not
embedded as a web view, so every control stays wired to the real engines.

- **Design kit** (`apps/frameforge-pro/src/ui.rs`): the mock-up's palette — accent
  `#35c2b5`, dim `#1b4a46`, highlight `#62dccf`; backgrounds `#151515`, `#1e1e1e`,
  `#262626`, `#2e2e2e`, `#181818`; text `#e8e8e8`, `#b3b3b3`, `#7c7c7c`; clip violet
  `#8b79d1`, audio green `#3f9b62`, title pink `#d36bc8` — plus the compact type
  scale, 24 px panel headers, tabs, cards, guides, badges and bars. Installed once
  with egui 0.36's themed style API (`Theme::Dark`).
- **Shell** (`app.rs`): a 30 px menu band plus a 30 px workspace-tab band, the
  eight workspaces below, a 24 px status bar, and draggable horizontal/vertical
  splitters. Layout dimensions come from the TSX: 320 px source column, 340 px
  inspector, 250 px timeline.
- **Edit** (`edit.rs`): three columns — source monitor over the media browser
  (media / markers / history tabs, search and cards); centre program monitor over
  the timeline (toolbar, ruler, tracks, themed clips); full-height inspector. Both
  monitors still render through the engines, with real transport and scrubbing.
- **Text & Graphics** (`graphics.rs`): real `graphics.list/newText/newShape/set/
  setText/setTextType/selectLayer/bringForward/sendBackward/alignFrame/
  deleteLayer` calls, layer list, text/shape drafting, position/scale/visibility.
- **Masking** (`masking.rs`): real `masks.list/add/set/select/remove/track` over an
  opacity effect, with feather, opacity, inversion, mode and expansion. The tracker
  button is FilmCraft's engine mask tracker, **not XMem**; the XMem panel states
  "not wired in this build" rather than pretending.
- **Compositing** (`compositing.rs`): real `Composition` layer list, toggle,
  removal and text/solid creation, recorded through `record_comp_change` into the
  shared undo order. A composition only exists after importing an EffectCraft
  project, and the empty state says so.
- **Preserved behaviour**: ripple delete, roll/slip/slide, transitions, markers,
  Lift/Extract, mixer, effect chains and export still call the same engine commands
  and keep their tests; the UI queries that must not dirty the document
  (`graphics.list`, `masks.list`) call the session directly.
- **Limits**: fixed layout sizes (no docking persistence), screens depend on the
  selected clip/project, and U1 changes presentation only — no engine behaviour.
- **CI**: run `40794727835` — fmt, clippy `-D warnings`, check, 54 tests, release
  build, binary self-test including `roto-bench 4`; artifact
  `frameforge-pro-windows-dev` is 25,842,805 bytes.

## U2 - UI interaction tests through the real frame loop (CI-verified, run 42340371967)

Ten parity rows carried the same note: "UI automation is a separate gap". The UI is
almost entirely custom-painted rectangles, so an accessibility-tree driver
(`egui_kittest`) would mostly assert on a tree the timeline and transport never
populate. The decision was to drive the real loop by hand, with no new dependency.

- **Seam** (`app.rs`): the body of `eframe::App::ui` moved to
  `App::ui_frame(&mut self, ui: &mut egui::Ui)`; the trait method is a one-line
  delegate. Tests call the same function with no `eframe::Frame`, so the tested
  path and the shipped path cannot drift.
- **Harness** (`ui_tests.rs`): `egui::Context::default()` calling `run_ui` at a
  fixed 1600x900, with a `RawInput` whose `viewports` map holds
  `ViewportId::ROOT` and whose `focused` flag is set. One frame per event: a key
  press is the modifier change plus key-down, then key-up, then the modifiers
  released.
- **What egui 0.36 actually takes**: `RawInput` has no `modifiers` field; modifier
  state reaches `InputState` only through `Event::ModifiersChanged`. The first
  harness draft failed E0560 in run `42335553468`.
- **Headless output**: `TexturesDelta` debug-asserts on drop when a backend has not
  applied its deltas, and the harness drops `run_ui`'s `FullOutput`, so it calls
  `textures_delta.clear()` first (that was the failure in run `42338768862`).
- **Fixture**: `App::new()` + `file.newSequence` + an `edit_project` that adds a
  `Generator::BarsAndTone` item + `edit::place_at_playhead`. `BarsAndTone` has both
  streams, so V1 and A1 each get a clip; `Generator::Tone` is audio-only and made
  the first fixture place nothing on V1 (run `42336661855`).
- **Covered now**: Ctrl+Z / Ctrl+Shift+Z undo and redo the placement (and the media
  item survives, proving the history reached the placement, not the item edit);
  Ctrl+K splits V1 and A1 at the playhead and reports "Add Edit (all tracks)"; M
  adds one marker on the snapped playhead and Shift+M jumps to it without adding.
- **Assertions**: engine document and editor state only, never pixels, so a
  restyle cannot break a test that is really about what the command did.
- **Limits at U2**: keyboard paths only. Pointer gestures needed a shared rect
  helper that reports where the timeline drew each control; U3 adds it for the
  body and the clips, and U2's follow-up (run `42389027715`) already reached the
  track header squares through `Context::read_response`.
- **CI**: run `42340371967` — fmt, clippy `-D warnings`, check, **72 tests**
  (69 + the 3 UI tests), release build and the binary self-test. A later
  pointer test (run `42389027715`) adds the A1 mute click and reaches the
  control through `Context::read_response`, so no second geometry helper is
  needed.

## U3 - scripted pointer gestures on the timeline (CI-verified, run 42401469221)

U2 could press keys; the clip move, the two edge trims and the ruler scrub were
still code-inspected because the custom-painted timeline exposed no addressable
geometry.

- **Body id** (`edit.rs`): the timeline body is allocated with `Sense::hover`,
  then re-registered with `body_ui.interact(alloc, Id::new("ff-timeline-body"),
  Sense::click_and_drag())`. `Context::read_response` now yields the body rect,
  the same route the track-header squares already took. Painting is unchanged.
- **One geometry function**: `edit::timeline_clip_rect(alloc, row, start,
  duration, zoom)` is called by the draw loop and the tests, so a scripted drag
  grabs exactly the rectangle a person sees instead of a second copy of the
  maths. `RULER_H` is `pub(crate)` so a test can aim above the clip rows.
- **Harness** (`ui_tests.rs`): `click_at(pos)` sends hover, press, release;
  `drag(from, path)` sends hover, press, one frame per waypoint, release. egui
  starts a drag on the first waypoint past its 6 px click tolerance
  (`max_click_dist`), and the timeline decides move-vs-trim from the pointer at
  that instant. A trim test therefore presses inside the 8 px edge band and
  keeps the drag-start waypoint inside it too, moving just far enough (6.9 px)
  to be a drag.
- **Measured, not guessed**: the tests set `zoom = 40.0` and render one frame
  before reading the rects, so 40 px = 1 s. The move drag is 60 px (1.5 s); the
  out trim is -40 px (-1 s); the in trim is +20 px (0.5 s). Assertions allow
  0.02 s for the f32 pixel-to-tick path and the engine's frame grid.
- **Covered**: a ruler click scrubs the playhead to 2 s and clears the
  selection; a mid-clip drag moves V1 by 1.5 s, keeps its length and reports
  "Moved clip", and Ctrl+Z restores the start; an out-edge drag reports
  "Trimmed out edge", shortens the tail to 9 s and an undo restores 10 s; an
  in-edge drag reports "Trimmed in edge", moves the head to 0.5 s, shortens the
  clip to 9.5 s and leaves `start + duration` exactly where it was.
- **Select and delete** (follow-up, run `42406648700`):
  `clicking_a_clip_selects_it_and_clicking_empty_space_deselects` clicks the V1
  clip's centre and asserts the engine clip id in `app.selection`, then clicks
  the same row past its right edge and asserts the selection cleared and the
  playhead scrubbed. `delete_removes_the_selected_clip_and_ctrl_z_restores_it`
  selects with a click, presses Delete and asserts `edit.clear` removed V1 and
  its linked A1 ("Clip removed", selection empty), with Ctrl+Z restoring both.
  No production change was needed: a raw `Ui::interact` click does not take
  keyboard focus, so `egui_wants_keyboard_input` stays false and the Delete
  shortcut still fires.
- **Limits**: screenshot regression is not automated. The mouse wheel,
  shift-click ranges and multi-clip marquee are not covered.
- **CI**: run `42401469221` — fmt, clippy `-D warnings`, check, **83 tests**
  (79 + the 4 pointer tests), release build and the binary self-test. The
  select/delete follow-up is run `42406648700`, **85 tests**.

## E1 - export presets and an explicit range (CI-verified, run 42391495128)

The Export workspace had one hard-wired settings set and always passed
`range: None`, which the engine resolves as "In/Out if set, else the whole
sequence". That is a good engine default but a poor UI: the user could not tell
what would actually be rendered.

- **Presets** (`export.rs`): `PRESETS` carries four named combinations — H.264
  Web 1080p (20 Mbps), H.264 small file (8 Mbps), ProRes mastering and WAV audio
  only. `apply_preset` writes format, bitrate and audio flag together and refuses
  an index past the table without half-applying. Editing any of the three by
  hand clears the selection, so the row reads "Custom" again.
- **Range**: `ExportRange::{EntireSequence, InOut}`. The whole sequence becomes
  an explicit `TimeRange::new(ZERO, duration)`; In/Out stays `None` and lets the
  engine apply its documented In/Out-else-whole resolution, so cleared marks
  never produce an empty file. The In/Out radio is enabled only when both marks
  exist and falls back to the whole sequence when they are cleared.
- **Tests**: every preset maps to an offered format and sets all three fields;
  an unknown index changes nothing; the whole-sequence range spans
  `[0, duration)` and In/Out defers to the engine.
- **Limits**: one foreground job at a time; E2 adds the waiting queue behind
  it.
- **CI**: run `42391495128` — fmt, clippy `-D warnings`, check, **76 tests**,
  release build and the binary self-test.

## E2 - the export queue (CI-verified, run 42395055258)

Before this slice Start Export was disabled while a render ran, so a second
deliverable meant waiting and clicking again.

- **Snapshot jobs** (`export.rs`): `job_from` turns the current settings into a
  `QueuedExport` — label, resolved output path, format, resolved range, audio
  flag and bitrate. The queue therefore holds snapshots: editing the panel after
  queuing cannot mutate a job that is already waiting.
- **Overwrite at the click**: `start` resolves the path and asks once, at the
  moment of the click, then enqueues. The background queue never opens a dialog.
  A file that appears between enqueue and start is still replaced without a
  second prompt; that is the price of never blocking the frame loop on a
  dialog.
- **Duplicate paths**: `enqueue` refuses a second job targeting the same output
  path, whether that path is running or waiting — otherwise the first result
  would be silently overwritten.
- **Driven by the frame loop**: `begin_next` pops the oldest job and spawns the
  same worker thread the single-job path used; `poll` takes the finished
  result, appends to a five-line report, removes the partial file when the
  engine reports `Cancelled`, and immediately starts the next job. `poll` is
  called from `App::ui_frame`, so the queue advances while another workspace is
  open, and `ExportState::is_busy` keeps the 100 ms repaint timer alive.
- **UI**: the panel draws the running job's label, a per-row ✕ and Clear
  Queue, and the finished report. Start Export stays enabled while busy and
  says it will queue.
- **Tests**: the job snapshot (extension, explicit whole-sequence range,
  In/Out deferral, zero-bitrate default), first-in-first-out order, the
  duplicate-path refusal, and `is_busy` ordering the frame loop.
- **Limits**: the helpers and the duplicate rule are tested; the threaded
  hand-off (`begin_next` spawning, `poll` taking the result and starting the
  next job) is code-inspected, not yet driven by a test, because a real render
  is too slow for the unit suite.
- **CI**: run `42395055258` — fmt, clippy `-D warnings`, check, **79 tests**,
  release build and the binary self-test.

## M13 - relinking missing media from the shell (CI-verified, run 42419874435)

The engine could already find and repair missing media (`media.findMissing`,
`media.relink`, `media.autoRelink`, `media.search`, `media.offlineAll`), but a
moved file could only be reconnected through the document API: row 29 had no UI.

- **The dialog carries nothing the engine owns** (`apps/frameforge-pro/src/relink.rs`):
  `RelinkDialog` holds the rows, the selected index, the path field and the last
  message; `link` calls `App::execute("media.relink", …)`, so the candidate
  check (name, extension, fingerprint, duration, metadata), the folder search
  for the others and the single undo step are the engine's own. `MissingRow`
  parses the engine's `media.findMissing` result rather than inventing a shape.
- **One scan per open, not per frame**: `App::refresh_offline` runs
  `media.findMissing` directly on the session (journal-free, so it neither
  dirties the document nor enters the undo history) from `App::from_parts`, and
  again after a link. The badge and the dialog read the cached rows, so they
  cannot disagree with the document.
- **Addressable controls**: the badge is a painted rect registered under
  `ff-relink-open`; the dialog's rows, Link, Browse and Close are painted rects
  under `ff-relink-row-<n>`, `ff-relink-link`, `ff-relink-browse` and
  `ff-relink-close`, the same `Ui::interact` trick the track headers already
  use, so a scripted click reaches them and a restyle does not break a test.
  The dialog floats over any workspace from `App::ui_frame`.
- **Force and the others**: the Force checkbox is passed to `link`; the
  engine's default folder search for the others is named in a hint rather than
  duplicated.
- **Tests** (`ui_tests.rs`, four): a generator-only project has no badge; a
  hand-built item naming a file that is not there is listed with its project
  id, file name, `missing` status and recorded path; linking it repoints the
  project, clears the badge and rows and undoes as one step; a mismatched file
  is refused until Force; and a scripted click on the badge opens the dialog, a
  row click selects, a Link click repairs. The fixture reads the candidate's
  properties with `filmcraft_media::open_path` (read whole, handle dropped) and
  never deletes the missing file, because an imported file is held open by the
  Windows streaming reader — so the "moved out from under a cached source"
  crash path is not exercised.
- **Limits**: `media.autoRelink`, `media.search` and `media.offlineAll` have no
  dedicated button; the path is typed or pasted (Browse uses `rfd`), not
  matched with a live candidate list.
- **CI**: run `42419874435` — fmt, clippy `-D warnings`, check, **89 tests**,
  release build and the binary self-test (the first attempt, run `42418195009`,
  failed on `ui::hint` taking `&str`; `9651b86` fixed it).

## R1 - cached roto masks in the one render path (✔ CI-verified, run 40056112214)

The Roto AI plan's first executable slice: the renderer reads a mask cache and applies the matte to
the clip's alpha, without any model, Python or weights.

- **`crates/bridge/src/roto.rs`**: versioned disk cache (`manifest.json` + 8-bit grayscale PNGs named
  by media frame index), a validated 16-hex cache key (source identity, model SHA-256, working size,
  exact frame rate, ordered correction ids, pinned XMem commit), and `RotoCacheKey::cache_key()`.
  Frames outside the declared range pass through; a missing frame **inside** it is a clear error, not
  a silently unmasked export.
- **`ComposedSource`**: `with_roto_cache` multiplies straight alpha after the effect chain or comp
  output. `ProjectSources` opens the cache named by `Composition.roto`; a missing cache becomes a
  per-source error instead of an unmasked fallback. Preview and export share this method and therefore
  the same mask bytes.
- **`.ffproj` schema 3**: `roto` is optional; schema-2 files load unchanged, and schema-2 readers
  refuse schema-3 projects rather than silently dropping masks.
- **The write side** (`MaskCache::adopt`, added 2026-10-31, run `41255268402`): a worker's own
  `frame_<index>.png` output directory is validated (every promised frame exists, decodes and is
  exactly the working size — **before** anything is published), copied into `root/<key>/`, and the
  manifest is written last to a temporary file that is renamed into place. `MaskCache::open` therefore
  sees either no manifest (`Missing`) or a complete cache, and a short or mis-sized run publishes
  nothing.
- **Tests** (`crates/bridge/tests/r1_roto.rs`, 9): exact alpha inside coverage and pass-through
  outside, missing in-range frame error, every key input changes the address, a correction edit misses
  the old cache, a two-track project renders + exports the background through the transparent half,
  and four adoption tests: a worker's masks are adopted and reach the composed picture, a short run is
  refused **without** publishing a manifest, a mis-sized mask is refused, and a run that starts at a
  later frame covers only that range. No new crate dependency: `image` was already a workspace
  dependency of the app; the bridge now uses it too.
- **Limitations**: mask and frame sizes must match (no proxy stretching; R3 regenerates at the working
  size); decoded masks are not held in an in-memory LRU; publishing is two decode passes (validate,
  then copy) so memory stays at one plane, at the cost of decoding twice; `correction_ids` is still
  always empty, so a re-run with the same inputs republishes the same directory.

## R2 — worker transport and frame exporter (✔ CI-verified, run 40059782293)

R2 builds the boundary the real XMem worker will use, and proves it without pretending to run XMem.

- **`crates/bridge/src/roto_worker.rs`**: protocol 1 JSON lines for `hello`, `propagate`, `cancel`
  and `shutdown`, with typed `ready` / `progress` / `done` / `error` / `cancelled` events.
  `XmemWorker` spawns a real child process, requires a matching handshake, caps one event line at
  1 MiB, and reports a closed pipe with the captured stderr tail. On Windows it starts the child with
  `CREATE_NO_WINDOW`.
- **`export_frames`**: decodes a range through FilmCraft's codecs, fits the working size without
  upscaling, and writes `frame_<media index>.png` in order for the worker.
- **`frameforge-pro roto-bench [frames]`**: times this Rust-side decode/scale/PNG path on synthetic
  Bars & Tone. CI runs it with 4 frames. It says "XMem CPU inference: not measured" because CI has no
  optional runtime or consented weights; the local `tools/` probe is separate and is not an inference
  number invented by `roto-bench`.
- **Deadlines and kill** (added 2026-10-28, run `40814011414`): stdout is parsed on a reader thread
  and delivered over a channel, so `recv_timeout` and `handshake_timeout` return
  `WorkerError::Timeout` instead of blocking forever, and `kill()` makes the teardown explicit for a
  job runner (a deadline or ceiling). `Drop` still kills and joins both threads.
- **Job runner** (`crates/bridge/src/roto_job.rs`, added 2026-10-30, run `41144133581`): `run_job`
  validates a `RotoJobSpec` *before* sending anything (inverted or negative range, more than
  `max_frames`, zero or >100-megapixel working size, empty paths, empty model name, non-hex or
  wrong-length model SHA-256), then drives `propagate` under a stall watchdog, an overall budget and
  a cancel-acknowledgement wait. It polls the cancel flag every 50 ms, rejects a `total` that does not
  match the request or a progress frame outside the range, caps events at 4 per frame + 64, and kills
  the worker on a stall, a deadline or an unacknowledged cancel. A cancel that had to kill reports
  `killed: true`, so a killed job is never presented as a clean pass.
- **Memory ceiling** (added 2026-10-31, run `41255268402`): `progress` gained an optional `rss_mb`, and
  `RotoJobLimits::memory_ceiling_mb` (default 4 GiB, `None` disables) kills a run whose reported
  resident set exceeds it. The `on_progress` callback carries the worker's own figure, so a panel can
  show it and the runner never invents one. A worker that reports nothing cannot be ceiling-enforced —
  the limits' documentation and the docs say so rather than implying otherwise.
- **Tests** (`crates/bridge/tests/r2_roto_worker.rs`, 12): a real PowerShell fake worker completes
  ready → progress → done → cancel → shutdown over pipes; a quiet worker yields a `Timeout` inside
  the deadline; a silent worker times out during the handshake and is killed; the job runner drives a
  fake worker to `Done` with progress in media order; it refuses bad requests without disturbing the
  worker (a valid job still runs afterwards); a stalling worker is reported as a stall and killed
  inside the deadline; a worker that ignores `cancel` is killed and reported as `Cancelled { killed:
  true }`; a worker that reports an enormous resident set is killed with `MemoryCeiling` naming both
  numbers; the same worker finishes when no ceiling is set; the progress callback carries the worker's
  own memory figure; the exporter writes scaled PNGs in media order; an empty range is rejected. The
  fake workers are not XMem and the tests say so.
- **One honest CI failure and its fix**: run `41146756706` failed three of these tests on a docs-only
  commit because nine parallel PowerShell starts starved each other and one `hello` handshake missed a
  20 s deadline. The fixtures now start under a lock (one interpreter at a time) with a 60 s
  failure bound (run `6bcbf9b`/`41149355637` is green), so the tests bound failure without racing each
  other's process start.
- **A second straggler and its fix** (run `42235380722`): one interpreter still took longer than the
  60 s bound while the fixtures queued behind the same lock passed, so a slow start was reported as
  `Timeout("no event within 60.0s")`. `spawn_ready` now attempts the start once more after killing
  the first worker (`ready_or_retry`); the deadline still bounds a fixture that is genuinely broken,
  and the product's own timeout behaviour is covered separately by the silent-worker test.
- **Measured limits**: the runner exists and is tested, but nothing yet starts a worker from the app
  (no runtime discovery/consent flow, which is R5) and `request_shutdown` still sends a request
  (teardown kills the child). No Python runtime or weights ship in the app; `tools/` holds the local
  worker/probe (not part of the Rust build); initial-mask UI, job queue and UI wiring are still
  absent.
- **Size**: artifact `frameforge-pro-windows-dev` is **25,512,804 bytes** ZIP (R1 was 25,418,672;
  M12 25,229,770); the deadline change is 25,842,162 bytes and the job runner 25,840,962 bytes; no
  new external crate.

## R3 — real XMem worker probe (local evidence, not CI; 2026-10-27)

`tools/xmem_worker.py` is the protocol-1 adapter over the official MIT XMem
network/inference core. It imports the model from an external `--xmem-source`
checkout, verifies the caller's SHA-256 before loading the user-supplied weights,
propagates forward or backward, writes `frame_<index>.png` masks, reports progress
and honors a mid-job `cancel`. `tools/xmem_r3_probe.py` builds a moving-rectangle
clip, drives the worker over pipes and computes IoU.

- **Local run**: PowerShell on this Windows box:
  `py -3.11 tools/xmem_r3_probe.py --xmem-source C:\Users\augus\Downloads\XMem-official --weights C:\Users\augus\Downloads\XMem.pth`.
  Runtime: torch 2.14.1+cpu, XMem commit `f3b841d`, weights SHA-256
  `27776291d2f0639b4e6b372a67651579b51180aa4d8f8f89bbff2dcc09ebf6f9`,
  AMD Ryzen 5 7530U (the target laptop).
- **160x96, 12 frames**: forward mean IoU **0.9539** (min 0.9231), backward mean
  **0.9512** (min 0.8700); wall 10.6 s / 4.8 s including model load, reported
  inference 5.09 s / 4.06 s.
- **480x270, 6 frames** (`--width 480 --height 270 --measure-memory`): forward
  mean IoU **0.9883** (min 0.9796), backward mean **0.9945** (min 0.9912); wall
  7.3 s / 4.8 s, reported inference 4.31 s / 4.40 s, peak worker RSS **710.2 MB**
  / **610.7 MB** (the forward peak includes model load; the backward peak is
  after it).
- **Cancel**: a 30-frame 480x270 job cancelled after the first progress event
  emitted `cancelled` 1.05 s later and stopped after 2 written masks.
- **Connected since**: the manifest writer now exists (`MaskCache::adopt`) and is exercised end to end
  — a worker's masks are published and drive the composed picture and the export (run `41255268402`) —
  and the ceiling is enforced on the worker's own reported RSS. What is still **not met**: no job
  queue (one job at a time), no resume/segmented correction runs, a ceiling that cannot be enforced on
  a worker that reports nothing, and no CI coverage of real inference (CI has no torch or weights), so
  this local evidence is not an R3 acceptance pass; R3 and R4 remain open. The tools-only commit
  `23d03da` ran CI green (run `40517500476`, 54 tests, 25,564,165-byte artifact), confirming the Rust
  tree was unchanged.

## R5 — consent gate and runtime/weights verification (partial; bridge, run 41161529133)

R5 says the app must never silently download a runtime or weights. Two bridge modules now hold that
policy without any new dependency.

- **`crates/bridge/src/roto_runtime.rs`**: `RotoRuntime::resolve` checks, in order, that
  `RotoConsent::accepted` is true (nothing is read before this — the test passes non-existent paths
  and still gets `NotConsented`), that the interpreter, worker script and weights exist, that the
  checkout is at the pinned commit `f3b841d…`, and that the weights SHA-256 equals the caller's
  expectation. The commit is read **without running git**: loose ref, `packed-refs`, `gitdir:` file
  and detached HEAD are all handled, and the checkout must contain `model/`. `python_candidates` looks
  for `FRAMEFORGE_PYTHON` first, then `python.exe`/`py.exe`/`python3.exe` on `PATH` — it looks for
  files, it never runs an interpreter to ask its version. `RotoRuntime::worker_command` returns
  `python -u <script>` (unbuffered, so progress arrives during the job) and `describe()` renders a
  status line naming the commit, the weights and the consent note.
- **`crates/bridge/src/sha256.rs`**: a self-contained FIPS 180-4 SHA-256 (streaming, 64 KiB file
  reads) with the published test vectors, so verifying weights added **no crate and no licence**.
  It is a content check, not authentication, and the module says so.
- **Tests**: `crates/bridge/tests/r5_runtime.rs` (9) covers a verified resolve and its description,
  the three ref layouts reading the same commit, consent-before-any-read, a weights hash mismatch
  (reporting both hashes), a checkout at another commit (both ids), a directory without `model/`,
  a malformed expected hash, the consent note reaching the status line, and the interpreter candidate
  order. `sha256.rs` carries three unit tests (four FIPS vectors, split updates, a file digest).
- **Still open**: no packaging of `tools/xmem_worker.py` for a shipped build (the panel takes its path),
  no in-app deletion or reuse of a verified runtime, no saved consent record between sessions, no
  memory ceiling, and no evidence yet that the masks a job writes are the bytes preview/export read.
  The paths and the consent are collected by the Roto AI panel (run `41238338683`). R5 is therefore
  **partial**, and nothing in the UI claims otherwise.

## R4 — the in-app Roto AI panel (partial; run 41238338683)

The Masking workspace hosts a real XMem panel (`apps/frameforge-pro/src/roto.rs`); **Help ▸ Roto AI
Status** already switches there. It is a single-clip, single-pass slice: no brush, no corrections.

- **Configuration**: interpreter, XMem checkout, weights, worker script and the expected SHA-256,
  each with a browse button; when the interpreter field is empty the panel offers the first result of
  `python_candidates_from_env` rather than guessing silently.
- **Consent**: a check box plus an optional note. Nothing is read until it is ticked — *Verify
  runtime* is `RotoRuntime::resolve`, which shows either `describe()` (commit, weights, consent note)
  or the exact refusal (missing field, wrong commit, hash mismatch).
- **The job**: needs a selected clip and an initial-mask PNG. The panel computes the clip's media
  frame range with `frame_range` (the same arithmetic `export_frames` uses), then on a
  `frameforge-roto-job` thread it exports the frames, spawns `python -u <script>`, handshakes with a
  120 s deadline, and drives `run_job` under the default limits, reporting each progress event
  (frames done, frame number, worker fps). **Cancel** sets the runner's flag; a worker that will not
  acknowledge is killed, and the panel says so. The result names the frames, seconds and output
  directory it actually produced.
- **Cross-check**: after exporting, the thread compares `exported.first_frame/last_frame` with the
  numbers it asked the worker for and fails the job if they differ, so a numbering drift can never be
  reported as a pass.
- **Never hangs**: a thread that stopped without writing a result (a panic) is reported as such, so
  the panel cannot sit on "running" forever.
- **The brush** (added 2026-10-31, run `41255268402`): an in-panel canvas at the working size with
  paint/erase, a round brush radius, a drag rectangle, clear and invert, a live coverage percentage,
  and *Save as PNG…* which writes exactly the 8-bit grayscale plane the worker reads as frame 0. The
  canvas is cleared whenever the working size changes, so a mask painted at one size is never reused
  at another. **Limit**: the canvas has no frame backdrop — the user paints without seeing the picture
  underneath (the clip's frames are exported later, for the worker).
- **Publishing and attaching**: after a complete run the thread validates and publishes the masks into
  the R1 cache (`MaskCache::adopt`) and reports the key; *Use this matte on the clip* then sets
  `Composition.roto` as one undoable click. A short or mis-sized run publishes nothing and the failure
  names the frame.
- **The ceiling in the panel**: every progress line shows the worker's reported RSS against the
  ceiling (4 GiB by default), and an over-ceiling run is killed by the runner with both numbers in the
  report.
- **Not met**: no corrections/resume runs, no job queue, and the ceiling is only as good as the
  worker's own reporting. Nothing is downloaded or bundled; an interpreter, a checkout and weights are
  the user's.
- **Tests**: the app tests cover the pure parts (request validation naming the missing field, consent
  refusal/acceptance, frame-range arithmetic and cache identity, canvas painting/erasing/inverting, a
  rectangle fill, a saved canvas round-tripping as the frame-0 PNG, and a resize clearing stale
  paint); the panel itself is not UI-automated.
- **CI**: run `41255268402` — fmt, clippy `-D warnings`, check, **87 tests**, release build, binary
  self-test including `roto-bench 4`; artifact `frameforge-pro-windows-dev` is 26,040,820 bytes.

## R7 — the multi-model rotoscoping engine (partial; `crates/roto`)

XMem is no longer the only tracking engine and is no longer the default:
`crates/roto` (`frameforge-roto`) is a model-independent engine — selection
prompts, tracking backends (Cutie preferred, SAMURAI optional, XMem legacy
fallback), model-independent temporal quality control, deterministic matting and
refinement, a multi-object mask cache, a job queue with pause/resume/cancel,
mask/alpha/RGBA export, the model catalogue and manager, and hardware *reporting*
that never claims an unverified provider.

The full module map, the verified licence and size table for every model, what is
implemented versus what still needs a real runtime, and the ten-point report live
in **[`ROTO_ENGINE.md`](ROTO_ENGINE.md)**. Two rules are structural, not promises:

* no model inference runs anywhere in this repository's build or tests — backend
  adapters *plan* a run (command, protocol, weights) and stay dormant until the
  workspace asks, and the adapter tests use a probe that finds nothing;
* SAM 3 and SAM 3.1 appear nowhere, and a catalogue test fails the build if they
  ever do.

The XMem path documented in R1–R5 above is unchanged and remains the one backend
with a real, user-installed runtime.

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
| M10 | source-patched multi-track editing: per-track target/source-patch headers on the engine's `timeline.setTargeting`, placement on patched V2/A2, audio-only and unpatched-destination tests | ✔ (CI-verified, run 39887395556) |
| M11 | one chronological undo history: composition snapshots paired with project-neutral engine steps, gesture coalescing, limit-aligned redo, composition rollback on failure, seven history tests plus an App-level Ctrl+Z test | ✔ (CI-verified, run 40037973347) |
| M12 | export safety and close guard: overwrite confirmation on the exact resolved path, real cancellation through `Progress.cancel` with partial-file cleanup, and a Save/Discard/Cancel guard on the window close request | ✔ (CI-verified, run 40045638522) |
| R1 | cached roto masks: validated cache key + manifest/PNG reader, alpha in `ComposedSource`, schema 3, preview + H.264 export test and cache-invalidation test | ✔ (CI-verified, run 40056112214) |
| R2 | XMem worker transport: JSON-lines protocol + child-process client, frame exporter, `roto-bench` command; fake-worker round-trip and PNG export tests; event channel with `recv_timeout`/`handshake_timeout`/`kill`, the tested `run_job` deadline/stall/cancel runner and the reported-RSS memory ceiling | ✔ (CI-verified, run 40059782293; deadlines run 40814011414; job runner run 41144133581; memory ceiling run 41255268402; XMem CPU benchmark still open) |
| R3 (partial) | Real XMem protocol worker and local probe: forward/backward IoU, cancel and peak RSS on the target Ryzen 5 7530U | 🟡 Local evidence only; resume/corrections and a job queue remain open. The bridge's deadline/stall/cancel policy is `run_job`, the ceiling is enforced on reported RSS, and a worker's masks are published into the R1 cache and drive the composed picture (runs `41144133581`, `41255268402`) |
| R4 (partial) | In-app Roto AI panel: runtime fields, consent, verification, one clip job through `export_frames` → `XmemWorker` → `run_job`, an initial-mask brush, publishing into the R1 cache and an undoable attach to the clip | 🟡 CI-verified, run `41255268402`; still no corrections/resume, no job queue, and the mask canvas has no frame backdrop to paint over |
| R5 (partial) | Consent gate and runtime/weights verification: explicit consent recorded before any read, pinned-commit checkout check without running git, streamed SHA-256 weights check, `python -u` worker command, interpreter candidates | 🟡 Bridge only (CI-verified, run 41161529133); no UI to collect paths/consent, no worker-script packaging, no memory ceiling |
| T1 | timeline track state: real `timeline.setTrack` visibility/mute/solo/lock header buttons with undo, plus the existing targeting/source-patch controls | ✔ (CI-verified, run 40063686209) |
| T2 | sequence markers: ruler drawing, click-to-jump, `markers.add` / `markers.clearCurrent` undoable add & clear, previous/next navigation and the M shortcut; lazy audio-device open | ✔ (CI-verified, run 40128137965) |
| T3 | snapping: a toolbar Snap toggle on `sequence.snap`; moves snap their start or end, trims snap the edge, to zero / the playhead / every other clip edge within 8 screen pixels; pure-function tests | ✔ (CI-verified, run 40133164791) |
| T4 | insert / overwrite: bin-item selection, Insert and Overwrite at the playhead through the engine's `insert` flag, undoable; test drives a real App | ✔ (CI-verified, run 40138769007) |
| T5 | Source monitor: real `render_item` preview, source timecode/transport/seek, engine-backed In/Out marks that persist, source Insert/Overwrite over the marked range, and the Ripple Markers toggle (default on) with boundary tests | CI-verified, run 40156687200 |
| T6 | Ripple Delete (button + Shift+Delete) through `edit.rippleDelete`: linked A/V removal, gap closing on affected/sync-locked tracks, marker removal/move policy, locked-track refusal and one-step undo | CI-verified, run 40163011141 |
| T7 | Separate Roll, Slip and Slide through the engine's `timeline.roll` / `timeline.slip` / `timeline.slide`, with trim-row buttons, Alt/Shift/Ctrl arrow nudges and model-level undo tests | CI-verified, run 40163011141 |
| T8 | Real 12-frame video cross-dissolve and audio crossfade at the selected In edge (`sequence.applyVideoTransition` / `sequence.applyAudioTransition`), with deterministic rendered-pixel and mixed-sample tests | CI-verified, run 40163011141 |
| T9 | Program In/Out marks (`markers.markIn`/`markOut`/`clearInOut`) with Lift (`sequence.lift`) and Extract (`sequence.extract`), toolbar buttons plus `;`/`'` shortcuts and a model-level linked-A/V undo test | CI-verified, run 40236814552 |
| T10 | Rename and comment the sequence marker at the playhead through a custom undoable project edit, with name/comment fields in the marker row and an undo test | CI-verified, run 40252471033 |
| T11 | Chosen transition length (1-600 frames) on Dissolve/Crossfade plus `sequence.setTransition` duration/alignment editing for the transition at the selected clip's In edge, with an apply/update/undo test | CI-verified, run 40256569182 |
| T12 | Marker CSV export with RFC 4180 quoting, exact ticks, padded start timecode and a byte-for-byte file test | CI-verified, run 40260600879 |
| T13 | Marker length (0-6000 frames) and colour editing through the existing undoable project edit, with the extended field/undo test | CI-verified, run 40513248065 |
| T14 | Source-monitor audio: the open clip's own sound through a `play_item` mix that shares the Program stream, clock and silent fallback, with the render loop, item-audio fold and advance boundary tested without a device | CI-verified, run 42429090124 |
| T15 | Source range bar and marker strip: click-to-seek, drag either In/Out handle or the whole band through the engine's `adjust_range` with one undo step per gesture, plus a marker list that adds, seeks and undoably deletes through the Source-targeted commands | CI-verified, run 42499750102 |
| T16 | Sequence In/Out ruler drag: the Program range paints as a band with an In and an Out handle, each drag frame-snaps and clamps at the other mark through a merged project edit with one undo per gesture | CI-verified, run 42506032070 |
| T17 | Split video/audio In/Out: four header chips run the engine's `markers.markSplit*` at the playhead, and channel-coloured ruler tabs drag through one merged edit, clamping at the channel's effective opposite | CI-verified, run 42527534314 |
| T18 | Source marker drag and marker CSV import: bar handles move a source marker through one merged edit (root-media target, frame-snapped, clamped), and the Markers CSV importer reads the export's RFC 4180 format as one undoable edit that aborts wholly on a bad row | CI-verified, run 42537235054 |
| T19 | Sequence marker ruler drag: every timeline marker gains a top-band grab target, and a drag frame-snaps and clamps it to the sequence through one merged edit (one undo per gesture) | CI-verified, run 42731129937 |
| T20 | Timeline marker list: a floating window (header chip or Markers menu) lists markers with their colour, timecode, name and comment; a row seeks both playheads to its marker, and its own delete removes exactly that marker through one undoable edit | CI-verified, run 42749131608 |
| T21 | Snapping: moves and trims target every sequence marker, and a linked A/V pair is left out of its own edges so it no longer snaps back onto its other half | CI-verified, run 42761241822 |
| T22 | Sequence marker EDL import: `* LOC:` locator lines from a CMX 3600 list become markers at their record timecodes (hour-relative), with the Avid colour words mapped and one undoable edit for the whole file | CI-verified, run 42834979022 |
| T23 | Speed / Duration: a floating dialog drives the engine's rate, reverse, ripple and time interpolation, and Freeze Frame holds a source frame at the clip's in point, out point or the Program playhead | CI-verified, run 42949817569 |
| T24 | Transition browser: a floating window over the engine's own `effects.list` catalog, with video/audio tabs, folder paths and a filter, applying the chosen effect, length and alignment at the selected edit and selecting or removing the transition there | CI-verified, run 43090989519 |
| T25 | Transitions on the timeline: each transition paints as a band over its cut with the selection state from the engine, a click makes it the engine's transition selection and selects the incoming clip, and an edge or body drag writes the engine-clamped span in one undo | CI-verified, run 43138585328 |
| T26 | Caption editor: a floating window over the engine's own `captions.list` that adds a track and a cue at the playhead, selects a cue through the engine's caption selection, edits its text and timing and deletes it, each write its own undo step | CI-verified, run 43369355615 |
| T27 | Caption import/export, split and merge-up: Import reads a picked caption file into a frame-snapped track as one undoable edit, Export writes the target track as SubRip or WebVTT without dirtying the project, Split at Playhead cuts the selected cue at the App playhead and Merge Up folds it into the previous cue on its track | CI-verified, run 43439895270 |
| T28 | Caption tracks on the timeline: every caption track paints a row under the audio rows with its cues as bars at their own times, selected through the engine's caption selection; a cue click is a journal-free selection that keeps an open Caption window in step | CI-verified, run 43495417982 |
| T29 | Caption cue drag and trim: a press on a cue's body moves it and a press on either 8 px edge trims it, committed by `captions.move` / `captions.trim` on release as one undo step, with a ghost preview and the cue as the journal-free caption selection | CI-verified, run 43537808604 |
| T30 | Snapping for caption gestures: caption moves try both their start and end and caption trims push the dragged edge to the nearest sequence marker, playhead or other cue edge inside the 8 px radius, reported as "(snapped)" only when the delta changed | CI-verified, run 43544459466 |
| U1 | TSX workspace port: design kit, eight-workspace shell, themed Edit layout, Text & Graphics, Masking and Compositing screens over real engine commands | CI-verified, run 40794727835 |
| U2 | UI interaction tests: an in-crate harness that runs the real `App::ui_frame` through `egui::Context::run_ui` with real `RawInput` key events, covering undo/redo, Add Edit and markers | CI-verified, run 42340371967 |
| U3 | Scripted pointer gestures: a stable `ff-timeline-body` id and the draw loop's own `timeline_clip_rect` drive a ruler scrub, clip select/delete, a clip move and both edge trims through real pointer events, asserted on the engine document with undo | CI-verified, runs 42401469221 and 42406648700 |
| E1 | Export presets and range selection: four named presets that apply format/bitrate/audio together, and an explicit whole-sequence vs In/Out range choice | CI-verified, run 42391495128 |
| E2 | Export queue: snapshot jobs behind the foreground render, driven from the frame loop, with duplicate-path refusal, per-row removal and a finished report | CI-verified, run 42395055258 |
| M13 | Relink missing media: a journal-free offline scan per project open, an OFFLINE badge in the media browser, and a floating Link Media dialog over the engine's `media.relink` (path, Force, folder search), with scripted click tests | CI-verified, run 42419874435 |
| R7 | Multi-model rotoscoping engine: `crates/roto` (selection, tracking backends with Cutie preferred and XMem legacy, temporal QC, matting, refinement, cache, scheduler, export), model catalogue/manager and the workspace panel | 🟡 Partial: everything deterministic is tested, no model is wired to a real runtime yet. See `ROTO_ENGINE.md` |

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

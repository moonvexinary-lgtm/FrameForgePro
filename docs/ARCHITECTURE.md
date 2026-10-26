# FrameForge Pro — Architecture

Last updated: T9 (2026-10-25) · Branch: `frameforge-rust-rewrite`

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
  `40133164791`), T4 adds one, T5 adds four, T6 adds one, T7 adds two, T8 adds two and T9 adds one, so the current suite is 51 (run `40236814552`).
- **Limitations**: native dialogs and the viewport command are code-inspected,
  not UI-automated; export remains one foreground job (no queue/presets); cancel
  deletes the partial output rather than resuming.

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
- **Limitations**: no marker rename/comment editor or marker drag on the ruler;
  marker colours are a fixed UI mapping; no CSV/EDL export wiring yet.

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
  In/Out controls. Playback is a **video-only wall clock**: the audio clock is
  bound to the Program sequence, so source playback is silent and labelled so.
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
- **Limitations**: source playback has no audio; there is no source-marker panel
  or source range drag; the Program monitor still owns In/Out for the sequence
  (sequence in/out UI is still open).

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

## R1 — cached roto masks in the one render path (✔ CI-verified, run 40056112214)

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
- **Tests** (`crates/bridge/tests/r1_roto.rs`, 5): exact alpha inside coverage and pass-through
  outside, missing in-range frame error, every key input changes the address, a correction edit misses
  the old cache, and a two-track project renders + exports the background through the transparent
  half. No new crate dependency: `image` was already a workspace dependency of the app; the bridge now
  uses it too.
- **Limitations**: mask and frame sizes must match (no proxy stretching; R3 regenerates at the working
  size); decoded masks are not held in an in-memory LRU; no mask writer, initial-mask UI or XMem
  inference yet.

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
  Bars & Tone. CI runs it with 4 frames. It says "XMem CPU inference: not measured" because the
  optional runtime and consented weights are absent; no inference number is invented.
- **Tests** (`crates/bridge/tests/r2_roto_worker.rs`, 3): a real PowerShell fake worker completes
  ready → progress → done → cancel → shutdown over pipes; the exporter writes scaled PNGs in media
  order; an empty range is rejected. The fake worker is not XMem and the test says so.
- **Measured limits**: the client is blocking; a hung worker needs the R3 job runner's timeout/kill
  policy. `request_shutdown` sends the request and teardown kills the child. No Python, weights, real
  worker script, job queue, initial-mask UI or UI wiring ships in R2.
- **Size**: artifact `frameforge-pro-windows-dev` is **25,512,804 bytes** ZIP (R1 was 25,418,672;
  M12 25,229,770); no new external crate.

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
| R2 | XMem worker transport: JSON-lines protocol + child-process client, frame exporter, `roto-bench` command; fake-worker round-trip and PNG export tests | ✔ (CI-verified, run 40059782293; XMem CPU benchmark still open) |
| T1 | timeline track state: real `timeline.setTrack` visibility/mute/solo/lock header buttons with undo, plus the existing targeting/source-patch controls | ✔ (CI-verified, run 40063686209) |
| T2 | sequence markers: ruler drawing, click-to-jump, `markers.add` / `markers.clearCurrent` undoable add & clear, previous/next navigation and the M shortcut; lazy audio-device open | ✔ (CI-verified, run 40128137965) |
| T3 | snapping: a toolbar Snap toggle on `sequence.snap`; moves snap their start or end, trims snap the edge, to zero / the playhead / every other clip edge within 8 screen pixels; pure-function tests | ✔ (CI-verified, run 40133164791) |
| T4 | insert / overwrite: bin-item selection, Insert and Overwrite at the playhead through the engine's `insert` flag, undoable; test drives a real App | ✔ (CI-verified, run 40138769007) |
| T5 | Source monitor: real `render_item` preview, source timecode/transport/seek, engine-backed In/Out marks that persist, source Insert/Overwrite over the marked range, and the Ripple Markers toggle (default on) with boundary tests | CI-verified, run 40156687200 |
| T6 | Ripple Delete (button + Shift+Delete) through `edit.rippleDelete`: linked A/V removal, gap closing on affected/sync-locked tracks, marker removal/move policy, locked-track refusal and one-step undo | CI-verified, run 40163011141 |
| T7 | Separate Roll, Slip and Slide through the engine's `timeline.roll` / `timeline.slip` / `timeline.slide`, with trim-row buttons, Alt/Shift/Ctrl arrow nudges and model-level undo tests | CI-verified, run 40163011141 |
| T8 | Real 12-frame video cross-dissolve and audio crossfade at the selected In edge (`sequence.applyVideoTransition` / `sequence.applyAudioTransition`), with deterministic rendered-pixel and mixed-sample tests | CI-verified, run 40163011141 |
| T9 | Program In/Out marks (`markers.markIn`/`markOut`/`clearInOut`) with Lift (`sequence.lift`) and Extract (`sequence.extract`), toolbar buttons plus `;`/`'` shortcuts and a model-level linked-A/V undo test | CI-verified, run 40236814552 |

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

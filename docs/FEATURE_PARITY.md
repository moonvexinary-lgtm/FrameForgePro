# Feature parity and status

Status legend:

- **✅ verified** — real code, and an automated test or CI measurement exercises that exact path
  (a bridge test, a pinned upstream test that FrameForge calls, or a measured CI run). The evidence
  is named in the row.
- **🟡 partially implemented / code-inspected** — real code for part of the capability, sometimes
  with tests for that slice, but the capability is incomplete or no automated test drives the full
  layer. This is not a claim of production readiness.
- **⬜ not implemented.**

No row is marked ✅ on the strength of a screenshot, a build succeeding, or documentation. Tests run
on GitHub Actions (`windows-2025`); the measured baseline is `docs/BASELINE_AUDIT.md`.

## P0 — reliable editing, playback, export

| Capability | Status | Evidence / remaining work | Completion criterion |
|---|---|---|---|
| New / open / save / save-as (`.ffproj`) | ✅ | Atomic save with round-trip validation; M2 reopens and renders byte-identical frames; M9 saves and reloads | — |
| Versioned project schema + refusal of newer files | ✅ | M9 `newer_schema_is_refused` (schema 999 is a named error) | — |
| Crash recovery / autosave | ✅ | M9 `autosave_round_trip_newer_than_and_clear`, `corrupt_autosave_is_a_clear_error_not_a_crash`; the shell's 20 s timer calls exactly this module | — (window close is guarded in M12) |
| Window close with unsaved changes | ✅ | M12 `app::tests::close_guard_blocks_only_a_dirty_project_when_discard_is_denied` covers the decision table; `App::ui` sends `ViewportCommand::CancelClose`; the dialog offers Save / Discard / Cancel | — (native dialogs are code-inspected) |
| Undo / redo of timeline edits | ✅ | M11 `history.rs` executes `Session` edits, undoes/redoes them through `EditHistory`, and asserts the project name and document steps; `app::tests::ctrl_z_after_a_composition_edit_leaves_the_timeline_edit_alone` drives the shell's `undo`/`redo` | — (UI automation is a separate gap) |
| Undo / redo of effect & composition edits | ✅ | M11 `history.rs`: composition undo/redo shares the timeline's order, one drag is one step, a new edit clears redo, limits stay aligned; the App-level test asserts the composition map after undo/redo; the egui gesture bracket is code-inspected | — (UI automation is a separate gap) |
| Media import (generators, files) | ✅ | M3 fixture export → re-import through `file.import`/codecs → render | — |
| Media bin folders / tags / search / metadata | ⬜ | Flat list today | Bin model + UI, persisted in the project |
| Thumbnails / audio waveforms | ⬜ | — | Generated off the UI thread, cached, bounded |
| Relink missing media | 🟡 | `FrameForgeProject::relink` and the offline slate are tested (M3); no UI dialog | Relink dialog + offline badge |
| Multi-track timeline (2V + 2A) | ✅ | Source-patched placement on V2/A2 through `timeline.place`; `edit::tests::placement_uses_the_patched_video_and_audio_tracks` executes the real engine and asserts both destination tracks | — (UI automation is a separate gap) |
| Track targeting / source patching (choose V2/A2) | ✅ | Timeline headers expose the engine's real targeting (T) and source patch (S) through `timeline.setTargeting`; placement follows the patched tracks; audio-only and unpatched cases tested in `edit.rs` | — |
| Track mute / solo / lock / visibility in the timeline | 🟡 | T1 (run `40063686209`): header buttons call the real `timeline.setTrack` keys (`enabled`, `muted`, `solo`, `locked`) through the same helper `edit::tests::track_header_visibility_mute_solo_and_lock_are_undoable_set_track_edits` drives; the test asserts the sequence state and undo. UI click-through and a render-difference test are not automated yet | Scripted UI click + hidden/muted/solo render test |
| Select / delete / razor / drag-move / trim | 🟡 | Engine commands and their upstream tests; M7 shell gestures; no FrameForge gesture test | A scripted edit test per gesture |
| Insert / overwrite | 🟡 | T4 (run `40138769007`): a bin selection plus timeline Insert/Overwrite buttons drive `timeline.place`'s real `insert` flag, checked by `edit::tests::insert_and_overwrite_at_the_playhead_use_the_real_engine_insert_flag`. T5 adds the Source panel's Insert/Overwrite over the marked source range | Source in/out drag, source monitor audio |
| Ripple delete, roll, slip, slide | 🟡 | T6 (run `40163011141`): Shift+Delete and the timeline Ripple Delete button run `edit.rippleDelete` with linked A/V, affected/sync-locked gap closing, the marker policy, a locked-track refusal and one-step undo. T7: the Trim row and Alt/Shift/Ctrl arrow nudges run `timeline.roll` / `timeline.slip` / `timeline.slide`; tests assert exact 12-frame results and undo | Ripple targeting controls, drag gestures, arbitrary edit-point picking |
| Snapping | 🟡 | T3 (run `40133164791`): a timeline Snap checkbox drives the engine's `sequence.snap` state; moves snap start/end and trims snap the edge to zero, the playhead or another clip edge (8 px radius); `edit::tests::snap_tick_*` and `snap_targets_*` cover the maths. Markers are not targets yet; snapping is not linked-pair aware | Marker targets, linked-pair snapping, ripple-aware snapping |
| Source monitor + source In/Out marks | 🟡 | T5: separate Program/Source monitors, `render_item` previews, source timecode/transport/seek, engine-backed In/Out marks (I/O, Shift+I/O, go/clear) that persist on the media item, and the source-range Insert/Overwrite; `edit::tests::source_*` cover rendering, independence, range normalization, marker ripple and save/reopen. Source playback is video-only/silent | Source-marker panel, source range drag, source audio |
| Sequence in / out ranges | 🟡 | T9 (run `40236814552`): Mark In/Mark Out/Clear I/O plus Lift (`;`) and Extract (`'`) run the engine's `markers.markIn`/`markOut`/`clearInOut` and `sequence.lift`/`extract`; with no source clip open, `I`/`O` mark the sequence. Lift keeps the gap, Extract closes it on targeted + sync-locked tracks, ripples markers, clears the marks and parks the playhead. The linked-A/V undo test drives a real three-clip sequence. No ruler drag or split In/Out yet | Ruler In/Out drag, split (video/audio) In/Out, In/Out-aware export ranges wired in the UI |
| Sequence markers | 🟡 | T2: markers drawn on the ruler, click-to-jump, undoable Add/Clear through `markers.add` / `markers.clearCurrent`, Prev/Next navigation and the M shortcut; `edit::tests::marker_helpers_add_clear_and_navigate_through_the_real_engine` covers the real engine. T10 (run `40252471033`): marker name and comment fields plus a Rename Marker button edit the marker at the playhead through a custom undoable project edit by `MarkerId`; `edit::tests::marker_rename_is_an_undoable_project_edit` covers rename, comment, undo and the no-marker guard. No marker drag, colour/duration editing or CSV export yet | Marker drag, colour/duration editing, CSV/EDL export, marker-list selection |
| Speed / freeze / reverse | ⬜ | — | Engine command + verified timing |
| Transitions (cross-dissolve etc.) | 🟡 | T8 (run `40163011141`): Dissolve/Crossfade (Ctrl/Cmd+D, Ctrl/Cmd+Shift+D) apply a real 12-frame `cross_dissolve` / `constant_power` transition at the selected clip's In edge, resolving a linked partner; tests sample rendered pixels and mixed audio samples at the cut. Each transition is one undo step. No browser, custom duration or alignment UI yet | Transition browser, custom duration/alignment, handles, audio transition variants |
| Program monitor | ✅ | M2 renders through the same compositor export uses; panel code-inspected | — (UI automation is a separate gap) |
| Preview quality / proxy / cache | ⬜ | Full-resolution CPU preview only | Half/quarter preview, proxy workflow, bounded cache, bypass |
| Exact time / fractional rates | ✅ | `frameforge-core` verifies 11 broadcast rates, sample and container math | — |
| A/V sync | 🟡 | Playhead is the audio device's consumed frames; wall-clock fallback is labelled; no sync measurement test | A drift test over a mixed sequence |
| Export H.264 + AAC (MP4) | ✅ | M2 re-decodes the export and verifies resolution, rate, duration, pixels and audio | — |
| Export ProRes / DNxHR / MJPEG / WAV | 🟡 | Real encoders offered; only H.264 is decode-verified in CI | Per-format decode test |
| Export HEVC / AV1 / alpha | ⬜ | Needs a registered encoder; not offered | Capability-detected hardware or tested software path |
| Export overwrite confirmation | ✅ | M12 `export::tests::output_path_adds_the_selected_extension_once` resolves the exact name; `start` asks Yes/No when it exists, and No keeps the existing file and starts nothing | — |
| Export cancel | ✅ | M12 Cancel stores FilmCraft's `Progress.cancel`; the worker returns `ExportError::Cancelled`; `export::tests::a_cancelled_export_is_reported_as_a_cancellation` checks the sentinel; the truncated partial file is removed | — (UI automation is separate) |
| Export queue / presets / range selection | ⬜ | One foreground job at a time | Queue, presets, range |
| Hardware encode/decode | ⬜ | Not assumed on AMD iGPU | Detect, test, fall back |

## P1 — effects, color, compositing, audio

| Capability | Status | Evidence / remaining work | Completion criterion |
|---|---|---|---|
| Unified effect registry (both engines) | ✅ | `catalog()`; merged count asserted; duplicate audit (M3) | — |
| Searchable effects browser | 🟡 | Effects workspace filtering over the catalog; no UI automation | Category filter + both engines in one browser, tested |
| Real processing through preview and export | ✅ | 307/307 EffectCraft effects run through `ComposedSource`; M2 verifies keyframed blur in the exported file | Golden frames for every promoted effect |
| Typed parameters, defaults, ranges | ✅ | Bridge `ParamInfo`/`EffectParamUi`, asserted by the catalog test | — |
| Parameter UI (slider, number, %, angle, px, bool, popup, color) | 🟡 | Real widgets writing the document; no UI automation | An interaction test at the document level per control type |
| Parameter UI for text / curves / wheel / path | ⬜ | Shown as "not editable yet" — never faked | Editors for these types |
| Keyframes: scalar | 🟡 | M2 proves keyframed evaluation through render/export; the stopwatch/key-list UI is code-inspected | A UI-driven keyframe test |
| Keyframes: bool / color / choice | ⬜ | Only scalar keys today | Same keyframe UI for all animatable types |
| Keyframe-preserving parameter writes | ✅ | M9 `scalar_writes_preserve_keyframes`; the shell calls `Composition::set_scalar`; all writes now mark the monitor dirty | — |
| Effect enable / bypass | 🟡 | Per-effect checkbox and `ComposedSource`'s enabled filter; no dedicated render test | A frame-identical bypass test |
| Effect reorder / duplicate / reset / presets | ⬜ | Add/remove/toggle only | Real serialized presets; reorder with render-order tests |
| Deterministic chain evaluation | ✅ | M2: direction independence + identical reopen; M9 save/load | — |
| Golden-frame test coverage | 🟡 | Gaussian blur (source + export), keyframed text, relink, comp layers | Golden frames per promoted effect and transition |
| Color workspace (FilmCraft stack) | 🟡 | `effects.apply/setParam/toggleEnabled/remove` with upstream tests; panel code-inspected | A FrameForge test driving the same commands |
| Color scopes / curves / wheels / LUT / before-after | ⬜ | — | Scopes computed from frames; LUT loading with licence records |
| Compositing comp layers (text/solid add, toggle, remove) | 🟡 | Bridge `Composition` helpers over the project the M4 test renders; helpers themselves are code-inspected | A test of add/toggle/remove and their effect on pixels |
| Masks / chroma key / matte UI | ⬜ | Engine effects exist in the registry; no mask UI or chroma render test | Mask authoring + alpha preview + render test |
| Audio mixer faders / pan / mute / solo | 🟡 | `mixer.setStrip` with parameter names verified against upstream tests; panel code-inspected | A FrameForge test asserting the mixed result |
| Audio meters / automation / inserts / loudness | ⬜ | — | Peak meters from real audio; automation lanes; effect inserts |
| Audio effects UI | ⬜ | FilmCraft audio-effect defs exist | Add/param UI with audio render checks |

## P2 — professional workflow and AI

| Capability | Status | Notes |
|---|---|---|
| Adjustment clips / nested sequences | ⬜ | Engine has concepts; no shell workflow |
| Titles / graphics workspace | 🟡 | Comp text layer can be added; text content/size are not editable in the UI |
| Templates / preset manager | ⬜ | Requires presets first |
| Multicam | ⬜ | — |
| Captions / subtitles | ⬜ | FilmCraft has caption support; not exposed |
| OTIO / EDL interchange | ⬜ | Licence-safe interchange target |
| Plugin host (OpenFX) | ⬜ | Evaluate only after the effect architecture is stable |
| Motion tracking / planar tracking / stabilization | ⬜ | — |
| **Roto AI (XMem)** | 🟡 | R1 cache + alpha seam and R2 worker protocol/exporter are implemented and CI-verified (runs `40056112214`, `40059782293`); real XMem inference, initial-mask UI, corrections and the workspace are **not** implemented. See `docs/ROTO_AI_PLAN.md` |
| AI pack (weights, runtime) | ⬜ | Optional, consent-based download only; no pack or weights are bundled or installed |
| Project archive / collect media | ⬜ | — |
| Installer | ⬜ | Tag builds a ZIP; an installer is future work |

## UI/UX acceptance checklist (from the UI brief)

| Item | Status | Evidence |
|---|---|---|
| Builds and launches on Windows 11 x64 | ✅ | CI self-test + the delivered exe ran on the target laptop |
| Workspace switcher changes real layouts, preserving project state | 🟡 | Real workspaces over one `Session`; no UI automation |
| Panels resize | 🟡 | Draggable gutters; code-inspected |
| Layout saved/restored, dockable | ⬜ | — |
| Media import → preview → timeline edit → effect → save/reopen → export | ✅ | The pipeline is exactly M2/M3/M9; the shell calls the same code, click-through not automated |
| Visible effect parameters change preview and export | 🟡 | Monitor-cache invalidation fixed in M9; no UI automation |
| XMem Roto AI controls wired to a real worker | ⬜ | — |
| Mask cache reusable in compositing/export, invalidates on edits | ✅ | R1 `r1_roto.rs`: a synthetic matte changes preview **and** H.264 export; every cache-key input (including ordered correction ids) addresses a different directory; a missing in-range mask is a clear error. No mask UI yet |
| No placeholder buttons / fake progress | ✅ | M9 audit inspected every enabled control |
| Unit / integration / render tests pass | ✅ | Run `40252471033`: **52 tests** (M2/M3/M4/M9 durability, M11 history, R1 roto, R2 worker/exporter, T1 track state, T2 markers, T3 snapping, T4 insert/overwrite, T5 source monitor, T6-T8 edit/transition, T9 In/Out, T10 marker edit); CI on windows-2025 |
| CI on Windows; failures documented | ✅ | `ci.yml` + `ci-diagnostics` branch |
| Licences, sizes, limitations documented | ✅ | `NOTICE`, `docs/PROVENANCE.md`, this file; AI pack sizes pending |

## Test matrix gaps (honest)

- No UI automation or screenshot regression yet; UI behaviour is verified by code inspection and the
  underlying document/render tests, not by driving the window.
- No stress test for hundreds of clips/effects on a long timeline.
- No fractional-rate *project* round-trip test through the shell (the foundation verifies the math).
- No non-ASCII / spaces-in-path test yet; no missing-media UI test.
- No 1080p editing/playback/export benchmark on the target laptop yet; CI timings are not target
  numbers.
- No XMem CPU inference benchmark on the target laptop yet. `roto-bench` measures only the Rust
  decode/scale/PNG path and prints "XMem CPU inference: not measured"; the real measurement needs the
  R3/R5 runtime and weights. No fabricated numbers will be reported.

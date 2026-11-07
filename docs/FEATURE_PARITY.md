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
| Undo / redo of timeline edits | ✅ | M11 `history.rs` executes `Session` edits, undoes/redoes them through `EditHistory`, and asserts the project name and document steps; `app::tests::ctrl_z_after_a_composition_edit_leaves_the_timeline_edit_alone` drives the shell's `undo`/`redo`; `ui_tests::ctrl_z_and_ctrl_shift_z_undo_and_redo_an_engine_edit` sends the real Ctrl+Z / Ctrl+Shift+Z keys through the frame loop and asserts the clip is gone, the media item survives (so the history reached the placement) and redo restores both streams (run `42340371967`) | — |
| Undo / redo of effect & composition edits | ✅ | M11 `history.rs`: composition undo/redo shares the timeline's order, one drag is one step, a new edit clears redo, limits stay aligned; the App-level test asserts the composition map after undo/redo; the egui gesture bracket is code-inspected | — (UI automation is a separate gap) |
| Media import (generators, files) | ✅ | M3 fixture export → re-import through `file.import`/codecs → render | — |
| Media bin folders / tags / search / metadata | 🟡 | FilmCraft's bin model is real and persisted: the media browser creates bins, navigates them (breadcrumb + Up), lists child bins, and moves the selected item into any bin through `Project::move_to_bin` as one undoable edit. Tags are stored in the item's `metadata["Tags"]`, edited by a comma-separated field (normalised, case-insensitively deduped), shown as a dot and a tooltip, and listed in the tag filter; search covers the name, tags and every other metadata value. Imports and new tones land in the current bin. Tests: `edit::tests::{tags_are_parsed_deduped_searchable_and_undoable, search_also_covers_other_metadata_values, bins_nest_and_moving_an_item_is_one_undo_step}`. Still open: drag-and-drop between bins, a rename dialog, saved search bins, and persisted column/metadata UI | Drag/drop, rename UI, search bins |
| Thumbnails / audio waveforms | 🟡 | `apps/frameforge-pro/src/previews.rs` reads both from the same `SharedSource` the renderer uses, on two background threads (one for pictures, one for sound), and polls them once per frame, so a slow file never stalls the UI. A thumbnail is the middle frame of the media, decoded near 160 px wide and box-averaged to at most 160x90 with the aspect kept; the browser draws it in place of the letter badge. The envelope is one peak column per 1/600 of the media (the loudest sample of any channel), and the timeline draws the trimmed span of each audio clip behind its name. The cache holds at most 128 items with a least-recently-used eviction (about 7 MB of pictures and 300 KB of envelopes), never evicts a request in flight, caches a failure so a broken file is not retried every frame (the message shows in the row tooltip), and clears when an import could reuse an undone item id. Media longer than 600 000 samples at 2 kHz is sampled with one window per column, so a three-hour file costs the same as a five-minute one. Pure helpers (`thumb_size`, `downscale`, `waveform_peaks`, `peak_index`, `sample_windows`) and the cache bound are unit-tested and run green in CI run `42288013685` (commit `e69e7c7`), which also builds the shell and passes the binary self-test; the threaded read of a real media file itself is still unexercised here (no media runtime in this environment) | Waveform zoom detail, filmstrip thumbnails along a video clip, proxy-aware decoding |
| Relink missing media | 🟡 | `FrameForgeProject::relink` and the offline slate are tested (M3); no UI dialog | Relink dialog + offline badge |
| Multi-track timeline (2V + 2A) | ✅ | Source-patched placement on V2/A2 through `timeline.place`; `edit::tests::placement_uses_the_patched_video_and_audio_tracks` executes the real engine and asserts both destination tracks | — (UI automation is a separate gap) |
| Track targeting / source patching (choose V2/A2) | ✅ | Timeline headers expose the engine's real targeting (T) and source patch (S) through `timeline.setTargeting`; placement follows the patched tracks; audio-only and unpatched cases tested in `edit.rs` | — |
| Track mute / solo / lock / visibility in the timeline | 🟡 | T1 (run `40063686209`): header buttons call the real `timeline.setTrack` keys (`enabled`, `muted`, `solo`, `locked`) through the same helper `edit::tests::track_header_visibility_mute_solo_and_lock_are_undoable_set_track_edits` drives; the test asserts the sequence state and undo. The click-through is now automated too: `ui_tests::clicking_the_a1_mute_button_is_one_undoable_track_edit` resolves the custom-painted `ff-track-mute` square through egui's widget registry, clicks its centre and asserts A1 is muted, then Ctrl+Z restores it (run `42389027715`). A render-difference test for hidden/muted/solo is still missing | Hidden/muted/solo render-difference test |
| Select / delete / razor / drag-move / trim | 🟡 | Engine commands and their upstream tests; M7 shell gestures. The razor path is now driven end to end: `ui_tests::ctrl_k_splits_the_clips_under_the_playhead_through_the_real_key_map` sends Ctrl+K through the real key map and asserts V1 and A1 each split into two clips with the "Add Edit (all tracks)" status (run `42340371967`). Select, delete, drag-move and trim are still code-inspected | Scripted select / delete / drag / trim gestures (needs a shared rect helper) |
| Insert / overwrite | 🟡 | T4 (run `40138769007`): a bin selection plus timeline Insert/Overwrite buttons drive `timeline.place`'s real `insert` flag, checked by `edit::tests::insert_and_overwrite_at_the_playhead_use_the_real_engine_insert_flag`. T5 adds the Source panel's Insert/Overwrite over the marked source range | Source in/out drag, source monitor audio |
| Ripple delete, roll, slip, slide | 🟡 | T6 (run `40163011141`): Shift+Delete and the timeline Ripple Delete button run `edit.rippleDelete` with linked A/V, affected/sync-locked gap closing, the marker policy, a locked-track refusal and one-step undo. T7: the Trim row and Alt/Shift/Ctrl arrow nudges run `timeline.roll` / `timeline.slip` / `timeline.slide`; tests assert exact 12-frame results and undo | Ripple targeting controls, drag gestures, arbitrary edit-point picking |
| Snapping | 🟡 | T3 (run `40133164791`): a timeline Snap checkbox drives the engine's `sequence.snap` state; moves snap start/end and trims snap the edge to zero, the playhead or another clip edge (8 px radius); `edit::tests::snap_tick_*` and `snap_targets_*` cover the maths. Markers are not targets yet; snapping is not linked-pair aware | Marker targets, linked-pair snapping, ripple-aware snapping |
| Source monitor + source In/Out marks | 🟡 | T5: separate Program/Source monitors, `render_item` previews, source timecode/transport/seek, engine-backed In/Out marks (I/O, Shift+I/O, go/clear) that persist on the media item, and the source-range Insert/Overwrite; `edit::tests::source_*` cover rendering, independence, range normalization, marker ripple and save/reopen. Source playback is video-only/silent | Source-marker panel, source range drag, source audio |
| Sequence in / out ranges | 🟡 | T9 (run `40236814552`): Mark In/Mark Out/Clear I/O plus Lift (`;`) and Extract (`'`) run the engine's `markers.markIn`/`markOut`/`clearInOut` and `sequence.lift`/`extract`; with no source clip open, `I`/`O` mark the sequence. Lift keeps the gap, Extract closes it on targeted + sync-locked tracks, ripples markers, clears the marks and parks the playhead. The linked-A/V undo test drives a real three-clip sequence. No ruler drag or split In/Out yet | Ruler In/Out drag, split (video/audio) In/Out |
| Sequence markers | 🟡 | T2: markers drawn on the ruler, click-to-jump, undoable Add/Clear through `markers.add` / `markers.clearCurrent`, Prev/Next navigation and the M shortcut; `edit::tests::marker_helpers_add_clear_and_navigate_through_the_real_engine` covers the real engine. T10 (run `40252471033`): marker name and comment fields plus an Update Marker button edit the marker at the playhead through a custom undoable project edit by `MarkerId`; `edit::tests::marker_fields_update_is_an_undoable_project_edit` covers rename, comment, length, colour, undo and the no-marker guard. T12 (run `40260600879`): Markers CSV exports RFC 4180-quoted name/comment, exact ticks, padded timecode and the colour label; the test checks the exact bytes and the file write. T13 (run `40513248065`): the same Update Marker path edits length (0-6000 frames) and colour, with undo. The M shortcut is driven through the real frame loop: `ui_tests::m_adds_a_marker_and_shift_m_jumps_to_the_next_one` asserts M adds exactly one marker on the snapped playhead and Shift+M jumps to it without adding a second (run `42340371967`). No marker drag, CSV import or marker-list selection yet | Marker drag, CSV/EDL import, marker-list selection |
| Speed / freeze / reverse | ⬜ | — | Engine command + verified timing |
| Transitions (cross-dissolve etc.) | 🟡 | T8 (run `40163011141`): Dissolve/Crossfade (Ctrl/Cmd+D, Ctrl/Cmd+Shift+D) apply a real `cross_dissolve` / `constant_power` transition at the selected clip's In edge, resolving a linked partner; tests sample rendered pixels and mixed audio samples at the cut. T11 (run `40256569182`): the Transition row chooses the length (1-600 frames) and alignment; Dissolve/Crossfade use that length and Update Transition edits the existing transition's frames/alignment via `sequence.setTransition` in one undo step; the apply/update/undo test asserts exact 24- and 8-frame spans. No browser, drag handle or alignment-at-apply yet | Transition browser, drag handles, audio transition variants, alignment at apply, per-transition selection |
| Program monitor | ✅ | M2 renders through the same compositor export uses; panel code-inspected | — (UI automation is a separate gap) |
| Preview quality / proxy / cache | ⬜ | Full-resolution CPU preview only | Half/quarter preview, proxy workflow, bounded cache, bypass |
| Exact time / fractional rates | ✅ | `frameforge-core` verifies 11 broadcast rates, sample and container math | — |
| A/V sync | 🟡 | Playhead is the audio device's consumed frames; wall-clock fallback is labelled; no sync measurement test | A drift test over a mixed sequence |
| Export H.264 + AAC (MP4) | ✅ | M2 re-decodes the export and verifies resolution, rate, duration, pixels and audio | — |
| Export ProRes / DNxHR / MJPEG / WAV | 🟡 | Real encoders offered; only H.264 is decode-verified in CI | Per-format decode test |
| Export HEVC / AV1 / alpha | ⬜ | Needs a registered encoder; not offered | Capability-detected hardware or tested software path |
| Export overwrite confirmation | ✅ | M12 `export::tests::output_path_adds_the_selected_extension_once` resolves the exact name; `start` asks Yes/No when it exists, and No keeps the existing file and starts nothing | — |
| Export cancel | ✅ | M12 Cancel stores FilmCraft's `Progress.cancel`; the worker returns `ExportError::Cancelled`; `export::tests::a_cancelled_export_is_reported_as_a_cancellation` checks the sentinel; the truncated partial file is removed | — (UI automation is separate) |
| Export queue / presets / range selection | 🟡 | Presets and range selection are wired. Four named presets (H.264 Web 20 Mbps, H.264 small file 8 Mbps, ProRes mastering, WAV audio only) apply format, bitrate and audio together, and editing any of them by hand returns the row to "Custom"; `export::tests::{every_preset_applies_an_offered_format_and_sets_all_three_fields, an_unknown_preset_is_refused_without_changing_the_settings}` cover the mapping and the refusal. The range row exports the whole sequence as an explicit `TimeRange` or defers to the engine's documented In/Out-else-whole resolution; In/Out is enabled only when both marks exist and falls back when they are cleared; `a_whole_sequence_range_is_explicit_and_in_out_defers_to_the_marks` covers both. Still one foreground job at a time (run `42391495128`) | Export queue |
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
| Masks / chroma key / matte UI | 🟡 | U1 (run `40794727835`): the Masking workspace authors real engine masks (`masks.add/set/select/remove`) with feather, opacity, inversion, mode and expansion, plus the engine's own mask tracker. No chroma-key UI, no alpha preview and no mask render test yet | Mask authoring + alpha preview + render test |
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
| **Roto engine (multi-model)** | 🟡 | `crates/roto`: model-independent engine with Cutie as the preferred tracker, SAM 2.1/EdgeTAM selection, SAMURAI optional, XMem legacy; temporal QC, deterministic matting/refinement, multi-object cache, job queue, mask/alpha/RGBA export, model catalogue + manager (plan/download/resume/verify/remove) and hardware reporting that claims no unverified provider. Run `41754368076` verifies the run path: the licence/consent gate, the Cutie and XMem workers as shipped scripts, the double publish into both caches, and a temporal-QC review list that reads the masks back and flags ranges with a suggested fix. The Cutie worker has never been executed (no runtime here); XMem inference was measured locally only. Everything deterministic is tested in CI. The Masking workspace now draws the prompts itself (brush, add/remove point, box) over the clip's exported frame or a hand-loaded one; Roto AI plans a `segment-1` selection, shows the selector's compatibility route and offers a no-load runtime scan, and *Run selection* exports the clip's first frame, spawns the worker on a thread with a timeout and cancel, checks the reply against the request and adopts the mask into the canvas. Selections are cached per model, request digest and pinned checkpoint digest; an unpinned checkpoint is not cached. See `docs/ROTO_ENGINE.md` and `docs/ROTO_SELECTION.md` | The queue wired to runners, mid-clip selection and corrections, a verified EdgeTAM adapter, the R6 corpus |
| **Roto AI (XMem)** | 🟡 | R1 cache + alpha seam and R2 worker protocol/exporter are CI-verified (runs `40056112214`, `40059782293`); a real XMem protocol worker and local R3 probe run the official model on CPU (forward/backward IoU, cancel, peak RSS) **locally, not in CI**. The bridge now has the consent/verification gate (run `41161529133`) and the `run_job` deadline/stall/cancel runner (run `41144133581`), and the Masking workspace's Roto AI panel drives both for one job (run `41238338683`). The panel also paints the frame-0 mask, publishes a finished run into the R1 cache and attaches it to the clip as one undoable edit, the runner kills a run that exceeds the worker's reported memory ceiling (run `41255268402`), and the Roto AI tab now runs the selected backend through that same job runner, publishes into both caches and reviews the result (run `41754368076`). Still open: a job queue, resume/corrections, a queue runner for selections, and a signed optional pack. No pack or weights ship. See `docs/ROTO_AI_PLAN.md` and `docs/ARCHITECTURE.md` | Job queue, resume, signed optional pack |
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
| XMem Roto AI controls wired to a real worker | 🟡 | The Masking panel's Roto AI tab drives `run_job` with the real XMem worker, gates it behind runtime/weights/consent checks, publishes into the R1 cache and reviews the run; the worker itself was exercised locally by `tools/xmem_r3_probe.py` (IoU and RSS in `docs/BASELINE_AUDIT.md`), not in CI. Click-through is not automated |
| Mask cache reusable in compositing/export, invalidates on edits | ✅ | R1 `r1_roto.rs`: a synthetic matte changes preview **and** H.264 export; every cache-key input (including ordered correction ids) addresses a different directory; a missing in-range mask is a clear error. The Masking workspace authors real engine masks and the Roto AI tab attaches a published run as an undoable `RotoRef` edit |
| No placeholder buttons / fake progress | ✅ | M9 audit inspected every enabled control |
| Unit / integration / render tests pass | ✅ | Run `42391495128`: **76 tests** (the earlier 72 plus the track-header click and three export preset/range tests; the suite covers M2/M3/M4/M9 durability, M11 history, R1 roto, R2 worker/exporter, T1 track state, T2 markers, T3 snapping, T4 insert/overwrite, T5 source monitor, T6-T8 edit/transition, T9 In/Out, T10/T13 marker edit, T11 transition edit, T12 CSV, bin nesting/tags/search, preview helpers, the four `ui_tests` interaction paths, and the export presets/range); CI on windows-2025. The `tools/` XMem probe is local-only, not CI. |
| CI on Windows; failures documented | ✅ | `ci.yml` + `ci-diagnostics` branch |
| Licences, sizes, limitations documented | ✅ | `NOTICE`, `docs/PROVENANCE.md`, this file; AI pack sizes pending |

## Test matrix gaps (honest)

- Keyboard UI paths are automated (three `ui_tests` through the real frame loop: undo/redo, Add Edit and
  markers) and so is one pointer path: the A1 mute square is clicked at the rect egui already tracks for
  `ff-track-mute`. Clip drag/trim, ruler clicks and screenshot regression are still not automated; those
  paths are verified by code inspection and the underlying document/render tests.
- No stress test for hundreds of clips/effects on a long timeline.
- No fractional-rate *project* round-trip test through the shell (the foundation verifies the math).
- No non-ASCII / spaces-in-path test yet; no missing-media UI test.
- No 1080p editing/playback/export benchmark on the target laptop yet; CI timings are not target
  numbers.
- XMem CPU inference was measured **locally** on the target Ryzen 5 7530U by `tools/xmem_r3_probe.py`
  (480x270, 6 frames: 0.818 fps forward / 1.252 fps backward wall, 710.2 / 610.7 MB peak RSS, mean IoU
  0.9883 / 0.9945). `roto-bench` itself still measures only the Rust decode/scale/PNG path and prints
  "XMem CPU inference: not measured"; the probe is not a CI gate. No fabricated numbers are reported.

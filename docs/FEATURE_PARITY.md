# Feature parity and status

Status legend:

- **✅ verified** — real code, and an automated test or CI measurement exercises that exact path
  (a bridge test, a pinned upstream test that FrameForge calls, or a measured CI run). The evidence
  is named in the row.
- **🟡 implemented, code-inspected** — real code that the shell uses, but no automated test drives
  that layer yet. This is not a claim of production readiness.
- **⬜ not implemented.**

No row is marked ✅ on the strength of a screenshot, a build succeeding, or documentation. Tests run
on GitHub Actions (`windows-2025`); the measured baseline is `docs/BASELINE_AUDIT.md`.

## P0 — reliable editing, playback, export

| Capability | Status | Evidence / remaining work | Completion criterion |
|---|---|---|---|
| New / open / save / save-as (`.ffproj`) | ✅ | Atomic save with round-trip validation; M2 reopens and renders byte-identical frames; M9 saves and reloads | — |
| Versioned project schema + refusal of newer files | ✅ | M9 `newer_schema_is_refused` (schema 999 is a named error) | — |
| Crash recovery / autosave | ✅ | M9 `autosave_round_trip_newer_than_and_clear`, `corrupt_autosave_is_a_clear_error_not_a_crash`; the shell's 20 s timer calls exactly this module | UI close-path prompt (still absent) is separate |
| Undo / redo of timeline edits | 🟡 | FilmCraft `Session` history and command tests upstream; shell buttons/keys wired, no FrameForge-level test | A FrameForge test that executes an edit, undoes and asserts the document |
| Undo / redo of effect & composition edits | ⬜ | Effects are edited on the bridge document, outside `Session` history | Composition edits enter a real undo history; Ctrl+Z never undoes the wrong thing |
| Media import (generators, files) | ✅ | M3 fixture export → re-import through `file.import`/codecs → render | — |
| Media bin folders / tags / search / metadata | ⬜ | Flat list today | Bin model + UI, persisted in the project |
| Thumbnails / audio waveforms | ⬜ | — | Generated off the UI thread, cached, bounded |
| Relink missing media | 🟡 | `FrameForgeProject::relink` and the offline slate are tested (M3); no UI dialog | Relink dialog + offline badge |
| Multi-track timeline (2V + 2A) | 🟡 | Engine tracks and the shell's 2+2 sequence; placement via `timeline.place`; no test of a two-video-track edit | A test that edits V2/A2 and renders the result |
| Track targeting (choose V2/A2) | ⬜ | Placement always V1/A1 | Track target buttons; place on the selected track |
| Track mute / solo / lock / visibility in the timeline | ⬜ | Audio mute/solo exists in the mixer; no track headers | Track header controls bound to commands |
| Select / delete / razor / drag-move / trim | 🟡 | Engine commands and their upstream tests; M7 shell gestures; no FrameForge gesture test | A scripted edit test per gesture |
| Insert / overwrite, ripple delete, roll, slip, slide | ⬜ | Engine supports much of this; no shell wiring | Commands wired + timeline tests |
| Snapping, markers, ranges | ⬜ | — | Toggleable snapping; markers persisted |
| Speed / freeze / reverse | ⬜ | — | Engine command + verified timing |
| Transitions (cross-dissolve etc.) | ⬜ | FilmCraft transition defs exist; no UI | Apply a transition at a cut, render test |
| Source monitor | ⬜ | Only the program monitor exists | Source monitor with in/out and insert/overwrite |
| Program monitor | ✅ | M2 renders through the same compositor export uses; panel code-inspected | — (UI automation is a separate gap) |
| Preview quality / proxy / cache | ⬜ | Full-resolution CPU preview only | Half/quarter preview, proxy workflow, bounded cache, bypass |
| Exact time / fractional rates | ✅ | `frameforge-core` verifies 11 broadcast rates, sample and container math | — |
| A/V sync | 🟡 | Playhead is the audio device's consumed frames; wall-clock fallback is labelled; no sync measurement test | A drift test over a mixed sequence |
| Export H.264 + AAC (MP4) | ✅ | M2 re-decodes the export and verifies resolution, rate, duration, pixels and audio | — |
| Export ProRes / DNxHR / MJPEG / WAV | 🟡 | Real encoders offered; only H.264 is decode-verified in CI | Per-format decode test |
| Export HEVC / AV1 / alpha | ⬜ | Needs a registered encoder; not offered | Capability-detected hardware or tested software path |
| Export cancel / queue / presets / overwrite confirm | ⬜ | Progress + report only | Cancel, queue, presets, confirmation |
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
| **Roto AI (XMem)** | ⬜ | Mandatory design; see `docs/ROTO_AI_PLAN.md`. Nothing implemented or claimed yet |
| AI pack (weights, runtime) | ⬜ | Optional, consent-based download only |
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
| Mask cache reusable in compositing/export, invalidates on edits | ⬜ | — |
| No placeholder buttons / fake progress | ✅ | M9 audit inspected every enabled control |
| Unit / integration / render tests pass | ✅ | Run 39878432877: 14 tests (M2/M3/M4/M9 durability); the follow-up adds the schema-refusal test |
| CI on Windows; failures documented | ✅ | `ci.yml` + `ci-diagnostics` branch |
| Licences, sizes, limitations documented | ✅ | `NOTICE`, `docs/PROVENANCE.md`, this file; AI pack sizes pending |

## Test matrix gaps (honest)

- No UI automation or screenshot regression yet; UI behaviour is verified by code inspection and the
  underlying document/render tests, not by driving the window.
- No stress test for hundreds of clips/effects on a long timeline.
- No fractional-rate *project* round-trip test through the shell (the foundation verifies the math).
- No non-ASCII / spaces-in-path test yet; no missing-media UI test.
- No 1080p benchmark on the target laptop yet; no fabricated numbers will be reported.

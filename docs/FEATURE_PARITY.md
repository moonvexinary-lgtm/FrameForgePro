# Feature parity and status

Status legend: **✅ works and is covered by an automated test** · **🟡 partial / works in code but not fully
tested or not exposed in the UI** · **⬜ not implemented**. No row is marked ✅ without a test that exercises
it on a real code path. Tests run on GitHub Actions (`windows-2025`); see `docs/BASELINE_AUDIT.md` for the
audit that produced this table.

## P0 — reliable editing, playback, export

| Capability | Status | Evidence / remaining work | Completion criterion |
|---|---|---|---|
| New / open / save / save-as (`.ffproj`) | ✅ | Atomic save with JSON round-trip; M2 reopen renders byte-identical frames | — |
| Versioned project schema + refusal of newer files | ✅ | `FFPROJ_SCHEMA` 2; schema-1 loads with defaults; newer → clear error | — |
| Crash recovery / autosave | 🟡 | Autosave sidecar + newer-than recovery + unsaved-change guard (this slice) | Test proven in CI; user-visible recovery message |
| Undo / redo of timeline edits | ✅ | `Session` history; engine commands; UI buttons + Ctrl+Z/Ctrl+Shift+Z | — |
| Undo / redo of effect & composition edits | ⬜ | Effects are edited on the bridge document, outside `Session` history | Composition edits enter a real undo history; Ctrl+Z never undoes the wrong thing |
| Media import (generators, files) | ✅ | `file.import` + codec registry; M3 fixture round-trip | — |
| Media bin folders / tags / search / metadata | ⬜ | Flat list today | Bin model + UI, persisted in the project |
| Thumbnails / audio waveforms | ⬜ | — | Generated off the UI thread, cached, bounded |
| Relink missing media | 🟡 | `FrameForgeProject::relink` tested; no UI | Relink dialog + offline badge |
| Multi-track timeline (2V + 2A) | ✅ | Engine timeline; clips placed via `timeline.place` | — |
| Track targeting (choose V2/A2) | ⬜ | Placement always V1/A1 | Track target buttons; place on selected track |
| Track mute / solo / lock / visibility in the timeline | ⬜ | Mixer has mute/solo for audio; no track headers | Track header controls bound to commands |
| Select / delete / razor / drag-move / trim | ✅ | M7; engine commands with ghost preview | — |
| Insert / overwrite, ripple delete, roll, slip, slide | ⬜ | Engine supports much of this; no shell wiring | Commands wired + timeline tests |
| Snapping, markers, ranges | ⬜ | — | Toggleable snapping; markers persisted |
| Speed / freeze / reverse | ⬜ | — | Engine command + verified timing |
| Transitions (cross-dissolve etc.) | ⬜ | FilmCraft transition defs exist; no UI | Apply transition at a cut, render test |
| Source monitor | ⬜ | Only the program monitor exists | Source monitor with in/out and insert/overwrite |
| Program monitor | ✅ | Same renderer as export; cached by revision/frame | — |
| Preview quality / proxy / cache | ⬜ | Full-resolution CPU preview only | Half/quarter preview, proxy workflow, bounded cache, bypass |
| Exact time / fractional rates | ✅ | `frameforge-core` verifies 11 rates, sample/container math | 23.976/29.97 project round-trip test when the UI exposes rate choice |
| A/V sync | ✅ | Playhead is the audio device's consumed frames; wall-clock fallback is labelled | — |
| Export H.264 + AAC (MP4) | ✅ | M2: exported file re-decoded and verified (pixels + audio) | — |
| Export ProRes / DNxHR / MJPEG / WAV | 🟡 | Real encoders offered; only H.264 is decode-verified in CI | Per-format decode test |
| Export HEVC / AV1 / alpha | ⬜ | Needs a registered encoder; not offered | Capability-detected hardware or tested software path |
| Export cancel / queue / presets / overwrite confirm | ⬜ | Progress + report only | Cancel, queue, presets, confirmation |
| Hardware encode/decode | ⬜ | Not assumed on AMD iGPU | Detect, test, fall back |

## P1 — effects, color, compositing, audio

| Capability | Status | Evidence / remaining work | Completion criterion |
|---|---|---|---|
| Unified effect registry (both engines) | ✅ | `catalog()`; merged count asserted; duplicate audit | — |
| Searchable effects browser | ✅ | Effects workspace search over the compositing catalog | Category filter + both engines in one browser |
| Real processing through preview and export | ✅ | 307/307 EffectCraft effects run through `ComposedSource`; M2 keyframed blur verified in export | Golden frames for each promoted effect |
| Typed parameters, defaults, ranges | ✅ | Bridge `ParamInfo`, EffectCraft `ParamUi` | — |
| Parameter UI (slider, number, %, angle, px, bool, popup, color) | ✅ | Effects workspace editors | — |
| Parameter UI for text / curves / wheel / path | ⬜ | Shown as "not editable yet" — never faked | Editors for these types |
| Keyframes: scalar | ✅ | Stopwatch, key list, delete, clear; media time | — |
| Keyframes: bool / color / choice | ⬜ | Only scalar keys today | Same keyframe UI for all animatable types |
| Keyframe-preserving parameter writes | 🟡 | Fixed in this slice (animated param gains a key instead of losing its track) | Test in CI |
| Effect enable / bypass | ✅ | Per-effect checkbox in both stacks | — |
| Effect reorder / duplicate / reset / presets | ⬜ | Add/remove/toggle only | Real serialized presets; reorder with render-order tests |
| Deterministic chain evaluation | ✅ | M2 direction-independence + identical reopen | — |
| Golden-frame test coverage | 🟡 | Gaussian blur (source + export), keyframed text, relink, comp layers | Golden frames per promoted effect and transition |
| Color workspace (FilmCraft stack) | ✅ | `effects.apply/setParam/toggleEnabled/remove`; verified command shapes | — |
| Color scopes / curves / wheels / LUT / before-after | ⬜ | — | Scopes computed from frames; LUT loading with licence records |
| Compositing comp layers (text/solid add, toggle, remove) | ✅ | Bridge `Composition` methods; same project the M4 test renders | Editable text/shape properties |
| Masks / chroma key / matte UI | ⬜ | Engine effects exist in the registry; no mask UI or chroma render test | Mask authoring + alpha preview + render test |
| Audio mixer faders / pan / mute / solo | ✅ | `mixer.setStrip`; exact upstream parameter names | — |
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
| Plugin host (OpenFX) | ⬜ | Evaluate only after effect architecture is stable |
| Motion tracking / planar tracking / stabilization | ⬜ | — |
| **Roto AI (XMem)** | ⬜ | Mandatory design; see `docs/ROTO_AI_PLAN.md`. Nothing implemented or claimed yet |
| AI pack (weights, runtime) | ⬜ | Optional, consent-based download only |
| Project archive / collect media | ⬜ | — |
| Installer | ⬜ | Tag builds a ZIP; an installer is future work |

## UI/UX acceptance checklist (from the UI brief)

| Item | Status |
|---|---|
| Builds and launches on Windows 11 x64 | ✅ (CI self-test + first run on the target laptop) |
| Workspace switcher changes real layouts, preserving project state | ✅ (Edit / Effects / Color / Audio / Export; shared `Session`) |
| Panels resize | ✅ (draggable gutters) |
| Layout saved/restored, dockable | ⬜ |
| Media import → preview → timeline edit → effect → save/reopen → export | ✅ (bridge tests; the shell path is the same code) |
| Visible effect parameters change preview and export | 🟡 (bug D2 fixed in this slice: monitor cache now invalidates) |
| XMem Roto AI controls wired to a real worker | ⬜ |
| Mask cache reusable in compositing/export, invalidates on edits | ⬜ |
| No placeholder buttons / fake progress | ✅ (planned entries are labelled; no fake panels) |
| Unit / integration / render tests pass | ✅ (11 tests, run `39729926210`) |
| CI on Windows; failures documented | ✅ (`ci-diagnostics` branch) |
| Licences, sizes, limitations documented | ✅ (`NOTICE`, `docs/PROVENANCE.md`, this file; AI pack sizes pending) |

## Test matrix gaps (honest)

- No UI automation or screenshot regression yet; UI behaviour is verified by code inspection and the
  underlying document/render tests, not by driving the window.
- No stress test for hundreds of clips/effects on a long timeline.
- No fractional-rate *project* round-trip test through the shell (the foundation verifies the math).
- No non-ASCII / spaces-in-path test yet; no missing-media UI test.
- No 1080p benchmark on the target laptop yet; no fabricated numbers will be reported.

# Baseline audit — FrameForge Pro

Audit date: 2026-10-22 · Auditor: coding agent (source inspection + CI evidence, no assumptions)

## 1. Exact refs inspected

| Ref | Commit | What it is |
|---|---|---|
| `frameforge-rust-rewrite` | `d13d05d` | **The maintained implementation.** Rust workspace: FilmCraft editing engine + EffectCraft compositing engine + the egui application shell. |
| `frameforge-dev` | `2601eb5` | The original C++/Qt/QML application (a Kdenlive fork, GPL-3.0, 2 003 files). It is an **ancestor** of the Rust branch (`git merge-base --is-ancestor` → true), preserved unchanged, and is not built or linked. |
| `frameforge-cpp-legacy` (tag, documented in README) | `2601eb5` | Recovery point for the Qt application. |
| Upstream FilmCraft | `6c3948a36c80e4996d924766ba7157eec4fca85e` | Pinned git dependency (`filmcraft-*`). |
| Upstream EffectCraft | `ef74d82902ba486891b02afa87e42963cc9b84cc` | Pinned git dependency (`effectcraft-*`). |

`frameforge-dev` is **not** the branch to develop: pointing it at the Rust tree would replace the legacy state
that the README promises to preserve. If the owner wants `frameforge-dev` to become the Rust line, that is a
single fast-forward and should be an explicit decision, not a side effect of this audit.

## 2. How this audit was performed

- Read the brief, the UI/UX brief, the README, `docs/ARCHITECTURE.md`, every source file under `apps/` and
  `crates/`, the CI and release workflows, both upstream licenses and the FilmCraft engine command registry.
- Verified command shapes against FilmCraft's own tests (`effects.apply`, `effects.setParam`,
  `effects.toggleEnabled`, `effects.remove`, `mixer.setStrip`) rather than trusting the shell's call sites.
- Read the last green CI run's logs (`39729926210`) for measured test and effect-sweep numbers.
- **No local Cargo build was run** (storage policy). CI is the compiler. Disk free at audit time: **53.6 GB**.

## 3. Workspace inventory

| Path | Lines | Responsibility |
|---|---|---|
| `crates/core` | 117 + tests | Shared foundation: exact time, cross-engine checks (`verify_foundation`). |
| `crates/bridge` | ~900 + 744 test lines | The only place the engines meet: `ComposedSource`, `.ffproj` document, catalog, project sources, comp layer helpers. |
| `apps/frameforge-pro` | ~1 950 | egui shell: app state/session, Edit, Effects, Color, Audio (mixer), Export, audio clock, program monitor. |
| CI | `ci.yml`, `release.yml` | windows-2025: fmt, clippy `-D warnings`, check, test, release build, binary self-test, artifact upload, diagnostics branch, tag-triggered release ZIP. |

Dependencies are pinned git revisions; both upstreams are **MIT OR Apache-2.0**. The Rust branch contains no
GPL code. The Qt branch (GPL-3.0) is never linked into this binary.

## 4. What genuinely works (verified in code and/or green CI)

| Capability | Evidence |
|---|---|
| Exact, shared time base across both engines | `frameforge-core::verify_foundation`; binary self-test in CI. |
| Real effects on timeline media | `ComposedSource` runs EffectCraft's registry through the renderer; M2 test proves keyframed Gaussian blur (monotonic softening, direction-independent, identity at 0). |
| Save / reopen fidelity | `.ffproj` schema 2, atomic save with round-trip validation; M2 test renders byte-identical frames after reopen. |
| Real export with verified output | H.264 + AAC export, decoded back and checked (resolution, rate, duration, audio peak, blur present) in `m2_poc.rs`. |
| File media, offline slate, relink, `.fcproj` / `.ecproj` import | `m3_media_catalog.rs`, `m4_comp.rs`; missing files render the offline slate and heal on relink. |
| Compositions with real layers | EffectCraft comp renders over FilmCraft footage at identical ticks; keyframed text measured moving; byte-identical reopen. |
| Session-owned editing + undo | Every timeline mutation goes through `filmcraft_engine::Session::execute`; M6 tests + shell code. |
| Drag-move / trim / razor | Engine `timeline.move` / `timeline.trim` / `sequence.addEditAllTracks`; one undo step per gesture. |
| Keyframe editor (scalar compositing params) | Keys in media time; evaluated by the same `Param` the renderer reads. |
| Color workspace | FilmCraft's own video effects (`effects.apply/setParam/toggleEnabled/remove`), verified command shapes against upstream tests. |
| Audio mixer | `mixer.setStrip` with `volumeDb`, `pan`, `muted`, `solo` — exact upstream parameter names verified. |
| Audio playback clock | cpal stream fed by a worker mixing through the engine's mixer, playhead = device frames; honest wall-clock fallback. |
| Panic containment | `catch_unwind` around the frame; session survives. |
| Cloud-only build + downloadable dev build | Run `39729926210`: fmt/clippy/check/test/release green, 11 tests, 24 MB artifact. |

Measured in run `39729926210`'s test log:

- **307 of 307** EffectCraft effects ran through the real composed-source path with default parameters
  (0 reported unsupported), on a 32×32 Bars frame.
- FilmCraft's catalog is asserted at **≥ 60** effect definitions by the same test.
- 11 tests total across the four suites; all passed.

These counts are *registry entries exercised through the bridge*, not a claim that every effect is
artistically complete or parameter-swept at multiple values. Golden-frame coverage exists for Gaussian blur
and the comp/text path, not for all 307.

## 5. Partially implemented (real code, incomplete workflow)

| Area | What exists | What is missing |
|---|---|---|
| Media bin | Import generators/files, list, place, remove | folders, search, tags, metadata, thumbnails, waveforms, relink UI, proxy generation |
| Timeline | 2 video + 2 audio tracks, select, drag-move, trim, razor, delete, scrub | track targeting (new clips always go to V1/A1), insert/overwrite, ripple/roll/slip/slide, snapping, markers, transitions UI, speed/freeze/reverse, waveforms, thumbnails |
| Effect stack | add/remove/toggle, typed params, scalar keyframes, comp layers | undo, reorder, duplicate, reset, presets, full keyframe types (bool/color/choice), graph editor |
| Color | FilmCraft video-effect stack on the selected clip | scopes, curves UI, wheels, before/after, LUT management |
| Audio | faders, pan, mute, solo | meters, audio-effect inserts UI, automation lanes, per-clip volume |
| Export | 5 built-in formats, progress, report | cancel, queue, presets, range selection, overwrite confirmation, progress detail |
| Project | new/open/save/save-as, atomic write | autosave, crash recovery (this slice adds it), recent files, unsaved-change guard (this slice adds it) |
| UI | workspaces, resizable gutters, shortcuts (Space, Ctrl+Z/Y, Ctrl+K, Del) | command palette, configurable shortcuts, layout persistence, UI scaling, source monitor, safe areas, accessibility pass |

## 6. Not implemented at all (no code path exists)

- **Roto AI / XMem** — no workspace, no worker, no masks, no model management. See
  `docs/ROTO_AI_PLAN.md`. Nothing is presented as working.
- Motion/Graphics workspace (titles exist only as comp layers added from the Effects panel).
- Masks, chroma key UI, tracking, adjustment clips, nested sequences UI, multicam, captions, OTIO/EDL.
- GPU preview/effects (the bridge path is CPU), proxies, caches, background rendering.
- Plugin host (OpenFX), AI pack, installers (only a ZIP of the exe on tag).

No fake panels or decorative controls were found: every enabled control maps to a real command or a real
document mutation. The Color/Audio tabs that were disabled in M5 are now real (M8).

## 7. Defects found by this audit (severity order)

| # | Severity | Defect | Location |
|---|---|---|---|
| D1 | **S1 — data loss** | Writing a scalar effect parameter replaces the whole `Param`, **deleting its keyframes**. Editing an animated value silently destroys the animation. | `apps/frameforge-pro/src/effects.rs` (`edit_scalar`) |
| D2 | **S1 — stale preview** | Parameter edits never call `mark_dirty()`, so the program-monitor cache does not invalidate: a changed effect is not shown until an unrelated edit bumps the revision. | same file, all four `edit_*` helpers |
| D3 | **S1 — data loss** | `New` / `Open` discard unsaved changes without confirmation; there is no autosave or crash recovery. | `app.rs` |
| D4 | S2 | Composition/effect edits bypass `Session` history; Ctrl+Z after an effect change undoes an unrelated timeline edit. | `app.rs`, `effects.rs` |
| D5 | S2 | Removing an effect before the selected one leaves `selected_effect` pointing at a different effect. | `effects.rs` |
| D6 | S2 | Export overwrites an existing file without confirmation; no cancel. | `export.rs` |
| D7 | S2 | Track targeting absent; the second video/audio tracks cannot be used from the UI. | `edit.rs` |
| D8 | S3 | `README.md` still says "milestone M1 — not usable as an editor"; `ARCHITECTURE.md` header still says M1. Documentation lags reality. | docs |

## 8. Build, test, CI, sizes

- CI: `windows-2025`, fmt → clippy `-D warnings` → check → test → release → self-test → artifact.
- Last green code run: **39729926210**; docs run: **39782549838**. 11 tests, 0 failed.
- Artifact: `frameforge-pro-windows-dev`, **24 MB** ZIP, 7-day retention. Extracted exe: **61.3 MB**.
- No installer yet; the tag workflow produces `frameforge-pro-<tag>-windows-x64.zip`.
- Local storage: no `target/` directory; 53.6 GB free on C:.

## 9. Licenses

| Component | License | Redistribution |
|---|---|---|
| FrameForge Pro Rust code | MIT OR Apache-2.0 | yes, with `NOTICE`/licenses |
| FilmCraft (pinned) | MIT OR Apache-2.0 | yes, attribution required |
| EffectCraft (pinned) | MIT OR Apache-2.0 | yes, attribution required |
| egui/eframe, rfd, cpal, image, serde | MIT/Apache/BSD/Zlib family | per-crate notices |
| Legacy Qt app on `frameforge-dev` | GPL-3.0 | **not** bundled, not linked |
| XMem (future optional pack) | MIT (code) | weights redistributed only with consent + notice; not bundled by default |
| XMem weights `v1.0` | release assets, 237.5 MB (`XMem.pth`) | download-on-consent; licence/notice recorded at download |

The brief's allowlist (MIT/Apache/BSD/ISC/Zlib/CC0/Unicode/BSL) is satisfied today. FFmpeg does not appear in
the dependency graph; if it is added later it must be an LGPL build with a per-build licence record.

## 10. Selected highest-impact improvement for this session

**Durability and correctness of edits (D1–D3).** The end-to-end journey already works on CI; the worst
remaining failures destroy or hide the user's work:

1. **Autosave + crash recovery** — write an atomic `.ffproj.autosave` sidecar while dirty, detect it on open
   when newer than the project, recover it, and clear it on a successful save.
2. **Unsaved-changes guard** — New/Open ask before discarding.
3. **Keyframe-preserving parameter writes** — an animated parameter gains/updates a key at the playhead
   (the editing engine's own `effects.setParam` semantics) instead of losing its track.
4. **Preview invalidation** — every parameter write marks the document dirty so the monitor re-renders.

Acceptance criteria:

- Bridge tests: autosave round-trip, newer-than detection, clear, corrupt-file error; keyframe preservation
  (animated param keeps its keys and gains the new key; static param updates in place).
- App: autosave runs at most every 20 s while dirty and a project path exists; saving clears it;
  opening a project with a newer autosave offers recovery (Yes recovers, No discards the sidecar);
  New/Open confirm when dirty.
- CI green with the existing artifact; docs updated (README status, ARCHITECTURE M9 row).
- No new runtime dependency.

Deliberately **not** in this slice: effect-edit undo (D4), track targeting (D7), XMem (needs its own staged
plan and a real worker benchmark). They are recorded in `docs/FEATURE_PARITY.md` with priorities.

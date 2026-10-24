# Baseline audit — FrameForge Pro

Audit date: 2026-10-22 · Updated: 2026-10-24 with R1/R2/T1–T4 evidence · Auditor: coding agent (source inspection + CI evidence, no assumptions)

## 1. Exact refs inspected

| Ref | Commit | What it is |
|---|---|---|
| `frameforge-rust-rewrite` | `9d0e167` | **The maintained implementation.** Rust workspace: FilmCraft editing engine + EffectCraft compositing engine + the egui application shell. R1/R2 add the cached roto mask seam and the XMem worker transport; T1 adds real track visibility/mute/solo/lock headers; T2 adds sequence markers; T3 adds move/trim snapping; T4 adds insert/overwrite at the playhead. |
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
  R1, R2 and T1–T4 were verified by their own green runs (`40056112214`, `40059782293`, `40063686209`,
  `40128137965`, `40133164791`, `40138769007`); the R2 and T1–T4 runs also execute `roto-bench 4` in
  the binary self-test step.
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
| Save / reopen fidelity | `.ffproj` schema 3, atomic save with round-trip validation; M2 test renders byte-identical frames after reopen; schema-2 projects load with no roto reference. |
| Cached roto alpha in preview and export | R1 (`40056112214`): `MaskCache` + `ComposedSource` multiply straight alpha on the one render path; a synthetic matte changes preview **and** decoded H.264 pixels; every cache-key input invalidates the address. |
| XMem worker transport and frame exporter | R2 (`40059782293`): protocol-1 JSON lines over a real child process (fake worker), typed events, 1 MiB line cap, stderr tail, `export_frames` PNGs verified for count/size/order; `roto-bench` runs in CI. **No XMem inference or weights.** |
| Timeline track state | T1 (`40063686209`): header buttons call real `timeline.setTrack` keys for video visibility (`enabled`), audio mute (`muted`), solo and lock; the edit test asserts sequence state and undo. UI click-through and render-difference tests are still open. |
| Sequence markers | T2: markers are drawn on the ruler and click-to-jump; Add/Clear run the real undoable `markers.add` / `markers.clearCurrent`; Prev/Next navigate. The edit test asserts placement, navigation, clear and undo. No rename editor or marker drag yet. |
| Snapping | T3: a toolbar Snap checkbox drives the engine's `sequence.snap` editor state; moves snap their start or end and trims snap the edge to zero, the playhead or another clip edge within 8 screen pixels. The pure snap functions are unit-tested; markers are not targets yet. |
| Insert / overwrite | T4: a project-panel selection plus timeline **Insert** / **Overwrite** buttons place the selected media at the playhead through `timeline.place`'s `insert` flag (undoable). The test drives a real `App` and checks ripple vs trim. No source in/out marks yet. |
| Real export with verified output | H.264 + AAC export, decoded back and checked (resolution, rate, duration, audio peak, blur present) in `m2_poc.rs`. |
| File media, offline slate, relink, `.fcproj` / `.ecproj` import | `m3_media_catalog.rs`, `m4_comp.rs`; missing files render the offline slate and heal on relink. |
| Compositions with real layers | EffectCraft comp renders over FilmCraft footage at identical ticks; keyframed text measured moving; byte-identical reopen. |
| Session-owned editing + undo | Every timeline mutation goes through `filmcraft_engine::Session::execute`; M6 tests + shell code. |
| Drag-move / trim / razor | Engine `timeline.move` / `timeline.trim` / `sequence.addEditAllTracks`; one undo step per gesture. |
| Keyframe editor (scalar compositing params) | Keys in media time; evaluated by the same `Param` the renderer reads. |
| Color workspace | FilmCraft's own video effects (`effects.apply/setParam/toggleEnabled/remove`), verified command shapes against upstream tests. |
| Audio mixer | `mixer.setStrip` with `volumeDb`, `pan`, `muted`, `solo` — exact upstream parameter names verified. |
| Audio playback clock | cpal stream fed by a worker mixing through the engine's mixer, playhead = device frames; honest wall-clock fallback. T2 opens the device lazily on the first play (construction never touches WASAPI), which removed an intermittent test-binary `STATUS_ACCESS_VIOLATION`. |
| Panic containment | `catch_unwind` around the frame; session survives. |
| Cloud-only build + downloadable dev build | Run `40138769007`: fmt/clippy/check/test/release green, self-test + `roto-bench 4` green, **41 tests**, 25,521,310-byte artifact. |

Measured in run `39729926210`'s test log:

- **307 of 307** EffectCraft effects ran through the real composed-source path with default parameters
  (0 reported unsupported), on a 32×32 Bars frame.
- FilmCraft's catalog is asserted at **≥ 60** effect definitions by the same test.
- The M8 log behind those measurements ran 11 tests; the M12 suite was **28 tests**, R2 took it to
  **36 tests** (run `40059782293`), T1 took it to **37** (run `40063686209`), T2 to **38**
  (run `40128137965`), T3 to **40** (run `40133164791`) and T4 to **41** (run `40138769007`).

These counts are *registry entries exercised through the bridge*, not a claim that every effect is
artistically complete or parameter-swept at multiple values. Golden-frame coverage exists for Gaussian blur
and the comp/text path, not for all 307.

## 5. Partially implemented (real code, incomplete workflow)

| Area | What exists | What is missing |
|---|---|---|
| Media bin | Import generators/files, list, place, remove | folders, search, tags, metadata, thumbnails, waveforms, relink UI, proxy generation |
| Timeline | 2 video + 2 audio tracks, select, drag-move, trim, razor, delete, scrub, per-track target/source-patch and visibility/mute/solo/lock headers, placement on the patched tracks, ruler markers with add/prev/next/clear, move/trim snapping, insert/overwrite at the playhead | ripple/roll/slip/slide, marker rename/drag, transitions UI, speed/freeze/reverse, waveforms, thumbnails |
| Effect stack | add/remove/toggle, typed params, scalar keyframes, comp layers, undo through the combined history | reorder, duplicate, reset, presets, full keyframe types (bool/color/choice), graph editor |
| Color | FilmCraft video-effect stack on the selected clip | scopes, curves UI, wheels, before/after, LUT management |
| Audio | faders, pan, mute, solo | meters, audio-effect inserts UI, automation lanes, per-clip volume |
| Export | 5 built-in formats, progress, report, overwrite confirmation, cancel (partial file removed) | queue, presets, range selection, progress detail |
| Project | new/open/save/save-as, atomic write, autosave/recovery, New/Open and window-close guards | recent files |
| Roto AI | R1 mask-cache reader + alpha in `ComposedSource`; R2 worker protocol, child-process client, frame exporter and `roto-bench` | real XMem inference, Python/CPU runtime pack, consent download/weights, initial-mask and correction UI, workspace, job queue/timeout, in-memory mask LRU |
| UI | workspaces, resizable gutters, shortcuts (Space, Ctrl+Z/Y, Ctrl+K, Del) | command palette, configurable shortcuts, layout persistence, UI scaling, source monitor, safe areas, accessibility pass |

## 6. Not implemented at all (no code path exists)

- **Roto AI / XMem inference** — no Roto AI workspace, no Python worker process, no PyTorch, no
  weights and no model management. The R1 mask cache/alpha seam and R2 protocol/exporter exist, but
  nothing in the UI presents Roto AI as working. See `docs/ROTO_AI_PLAN.md`.
- Motion/Graphics workspace (titles exist only as comp layers added from the Effects panel).
- Mask/chroma-key authoring UI, tracking, adjustment clips, nested sequences UI, multicam, captions, OTIO/EDL.
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
| D4 | S2 | Composition/effect edits bypass `Session` history; Ctrl+Z after an effect change undoes an unrelated timeline edit. **Fixed in M11** (one chronological history; tests in `apps/frameforge-pro/src/history.rs` and `app.rs`). | `app.rs`, `effects.rs`, `history.rs` |
| D5 | S2 | Removing an effect before the selected one leaves `selected_effect` pointing at a different effect. | `effects.rs` |
| D6 | S2 | Export overwrites an existing file without confirmation; no cancel. **Fixed in M12** (Yes/No overwrite prompt on the exact resolved path; real `Progress.cancel` with partial-file cleanup; the close-window guard is part of the same slice). | `export.rs`, `app.rs` |
| D7 | S2 | Track targeting absent; the second video/audio tracks cannot be used from the UI. **Fixed in M10** (track headers + source-patched placement). | `edit.rs` |
| D8 | S3 | `README.md` still says "milestone M1 — not usable as an editor"; `ARCHITECTURE.md` header still says M1. Documentation lags reality. | docs |

Post-audit status: D1, D2, D3, D5 and D8 were fixed in M9; D7 was fixed in M10 (tests in
`apps/frameforge-pro/src/edit.rs`); D4 was fixed in M11 (combined timeline/composition history,
tests in `apps/frameforge-pro/src/history.rs`). D6 was fixed in M12 (export overwrite/cancel in
`apps/frameforge-pro/src/export.rs`, window-close guard in `app.rs`). No S1/S2 defect from this
audit remains open.

## 8. Build, test, CI, sizes

- CI: `windows-2025`, fmt → clippy `-D warnings` → check → test → release → self-test → artifact.
- Last green code run: **40138769007** (T4); all **41** tests passed and the artifact is
  `frameforge-pro-windows-dev`, **25,521,310 bytes** ZIP, 7-day retention. The T3 artifact was
  25,521,206 bytes, T2 25,521,671, T1 25,517,128, R2 25,512,804, R1 25,418,672 and M12 25,229,770;
  the feature slices added no new external crate (`image` was already a workspace/app dependency; the
  bridge now uses it too). The last extracted exe was **64,305,152 bytes** (M10/M12 measurement); the
  T4 ZIP is the same order of size, but the T4 exe was not extracted locally.
- `frameforge-pro roto-bench 4` runs in the binary self-test; it measures the Rust decode/scale/PNG
  path only and prints "XMem CPU inference: not measured". No XMem speed figure exists.
- No installer yet; the tag workflow produces `frameforge-pro-<tag>-windows-x64.zip`.
- Local storage: no `target/` directory; ~53 GB free on C:.

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
| PowerShell fake worker (`tests/fixtures/fake_xmem_worker.ps1`) | original test code (this project) | test-only, not shipped |

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

Deliberately **not** in the M9 slice: effect-edit undo (D4, fixed in M11), track targeting (D7,
fixed in M10), XMem (needs its own staged plan and a real worker benchmark). They are recorded in
`docs/FEATURE_PARITY.md` with priorities.

Subsequent sessions completed R1 (mask cache/alpha), R2 (worker transport/exporter), T1 (real track
visibility/mute/solo/lock headers), T2 (sequence markers on the ruler), T3 (move/trim snapping) and
T4 (insert/overwrite at the playhead). The next highest-impact work is the rest of timeline depth
(ripple/roll/slip/slide, source monitor, transitions), followed by R3's real XMem CPU run once the
optional runtime and weights are installed with consent.

# FrameForge Pro — Development Log

Concise, honest record of what was actually done, verified, and what remains.

## Session 1 — 2026-10-12 (Milestone 0)

**Repository audit (verified):**
- Repo is the authentic KDE Kdenlive tree: origin `https://github.com/KDE/kdenlive.git`, local HEAD `fed499892d3129560ddf26a5251eb40cd3e488ac`, which is the parent of upstream master `9d98e2d` (committed 2026-10-12 by Jean-Baptiste Mardelle). Working tree clean. Version 26.11.70-dev, GPL-3.0.
- Created branch `frameforge-dev` (master kept as clean upstream reference). Nothing upstream was overwritten or deleted.
- Mapped the architecture (see `FRAMEFORGE_ARCHITECTURE.md` §1): Core singleton w/ MLT++ leaks, MLT connection/controller layer, timeline2 model (TimelineItemModel/ClipModel/TrackModel/CompositionModel/GroupsModel/SnapModel + timelinefunctions + undo), asset system (XML-defined effects over MLT services; 14 movit GPU effects; 93 effect XMLs; transitions), .kdenlive project doc, monitor stack with existing d3d/opengl/metal video widgets, render/renderpresets, jobs (proxy/audiolevels/stabilize), Catch2 test suite (~40 suites, vendored catch.hpp + fakeit).

**Adobe Premiere research (verified, official docs only):**
- Direct helpx fetch is blocked (HTTP 403); archived 49 official pages via the r.jina.ai reader to `research/adobe/raw/` with `MANIFEST.md` (fetch date, source URLs); extracted headings/tables to `research/adobe/extracted-*.md`.
- Documented Premiere version: current release 26.5 (Sept 2026), notes history 25.0→26.5.2.
- Built `PREMIERE_FEATURE_PARITY.md` — full documented inventory: ~40 editing features, 16 effect categories w/ per-effect tables, complete transition inventory (46 incl. removed set), complete audio-effect inventory (38 across 9 categories), color management, graphics, AI features (cloud vs local marked), export/interop. Known gaps recorded honestly: GPU/32-bit/YUV classification lists (image-grid pages, not machine-extractable), audio-crossfades page not yet fetched, effects' parameter details live in archived raw pages.

**REA / Resolve (verified):**
- REA MCP: connected, 170 tools, win32/x64. `binary_session` doctor run: no target open; **Ghidra provider not configured** (`GHIDRA_INSTALL_DIR` unset, no JDK) — native PE deep analysis blocked until install is approved. Format-level tools available. macOS-only tools unavailable on this host (expected).
- DaVinci Resolve 21.1.0.17 present at the expected path (608 MB exe). davinci-resolve MCP server (162 tools) available for behavioral/scripting-level investigation once its in-app bridge is started. No investigation executed yet; plan in `FRAMEFORGE_ARCHITECTURE.md` §4.

**Environment audit (verified):**
- Missing for building: CMake, any C compiler toolchain (no MSVC/MinGW), Qt, KF6, MLT, OTIO, Imath, KDDockWidgets, ninja. Present: git, FFmpeg 8.1.2 CLI, ~60.7 GB free disk, Windows 11 26300, AMD Radeon iGPU, 15.3 GB RAM.
- Recommended bootstrap: KDE Craft (in-tree `.craft.ini`), ~10–15 GB. Awaiting user approval before installing anything.

**Not done / next (M1):**
- No build yet (prerequisites missing — approval requested).
- No source modifications made (docs + research only).
- Next: install toolchain via approved path → build unmodified baseline → run Catch2 suite → record baseline benchmarks → begin rebrand shell.

**Subagent note:** three background research subagents were externally cancelled mid-session; their scopes (Adobe catalogs) were completed inline instead. Kdenlive deep-map was done via direct reads.

---

## Session 1 (continued) — 2026-10-12 (first implementation slice)

**User constraints adopted:** no installs (storage-protected, ~55 GB free), real source changes one feature at a time, keep MLT working, no davinci-resolve MCP (no CursorBridge), REA-only for Resolve with format-level tools, changes marked unverified until a build exists.

**Feature: Corner Pin (Premiere-documented effect, parity B5).**
- Verified locally that FFmpeg 8.1.2 has **no** `cdl` filter and **no** `levels` filter (both candidate ideas discarded — this is why every mapping is verified against the local toolchain before writing XML).
- Verified `avfilter.perspective` exists with options x0..y3 (expression strings), interpolation (linear/cubic), sense (source/destination) via `ffmpeg -h filter=perspective`.
- Found why Kdenlive never shipped it: the bare MLT service auto-exposes expression-string params → "NOT USABLE WITH A SIMPLE GUI" section of `data/excluded_effects.txt` (line 324).
- Implementation:
  - New `data/effects/avfilter/avfilter_perspective.xml` — effect id `avfilter.perspective`, name "Corner Pin"; 8 `constant` pixel params (Upper/Lower Left/Right X/Y) with `%width`/`%height` substitution for min/max **and identity defaults** (verified in `src/assets/model/assetparametermodel.cpp:1170-1199` that `parseAttribute` substitutes `%width`/`%height` in all attributes including `default`); interpolation + sense lists (default Destination = Premiere-style corner behavior).
  - `data/effects/avfilter/CMakeLists.txt`: added to install list (alphabetical).
  - `data/excluded_effects.txt`: removed `avfilter.perspective` (GUI now provided).
  - `data/kdenliveeffectscategory.rc`: added to "Transform, Distort, and Perspective" group.
  - Deliberately NOT added to `included_effects.txt` ("tested by the team") — cannot honestly claim tested status without a build.
- Verification status: **UNVERIFIED — no build environment on this machine (user-approved constraint).** Verified statically: XML conforms to the kdenlive-assets schema patterns (compared against avfilter_gblur/limiter/drawbox/zoompan), param names `av.x0…av.y3` match MLT avfilter property convention, effect registration path confirmed in `abstractassetsrepository.ipp` (custom XML assets register independently of the excluded list). Runtime behavior (actual warping, monitor preview, render) must be confirmed at Milestone 1.
- Not investigated via REA this slice: Resolve's equivalent (Transform/Corner Pin sizing) — requires the approved Ghidra setup or in-app observation; deferred with the limitation recorded (Ghidra not installed per user storage decision).

**Next candidates (queue):** Audio DeHummer gap analysis, Cross Zoom transition (needs luma/affine work), Scene Edit Detection (local algorithm, larger), EDL export. Each gets the same verify-locally-first treatment.

---

## Session 1 (continued) — 2026-10-12 (slice 2)

**Slice 2a — Audio noise-reduction gap analysis (negative result, documented):**
- Adobe's documented DeHummer/DeNoise/Click-remover behaviors extracted from the archived `audio-effects-library.md` (root frequency, Q, gain, number of harmonics, harmonic slope, output-hum-only).
- Verified locally that `avfilter.afftdn` would map to Premiere's DeNoise (nr/nf/noise_type/output_mode, animatable) — **but** upstream Kdenlive excludes it and its family (`adeclick`, `adeclip`, `anlmdn`, …) under "NOT WORKING AUDIO AVIFILTER" (`data/excluded_effects.txt`). Shipping an XML for an upstream-flagged-broken service without the ability to test would violate the project's no-unverified-claims rule → **kept the exclusion; documented the blocker and the Band-Reject-stacking workaround in the parity matrix (§D)**; re-test afftdn against current MLT at the M1 build; full DeHummer/DeNoise deferred to FrameForge's native audio DSP.
- Discovered along the way: Kdenlive's "Noise Reduction and Audio Restoration" category contains only `ladspa_librnnoise` (RNNoise) — real ML-based noise suppression already ships.
- Local clone is shallow (depth 1) — no git history evidence available; recorded.

**Slice 2b — Five missing Premiere-documented wipe transitions (implemented):**
- Evidence chain: luma transition (`data/transitions/luma.xml`) auto-discovers PGMs via `%lumaPaths`; KNewStuff wipe packs (`kdenlive_wipes.knsrc`) install bare PGMs into `lumas/HD` with **no preview thumbnails required** — so new luma files are the officially-supported extension path.
- Diffed the shipped 18 lumas (HD+PAL, GIMP-exported binary P5 verified, dark-reveals-first convention verified on `linear_x.pgm`) against Premiere's documented 17-wipe inventory → 5 missing, cleanly procedural.
- Generated, committed, and registered in `data/lumas/CMakeLists.txt`: **band_wipe, barn_doors, random_blocks, wedge, zigzag_blocks** (each in HD 1920×1080 + PAL 720×576). Deterministic (fixed-seed LCG for random_blocks), reproducible via `research/tools/generate_missing_wipes.ps1 -Verify` (structure/range verified: P5 header, exact raster size, full 0–255 range, correct anchor semantics).
- Status: **UNVERIFIED in-app** (needs M1 build to confirm the wipes appear in the Luma "Wipe Method" dropdown and render correctly); PGM format verified structurally.

**Updated queue:** Scene Edit Detection (needs design; larger), EDL export (bigger, C++), re-test excluded audio avfilters at M1, Cross Zoom (deferred — needs affine keyframing, not luma), remaining wipe gaps (Inset/Paint Splatter/Random Wipe — deferred).

---

## Session 1 (continued) — 2026-10-12 (slice 3)

**Slice 3 — Scene Edit Detection audit + full false-gap sweep (matrix integrity pass):**
- Set out to *implement* Scene Edit Detection → discovered Kdenlive already ships it completely: `SceneSplitTask` (`src/jobs/scenesplittask.*`, 2021) — dialog with threshold, markers (category + range), subclips, min duration; runs the system FFmpeg; parses `pts_time:` into markers/zones.
- **Real verification without a build:** executed the task's exact FFmpeg command (`-filter:v "select='gt(scene,X)',showinfo" -fps_mode vfr -f null -`) against `tests/small.mkv` with local FFmpeg 8.1.2 → exit 0, and the `[Parsed_showinfo`/`pts_time:` tokens `processLogInfo()` parses are emitted exactly as expected. The existing feature is verified compatible with current FFmpeg. (An earlier apparent exit-code 1 was a PowerShell early-pipeline-termination artifact; a direct run exits 0.)
- Minor observations recorded, not changed (no benchmarks yet): the command muxes audio needlessly (`-an` micro-opt), and the task checks `exitStatus()` not `exitCode()` (benign here).
- **Full effect-name audit sweep:** extracted all ~290 effect display names from `data/effects` XMLs and diffed against every ✘ in the parity matrix. Corrections: ASC CDL exists as `frei0r.sopsat` (SOP/Sat — description literally says "according to the ASC CDL"), Directional Blur = `avfilter_dblur`, Find Edges = `avfilter_edgedetect` (+sobel/scharr/prewitt/kirsch/roberts), RGB Curves = `avfilter_curves` with a dedicated `av_curve` interactive widget, Echo ≈ `avfilter_tmix`, plus approximations for Camera Blur, Color Emboss, Ramp, and the Scene Split corrections in §A/§G. All edits are evidence-cited in the matrix.
- Remaining confirmed genuine video-effect gaps after the sweep (each lacks a usable MLT/FFmpeg service today → native engine M4+ or deferral): Extract, Levels (full UI), Lighting Effects, Reduce Interlace Flicker, Magnify, Spherize, Turbulent Displace, Twirl, Rolling Shutter Repair, 4-Color Gradient, Lens Flare, Lightning, Alpha Glow, Replicate, Roughen Edges, Edge Feather, Cineon Converter, Auto Reframe.
- Process lesson (now enforced): every "Kdenlive: ✘" claim must be checked against the actual effect inventory and a source search before becoming an implementation task.

**Updated queue:** remaining procedural wipe lumas (Inset/Random Wipe), EDL export design record, native-engine interface scaffold (M3 prep: pure-interface headers behind which MLT will sit), REA format-level probe of Resolve artifacts (still pending, Ghidra blocked per user decision).

---

## Session 1 (continued) — 2026-10-12 (slice 4)

**Slice 4a — Audio crossfade curve choice (Premiere parity §C, implemented):**
- Archived Adobe's "Audio crossfade transitions" page (last unfetched research gap): Constant Gain, Constant Power, Exponential Fade documented behaviors extracted and quoted in the matrix.
- Read MLT's upstream `transition_mix.c` (LGPL): `mix_audio()` implements **both** a linear crossfade (`gain_b = mix`) and an equal-power sin/cos crossfade, selected by the transition's `start` property being `< -1.0`. Kdenlive hardcodes `start=-1` (linear) at both mix-creation sites (trackmodel.cpp:2120, :2413) and exposed no curve parameter.
- **Implemented as a pure `data/transitions/mix.xml` change**: new "Crossfade Curve" list parameter (`start`: -1 = Constant Gain [default, preserves current behavior], -2 = Constant Power). Verified in Kdenlive source, end-to-end: creation initializes `start=-1` (matches default); UI edits flow via `AssetParameterModel::internalSetParameter` → `m_asset->set()` which MLT re-reads per audio frame (`transition_get_audio` reads `start` at mix time); project persistence flows via `loadMix` which copies actual transition properties into `value` attributes before model construction (no default-clobbering); render-copy uses the same value-attribute pattern. `name="start"` has precedent in shipped effect XMLs with no special handling in src/assets. Also fixed mix.xml's meaningless description.
- Exponential Fade (logarithmic): no MLT equivalent — deferred to the native engine; recorded.
- Legacy note: for very old projects whose mix carries no `start` property at all, the new parameter's default now initializes `start=-1` (a proper position-based crossfade instead of a constant 50/50 blend) — arguably a fix; flagged in commit message.
- Status: **UNVERIFIED** (no build available; all claims are source-reading-based).

**Slice 4b — REA/Resolve engineering record 1 (Source B kickoff):**
- REA doctor evidence recorded: native PE analysis blocked (Ghidra not installed per user storage decision; `inspect_binary_layout` ELF-only; `inspect_artifact` archive-only). davinci-resolve MCP not authorized by user. No internal-implementation claims made.
- Observable deployment evidence recorded (research/rea/resolve-investigation-1.md): Resolve 21.1 ships a full Qt stack (Qt/Qml/Quick/scenegraph/platforms/sqldrivers) — validates Kdenlive's Qt choice for a Windows pro NLE; embedded Python 3.14 runtime; no Developer\Scripting docs folder in this install.
- Filesystem page copy of the crossfades doc archived to research/adobe/raw + manifest entry.

**Updated queue:** remaining wipe lumas (deferred — behavior undocumented), EDL export (next real C++ feature), native-engine interface scaffold behind which MLT sits (M3), first build when toolchain approved.

---

## Session 1 (continued) — 2026-10-12 (slice 5)

**Slice 5 — EDL export (Premiere parity §H, implemented — first C++ feature of FrameForge):**
- Studied the existing OpenTimelineIO export as the integration template (class structure, action wiring, CMake source registration, UI resource entry, test pattern).
- **New module `src/export/`**: `edlexport.h/.cpp` — `EdlExport` class exporting the current timeline as a **CMX3600 EDL**:
  - Exports the bottom-most video track ("V1") — CMX3600 has no stacked-track concept; a `* COMMENT:` line notes additional video tracks.
  - Cuts-only ("C") events with mix/crossfade in/out absorption identical to `otioexport.cpp`'s logic (copied pattern: `getMixInfo` adjustments).
  - `TITLE:` from the sequence name (single-sequence case) → project filename → "SEQUENCE"; `FCM: NON-DROP FRAME`; record TC from 00:00:00:00 at project rate; source TC expressed in the media's original fps (`ProjectClip::getOriginalFps`).
  - Reels: sanitized ≤8-char uppercase from media filename, "AX" fallback; `* FROM CLIP NAME:` and `* FROM FILE:` comments for relinking.
  - Error handling mirrors `otioexport.cpp` (KMessageBox in the GUI, qWarning in tests).
- **Wiring (5 integration points, all mirroring existing patterns):** `src/export/CMakeLists.txt` (appends to `kdenlive_SRCS`), `add_subdirectory(export)` in src/CMakeLists.txt (alphabetical), MainWindow member + include, action block in `mainwindow_actions.cpp` ("E&DL Export…" → `export_edl`), menu entry in `kdenliveui.rc` next to `export_project`.
- **Test:** `tests/edltest.cpp` (Catch2, follows `otiotest.cpp`: OTIO import of the existing dataset → EDL export → content assertions: TITLE/FCM headers, ≥1 event matching the CMX event-line regex, FROM CLIP NAME comment); registered in `tests/CMakeLists.txt`.
- Verified before writing: every model API (`getTracksCount/getTrackIndexFromPosition/isAudioTrack/isSubtitleTrack/getTrackById/getClipsCount/getClipByRow/getClipPosition/getClipInOut/getClipName/getClipBinId/getClipPtr/getMixInfo/getMixDuration/getMixCutPosition`, `getSequenceNames`, `url()`, `getProjectFpsInfo`, `getClipByBinID/hasUrl/url/getOriginalFps`) exists with matching semantics in `otioexport.cpp`'s live usage. Avoided two nonexistent APIs found by checking first: a "documentname" doc property (doesn't exist) and `tractor()` public accessor (private — used `getSequenceNames()` instead).
- Static checks: brace balance verified in all 3 new files.
- Status: **UNVERIFIED — code cannot be compiled or run on this machine (user-approved constraint).** Compile/runtime verification happens at Milestone 1.
- Known v1 limitations (documented in the header + matrix): dissolves not emitted, audio tracks not exported, non-drop TC only, clips without media file use reel "AX".

**Updated queue:** native-engine interface scaffold (M3 prep), re-test excluded audio avfilters at first build, review EDL output against a real conform tool when build exists.

---

## Session 1 (continued) — 2026-10-12 (slice 6)

**Slice 6 — First FrameForge engine component: native DeHummer DSP (parity §D, implemented):**
- Deliberately NOT a pure interface scaffold (which the project rules prohibit as placeholder work): instead, the first engine module = one real algorithm + the minimal interface it exercises + tests.
- **`src/frameforge/interfaces/ieffect.h`** — `FrameForge::IEffect`: id/version/parameter-descriptors/setParameter/parameter/prepare/processPlanar, with an explicit threading and allocation contract (no allocation/locking/logging on the audio thread; prepare() resets; parameter changes at block boundaries).
- **`src/frameforge/audio/dehummer.h/.cpp`** — `FrameForge::Audio::DeHummer`: implements Adobe's documented DeHummer (root frequency, Q, gain dB, number of harmonics, harmonic slope, output-hum-only) as a per-channel cascade of RBJ peaking-EQ biquads with negative gain (partial notches, no FFT/latency), parameters clamped to declared ranges, parameter-driven chain rebuild with state reset, defensive channel handling. Pure C++ — no Qt/MLT dependency in the engine core.
- **`tests/frameforge_dsptest.cpp`** — Catch2 sections: metadata/descriptor contract, 50 Hz hum removed ~30 dB while 1 kHz content is preserved (Goertzel power measurements on synthetic signals), output-hum-only isolation, parameter clamping + 0 dB bypass. These tests are pure math and will run at the first M1 build.
- Registered: `add_subdirectory(frameforge)` in src/CMakeLists.txt (alphabetical), `frameforge_dsptest.cpp` in tests/CMakeLists.txt. Brace/paren balance statically verified in all four files.
- Design decisions documented in code: harmonics count = notches above the root (root always affected); slope multiplies each successive harmonic's attenuation (1.0 = uniform); gain 0 dB = bypass.
- Status: **UNVERIFIED — cannot compile here (user-approved constraint)**; DSP correctness is asserted by the test suite at M1. Playback integration (MLT filter-module wiring or engine audio path) is the M3 milestone; DeNoise/Automatic Click Remover remain blocked on MLT's broken audio-avfilter path, recorded in the matrix.

**Updated queue:** remaining M3-prep engine interfaces (only as implementations accompany them), re-test excluded audio avfilters at first build, EDL output review against a conform tool, first build when toolchain approved.

---

## Session 1 (continued) — 2026-10-12 (slice 7)

**Slice 7 — First FrameForge video engine component: native Levels (parity §B1, implemented):**
- Evidence first: Adobe's archived adjust-effects page documents Levels' behavior ("precise control over brightness and contrast… combines Color Balance, Gamma Correction, Brightness and Contrast, and Invert"); the current page format keeps the parameter enumeration in images — noted honestly in the matrix rather than claimed.
- **`src/frameforge/interfaces/ivideoeffect.h`** — `FrameForge::IVideoEffect`, the video seam: same parameter-descriptor/metadata contract as `IEffect`, plus `prepare(width,height)` and `processFrame(uint8_t *rgba, stride, w, h)` with an explicit v1 frame contract (packed 8-bit RGBA, straight alpha, stride-padded rows) and audio-thread-style guarantees (no allocation/locking on the render path). Planar/float formats are an M4 extension, documented.
- **`src/frameforge/video/levels.h/.cpp`** — `FrameForge::Video::Levels`: the classic Levels transform (input black/white, gamma, output black/white; identical transform on R/G/B; alpha untouched) precomputed into a 256-entry LUT on parameter change → table lookup per component in `processFrame` (no allocation, no floating point on the render path). Defensive range normalization, degenerate-input-range binary threshold documented and tested.
- **`tests/frameforge_videodsptest.cpp`** — Catch2: descriptor contract, identity-at-defaults across all 256 values (alpha untouched), transform-formula anchors (LUT rounding ±1), parameter clamping, degenerate-range threshold, stride-padded rows. Three test bugs caught and fixed before commit (missing `<algorithm>` for `std::clamp`, a stray padding write with a misleading comment, redundant `prepare()` in a loop).
- Registered in `src/frameforge/CMakeLists.txt` and `tests/CMakeLists.txt`; brace/paren balance verified in all four files.
- Status: **UNVERIFIED — no build environment (user constraint)**; the pure-math tests prove the DSP at M1. Kdenlive XML-side "Reuse lift/gamma/gain" remains the playback-available path until engine wiring (M3/M6).

**Updated queue:** more engine components only when they pair with real algorithms/tests (candidates: Extract, Edge Feather, Alpha Glow — all confirmed gaps), build-environment decision with the user when they're ready, matrix maintenance as evidence accumulates.

---

## Session 2 — 2026-10-13 (slice 8: Extract, Edge Feather, Alpha Glow)

**User constraints re-confirmed:** no build environment installs; verify gaps before implementing; cite sources; distinguish written vs executed tests; mark C++ UNVERIFIED; identify what is genuinely installable in existing Kdenlive; reviewable commits with hashes.

**Gap verification (requirement 1):** all three re-checked against the ~290-name effect inventory (no Extract / Edge Feather / Alpha Glow names exist) and the documented behaviors extracted from the locally archived Adobe pages:
- Extract (adjust-effects.md): "removes colors… grayscale… below Black Input Level or above White Input Level appear black, in between gray or white" — the matrix's earlier description ("Adjust-channel extractor") was wrong and is corrected.
- Edge Feather (transform-effects.md): "adds a soft black border… subtle vignette look… Amount value controls the width of the border."
- Alpha Glow (stylize-effects.md): "applies color along the edges of a masked alpha channel… fade out or gradually shift to a second color as it extends from the edge."
- Honest nuances recorded in the matrix: Edge Feather is *approximable* today via Planes Blur (`avfilter_boxblur` Alpha Radius — the existing XML exposes it); frei0r edgeglow / movit.glow are luminance-based, not the documented alpha-based Alpha Glow.

**Implementations (all behind `FrameForge::IVideoEffect`, pure C++, allocation-free render paths, scratch sized in prepare()):**
- **Extract** — `src/frameforge/video/extract.*` (commit `e554af7`): Rec.709 luminance window via a 256-entry LUT; black/white input levels, softness ramps, invert; grayscale output, alpha untouched.
- **Edge Feather + shared blur utility** — `src/frameforge/video/edgefeather.*` + `alphablur.*` (commit `8d18447`): separable prefix-sum box blur of the alpha channel (shared utility `boxBlurAlpha`); amount→radius = ceil(amount/2); RGB untouched.
- **Alpha Glow** — `src/frameforge/video/alphaglow.*` (this commit): glow mask = blurred alpha − original alpha; color_a→color_b gradient with falloff; glow composited under the original via straight-alpha over-composite; params glow/brightness/color_a/color_b (packed 0xRRGGBB).

**Tests — written, NOT executed (requirement 4):** `tests/frameforge_videodsptest.cpp` extended with Extract / Edge Feather / Alpha Glow TEST_CASEs. Every assertion anchor was computed by hand from the prefix-sum math (e.g., radius-2 blur of a hard edge: 255/191/153/153/102/51/0/0; radius-4 9-px window: 4/9, 3/9 of 255 → 113/85). They run at the first M1 build; until then **UNVERIFIED** (requirement 5). No test in this session has been compiled or executed.

**User-installable assets (requirement 6) — locations verified from source code:**
- Effect XMLs → `QStandardPaths::AppDataLocation`/"effects" (`EffectsRepository::assetDirs()`, effectsrepository.cpp) → Windows: `%APPDATA%\kdenlive\effects\`
- Transition XMLs → `AppDataLocation`/"transitions" (`TransitionsRepository::assetDirs()`, transitionsrepository.cpp) → `%APPDATA%\kdenlive\transitions\`
- Custom wipe lumas → `AppLocalDataLocation` (`MltConnection::refreshLumas()`, mltconnection.cpp:422; KNewStuff `kdenlive_wipes.knsrc` TargetDir `kdenlive/lumas/HD`) → `%LOCALAPPDATA%\kdenlive\lumas\HD\`
- Therefore **installable in an existing (≥ MLT 7.25) Kdenlive release without compiling**: `avfilter_perspective.xml` (Corner Pin) → effects dir; `mix.xml` (crossfade curves — replaces the shipped definition) → transitions dir; the 5 wipe PGMs (HD; PAL for SD profiles) → lumas/HD. Caveat: this machine has **no Kdenlive installed** (verified), so runtime behavior could not be observed here; format compatibility was verified against the shipped file formats (XML schema patterns, P5 PGM headers) and the loader source paths above. The new engine DSPs (Extract/Edge Feather/Alpha Glow/Levels/DeHummer) are **NOT** usable in Kdenlive until engine wiring (M3/M6).

**Limitations:** Extract parameter enumeration beyond black/white input levels (softness, invert) follows the classic Extract design (current Adobe page keeps parameters in images); Edge Feather/Alpha Glow use box-blur spreads (documented in headers); Alpha Glow colors are packed 0xRRGGBB doubles pending the M6 parameter-type system.

---

## Session 2 (continued) — 2026-10-13 (slice 9: Twirl, Spherize, Cineon Converter)

**Gap verification:** all three confirmed against the effect inventory (no matches) and the archived Adobe pages:
- Spherize: "distorts a layer by wrapping a region of the image onto a sphere." (B5)
- Twirl: "distorts an image by rotating a clip around its center. The image is distorted more sharply in its center than at the edges, causing a whirlpool result at extreme settings." (B5)
- Cineon Converter: "high degree of control over the color conversions of Cineon frames… The 10 bits of data available in each Cineon channel…" (B16)

**Implementations (FrameForge::IVideoEffect, pure C++, allocation-free render paths):**
- **`src/frameforge/video/sample.h`** — header-only shared bilinear sampler (`sampleBilinear`) used by both geometric effects.
- **`Twirl`** (`twirl.*`) — inverse mapping: rotate the offset back by angle·(1 − d/radius) around the frame center (the documented center-dominant falloff); bilinear over a packed frame copy (remaps never read overwritten pixels); params angle (−720..720°), radius (px).
- **`Spherize`** (`spherize.*`) — inverse-mapped bulge: sample offset = offset / (1 + strength·(1−d²)); seamless at the radius, center magnification. Adobe's exact curve is undocumented — this bulge model is the recorded v1 interpretation (matrix + header). Params amount, radius_percent.
- **`CineonConverter`** (`cineon.*`) — canonical Cineon 10-bit log curve with cross-checked anchors (ref white 685 → 1.0; 18% gray ≈ code 470 → 0.192; toe 0.0108 at black 95; 300 codes/decade), params conversion/black_point/white_point/gamma, per-channel 256-entry LUT. A double-scaling bug in the Linear→Log output path was caught and fixed before commit.

**Tests — written, NOT executed:** Twirl (180° near-center swap anchors: dest(cx±2,cy) samples the opposite side; outside-radius and zero-angle identity), Spherize (gray edge-blend anchor at d→0 — the split boundary at the center becomes a ~127 blend; boundary/outside identity; alpha preserved), Cineon (anchor math: v8 117 → 49±2, 171 → 255, toe < 8; Linear→Log 255 → 171±1; byte-level round-trip ≤ ±2 within the ≤685-code reference range; clamping). All assertions hand-computed; run at the first M1 build. **UNVERIFIED** until then.

**Status:** all three effects are engine DSPs — not usable in Kdenlive until engine wiring (M3/M6); the installable-without-compiling asset set is unchanged (Corner Pin XML, mix.xml crossfade curves, wipe PGMs — locations recorded in slice 8).

---

## Session 2 (continued) — 2026-10-13 (slice 10: Magnify, Replicate)

**Gap verification:** both confirmed (effect inventory + archived Adobe quotes):
- Magnify (B5): "lets you enlarge all or part of an image, like using a magnifying glass, or scale the whole image beyond 100% without losing quality."
- Replicate (B12): "divides the screen into tiles and displays the whole image in each tile. Adjust the number of tiles in each column and row by moving the slider."

**Implementations (`FrameForge::IVideoEffect`, pure C++, allocation-free render paths):**
- **`Magnify`** (`magnify.*`) — circular region inverse-mapped toward the center at 100/magnification; params center_x/center_y (percent), radius (px), magnification (100-1000%); bilinear over a packed frame copy; v1 hard circle edge (no falloff) — documented simplification.
- **`Replicate`** (`replicate.*`) — modulo-cell grid tiling (ceil-division handles partial last cells); each tile shows the whole frame bilinear-sampled from a packed copy; params horizontal/vertical (1-16); 1×1 = identity.

**Tests — written, NOT executed (no build environment):** Magnify (2× sampling anchors computed from the inverse map: dest(29,32)→src(30.25,31.75)=black, dest(34,32)→src(32.75,…)=white; outside-circle untouched; 100% identity), Replicate (exact anchor map on a distinct-value 4×4 gradient: dest(1,0)=src(3,0)=3, dest(1,3)=src(3,3)=51, cell origins reset to src(0,0); alpha replicated; 1×1 identity; clamping). Run at the first M1 build; **UNVERIFIED** until then.

**Remaining confirmed engine-level gaps after this slice:** Roughen Edges (Adobe quote already captured: "roughs up the edges of a clip's alpha channel by using calculations… like eroded metal"), Lighting Effects, Lens Flare, Lightning, 4-Color Gradient, Rolling Shutter Repair, Auto Reframe.

---

## Session 2 (continued) — 2026-10-13 (slice 11: Roughen Edges, 4-Color Gradient)

**Gap verification:** both confirmed (effect inventory + archived Adobe quotes — Roughen Edges from the stylize page, 4-Color Gradient from the generate-effects table).

**Implementations (`FrameForge::IVideoEffect`, pure C++):**
- **`RoughenEdges`** (`roughenedges.*`) — the alpha channel sampled at positions displaced by two channels of deterministic hash-based value noise (uint32 arithmetic with well-defined wraparound — no global RNG state, no UB, no platform drift); params border (px), scale (noise lattice step), evolution (animatable offset); RGB untouched; bilinear over a packed frame copy.
- **`FourColorGradient`** (`fourcolorgradient.*`) — normalized inverse-square blend of four effect-point colors (the closest point dominates, reproducing the documented "solid-color circles" with a smooth merge between); 12 params (4 positions as percent, 4 packed 0xRRGGBB colors, Premiere-style defaults red/green/yellow/blue); full-frame generator-style output (alpha 255).

**Tests — written, NOT executed:** Four-Color Gradient has fully hand-computable anchors (corner pixels exactly (255,0,0)/(0,255,0)/(255,255,0)/(0,0,255); the (3,3) blend weights 0.3332/0.2398/0.2398/0.1875 → (146,122,48)±2). Roughen Edges has provable invariants (deep-interior alpha stays 255 and far-corner alpha stays 0 because the ±8 px displacement provably keeps samples inside/outside the rectangle; RGB pass-through; zero-border identity) plus one **flagged** edge-ring assertion ("roughening occurred") that is deterministic once compiled but not hand-computable — explicitly noted in the test and here: if the hash field proves unlucky at the first M1 run, loosen the threshold rather than trusting it blindly. **UNVERIFIED until the M1 build.**

**Remaining confirmed engine-level gaps:** Lighting Effects, Lens Flare, Lightning, Rolling Shutter Repair, Auto Reframe.

---

## Session 2 (continued) — 2026-10-13 (slice 12: Lens Flare, Lightning)

**Gap verification:** both confirmed (effect inventory + archived Adobe quotes from the generate-effects table).

**Implementations (`FrameForge::IVideoEffect`, pure C++, allocation-free render paths):**
- **`LensFlare`** (`lensflare.*`) — additive synthetic flare: quadratic core glow at the flare center, halo ring at 60% of size, secondary ghost mirrored through the frame center; bounding-box loops keep the render tight; three-element model documented as the v1 simplification of real lens geometry; alpha passes through.
- **`Lightning`** (`lightning.*`) — bolt as a polyline between the two documented points, each vertex displaced perpendicular to the axis by deterministic uint32-hash noise, tapered to zero at the endpoints (pins them, per the documented two-point semantics); drawn additively with a DDA line walker; params endpoints/amplitude/segments/seed. The seed is the evolution knob for the documented "animates automatically" behavior until the engine passes media time at the M3 wiring; hosts animate it meanwhile.
- One compile-blocking bug caught before commit: an undefined `kPiDouble` constant in lightning.cpp (fixed by defining it).

**Tests — written, NOT executed:** Lens Flare anchors hand-computed (core >100 at (16,16), ring ~85 at (29,29) bounded (60,200), ghost >40 at (47,47), far pixel 0, alpha untouched, zero-brightness identity); Lightning provable anchors (endpoints pinned at (0,0) and (57,57)=round(0.9·63), lit-path lower bound ≥40 pixels, a point at axis-distance 38.9 > amplitude 30 provably untouched, additive alpha pass-through, clamping). **UNVERIFIED until the M1 build.**

**Remaining confirmed engine-level gaps:** Lighting Effects, Rolling Shutter Repair, Auto Reframe (motion-analysis territory — best gated behind the M1 build per the project rules on measurability).

---

## Session 2 (continued) — 2026-10-13 (slice 13: XML-side investigation, remote-build feasibility, storage estimate)

**User constraints:** keep ≥50 GB free (55 GB now); no local toolchain installs; keep the 17 unverified commits intact; investigate remote Windows builds; complete Craft storage estimate before any re-recommendation; answer the logo question before any exe exists.

**XML-side parity — the honest outcome:** the strongest candidate (Exponential Fade via `avfilter.afade`) was investigated to the bottom:
- **Capability verified:** `afade` supports the documented logarithmic curve (`curve=log`) plus `exp`, `esin`, `hsin`, `par`, etc. (local FFmpeg 8.1.2).
- **Exclusion category verified:** `avfilter.afade` sits under "UNIMPLEMENTED/UNUSABLE AVFILTER EFFECTS" (excluded_effects.txt §39-45) — the simple-GUI category that Corner Pin proved rescuable — NOT the broken-audio category.
- **But shipping is deferred on three unverified mechanisms:** (1) whether MLT's avfilter module passes clip-relative audio timing (the fade must start at the clip, not the source, head); (2) whether `duration` values survive MLT's avfilter property passing with the right units; (3) the `totime` XSD attribute — the only candidate conversion path — has **zero shipped usage** (timer.xml keeps its totime examples commented out), so it is itself unverified. Per the no-assumption rule for installable assets, an afade XML would be a guess, not a deliverable. Recorded in the parity matrix; first action at the M1 build.
- Conclusion: **the verified-installable XML well is dry** under the no-assumptions rule. The remaining XML-side candidates all fail the verified-mechanism bar without a running MLT.

**Remote Windows build feasibility (investigated, no installs):**
- **GitHub Actions `windows-latest` runners** can compile and test FrameForge Pro with zero local disk: MSVC preinstalled, ~14-20 GB free runner disk, 4 vCPU/16 GB RAM. Free for public repos (~2000 core-min/month private).
- Qt 6.10/KF6/MLT would come from vcpkg or KDE Craft invoked *inside* the CI job, with `actions/cache` making subsequent runs ~10-20 min (cold MLT build ~60-120 min once, cached after).
- Requirements: push this repo to GitHub (public = free; it is a GPL-3.0 fork so source availability is already an obligation, not a new one), plus ~half a day of workflow iteration. Artifacts (zip/installer ~1.5-2 GB) download locally on demand.
- This is the only path compatible with the user's ≥50 GB free constraint.

**KDE Craft local storage estimate (complete, per user request — NOT recommending it under the current constraint):**
| Component | Size |
|---|---|
| Craft + Python + CMake/Ninja/tools | ~0.8-1.3 GB |
| Compiler: MSVC Build Tools 2022 (or MinGW ~1.2 GB) | ~5-7 GB |
| Qt 6.10 installed | ~3.5-4.5 GB |
| KDE Frameworks 6.21 subset installed | ~1.5-2.5 GB |
| MLT 7.38 + FFmpeg + deps installed | ~1-2 GB |
| Craft download cache (deletable after build) | ~4-6 GB |
| Build tree / object files (deletable after build) | ~4-8 GB |
| Installed kdenlive/FrameForge app dir | ~1.5-2 GB |
| **Peak during build** | **~20-28 GB** |
| **Steady state after cache+build cleanup** | **~12-17 GB** |
With 55 GB free and a ≥50 GB floor, even the steady state does not fit; during a build free space would dip to ~27-35 GB. **Remote CI is the right answer; local Craft stays off the table until the constraint changes.**

**Logo answer (delivered in-response; applied at exe build time):** the complete icon wiring was verified in CMakeLists.txt:209-224 (`ecm_add_app_icon` consuming `sc-apps-kdenlive.svg` + PNGs 16→1024 + `kdenlive.ico` from `data/icons/`). Placement and dimensions are in the session report; nothing is overwritten yet — the swap happens with user approval at the packaging step.

---

## Session 2 (continued) — 2026-10-13 (slice 14: FrameForge logo icon set, staged and wired)

**User input:** custom logo SVG at `C:\Users\<user>\Downloads\frafeforge svg\GroupframeFoge pro.svg` — purple rounded tile (450×450, rgb(99,74,180)), rotated "F" in the Spicy Rice webfont (cyan stroke, black fill), two triangle accents; canvas 450×666.084.

**Pipeline (zero installs, fully verified):**
- Local FFmpeg has no SVG decoder (verified: "no decoder found for: svg") → rasterized via **Microsoft Edge headless** (`--headless=new --screenshot`, `--virtual-time-budget` for the webfont fetch). Critical fix: a separate `--user-data-dir` is required (Edge otherwise attaches to the running browser process and writes nothing).
- 4×-scale render (1800×2670) from a temp SVG with the declared width/height multiplied (viewBox untouched) for vector-crisp downscales.
- **Webfont load proven programmatically**: A/B renders with/without the `@font-face` style block differ by 45,732 sampled pixels; without the font the F overflows the tile (ink to Y 2204), with it the artwork fits inside the tile (ink ≤ 2002) — also proving the tile-crop icon is safe. (No eyes needed; no image-viewing available in this session, so all verification was pixel-math.)
- Tile-crop geometry verified against the viewBox math (rows 204..2004 at 4×, purple bbox exactly 1800 wide).
- Corner transparency: the artwork contains no white ink (color inventory: purple/black/cyan only) → white background keyed to alpha (`colorkey=white:0.10:0.15`); 256 px verified (497 transparent corner pixels).

**Staged into `data/icons/` (Kdenlive originals untouched on disk):**
- `sc-apps-frameforge.svg` — vector master, viewBox cropped to the tile (`0 24 450 450`). Caveat recorded: it keeps the remote `@font-face` (Spicy Rice via design.penpot.app), so faithful rendering of the SVG elsewhere needs that font or network; the PNG set is the build-time ground truth. A self-contained text-to-path version is a future improvement (needs a vector editor).
- `sc-apps-frameforge.svgz` — gzipped theme copy (1883 bytes).
- `16/22/32/48/64/128/256/512/1024-apps-frameforge.png` — Lanczos downscales from the 1800 px master, all dimension-verified; 16 px readability checked (185 purple vs 71 ink pixels — tile dominant).
- `frameforge.ico` — 256 px PNG-in-ICO (270,622 bytes). Note: `ecm_add_app_icon` (src/CMakeLists.txt:224) generates the final multi-resolution exe icon from the PNG set at build time, so the shipped ICO is a convenience/preview artifact.

**CMake wiring:** `src/CMakeLists.txt` `ICON_FILES` repointed from the `kdenlive` set to the `frameforge` set (lines 212-223) — the next build (remote CI) bakes the custom logo into the exe, satisfying the user's "custom logo before the exe" requirement. Runtime rebrand surfaces (icons.qrc window icon, .desktop/appdata icon names, mimetype icons, installer) remain for the full-rebrand slice; this slice delivers the app/exe icon, which is what was asked.

**Status:** icon files are generated artifacts (verified dimensions/transparency programmatically); the exe-embedded result is UNVERIFIED until the first build renders it. No installs were performed; the only network fetch was the ~60 KB Spicy Rice webfont during rendering (part of the user's own logo asset).

---

## Session 2 (continued) — 2026-10-13 (slice 15: remote CI workflow draft)

**Deliverable:** `.github/workflows/windows-build.yml` — the remote Windows build/test pipeline (GitHub Actions `windows-latest`, MSVC 2022 — matching the project's own `.craft.ini` target ABI).

**Design (two-phase, cache-first):**
- **Phase 1 (cached):** KDE CraftMaster + the official KDE blueprints build the dependency chain (`kde/kdemultimedia/kdenlive` blueprint → Qt 6, KF6 6.21, MLT 7.38+, OTIO, Imath, KDDockWidgets). `actions/cache` keys on `hashFiles('.craft.ini')` — the first run pays the full ~60-120 min; warm runs restore in minutes. Runs entirely on the GitHub runner — zero local disk.
- **Phase 2 (every run):** the FrameForge checkout is configured, built, and `ctest`-ed against the Craft-installed deps (`BUILD_TESTING=ON`), which runs the Catch2 suite including the FrameForge DSP tests from slices 5-12. Test evidence and the built binaries upload as artifacts.

**Honest caveats, stated in the workflow header:** the CraftMaster settings schema and the craft-environment activation are the two blocks that follow the documented pattern but may need one iteration on the first remote run (pointers to the craftmaster README and Craft docs are in the file). Requires pushing this repo to GitHub — the user's decision; public repos run free.

**Not verified:** the workflow itself cannot be validated locally (by design — it needs the remote runner). It is a draft expected to converge in one or two remote iterations.

**Status:** draft, ready for the GitHub push decision.

---

## Session 2 (continued) — 2026-10-13 (slice 16: public-repo hygiene + GitHub push)

**Public-repo hygiene:** in preparation for pushing to the now-public GitHub repository, the raw Adobe documentation dump (`research/adobe/raw/audio-crossfade-transitions.md`) was removed from tracking (kept locally; `research/adobe/raw/` was already gitignored — this file had been force-added as evidence). It is Adobe-copyrighted content; the parity matrix contains only short quotations with source URLs, which is the appropriate form for public redistribution. `research/adobe/raw/MANIFEST.md` (a list of source URLs and fetch dates — factual metadata) stays tracked. Local username paths in this log were scrubbed to `C:\Users\<user>`. A secrets scan over all tracked files found no tokens, keys, passwords, or emails. The workflow trigger was set to `workflow_dispatch` only, per the user's requirement that no remote build start before the push is verified. The developer email is protected in the pushed history via GitHub's noreply address on the squash commits.

**Push strategy (Option B, per user decision after Option A was blocked by the 50 GB floor):** the shallow clone cannot be pushed (GitHub rejects shallow pushes) and the full upstream history (~4.85 GB per the GitHub API) would drop free space to ~43.9 GB, below the floor. Option B: squash into two commits — commit 1 = pristine upstream import (tree of `fed4998`), commit 2 = all FrameForge work squashed — preserving the upstream reference, attribution, and license notices (the complete corresponding source ships with all file headers; upstream remains available at the recorded URL).

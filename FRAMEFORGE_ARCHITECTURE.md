# FrameForge Pro — Architecture Document

**Date:** 2026-10-12 · **Milestone:** 0 (audit) · **Branch:** `frameforge-dev` @ `fed4998`
**Baseline:** Authentic KDE Kdenlive source tree, verified against `github.com/KDE/kdenlive` (local HEAD is the parent of upstream master `9d98e2d`, 2026-10-12). Version 26.11.70-dev. License: GPL-3.0 (see `THIRD_PARTY_NOTICES.md`).

Strategy: keep Kdenlive as the working foundation; rebrand progressively; introduce FrameForge's own media engine *behind interfaces* with MLT retained as fallback; never break the working build. All statements below are verified against the local tree unless marked *(to verify)*.

---

## 1. Current architecture map (verified)

### 1.1 Process core
- `src/core.h/.cpp` — `Core` singleton (`pCore`), created before MainWindow. Owns/mediates: `ProjectManager`, `Bin`, `MonitorManager`, `MixerManager`, `LibraryWidget`, `TextBasedEdit`, `TimeRemap`, `SubtitleModel`, `JobManager` (`src/jobs/taskmanager.h`), shared `QQmlEngine`.
- **MLT leak #1:** `core.h` includes `<mlt++/MltPlaylist.h>` / `MltProfile.h` directly. Interface work must start here.
- `src/main.cpp` — entry point; `src/mainwindow.*` + `mainwindow_actions.cpp` — main shell and actions; `src/kdenlivesettings.kcfg` — central settings (KConfig XT).
- Docking: **KDDockWidgets-qt6 2.4** (customizable workspace); UI resources: `kdenliveui.rc` (XMLGui), `uiresources.qrc`, `icons.qrc`.

### 1.2 MLT integration layer
- `src/mltconnection.h/.cpp` — `MltConnection`: initializes the MLT factory, owns `Mlt::Repository`, locates `melt` + profile paths, refreshes luma lists.
- `src/mltcontroller/clipcontroller.*` — `ClipController`: wraps a `Mlt::Producer` for each bin clip (metadata, timecode, frame access, `producer()->get_frame()`). `clippropertiescontroller.*` — clip property queries.
- `src/mlt_config.h.in` — build-time MLT version defs.
- **Leak #2:** monitors, scopes and audio-level jobs consume `Mlt::Frame` objects emitted by ClipController/RenderController.

### 1.3 Timeline model (Qt Model/View + QML view)
- `src/timeline2/model/timelineitemmodel.*` — `TimelineItemModel`: the tree model (tracks, clips, compositions) all UI reads.
- `clipmodel.*`, `trackmodel.*`, `compositionmodel.*` (transitions), `groupsmodel.*` (grouping), `snapmodel.*` + `clipsnapmodel.*` (snapping), `moveableitem.*` base, `timelinefunctions.cpp` (all editing operations), `builders/` (build MLT tractor XML from model).
- Undo/redo: `src/undohelper.hpp` + `DocUndoStack` — every model mutation is a reversible command.
- View: `src/timeline2/view/` — **Qt Quick (QML)** timeline rendering; JS/C++ bridge via `monitorproxy` pattern. ToolType tools (razor, spacer, ripple…).

### 1.4 Effects, transitions, keyframes (asset system)
- `src/assets/abstractassetsrepository.*` — loads asset definitions; `src/assets/model/` — `AssetParameterModel` (parameters, keyframe-aware); `src/assets/keyframes/` — `KeyframeModel`, keyframe views/curves; `src/assets/view/` — effect-stack UI; `src/assets/assetlist/` — effect/transition browser with search + favorites.
- **Effect definitions are XML files** in `data/effects/` (93 files) + `data/effects/{avfilter,frei0r,ladspa,movit,sox}/` mapping parameters to **MLT services**. `included/excluded/hidden/preferred_effects.txt` curate the list. XSD: `data/kdenlive-assets.xsd`.
- **GPU today:** `data/effects/movit/` — 14 GPU effects (blur, glow, vignette, white balance, lift/gamma/gain, saturation, unsharp mask, opacity, mirror…) via MLT's movit (OpenGL) module.
- Transitions: `data/transitions/` (affine, composite, dissolve, luma, matte, mix, qtblend, region, slide, wipe, autoduck) + luma wipe images in `data/lumas/`.
- `data/luts/` (LUT library), `data/profiles/` + `profiles.xml` (MLT profiles), `src/profiles/` (ProfileModel).

### 1.5 Project format, bin, media
- `src/doc/` — `KdenliveDoc`: `.kdenlive` project XML (Kdenlive schema wrapping MLT producer/tractor XML), autosave/backup, guides, markers, `EffectStackModel` per clip/track.
- `src/bin/` — `ProjectItemModel`, clip list/bin views, drag-drop import, thumbnails, clip jobs (proxy, audiolevels, stabilization via `vidstab`).
- `src/project/` — `ProjectManager` (open/save/close/recent). `src/otio/` — OpenTimelineIO import/export. `src/xml/` — MLT XML helpers.

### 1.6 Playback, monitors, scopes, mixer
- `src/monitor/` — `AbstractMonitor` → `Monitor` (+ `MonitorManager`); **platform display widgets already exist: `d3dvideowidget.*` (Direct3D, Windows), `openglvideowidget.*`, `metalvideowidget.mm` (macOS)**, generic `videowidget.*`; `monitorproxy.*` + `qmlmanager.*` bridge to QML overlay scene (safe zones, guides).
- `src/scopes/` — vectorscope, waveform, RGB parade *(to verify: exact scope list in M1)*.
- `src/audiomixer/` — `MixerManager`, per-track volume/mute/solo, meters.
- Audio playback: MLT consumer audio + `MediaCapture` for recording; `src/jobs/audiolevels*` for waveforms.

### 1.7 Render/export, jobs
- `src/render/` — render controller building MLT jobs (consumer = avformat via MLT/FFmpeg); `src/renderpresets/` — encoding profiles; `data/encodingprofiles.rc`, `data/kdenlivetranscoding.rc`.
- `src/jobs/` — `JobManager`/`TaskManager`: proxy generation, audio levels, stabilize, transcode, clip thumbnail jobs, cancellable background `QThreadPool` tasks.
- Proxy system: `data/externalproxies.rc`, KdenliveSettings proxy params.

### 1.8 Graphics/titles
- `src/titler/` — QML-based titler; `src/qml/` — shared QML; templates in `data/titles/`; `glaxnimatelauncher.*`; also `dynamictext.xml` (timecode/name burn-in effect).

### 1.9 Tests (important — our safety net)
- `tests/` — **Catch2** (vendored `catch.hpp`) + **fakeit** mocks; ~40 suites: modeltest, trimmingtest, effectstest, keyframetest, effectsgrouptest, movetest, mixtest, snaptest, nestingtest, sequencetest, documenttest, otiotest, timewarptest, subtitlestest, timelinepreviewtest, colorscopestest, audiolevelstasktest, avcurvetest, cachetest… plus `small.mkv` test asset and `test_utils`.
- CI: `.gitlab-ci.yml`; clang-format/tidy configs, codespell, REUSE licensing checks present.

---

## 2. FrameForge engine — target module design

New code lives in `src/frameforge/` (pure interfaces + implementations). MLT stays the default backend behind the same interfaces until each replacement is proven (feature flag: `FrameForgeSettings::engineBackend = "mlt" | "frameforge-native"`, per-project override, automatic fallback on failure).

| Module | Interface (planned) | Backends |
|---|---|---|
| Media discovery & metadata | `IMediaBackend::probe(url) → MediaInfo` | `MltMediaBackend` (MLT properties/ffprobe), later native MF |
| Decoding | `IVideoDecoder`, `IAudioDecoder` (async, cancellable, frame-accurate seek) | MLT producer; later Media Foundation (H.264/H.265 first) |
| Frame scheduling & presentation | `IFrameScheduler` (play clock, audio-master sync) | MLT consumer; later native WASAPI+D3D11 presenter |
| Timeline scheduling | reuse `TimelineItemModel` + builders | — |
| Compositing | `ICompositor` (track stack, transforms, opacity, blend modes) | MLT tractor; later D3D11 compositor |
| GPU rendering | `IRenderBackend` | `MltRenderBackend`; `D3D11RenderBackend` generalizes `d3dvideowidget` |
| Effects | `IEffect` registry (id, version, params, keyframes, CPU/GPU caps, tests) | MLT services adapter; native CPU (SIMD) + GPU (HLSL) implementations |
| Keyframe evaluation | reuse `KeyframeModel` (linear + Bezier) | — |
| Proxies & cache | extend job system + `cachetest` patterns | bounded LRU frame cache, proxy profiles |
| Thumbnails/waveforms | extend existing jobs | — |
| Color management | color-space tags, GPU conversion, LUT (.cube) | OCIO-style transform chain *(design in M4)* |
| Encoding/export | `IExporter` (progress, cancel, error recovery) | MLT/FFmpeg; later MF hardware encode |
| Plugins | `IEffectPlugin` ABI + manifest | static registry; dynamic loading *(design in M6)* |

**Seams to fix first (leak list from §1):** (1) de-MLT `core.h` (move `MltPlaylist/MltProfile` members behind an interface), (2) define `IFFFrame` value type replacing `Mlt::Frame` at monitor/scope boundaries, (3) route all effect instantiation through `IEffectRegistry` instead of direct MLT service strings, (4) isolate `builders/` MLT-XML generation behind `IProjectSerializer`.

**Windows-native pipeline (M4):** Media Foundation decode (H.264/H.265 8/10-bit, MP4/MOV/MKV), D3D11 render backend + DXVA/D3D11 video access where supported, WASAPI audio. First target: the AMD Radeon integrated GPU on this dev machine (Direct3D 11 FL 12_x — *to verify feature level in M4*). FFmpeg remains for exotic formats; codec support matrix + measured benchmarks required before any "native by default" switch.

---

## 3. Development environment (audited 2026-10-12)

| Component | Status |
|---|---|
| OS | Windows 11 Pro build 26300, x64 |
| CPU/RAM/GPU | 15.3 GB RAM; AMD Radeon (TM) integrated graphics, driver 31.0.21914.18001 |
| Free disk (C:) | ~60.7 GB |
| Git | ✔ (repo = clean clone of KDE/kdenlive) |
| CMake / Ninja / MSVC / MinGW | ✘ none installed |
| Qt 6.10 / KF6 6.21 / MLT 7.38+ / OTIO / Imath / KDDockWidgets | ✘ none installed |
| FFmpeg 8.1.2 CLI (winget full build) | ✔ available for test-media generation (not the app's runtime lib) |
| DaVinci Resolve 21.1.0.17 | ✔ installed at `C:\Program Files\Blackmagic Design\DaVinci Resolve\Resolve.exe` |
| REA MCP | ✔ connected (170 tools, win32/x64) — Ghidra provider **not configured** (needs Ghidra 12.1.x + JDK 21, ~1.5 GB total) |
| davinci-resolve MCP | ✔ available (162 tools, requires CursorBridge script running inside Resolve) |

**Recommended build bootstrap (least disruptive):** KDE Craft — Kdenlive's own Windows blueprint is in-tree (`.craft.ini`); `craft kdenlive` installs Qt/KF6/MLT/dependencies reproducibly (~10–15 GB with Qt + KF6 + MLT + toolchain). Alternative: Visual Studio 2022 + Qt online installer + vcpkg (more manual control, harder to match Kdenlive's expected versions).

---

## 4. REA / DaVinci Resolve investigation plan

Status: REA MCP connected; deep native analysis **blocked** until Ghidra 12.1.x + JDK are installed and `GHIDRA_INSTALL_DIR` set (reported, not auto-installed). Format-level REA tools (artifact inspection, managed-PE analysis) are already usable.

Planned investigations (each produces an engineering record under `research/rea/`):
1. Resolve's public Python scripting API (ships in `Developer\Scripting\`) — timeline/clip property model, interchange behavior. *No REA needed — public docs on disk.*
2. Via davinci-resolve MCP (behavioral, non-invasive): timeline marker model, clip property changes, render settings — informs FrameForge's equivalent UX design.
3. Via REA+Ghidra (after user approval of install): a bounded binary question, e.g. which export pipelines Resolve.exe's launcher wires, or the D3D presentation path in a small Resolve DLL — only where public behavior is ambiguous and licensing is respected (no decompilation of DRM/licensing components; no code copying).

## 5. Milestone roadmap

| M | Goal | Acceptance |
|---|---|---|
| 0 (done) | Architecture, parity matrix, license audit, branch, research archive | This doc set; `PREMIERE_FEATURE_PARITY.md`; `THIRD_PARTY_NOTICES.md` |
| 1 | Build the **unchanged** baseline on Windows via Craft; run the Catch2 suite; record baseline startup/scrub/playback/export numbers | `kdenlive.exe` runs; `ctest` green; `research/benchmarks/baseline.md` |
| 2 | Reliability pass: import/save/reopen, editing, undo/redo, playback, audio, transition, export — verified by tests | manual QA checklist + new tests |
| 3 | Introduce `src/frameforge/` interfaces; isolate MLT behind them; feature flag; fallback proven | all existing tests still green with either backend flag |
| 4 | Native D3D11 render + MF decode prototype behind flag; correctness (frame hash compare) + benchmarks | published benchmark harness vs baseline |
| 5 | Core native effects: transform, opacity, crop, fades, blur, color basics + keyframes (GPU-first) | unit tests + visual golden tests |
| 6 | Effects expansion per parity matrix P1/P2; plugin SDK; effect browser UX | parity matrix statuses updated with test evidence |
| 7 | Advanced: proxies, multicam, EDL/AAF, color management, optional local AI | per-feature acceptance in matrix |
| 8 | Performance & release readiness: regression suite, profiling, crash handling, dependency audit, storage report | release checklist doc |

Non-goals: cloud AI features (Generative Extend etc. — see parity §G), .mogrt compatibility, VR/Immersive (deferred P3), obsolete Premiere effects.

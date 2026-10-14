# FrameForge Pro — Third-Party Notices & License Audit

**Audit date:** 2026-10-12 · **Auditor:** FrameForge Pro development session (Milestone 0)

FrameForge Pro is developed on top of the KDE Kdenlive source tree and remains Free Software. **All FrameForge Pro builds are GPL-3.0-covered** while they contain Kdenlive code. New FrameForge modules MUST be GPL-3.0-compatible. This file is the living license manifest; update it with every dependency change.

## 1. Base codebase

| Component | License | Evidence |
|---|---|---|
| Kdenlive source tree (this repo, all `src/`, `data/`, top-level tools) | GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL (per-file SPDX headers; whole-project default GPL-3.0) | `COPYING` (GPL-3.0 full text), per-file `SPDX-License-Identifier` headers, `REUSE.toml`, `LICENSES/` |

The repository is REUSE-compliant: every file carries SPDX metadata; `LICENSES/` holds the approved license texts (Apache-2.0, BSD-2-Clause, BSD-3-Clause, BSL-1.0, CC-BY-SA-4.0, CC0-1.0, GPL-2.0-only, GPL-3.0-only, GPL-3.0-or-later, LGPL-3.0-only, LGPL-3.0-or-later, LicenseRef-KDE-Accepted-GPL, LicenseRef-KDE-Accepted-LGPL, MIT).

**Obligations carried forward:** preserve copyright headers and license notices in every file we modify; state significant changes in modified files (GPL §5); keep the source available to any party we distribute binaries to; never relicense covered files.

## 2. Required dependencies (per root `CMakeLists.txt`, verified 2026-10-12)

| Dependency | Version req. | License | Verify how |
|---|---|---|---|
| Qt 6 (Core, Widgets, Svg, Quick, QuickControls2, Concurrent, QuickWidgets, Multimedia, MultimediaWidgets, NetworkAuth, SvgWidgets, Xml, DBus*) | ≥ 6.10.0 | LGPL-3.0/GPL-3.0/Commercial (per module; KDE uses GPL/LGPL releases) | Qt licensing doc at install time; Craft pins the open-source release |
| KDE Frameworks 6 (I18n, Archive, Bookmarks, Codecs, CoreAddons, Config, ConfigWidgets, KIO, WidgetsAddons, NotifyConfig, NewStuff, XmlGui, Notifications, GuiAddons, TextWidgets, IconThemes, Solid, FileMetaData, Purpose, DocTools*, Crash*, DBusAddons*) | ≥ 6.21.0 | Mixed per-framework: mostly LGPL-2.1+/BSD/GPL-2+ (each KF ships its own LICENSES) | per-framework `LICENSES` at pinning time |
| MLT 7 (`xml` component) | ≥ 7.38.0 | **LGPL-2.1** (verified via GitHub API 2026-10-12: spdx `LGPL-2.1`) | `mltframework/mlt` repo license |
| FFmpeg (AVFORMAT, AVCODEC, SWRESAMPLE, AVUTIL) | – | LGPL-2.1+ **or** GPL-2+ depending on build configuration; binary distribution must match linked build's license; H.264/H.265 codecs carry patent considerations — FrameForge makes no universal-compat claim | `FFmpeg COPYING.LGPLv2.1`/`COPYING.GPLv2` of the exact build used; record build flags |
| OpenTimelineIO | ≥ 0.18 recommended | Apache-2.0 (ASWF project) — *verify at pinning* | upstream `LICENSE` |
| Imath | – | BSD-3-Clause — *verify at pinning* | upstream `LICENSE.md` |
| KDDockWidgets-qt6 | ≥ 2.4.0 | GPL-3.0-or-later (or commercial) — *verify: this is a hard GPL dependency for the dock UI* | upstream `LICENSES/` |
| extra-cmake-modules (ECM) | ≥ 6.21.0 | BSD-2/3-Clause | KDE repo |
| SharedMimeInfo | – | GPL-2.0+ (data) | upstream |
| DrMinGW (Windows, crash handler, optional) | – | Apache-2.0 — *verify* | upstream repo |
| RTTR (optional, fuzz tests) | 0.9.6 | MIT | upstream `LICENSE` |
| Catch2 (vendored `tests/catch.hpp`) | – | BSL-1.0 | `LICENSES/BSL-1.0.txt` in-tree |
| fakeit (vendored `tests/fakeit.hpp`) | – | MIT | upstream repo |
| Shared tools: clang-format/tidy configs, codespell, REUSE | – | tooling, permissive | – |

\* = optional components.

## 3. Runtime/bundled content in `data/`

- Effect/transition definitions (XML) — Kdenlive project, GPL-3.0 (in-tree).
- Luma wipe images (`data/lumas/`), LUTs (`data/luts/`), profiles — in-tree GPL unless a file declares otherwise (REUSE covers it); verify individual LUT provenance before any separate redistribution *(M1 task)*.
- Translations `po/` — GPL, KDE teams.

## 4. Planned FrameForge additions — license policy

| Planned addition | Candidate | License plan |
|---|---|---|
| Windows-native decode/encode | Microsoft Media Foundation (OS API), Direct3D 11, WASAPI (OS APIs) | System APIs — no distribution obligations; requires Windows SDK only |
| GPU shaders for effects | Self-written HLSL | GPL-3.0 (own code) |
| SIMD effect kernels | Self-written or rework of public-domain implementations | Must be GPL-3.0-compatible; record provenance per function |
| OpenColorIO (color management, if used) | OCIO 2 | BSD-3-Clause — verify; dynamic link |
| Local AI models (STT etc.) | VOSK (Apache-2.0) / whisper.cpp (MIT) | Verify at adoption; models downloaded only with user consent; disclose sizes |
| Any FFmpeg alternative bundling | – | Audit before bundling; prefer system/MLT-linked |

**Hard rules**
1. Do not bundle binaries from untrusted sources; pin everything through the build system.
2. No secrets/API keys in the repo; no telemetry services.
3. Every new dependency → new row in §2 with license evidence before merging.
4. Do not copy proprietary code, assets, or branding from Premiere Pro or DaVinci Resolve (see `PREMIERE_FEATURE_PARITY.md` — features are reimplemented independently; Adobe documentation describes behavior only).
5. Keep GPL-3.0 statements intact when rebranding the application.

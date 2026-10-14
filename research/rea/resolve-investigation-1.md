# REA / DaVinci Resolve — Engineering Record 1

**Date:** 2026-10-12 · **Record:** resolve-investigation-1
**Feature investigated:** Deployment architecture of a professional Windows NLE (DaVinci Resolve 21.1) as engineering reference for FrameForge Pro's Windows work (UI toolkit stack, GPU/display plugin layout, scripting runtime, project storage).

## 1. Target artifact

`C:\Program Files\Blackmagic Design\DaVinci Resolve\`
- `Resolve.exe` — version 21.1.0.17, 608,492,984 bytes (observed via file version info, 2026-10-12).

## 2. Tools and analysis methods used

- **REA MCP** (per user directive: REA-only for Source B): `binary_session` doctor output, tool-catalog searches (`inspect_artifact`, `inspect_managed_artifact`, `inspect_binary_layout`, `open_binary` capability checks).
- **Filesystem observation** (read-only directory listings; no execution, no modification).
- **MLT source reading** (upstream LGPL-2.1 code, `transition_mix.c`) — for the related crossfade investigation, not for Resolve.

## 3. Evidence

| Observation | Source | Confidence |
|---|---|---|
| REA connected: 170 tools, win32/x64 host | binary_session | Observed |
| **Native PE deep analysis is BLOCKED**: Ghidra provider not configured (`GHIDRA_INSTALL_DIR` unset, no JDK); user declined installing Ghidra/JDK to protect storage. `inspect_binary_layout` is ELF-only (Linux); `inspect_artifact` requires an open archive/app-package target (none applicable to a bare PE). | binary_session + tool catalog | Observed |
| Resolve 21.1 ships a **Qt deployment** on Windows: `Qt/`, `QtQml/`, `QtQuick/`, `QtQuick.2/`, `QtGraphicalEffects/`, `scenegraph/`, `platforms/`, `styles/`, `imageformats/`, `sqldrivers/` | filesystem listing | Observed (high) |
| Embedded scripting runtime: `ResolvePython/` contains `ResolvePython.exe`, `python314.dll` (Python 3.14), `vcruntime140.dll` | filesystem listing | Observed |
| Other top-level components: `Fusion/`, `audio/`, `CanonLUTs/`, `PGTools/`, `Plugins/`, `Skins/`, `UI_Resource/`, `Control/`, `Documents/` (contains `DaVinci Resolve.pdf`, `License.html`, `Third Party Licenses.rtf`) | filesystem listing | Observed |
| No `Developer\Scripting` folder in this install (public scripting-API docs path absent) | filesystem listing | Observed (absence) |

## 4. Confidence and uncertainty

- Deployment facts above: **high confidence** (directly observed, no inference).
- **Nothing about Resolve's internal implementation is claimed.** No decompilation, disassembly, or runtime tracing was performed — the required capabilities are not installed (Ghidra) or were declined (Resolve MCP/CursorBridge per user; no DaVinci Resolve Studio license).
- The observable Qt deployment does **not** prove which Qt version/components Resolve links at runtime; directory presence is the only evidence.

## 5. FrameForge implementation approach derived

- **None derived from Resolve internals.** The observation that a leading professional Windows NLE ships a full Qt stack validates Kdenlive's Qt-based architecture choice for FrameForge's Windows target (Qt + native widgets/plugins is a viable pro-NLE stack on Windows).
- The `platforms/`, `imageformats/`, `sqldrivers/` plugin layout is consistent with standard Qt deployment practice FrameForge will follow when packaging (standard Qt plugin directories, not copied assets).
- Embedded Python runtime: noted as a *reference pattern* for FrameForge's future scripting/plugin extensibility (documented public interfaces only; no code or assets taken from Resolve).

## 6. Tests

Not applicable — investigation record; no FrameForge behavior changed by this record.

## Blocker statement (per project rules)

Native binary analysis of `Resolve.exe` (or any Resolve DLL) is **blocked** in this session because the REA Ghidra provider is not configured and the user has declined the ~1.5–2 GB Ghidra+JDK install to protect disk space. Behavioral/scripting investigation via the davinci-resolve MCP is **not authorized** by the user (no Resolve Studio license). If a future feature genuinely requires native-binary evidence, the user can revisit the Ghidra decision; until then, Source B evidence is limited to observable deployment structure and public documentation.

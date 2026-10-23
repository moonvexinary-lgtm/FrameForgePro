# Architecture decision record — rendering and media backend

Status: **accepted** · Date: 2026-10-22 · Supersedes: none

## Context

FrameForge Pro is a Windows-11-first, Rust-native editor built on two compatible MIT/Apache Rust engines
(FilmCraft for editing, EffectCraft for compositing). The brief asks for an explicit comparison of a
Rust-first core against FFmpeg-, MLT- and GStreamer-based backends before a high-cost change, plus a decision
on how XMem (a PyTorch project) fits a Rust application.

Constraints that dominate the decision:

- AMD Ryzen 5 7530U, integrated Radeon, 16 GB RAM, ~238 GB SSD, **no guaranteed CUDA**.
- Cloud builds only; a thick native SDK or dozens of DLLs is a deployment and CI cost.
- No paid SDKs; offline editing must work; no mandatory AI.
- Preview and export must show the same picture.
- Existing Rust code works and is CI-verified; a rewrite is forbidden by the brief.

## Options considered

| Criterion | A. Rust-first (current) | B. FFmpeg filters/encode backend | C. MLT framework | D. GStreamer Editing Services |
|---|---|---|---|---|
| Effect coverage | 307 EffectCraft effects + ≥60 FilmCraft effects, all through one real path | Very large (FFmpeg filters) | Large via MLT/frei0r modules | Large via plugins |
| Preview/export parity | Guaranteed: one renderer, one source path | Two graphs unless the timeline is rendered by FFmpeg entirely | Better if MLT renders both | Same, if GES renders both |
| Windows packaging | One exe (~61 MB) + system libraries; no DLL farm | LGPL FFmpeg DLLs, licence-configurable builds, ~50–150 MB | MLT + Qt + modules + plugins; deployment-heavy | GStreamer runtime + plugins; deployment-heavy |
| Rust integration | Native crates, compile-time API | `ffmpeg-next`/`rsmpeg` bindings or a sidecar process; C ABI | C API; Rust bindings immature | `gstreamer-rs`, mature but large |
| AMD iGPU | CPU path proven; GPU effects possible with wgpu later | Hardware encoders detected at runtime; software fallback | Similar | Similar |
| Licences | MIT/Apache throughout | LGPL builds only; some filters pull GPL | LGPL + per-plugin audit | LGPL + plugin audit |
| Effort to adopt | zero (already working) | months to port the timeline/comp model | large; would replace the project model | large; would replace the project model |
| Maintenance risk | two pinned engines | bindings + native runtime matrix | framework + KDE stack | framework + plugin matrix |
| Offline | yes | yes | yes | yes |

## Decision

1. **Keep the Rust-first architecture.** The verified FilmCraft + EffectCraft integration stays the rendering
   authority: one `MediaSource` path (`ComposedSource`), one timeline renderer, one export pipeline. This is
   the only option that preserves preview/export parity today and keeps the binary self-contained.
2. **Do not adopt MLT or GStreamer.** Both would replace the working project/timeline model and add a large
   native deployment surface for capability the current engines already provide. Revisit only if a specific
   missing capability is demonstrably cheaper through one of them than through the Rust engines.
3. **FFmpeg is a future, optional codec/encode accelerator, not the effect engine.** If a needed codec is
   missing, add an LGPL-configured, dynamically loaded FFmpeg build behind a capability-detected adapter
   (import and/or export), with the software Rust path retained and tested. No GPL components, no mixing of
   project models, and a per-build licence record in `THIRD_PARTY_NOTICES.md` before bundling. This is not in
   the current dependency graph and is not promised until a prototype passes the same decode/verify tests.
4. **GPU work uses `wgpu` only where measured.** The EffectCraft CPU path is the reference; an optional wgpu
   path must be validated on the target AMD iGPU against reference frames, and must fall back cleanly. No
   "GPU accelerated" claim until the test exists.
5. **XMem runs out-of-process.** The official PyTorch implementation is the only temporal propagation engine.
   It is isolated in an optional worker (bundled embedded CPython + CPU PyTorch in an opt-in AI pack, or a
   user-provided environment), speaking a versioned JSON-lines protocol. The Rust editor never links PyTorch
   and never requires system-wide Python. Masks produced by the worker are cached as files and consumed by
   the same render path as every other effect. CUDA is never assumed; DirectML/Vulkan acceleration is
   investigated only after a CPU benchmark exists. Details and staged acceptance: `docs/ROTO_AI_PLAN.md`.

## Consequences

- Positive: the working editor stays shippable; one picture for preview and export; simple Windows deployment;
  licences stay clean; XMem can grow without destabilising the core.
- Negative: codec coverage is limited to the Rust codecs until FFmpeg is added; CPU effect rendering will be
  slower than mature GPU pipelines; the AI pack will be large (PyTorch + weights) and CPU inference may be
  slow enough to require proxies/preview-quality limits.
- Guardrails: any backend addition must ship with (a) a licence record, (b) a real render/import test,
  (c) measured package-size impact, (d) a tested fallback.

## Re-evaluation triggers

Revisit this ADR if any of these becomes true:

- A required delivery codec cannot be produced acceptably by the Rust encoders and an LGPL FFmpeg prototype
  passes the verification suite and size budget.
- Timeline/comp performance on the target laptop misses the playback target even after preview quality and
  caching, and profiling shows the bottleneck is the Rust compositor.
- An official GPU/ONNX path for XMem suitable for AMD hardware is published and benchmarked.

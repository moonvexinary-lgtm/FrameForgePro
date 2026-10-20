# FrameForge Pro — Architecture

Last updated: milestone M1 (2026-10-19) · Branch: `frameforge-rust-rewrite`

## Goal

One native Windows application that unifies:

1. nonlinear editing (the FilmCraft engine),
2. compositing, effects and motion graphics (the EffectCraft engine),
3. the FrameForge Pro identity and professional editing workflows.

Premiere Pro is a *behavioural reference for workflows only*. No Adobe code, assets, or
artwork. The legacy C++/Qt application is not part of this architecture; it remains
recoverable on `frameforge-dev` and the `frameforge-cpp-legacy` tag.

## Layered plan

| Layer | Responsibility | Status |
|---|---|---|
| L6 apps | `frameforge-pro` binary; workspaces: Edit / Effects / Color / Audio / Export | M1: self-test binary |
| L5 ui-egui | unified shell: docking, timeline, monitors, panels (egui — same UI stack as both upstreams) | planned |
| L4 engine | unified session: project, commands, undo; timeline items reference compositions | planned |
| L3 render/export | FilmCraft track compositor + EffectCraft layer compositor behind adapters; one export pipeline | planned |
| L2 edit/effects | edit algebra; effect-bridge: EffectCraft effects on FilmCraft timeline clips, shared keyframes | planned (next: M2) |
| L1 media | FilmCraft codecs/media (already shared by both engines upstream) | upstream, pinned |
| L0 foundation | time, geometry, colour | M1: time (verified) |

## M1 — what exists now

- **`crates/core` (`frameforge-core`)**: the shared foundation. The canonical time is
  FilmCraft's `Tick` / `FrameRate` / `TimeRange`; conversions to EffectCraft's identical
  types are zero-cost field moves. `verify_foundation()` cross-checks every shared
  property (time base, lossless conversion, audio-sample and container-timestamp math,
  frame duration/indexing/snapping on all 11 broadcast rates) against both engines; the
  test suite and the shipped binary both run it.
- **`apps/frameforge-pro`**: the application binary. At M1 it is the application's own
  self-test — real checks with a real exit code, not a placeholder.
- **CI** (`.github/workflows/ci.yml`, `windows-2025`): rustfmt check, clippy
  `-D warnings`, `cargo check`, `cargo test`, release build, binary self-test,
  disk-space reports before and after. **All compilation is cloud-only.**

## Milestones

| # | Milestone | Status |
|---|---|---|
| M1 | shared time foundation + cloud CI | ✔ (CI-verified) |
| M2 | integration proof: timeline clip + real EffectCraft effect + keyframed parameter; save/reopen; render; export; verified output (synthetic media) | next |
| M3 | unified project format (`.ffproj`), media references, effect registry bridge | planned |
| M4 | unified engine: timeline items reference compositions; nested compositing in the timeline | planned |
| M5 | application shell: egui workspaces over the unified session | planned |
| M6 | feature parity + machine-readable effects/feature registry and audit | planned |
| M7 | reliability: recovery, cancellation, missing media, audio sync, performance | planned |
| M8 | packaging: Windows release on CI, downloadable artifacts | planned |

## Dependency policy

- Upstream engines are consumed as **pinned git dependencies** (exact revisions in the
  root `Cargo.toml`), not copied source. FilmCraft is pinned to the *same revision the
  EffectCraft workspace builds against*, so both engines share one copy of each
  `filmcraft-*` crate in the graph (no duplicate media/time/colour engines).
- Licenses: MIT OR Apache-2.0 / BSD / ISC / Zlib / CC0 / Unicode / BSL only — the same
  allowlist both upstreams enforce. No GPL code, ever, in this branch.
- The workspace builds self-contained on GitHub Actions; no absolute local paths, no
  sibling-checkout requirements.

## Constraints honored

- Cloud-only builds; the local machine carries only git and a minimal Rust toolchain
  (formatting only; ~760 MB).
- Disk floor: ≥ 50 GB free on C: monitored before every local step.
- Honest status: a milestone is ✔ only when GitHub Actions proves it.

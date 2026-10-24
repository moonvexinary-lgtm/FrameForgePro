# FrameForge Pro

A unified, Rust-native video editor for Windows: **FilmCraft's** nonlinear editing engine
and **EffectCraft's** compositing and motion-graphics engine in one application, with
Premiere Pro-inspired professional workflows.

> **Status: milestone M12 — a working prototype editor, not yet a Resolve competitor.**
> The complete core journey runs on real code and is CI-verified: import media, edit a
> multitrack timeline, apply a real effect, preview it, save, reopen, export, and verify
> the rendered file. M9 added crash-recovery autosaves, an unsaved-changes guard, and
> keyframe-preserving parameter writes; M10 adds per-track targeting/source patching and
> placement on V2/A2; M11 puts timeline and composition/effect edits into one
> chronological undo history, so Ctrl+Z never undoes an unrelated edit; M12 adds export
> overwrite confirmation, real cancellation with partial-file cleanup, and a Save /
> Discard / Cancel guard when the window closes. Roto AI (XMem)
> is **designed, not implemented**:
> see [docs/ROTO_AI_PLAN.md](docs/ROTO_AI_PLAN.md). Nothing is stubbed or faked; the
> honest gap list is [docs/FEATURE_PARITY.md](docs/FEATURE_PARITY.md) and the current
> baseline audit is [docs/BASELINE_AUDIT.md](docs/BASELINE_AUDIT.md).

## One application, two engines

| Engine | Provides | Upstream |
|---|---|---|
| FilmCraft (`filmcraft-*`) | project model, bins, multitrack timeline, frame-accurate trimming, colour, audio mixing, pure-Rust media codecs, export | [storytold/filmcraft](https://github.com/storytold/filmcraft) |
| EffectCraft (`effectcraft-*`) | layers, keyframes, masks, the effects catalog, text and shapes, tracking, 3D, GPU compositing with CPU fallback | [storytold/effectcraft](https://github.com/storytold/effectcraft) |

Both engines already share the exact same time representation — an integer `Tick` at
254 016 000 000 per second — which is the property the whole integration stands on.
`frameforge-core` owns that shared surface, cross-checks it against both engines
(`verify_foundation`), and grows it as the integration proceeds.

## Branches

| Ref | What it is |
|---|---|
| `frameforge-dev` | The original C++/Qt/QML/MLT application (Kdenlive fork, GPL-3.0). **Preserved unchanged** as the recoverable pre-rewrite state. |
| `frameforge-cpp-legacy` (tag) | Marks the last C++ commit (`2601eb5`) before the rewrite. |
| `frameforge-rust-rewrite` (branch) | **This branch** — the active Rust rewrite. |

The legacy application is not built, bundled, or hidden underneath anything here: it is
gone from this working tree and remains fully recoverable on the refs above.

## Development workflow — cloud builds only

All compilation and testing runs on GitHub Actions; the local machine needs only git
(plus an optional minimal Rust toolchain for formatting).

1. Edit, commit, push to `frameforge-rust-rewrite`.
2. CI (`windows-2025`) runs: rustfmt check, clippy (`-D warnings`), `cargo check`,
   `cargo test`, `cargo build --release`, and the built binary's self-test.
3. Fix what CI reports; iterate.

No local Cargo builds. No Qt, no MSVC Build Tools, no LLVM, no KDE Craft, no CUDA.

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE),
[NOTICE](NOTICE) and [docs/PROVENANCE.md](docs/PROVENANCE.md).

No GPL code from the legacy application is included in this branch. FrameForge Pro is
not affiliated with Adobe, KDE, or Blackmagic; no proprietary code or assets are
included.

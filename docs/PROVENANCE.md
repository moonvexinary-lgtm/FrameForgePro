# Provenance and licensing

| Component | Origin | License | How it appears here |
|---|---|---|---|
| Rust workspace, `frameforge-core`, `apps/frameforge-pro`, CI workflows, docs | FrameForge Pro (this project) | MIT OR Apache-2.0 | new (M1) |
| `filmcraft-time` and future `filmcraft-*` crates | storytold/filmcraft @ `2c5ba7a72619935f0481f99d59409e0d1844b234` | MIT OR Apache-2.0 | pinned git dependency |
| `effectcraft-time` and future `effectcraft-*` crates | storytold/effectcraft @ `ef74d82902ba486891b02afa87e42963cc9b84cc` | MIT OR Apache-2.0 | pinned git dependency |
| Legacy C++/Qt/QML/MLT application | KDE Kdenlive fork | **GPL-3.0** | **not included in this branch** |

## Why the legacy tree is not in this branch

The old application is GPL-3.0 C++/Qt/QML/MLT. The Rust-first requirement, and the
permissive-license rule both upstream engines enforce (their licensing rules forbid
mixing GPL code into them), mean no line of it can be carried into the new
implementation. It remains fully recoverable:

- branch `frameforge-dev` — untouched, exactly as it was;
- tag `frameforge-cpp-legacy` → `2601eb5`, the last C++ commit.

The upstream design decisions documented in the legacy tree (interface layout,
workflow choices, Windows-native pipeline notes) are used as *behavioural reference
only*, which is the same clean-room posture both upstream engines take toward the
professional editors they model.

## Attribution

FilmCraft and EffectCraft are Copyright (c) 2026 ArtCraft Team and their contributors.
Their crates are used as pinned dependencies and remain under their own licenses; their
copyright and license texts are preserved in the pinned sources. See `NOTICE`.

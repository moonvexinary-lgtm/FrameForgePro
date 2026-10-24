# Provenance and licensing

| Component | Origin | License | How it appears here |
|---|---|---|---|
| Rust workspace, `frameforge-core`, `apps/frameforge-pro`, CI workflows, docs | FrameForge Pro (this project) | MIT OR Apache-2.0 | new (M1; extended through R2) |
| `filmcraft-*` crates | storytold/filmcraft @ `6c3948a36c80e4996d924766ba7157eec4fca85e` | MIT OR Apache-2.0 | pinned git dependency |
| `effectcraft-*` crates | storytold/effectcraft @ `ef74d82902ba486891b02afa87e42963cc9b84cc` | MIT OR Apache-2.0 | pinned git dependency |
| XMem (future optional pack) | hkchengrex/XMem @ `f3b841d50df058910bbf690229ddc15fb1aef7d6` | MIT (code) | **not included**; optional out-of-process runtime, not bundled |
| XMem weights `v1.0` (`XMem.pth`, 237.5 MB) | XMem GitHub release | weight terms require review; not redistributed | **not included**; download only with explicit consent, checksum and notice |
| PowerShell fake worker (`crates/bridge/tests/fixtures/fake_xmem_worker.ps1`) | FrameForge Pro test code | MIT OR Apache-2.0 | test fixture only; not shipped |
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

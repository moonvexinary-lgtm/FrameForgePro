# Roto AI — XMem integration plan (mandatory engine)

Status: **design only — nothing is implemented or claimed.** This document defines the architecture,
licensing, storage and acceptance tests that must pass before any Roto AI control appears in the UI.

## Non-negotiables

- **XMem is the temporal propagation engine**, from the official implementation:
  `https://github.com/hkchengrex/XMem`, pinned at commit `f3b841d50df058910bbf690229ddc15fb1aef7d6`
  (2024-11-15). MobileSAM, SAM 2 and other models are not substitutes.
- Inference is **out of process**: the Rust editor never links PyTorch and never requires a system-wide
  Python installation.
- Model weights are **never bundled** with the core application and are downloaded only with explicit user
  consent, showing size, source, version, licence and hardware requirements.
- Initial mask generation is **separate** from XMem propagation.
- No fabricated progress, speed, memory, backend or accuracy values. The UI binds to worker state or shows
  "not measured".
- The cached masks must be the same bytes used by preview and final export.

## Audit facts (verified 2026-10-22)

| Item | Fact |
|---|---|
| XMem code licence | MIT (Copyright 2024 Ho Kei Cheng), verified from the repository `LICENSE`. |
| Pinned commit | `f3b841d50df058910bbf690229ddc15fb1aef7d6`. |
| Official weights | GitHub release `v1.0` (2022-07-06): `XMem.pth` **237.5 MB**; variants `XMem-s0/s01/s012/s2/no-sensory/with-mose` also 228–238 MB. |
| Requirements (upstream) | Python 3.8+, PyTorch 1.11+, torchvision, OpenCV, `requirements.txt`. |
| Windows support (upstream) | "I have briefly tested the GUI on Windows" — not a supported platform guarantee. |
| Upstream speed claim | ~20 FPS "hardware dependent" (GPU); **no CPU figure**, so CPU must be measured, not assumed. |
| Optional click model | The upstream demo uses f-BRS for click-to-mask. Its repository is currently unreachable via the GitHub API and its code/weight licences are **unverified**; it is not adopted in this plan until a separate audit succeeds. |
| Weight provenance risk | Weights are trained on research datasets (DAVIS / YouTube-VOS). The code is MIT; weight redistribution/training-data terms require legal review before any bundle. Until then: download-on-consent, user-local cache, no redistribution. |

## Architecture

```
Rust UI (Roto AI workspace)
   │  mask overlay, brush strokes, correction frames, progress
   ▼
Roto session (Rust, in-process)                     optional AI pack
   │  frame exporter (FilmCraft decode → PNG cache)   │
   │  protocol client (JSON lines over stdio) ────────►│  xmem_worker.py
   │  mask cache + invalidation                        │  official XMem (PyTorch CPU)
   ▼                                                   │  model loaded once
ComposedSource / renderer ◄── cached alpha PNGs ───────┘
```

- **Worker**: a small Python program using the official `inference/inference_core.py`
  (`XMemInferenceCore`) — not a reimplementation. It loads one model per process, receives frame
  directories and an initial mask, propagates forward/backward in a chosen range, writes 8-bit mask PNGs,
  and emits progress lines `{"event":"progress","frame":n,"total":m,"elapsed":s,"fps":x}`. Cancellation is a
  protocol message; if the worker hangs, the Rust side terminates the process and reports it.
- **Frames**: the Rust side decodes through FilmCraft's codecs, scales to the working resolution, and writes
  PNGs to a bounded cache directory (no giant stdin pipes). The worker reads them with PIL in sorted order.
- **Initial mask**: authored in the Rust UI (brush add/erase, polygon fill, feather/expand), saved as a PNG;
  no AI model is required for this step. Positive/negative click-to-mask is a later stage, gated on a
  licensed, audited click model.
- **Corrections**: each correction frame is stored with its edited mask and a monotonically increasing
  correction id. Propagation runs between correction frames; changing one invalidates only the affected
  downstream range.
- **Drift handling (honest version)**: the worker reports per-frame mask area and centroid (cheap to compute
  from its own output). The UI flags sudden area/centroid jumps for review; this is a *heuristic review
  aid*, not automatic drift correction. The user corrects a frame and re-runs from it.
- **Cache**: `%LOCALAPPDATA%\FrameForgePro\roto\<key>\` where `key` hashes: source media identity (path,
  size, mtime and a sampled content digest), model file SHA-256, working resolution, frame rate, correction
  ids, XMem commit. A cache miss or key change rebuilds. Cache size is reported and user-clearable.
- **Render integration**: the Rust side loads the cached mask for the requested frame and multiplies the
  clip's alpha in `ComposedSource`, so the program monitor and export use the identical mask bytes. This is
  the same seam every EffectCraft effect already uses. FilmCraft-native mask integration is evaluated
  later; it is not required for the first working prototype.

## Packaging

| Layer | Contents | Size |
|---|---|---|
| Core editor | Rust exe + existing libraries | ~24 MB ZIP / ~61 MB exe (measured) |
| Roto AI pack (optional) | embedded CPython 3.11 (Windows x64), CPU PyTorch, XMem source (MIT notice), worker script | **to be measured** (PyTorch CPU is the dominant cost; no estimate is published as fact until the prototype is built) |
| Weights (optional, consent) | `XMem.pth` 237.5 MB from the official release, SHA-256 recorded, plus licence/notice | 237.5 MB |

The app checks for the pack and weights, explains size/source/licence/hardware, and offers download only
after consent. No download on first launch. Removing the pack disables the workspace with a clear message;
projects keep their mask cache and still render from it.

## Hardware policy

1. **CPU baseline first** on the target Ryzen 5 7530U (16 GB, integrated Radeon): benchmark at 480p and 720p
   working resolution, report actual FPS/memory, no projection from GPU claims.
2. AMD acceleration is investigated only if the CPU baseline is insufficient **and** a supported path exists
   (e.g. PyTorch-DirectML build, verified on this hardware). CUDA is never assumed.
3. Remote GPU inference is optional and off by default; it never becomes required.
4. Long clips: process at a reduced working resolution and cache; preview can show the mask at proxy
   resolution while export regenerates at full resolution (same model, documented difference).

## Staged delivery and acceptance tests

| Stage | Deliverable | Acceptance test (must actually run) |
|---|---|---|
| **R0** | This plan, licensing record, protocol spec | Reviewed; no UI controls presented as working |
| **R1** | Mask cache + alpha application in `ComposedSource`, synthetic mask | Render test: a known alpha matte changes pixels exactly as expected in preview **and** export; cache invalidation test |
| **R2** | Worker protocol + frame exporter, CPU benchmark command | Worker round-trip test with a fake model (no weights); benchmark recorded on the target laptop |
| **R3** | Real XMem CPU end-to-end on a short synthetic clip | Accuracy on a moving rectangle ≥ agreed IoU; forward and backward propagation; cancel/resume; memory ceiling; final-export mask equality |
| **R4** | Roto AI workspace (brush, initial mask, propagate, corrections, cache UI) | UI interactions bound to real worker state; no fake values; screenshots recorded |
| **R5** | Weights consent download + optional pack | Download shows size/source/licence; checksum verified; deletion works; core app still runs without it |
| **R6** | Occlusion / disappearance / reappearance corpus, drift review, edge refinement | Documented results on the corpus, including failures |

CI policy: protocol/cache/alpha tests run on every push (no PyTorch). The real XMem test is a separate,
owner-enabled workflow (manual dispatch) because it downloads ~2+ GB of runtime and weights and is slow on
CPU; its logs are the evidence for R3. It is never simulated in the normal suite.

## Not claimed

XMem integration is **not complete** until R3 passes with the real model and R4 is wired. The current app has
no Roto AI workspace, no worker, no mask cache and no weights; the tab is not shown. Any future status in
`docs/FEATURE_PARITY.md` will name the run that proved it.

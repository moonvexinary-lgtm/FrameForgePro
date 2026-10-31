# Roto AI — XMem integration plan (mandatory engine)

Status: **R1 and the R2 transport/exporter are implemented and CI-verified; a real XMem protocol
worker and local R3 probe now run the official model on CPU. XMem is not app-wired, and the Roto AI
workspace (R4), consent-based pack/weights (R5) and full R3 acceptance are not complete.** R1 (run
`40056112214`) reads a cached mask and multiplies the clip's alpha on the one preview/export path.
R2 (run `40059782293`) speaks the JSON-lines worker protocol to a real child process and writes
worker input PNGs; its fake worker test uses no PyTorch and no weights. The local probe (2026-10-27)
measured real forward/backward inference, IoU, cancel and peak RSS on the target Ryzen 5 7530U;
`roto-bench` itself still measures only the Rust frame-export path and reports XMem inference as
"not measured". The bridge's `run_job` runner (2026-10-30) validates a request, watches for stalls
and applies cancel/kill deadlines against the worker protocol; nothing in the app calls it yet, and
nothing in the UI presents Roto AI as working.

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

## Implemented in R1/R2 (2026-10-24)

### Mask cache (R1)

`crates/bridge/src/roto.rs` owns the cache reader and `ComposedSource` owns the alpha seam.

```
%LOCALAPPDATA%\FrameForgePro\roto\<key>\
  manifest.json
  frame_00000000.png
  frame_00000001.png
  ...
```

`manifest.json` is versioned:

```json
{
  "version": 1,
  "key": "0123456789abcdef",
  "width": 480,
  "height": 270,
  "frame_rate": { "num": 24000, "den": 1001 },
  "frame_start": 0,
  "frame_count": 120
}
```

Each frame is an 8-bit grayscale PNG at the declared working size. A frame inside `frame_start ..
frame_start+frame_count` whose PNG is missing is an error, never silently unmasked; a frame outside
the declared range is left untouched. Mask files are named by the media frame index, so the
worker and the renderer agree without a side table.

`RotoCacheKey::cache_key()` is a stable 64-bit FNV-1a digest of the source identity, model SHA-256,
working width/height, exact frame rate, ordered correction ids and the pinned XMem commit. Changing
any input addresses a different directory. The key is validated as 16 hex digits before it is joined
to the cache root, so a hand-edited `.ffproj` cannot traverse paths.

`ComposedSource::with_roto_cache` applies the mask after the effect chain or comp output:

```text
alpha_out = (alpha_in * mask + 127) / 255
```

RGB is untouched (the pixels are straight-alpha RGBA), and the preview and export both read the same
`MaskCache`. The tests are `crates/bridge/tests/r1_roto.rs` (5 tests, run `40056112214`): exact
left-transparent/right-opaque alpha inside coverage with outside-coverage pass-through; a missing
in-range frame is an error; every cache-key input invalidates the address; a correction edit misses
the old cache; and a two-track preview plus real H.264 export both show the background through the
transparent half. `.ffproj` schema is now 3; schema-2 readers refuse a schema-3 project instead of
silently dropping its mask reference.

Limitations: masks must match the requested frame size exactly (a proxy mask is not stretched onto a
full-resolution export; R3 regenerates at the requested working size); decoded masks are not kept in
an in-memory LRU yet, so every render request decodes its PNG; there is no mask writer or UI.

### Worker protocol and frame exporter (R2)

`crates/bridge/src/roto_worker.rs` defines protocol 1 as JSON lines:

```json
{"command":"hello","protocol":1}
{"event":"ready","protocol":1,"backend":"cpu","torch":"…","xmem":"…"}
{"command":"propagate","frames_dir":"…","first_frame":0,"last_frame":119,"initial_mask":"…","output_dir":"…","width":480,"height":270,"model":"XMem.pth","model_sha256":"…","direction":"forward"}
{"event":"progress","frame":0,"total":120,"elapsed":0.1,"fps":10.0}
{"event":"done","frames":120,"elapsed":12.0}
{"command":"cancel"} / {"event":"cancelled"}
{"command":"shutdown"}
```

`XmemWorker` spawns a child process, sends a `hello` handshake and returns a typed `WorkerInfo`; a
closed pipe is an error carrying the capped stderr tail, not a silent end of job. One event line is
capped at 1 MiB. The test `crates/bridge/tests/r2_roto_worker.rs` (3 tests, run `40059782293`) spawns
a real PowerShell fake worker and covers ready → progress → done → cancel → shutdown; the fake never
claims to be XMem.

`export_frames` decodes the requested range through FilmCraft's codecs, scales to fit a working size
without upscaling, and writes `frame_<index>.png` in media order. The same test exports three green
frames at 32×18 and checks dimensions, colour and order.

`frameforge-pro roto-bench [frames]` times this Rust-side decode/scale/PNG path on synthetic Bars &
Tone and CI runs it with 4 frames. It prints "XMem CPU inference: not measured" unconditionally — no
pack/weight detection or inference call is wired into `roto-bench` — so it never invents an inference
number. The target-laptop CPU benchmark is now measured **locally** by `tools/xmem_r3_probe.py`
(2026-10-27; see above), not by `roto-bench` and not in CI; 237.5 MB weights remain user-supplied.

Limitations: the client is blocking (a hung worker needs the R3 job runner's timeout/kill policy);
`request_shutdown` sends the request and process teardown kills the child; no Python runtime or weights
ship, and the `tools/` real worker script is not in the optional pack; initial-mask UI, job queue and UI
wiring are still absent.

## Packaging

| Layer | Contents | Size |
|---|---|---|
| Core editor | Rust exe + existing libraries | 25.5 MB ZIP (25,512,804 bytes, run `40059782293`); the last extracted exe was ~64 MB (M12) |
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

## Local R3 probe (2026-10-27)

`tools/xmem_worker.py` is our protocol-1 adapter over an external, user-supplied official XMem
checkout (MIT) and `XMem.pth`; it downloads nothing, verifies the weights SHA-256, runs on CPU and
writes `frame_<index>.png`. `tools/xmem_r3_probe.py` generates a moving-rectangle clip, drives the
worker over pipes, computes IoU and optionally samples peak RSS. Neither the checkout nor the
weights are committed, and CI does not run them (no torch, no weights).

- **Command**: `py -3.11 tools/xmem_r3_probe.py --xmem-source <XMem checkout> --weights <XMem.pth>`.
- **Environment**: AMD Ryzen 5 7530U (target laptop), Python 3.11, torch 2.14.1+cpu, XMem `f3b841d`,
  weights 249,026,057 bytes, SHA-256 `27776291d2f0639b4e6b372a67651579b51180aa4d8f8f89bbff2dcc09ebf6f9`.
- **160x96, 12 frames**: forward mean IoU 0.9539 (min 0.9231); backward mean 0.9512 (min 0.8700).
- **480x270, 6 frames**: forward mean IoU 0.9883 (min 0.9796), 0.818 fps wall, 710.2 MB peak RSS;
  backward mean 0.9945 (min 0.9912), 1.252 fps wall, 610.7 MB peak RSS. Wall time includes model
  load; worker-reported inference time was 4.31 s / 4.40 s. Peak RSS was 710.2 MB forward
  (including model load) and 610.7 MB backward (after load).
- **Cancel**: a 30-frame 480x270 job cancelled after the first progress event emitted `cancelled`
  1.05 s later and stopped after 2 masks.
- **Runner (2026-10-28 → 30)**: the transport gained `recv_timeout` / `handshake_timeout` / `kill`
  (run `40814011414`), and `crates/bridge/src/roto_job.rs` now holds `run_job` — request validation
  before send, a stall watchdog, an overall budget, a 50 ms cancel poll, an event cap and a kill on
  stall/deadline/unacknowledged cancel — with four runner tests (run `41144133581`).
- **Consent and verification (R5, bridge, run `41161529133`)**: `roto_runtime::RotoRuntime::resolve`
  refuses to read anything until `RotoConsent::accepted` is true, then checks the interpreter, worker
  script and weights exist, that the checkout is the pinned commit (read from loose refs,
  `packed-refs`, a `gitdir:` file or a detached HEAD — no `git` process), that `model/` is present, and
  that the weights match the caller's SHA-256. `sha256.rs` is a local FIPS 180-4 implementation with
  published vectors, so no crate and no licence were added. `python_candidates` only looks for files;
  it never runs an interpreter. No UI collects these paths yet.
- **Still open**: app/UI wiring and a job queue (the runner exists but nothing in the app calls it),
  a UI for the consent/paths flow (R5's storage and packaging), resume from corrections, a memory
  ceiling, and proof that worker masks are the bytes used by preview/export (the R1 seam exists but
  the app does not spawn the worker). This is **not** an R3 pass.

The 2026-10-28 UI milestone (run `40794727835`) added a **Masking** workspace, but it
drives FilmCraft's own engine mask objects (`masks.*`) and an engine mask tracker; it
does not start XMem, and its XMem panel says "not wired in this build". The bridge's
runtime/consent verification (run `41161529133`) is likewise not reachable from the UI
yet, so nothing in the app claims Roto AI works.



| Stage | Deliverable | Acceptance test (must actually run) |
|---|---|---|
| **R0** | This plan, licensing record, protocol spec | Reviewed; no UI controls presented as working |
| **R1** | Mask cache + alpha application in `ComposedSource`, synthetic mask | ✔ Implemented; 5 tests in `r1_roto.rs` (run `40056112214`): known alpha matte changes preview **and** H.264 export pixels; every key input invalidates the cache; a missing in-range mask is an error |
| **R2** | Worker protocol + frame exporter, CPU benchmark command | 🟡 Protocol round-trip against a real fake child and PNG exporter tested (3 tests, run `40059782293`); `roto-bench` runs in CI. **Partially met:** the local probe measured the target-laptop CPU XMem benchmark (480x270, 0.818/1.252 fps wall, 710.2/610.7 MB peak RSS, 2026-10-27); it is not a CI gate and `roto-bench` still reports inference as not measured |
| **R3** | Real XMem CPU end-to-end on a short synthetic clip | 🟡 Local probe: moving-rectangle IoU forward/backward, cancellation and peak RSS measured with the real model (see above). **Not met:** resume, memory ceiling, final-export mask equality, app integration and a repeatable CI/manual-dispatch job |
| **R4** | Roto AI workspace (brush, initial mask, propagate, corrections, cache UI) | UI interactions bound to real worker state; no fake values; screenshots recorded |
| **R5** (partial) | Weights consent download + optional pack | **Bridge part met (run `41161529133`):** `RotoRuntime::resolve` requires recorded consent before any read, verifies the pinned checkout commit without a git process, verifies the weights SHA-256 (local FIPS 180-4 implementation, `sha256.rs`), and `python_candidates` only looks for files. **Still open:** no UI collects the paths or the consent, no pack/packaging of the worker script, no in-app deletion, no download UI (there is deliberately no downloader at all) |
| **R6** | Occlusion / disappearance / reappearance corpus, drift review, edge refinement | Documented results on the corpus, including failures |

CI policy: protocol/cache/alpha tests run on every push (no PyTorch). The real XMem probe is local
and owner-run because the runtime and 237.5 MB weights are not CI dependencies; its logs are the
evidence for the partial R3 result. It is never simulated in the normal suite and CI never downloads
XMem or weights.

## Not claimed

XMem integration is **not complete** until R3 passes with the real model and R4 is wired. The current
app has no Roto AI workspace, no initial-mask UI, no app job runner and no UI path that spawns the
worker; no pack or weights ship. What exists is the R1 mask seam (cache reader + alpha in
`ComposedSource`), the R2 protocol/exporter, the local `tools/` real-XMem adapter/probe, and a
benchmark command that explicitly refuses to invent an inference number. The local probe is real
inference evidence, not an R3 acceptance pass; resume, memory ceiling and renderer mask equality stay
open. Any future status in `docs/FEATURE_PARITY.md` will name the run that proved it.

# The FrameForge Pro rotoscoping engine

A model-independent pipeline for interactive selection, video tracking, temporal
quality control, matting, refinement, caching, scheduling and export, built as
`crates/roto` (`frameforge-roto`) and driven from the **Masking** workspace.

> **Development rule this document is written under.** No model inference was
> executed while building this. No weights were loaded, no video was segmented, no
> FPS was benchmarked and no GPU memory was consumed. Every model claim below is
> either a publisher fact (a licence read from the project, a file size read with a
> HEAD request) or something this repository measured earlier and recorded. The
> engine is prepared to run models locally; it stays dormant until a user asks it
> to. Cutie has a real worker adapter (written, not executed — see §4); SAM 2.1,
> EdgeTAM and SAMURAI still need theirs.

---

## 1. Modules

| Module | Responsibility | Model needed? |
|---|---|---|
| `roto_engine` | The interface every backend implements (`RotoEngine`, `LaunchPlan`, capabilities), the runtime probe (file presence only) | no |
| `roto_run` (app) | Gate, frame export, worker spawn, job run and double publish for any backend | adapters only |
| `roto_selection` | Points, boxes and mask prompts, prompt sessions, box maths | no |
| `roto_segment` | The one-frame segmentation contract: a versioned request, a checked reply and a dormant launch plan shared by every selector family | adapters only |
| `roto_compat` | Per-model inference routes (PyTorch, ONNX Runtime, Windows ML) and detection of runtime files already on disk — presence only, nothing loaded | no |
| `roto_tracking` | Backends (Cutie / SAMURAI / XMem), per-object track state, resumable propagation segments | adapters only |
| `roto_temporal` | Model-independent mask metrics: area change, centroid jump, identity drift, disappearance, flicker, ragged edges; recovery suggestions | no |
| `roto_matting` | Feather, expand/contract, edge smoothing, invert, premultiply, alpha application; the seam for a learned matting backend | no |
| `roto_refinement` | Boolean combination, opening/closing, despeckle, polygon fill, correction keyframes | no |
| `roto_brush` | Paint/erase strokes with spacing along a drag and soft edges | no |
| `roto_cache` | Multi-object, multi-model mask cache: `root/<digest>/object_<id>/frame_%08d.png` + manifest written last | no |
| `roto_scheduler` | One job queue with priorities, pause, resume, cancel; starts nothing by itself | no |
| `roto_export` | Binary / grayscale / alpha sequences, RGBA sequences, sidecar manifest | no |
| `roto_models` | The catalogue, licences, install plans, downloads, verification, removal | no |
| `roto_runtime` | Hardware/provider reporting and the resolution, stride and batching policy (planning estimates, never measurements) | no |

The pipeline the modules implement:

```
 prompts ─▶ selection ─▶ tracking ─▶ temporal QC ─▶ matting ─▶ refinement ─▶ cache ─▶ export
                │            │             │            │           │
                └─ model ────┴─ model ─────┘  (all deterministic from here)
```

Everything to the right of *tracking* is FrameForge's own arithmetic: it runs
today, in CI, with no model, and the integration test
`crates/roto/tests/pipeline.rs` walks the whole path (prompts → mask → QC flags a
tracking break → brush correction → matting → cache → alpha and RGBA export →
queue).

## 2. Models, licences, sizes and storage

Licences below were read from each project's own repository on **2026-11-01**
(the `LICENSE` file, or the SPDX id GitHub reports for the repository). Sizes were
read with a `HEAD` request to the publisher's URL on the same date — metadata
only, no download.

| Model | Task | Licence (verified) | Redistribution | Weight file | Size (verified) | Runtime |
|---|---|---|---|---|---|---|
| SAM 2.1 Tiny | selection | Apache-2.0 | download offered | `sam2.1_hiera_tiny.pt` | 156,008,466 B (149 MB) | external Python |
| SAM 2.1 Small | selection | Apache-2.0 | download offered | `sam2.1_hiera_small.pt` | 184,416,285 B (176 MB) | external Python |
| EdgeTAM | selection | Apache-2.0 | download offered | publisher asset (URL unverified) | **not verified** | external Python |
| Cutie Base | tracking (**preferred**) | MIT | download offered | `cutie-base-mega.pth` | 140,443,788 B (134 MB) | external Python |
| SAMURAI | tracking (optional) | Apache-2.0 | download offered | `sam2.1_hiera_base_plus.pt` (it reuses SAM 2.1) | 323,606,802 B (309 MB) | external Python |
| XMem | tracking (legacy fallback) | MIT | download offered | `XMem.pth` | 249,026,057 B (238 MB) | external Python |
| Robust Video Matting | matting | **GPL-3.0-or-later** | **prohibited** | — (no download offered) | — | external Python |
| MatAnyone / MatAnyone 2 | matting | **S-Lab 1.0, non-commercial** (v2: unasserted custom licence) | **prohibited** | — (no download offered) | — | external Python |
| Temporal QC, edge matting | QC, matting | FrameForge's own (MIT OR Apache-2.0) | built in | none | 0 B | in-process Rust |

Where weights are stored: `%LOCALAPPDATA%\FrameForgePro\models\<model id>\…` by
default, configurable in the panel. Nothing is stored inside the project file, and
no weight is ever bundled with the application.

**Deliberately absent: SAM 3 and SAM 3.1.** A catalogue test fails the build if any
entry even mentions them.

### Licence consequences that shape the design

* **RVM (GPL-3.0)** and **MatAnyone (non-commercial)** cannot be bundled with or
  linked into FrameForge Pro. They are marked `Redistribution::Prohibited`, no
  download is offered for them, and the UI says "user-supplied only". Use would be
  an isolated external process the owner installed and cleared legally.
* EdgeTAM's repository is Apache-2.0, but its **weight asset URL and size could not
  be verified from this environment**, so the catalogue says so and the manager
  requires the user to confirm the file before it is trusted.
* No publisher SHA-256 was available for any checkpoint from this environment, so
  the manager computes the digest after download, shows it, and lets the user pin
  it — the same posture the existing XMem flow already takes.

## 3. Runtimes and hardware (what may and may not be claimed)

`roto_runtime` reports:

* **CPU** — available, always, because the process is running on one. The XMem
  CPU numbers in `docs/ROTO_AI_PLAN.md` (0.818 / 1.252 fps wall, 710.2 / 610.7 MB
  peak RSS at 480×270 on the target Ryzen 5 7530U) were measured on the owner's
  laptop, not in CI.
* **ONNX Runtime + DirectML, CUDA and Vulkan** — **not probed**. No model is
  claimed to run on them. Per the project rules, a provider may only be enabled
  once its operators and runtime compatibility have actually been verified.
* **Python + PyTorch (CPU)** — the path the existing XMem adapter uses; absent
  until the user installs a runtime and consents.

GPU memory and system RAM are **not probed** by this build (that needs an OS API
this crate deliberately does not add a dependency for). The manager therefore works
to a *disk* budget the user sets, and batching uses a conservative, documented
estimate rather than a guess about free memory.

## 4. What is implemented versus what still needs a real runtime

Implemented and covered by tests that run in CI without any model:

* prompt sessions (points, boxes, masks), box normalisation and clamping;
* backend registry and adapters: a run is **planned** (command line, protocol,
  weights) and refused with a named reason when the model is not installed or the
  prompt type is unsupported;
* resumable propagation segments that start from every user correction;
* temporal quality control with eight signal kinds, contiguous flagged ranges, a
  re-init suggestion and a seam for a learned QC scorer;
* brush strokes, boolean refinement, morphology, despeckle and polygon fill;
* matting (feather/expand/smooth/invert/premultiply) and alpha application;
* the multi-object mask cache with a digest over every input that changes masks;
* the job queue (priority, pause, resume, cancel, cancel-all, clear-finished);
* mask/alpha/RGBA export with a manifest;
* the model catalogue, install planning, download command rendering (including
  `-C -` resume), checksum verification and removal;
* runtime reporting, resolution fitting and batch sizing.

**Still requires a real runtime to validate** (and is therefore *not* claimed as
working): the actual selection, tracking and model-based matting of any backend.
`tools/roto_worker.py` is an honest protocol stub: it answers `hello`/`ready`,
`propagate`, `cancel` and `shutdown`, and refuses to fabricate a mask, an IoU or a
timing number. Wiring a real backend means installing its runtime and weights and
pointing the workspace at a real worker; nothing in the UI pretends otherwise.

### The Cutie worker (written, not executed)

`tools/cutie_worker.py` is a real adapter over a **user-supplied Cutie checkout**,
written to the same protocol-1 contract as the XMem worker. It:

* hands the caller's `hello`/`propagate`/`cancel`/`shutdown` traffic unchanged, so
  the bridge's runner drives it without a new transport;
* hashes the weights **before** importing torch and refuses a mismatch (all-zero
  means "not pinned yet", and the measured digest is reported instead);
* never downloads: no `get_default_model()`, no `torch.hub`, no network call;
* builds Cutie from the checkout's own Hydra config, and if that path differs in
  the user's version it reports the exact file it looked for rather than guessing;
* reports `rss_mb` with every progress event, so the memory ceiling is enforced on
  a real number, and honours cancel between frames;
* writes 8-bit grayscale `frame_%08d.png` masks, which both caches adopt as-is.

**It was not executed while building this**: no weights were loaded, no inference
ran, no GPU memory was touched. Its protocol, verification, frame conversion,
cancellation, progress and mask writing mirror the adapter that already runs for
XMem; only the model-construction step is unproven, and it fails loudly.

`--stride N` propagates every Nth frame and copies the previous mask's bytes onto
the frames in between. The cache still receives every frame — the renderer needs
the full range — so the trade is visible: `temporal::analyze` sees the repeats and
flags the stutter they cause rather than hiding it.

### The segmentation worker (written, not executed)

`tools/segment_worker.py` is the process `segment::plan_segment` describes for the
selector families (SAM 2.1 Tiny and Small). It:

* reads the `segment-1` request, validates the version, the image size and every
  prompt, and writes an **error reply**, never a plausible mask, when anything is
  missing;
* hashes the checkpoint before importing torch, and imports torch only after that
  check, so an argument mistake costs nothing;
* refuses `--device` other than `cpu`: DirectML and CUDA are not execution
  providers for these PyTorch checkpoints on Windows, and no ONNX export has been
  validated, so offering the flag would be a lie;
* refuses `edgetam` with the reason (its image-predictor API has not been verified
  in this build) instead of guessing at it;
* intersects the returned mask with a painted mask prompt, which is what "keep only
  what I painted" means — it does not fabricate model logits from a PNG;
* has a `--self-test` mode that exercises request validation and atomic replies
  with **no torch and no weights** (it passes, locally).

The **selection runner** (`roto_segment.rs`) exports the clip's first frame at
the working size, writes the plan's `request_json`, deletes any stale reply,
spawns the worker on a thread with a timeout and a cancel flag, drains its stderr,
requires a zero exit, checks the reply against the request it sent, and adopts the
mask only if it is exactly the requested size. The panel then paints it into the
frame-0 canvas, from which the tracker starts. What CI verifies is that wiring and
its failure paths (no reply, error reply, wrong frame, wrong size, wrong mask
size, unspawnable program, dormant plan); **no model runs in CI**, so the mask a
real SAM 2.1 checkpoint produces is still unproven.

### From a plan to a live run

The engine panel now runs a backend end to end: `RUN THE SELECTED BACKEND` asks for
consent, a checkout (Cutie/XMem), the initial mask and a selected clip, then
`roto_run::gate` refuses anything that is not licensed, installed and consented
*before* a thread is spawned. A passing gate exports the clip's frames, spawns the
worker with the plan's own command line plus the backend's checkout flags, drives
it under `run_job` (stall, deadline and memory ceilings), and publishes twice: the
multi-object cache of `frameforge-roto` (adopting the worker's bytes), and the
bridge's R1 cache the renderer reads. The finished run offers the same undoable
"Use this matte on the clip" edit the XMem panel uses, so a Cutie matte reaches the
timeline through the same seam.

The run ends with a **review pass** that is pure arithmetic on the published masks:
`roto_run::review` reads them back from the cache, builds a `MaskSummary` per frame
and runs `temporal::analyze`. The panel then lists the flagged ranges with their
kinds and severity, names the suggested fix for the worst signal (re-detect,
re-initialise from a box, or brush-correct and re-propagate) and offers each
flagged frame as a button that moves the playhead to it. A run whose masks cannot
be read back says so instead of showing an empty, falsely clean pass.

Above the verdict the panel states the run's **provenance** in one line —
`roto_run::provenance` joins the backend id, the SPDX licence the user consented
under and either the first twelve hex digits of the weights digest the worker was
told to expect, or the words "weights digest unpinned". An unpinned run is never
allowed to look like a hashed one.

The Masking workspace now draws the other half of the input: a **Brush**, **Add
point**, **Remove point** and **Box** tool over the same frame-0 canvas, with the
prompts listed, undoable and drawn as markers. Roto AI → *Selection prompts* shows
the chosen selector's compatibility route, offers the no-load runtime-file scan,
and builds the dormant `segment-1` plan from what was drawn. *Speed and quality*
carries the working resolution, the propagation stride and the memory budget,
labelled as planning estimates.

Known gaps, stated plainly:

* SAM 2.1 and EdgeTAM have a worker and a runner, but neither has been executed
  against a real checkpoint: the runner's wiring and refusal paths are tested, the
  model answer is not. EdgeTAM's adapter refuses by design until its API is
  verified. SAMURAI has neither and runs through the stub, which refuses.
* The Cutie worker's model-construction step is unproven (above).
* Occlusion/disappearance corpora and drift review (the plan's R6) have not been
  run against a real model.
* Temporal QC thresholds are arithmetic defaults; they have not been tuned against
  real tracker output.
* Resume-from-correction is planned and unit-tested, but not yet executed against a
  live tracking backend.
* The matting stage is deterministic; no learned mattter is enabled (licence
  reasons above).

## 5. Verification

* `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check`, `cargo test --workspace`, a release build and the binary self-test
  (including `roto-bench 4`) run in CI. The crate adds **no new dependency**: it
  reuses `image`, `serde`, `serde_json`, `thiserror` and the bridge's SHA-256.
* CI run **`41581127327`** (commit `ba87930`, `frameforge-rust-rewrite`) is green on
  all thirteen steps — fmt, clippy `-D warnings`, check, tests, release build,
  binary self-test including `roto-bench 4`, artifact upload — including the five
  tests added with the Cutie run path (four gate/run decisions, one cache
  adoption). Earlier green run **`41345085275`** (commit `309fcb8`) recorded **153
  tests** across the workspace (64 in `frameforge-roto` plus the 2 whole-pipeline
  integration tests) and a 26,088,741-byte artifact; **`41347694498`** (`a36edb9`)
  repeated it for the documentation change. Runs `41578955634` and `41579916787`
  are the two failures that produced this section's run-path fixes — a `&str`/`String`
  mismatch in the licence line, the missing `SourceProvider` import, and the
  `FrameRate` fields being `i64` rather than `i32`.
* CI proof does **not** cover inference: the Cutie worker is never started there
  (no torch, no weights), by design.
* Tests added by this work: catalogue/licence invariants (including "SAM 3 must not
  appear"), install planning and refusal, the download command and resume flag, the
  install state machine and verification (FIPS 180-4 "abc" vector), adapter
  planning dormancy, prompt geometry, segment planning around corrections, temporal
  signals (disappearance, reappearance, area explosion, identity swap, flicker,
  ragged edge, degenerate mask), plane maths (expand/contract/feather/premultiply),
  boolean and morphological refinement, polygon fill, brush strokes, cache key
  sensitivity and round-trip, queue behaviour, export naming/modes/RGBA, runtime
  reporting (no accelerator claimed), cache adoption from a worker's directory
  (missing and mis-sized frames publish nothing), the run gate's ordered refusals,
  the restricted-licence refusal, "no command line or no mask means no run",
  "a report without an R1 key is not attachable", the provenance line (a pinned
  digest prints its prefix, a zero or malformed digest is stated as unpinned,
  an empty licence is stated as unrecorded) and the review pass — a clean
  run reviews clean, a vanished mask is flagged and offered for review, and
  reviewing nothing is an error rather than a blank pass — plus the whole-pipeline
  integration test.
* No AI inference runs in CI, and none can: there is no weights download in the
  workflow, and the adapter tests use `NoRuntime`, which finds nothing.

## 6. Final report

1. **Integrated models.** Cutie (primary tracking), SAM 2.1 Tiny/Small (interactive
   selection), EdgeTAM (alternative selection), SAMURAI (optional motion-aware
   tracking), XMem (legacy fallback), plus built-in temporal QC and built-in edge
   matting. RVM and MatAnyone / MatAnyone 2 are catalogued but **disabled**: their
   licences forbid this app from distributing them.
2. **Licences.** Apache-2.0: SAM 2.1, EdgeTAM, SAMURAI. MIT: Cutie, XMem.
   GPL-3.0-or-later: RVM (prohibited here). S-Lab 1.0 non-commercial (v2: custom,
   unasserted): MatAnyone (prohibited here). FrameForge's own MIT OR Apache-2.0 for
   the built-ins.
3. **Weight sizes.** Verified with HEAD requests: SAM 2.1 Tiny 156,008,466 B;
   SAM 2.1 Small 184,416,285 B; SAM 2.1 base+ (SAMURAI) 323,606,802 B; Cutie Base
   140,443,788 B; XMem 249,026,057 B. EdgeTAM and the matting models: unknown /
   not offered.
4. **Where weights are stored.** `%LOCALAPPDATA%\FrameForgePro\models\<model id>\`
   by default (configurable), outside the project file, never bundled.
5. **Supported runtimes.** External Python processes speaking protocol 1 for every
   AI backend: XMem has a real worker, Cutie now has a real adapter over a
   user-supplied checkout (`tools/cutie_worker.py`, written, not executed), and the
   rest use the honest stub. ONNX Runtime and DirectML are *investigated and
   declared*, not enabled. The deterministic half of the pipeline runs in-process
   in Rust.
6. **Hardware limitations.** Only the CPU is claimed usable. DirectML, CUDA and
   Vulkan are "not probed"; no GPU, AMD, NVIDIA or Intel claim is made anywhere.
   RAM/GPU memory are not probed.
7. **Implemented features.** Prompted selection (points/boxes/masks), multi-object
   tracks with ids, renaming and visibility, forward/backward/both propagation
   planning, correction-split resumable segments, brush corrections, boolean and
   morphological refinement, polygon fill, temporal QC with flagged ranges and
   recovery actions, deterministic matting and alpha, multi-object caching,
   a job queue with pause/resume/cancel, mask/alpha/RGBA export with a manifest,
   a model manager (catalogue, plan, download with resume, verify, remove) and
   runtime reporting.
8. **Features needing inference validation.** Real masks from any backend; motion
   handling under occlusion; QC thresholds against real tracker output; resume from
   corrections on a live backend; any performance or memory measurement of the new
   models.
9. **Build and test results.** Recorded in the CI section of
   `docs/ARCHITECTURE.md` for each commit; the crate compiles and its tests pass in
   the same workflow as the rest of the workspace (fmt, clippy `-D warnings`, check,
   tests, release, binary self-test with `roto-bench`).
10. **Remaining work.** Real workers for SAM 2.1 / EdgeTAM / SAMURAI (Cutie's is
    written but unproven); a prompt-drawing canvas that feeds a selection backend;
    tracked-run playback with the QC review list; recording the model manager's
    verified digests in the consent record the XMem panel keeps; the R6
    occlusion/disappearance corpus and QC threshold tuning.

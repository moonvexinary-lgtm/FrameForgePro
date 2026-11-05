# Roto selection: prompts, segmentation, and making it fast enough

Status: **crate layer implemented and tested; workspace UI and the segmentation
worker are the next increment.** Nothing in this document is a benchmark, and no
model is named as the permanent default.

## Why this exists

Tracking in FrameForge starts from a mask. Selection is the step that *makes*
that mask from what the user can actually point at: clicks, a box, or a painted
correction. The first iteration of the roto engine shipped tracking end to end
(Cutie preferred, XMem legacy, SAM 2.1/EdgeTAM catalogued but workerless). This
document describes the other half — the prompts in, one mask out — and the
performance policy around it, on hardware where throughput cannot be assumed.

The target machine is stated so it can be designed for, not flattered: Ryzen 5
7530U, 16 GB RAM, integrated Radeon graphics. Every claim below is either
arithmetic or an explicit "unverified".

## What was decided

1. **Prompts are a first-class, model-independent contract.** `selection`
   already collects points, boxes and mask prompts. `segment` turns them into a
   versioned `SegmentRequest` and checks the `SegmentReply`. One worker script
   serves several selector families through `--model`; the family decides the
   adapter, not the protocol.
2. **Selection is not tied to SAM 2.1.** It is the first adapter because its
   checkpoint is the smallest verified official selector (156 MB), not because
   it is the answer. EdgeTAM shares the protocol; a future lightweight model
   needs an adapter, not a rewrite.
3. **Cutie stays the preferred *tracking* candidate.** "Preferred" means the
   default candidate. It is not a speed claim, and the UI must not imply one.
4. **Nothing is chosen permanently.** A model becomes the default only after its
   compatibility *and* throughput are established on the user's hardware. Until
   then the model manager and the compatibility matrix say "unverified".
5. **No inference in development.** The crate cannot load weights; plans are
   dormant unless the user starts a run; no test downloads or executes a model.
6. **No cloud inference is required.** The application runs on the user's own
   machine. A cloud path does not exist in this design.

## The segmentation contract

`frameforge_roto::segment`, protocol string `segment-1`:

```json
{
  "version": 1,
  "frame": 7,
  "width": 1920,
  "height": 1080,
  "prompts": [
    {"Point": {"x": 640.0, "y": 480.0, "positive": true, "frame": 7}},
    {"Box": {"rect": {"x0": 100.0, "y0": 100.0, "x1": 500.0, "y1": 700.0}, "frame": 7}}
  ],
  "mask": null,
  "image": null,
  "max_side": 480
}
```

Rules the crate enforces before a worker sees anything:

* the image is not empty and every prompt is tagged with the request's frame;
* coordinates must be finite, are clamped into the image, and a box must have
  area;
* a `Mask` prompt must carry the mask file it refers to (`mask`), because a
  prompt the worker cannot open is a question it cannot answer;
* `image` is the frame the worker reads. It is `null` while a plan is dormant;
  the runner adds it once the frame has been exported, and because it is part of
  the canonical JSON it is part of the digest;
* `max_side` is clamped to 64–4096 and the working size preserves aspect,
  never upscaling;
* the request's canonical-JSON SHA-256 is the cache key, so moving one point by
  one pixel addresses a different result.

The reply names the same frame and the size it actually produced, and the mask
file it wrote. A reply for another frame or size is rejected, never silently
resized:

```json
{"version": 1, "frame": 7, "width": 1920, "height": 1080,
 "mask": "C:/work/segment-7.png", "score": 0.94, "provider": "cpu", "elapsed_ms": 812}
```

`plan_segment` refuses a tracker (Cutie, XMem, SAMURAI) with a reason, refuses a
family without an adapter, refuses missing weights, and returns
`dormant: true` unless the caller reports a user-started action. The process is
described, never spawned, in this crate.

## The compatibility matrix (what is *not* claimed)

`frameforge_roto::compat` is deliberately unhelpful to wishful thinking.

| Model | Path today | ONNX export | Windows ML | Candidate providers |
|---|---|---|---|---|
| SAM 2.1 Tiny | PyTorch, external Python | unverified | unverified | CPU |
| SAM 2.1 Small | PyTorch, external Python | unverified | unverified | CPU |
| EdgeTAM | PyTorch, external Python | not available | not available | CPU |
| Cutie Base | PyTorch, external Python | not available | not available | CPU |
| SAMURAI | PyTorch, external Python | not available | not available | CPU |
| XMem | PyTorch, external Python | not available | not available | CPU |

Robust Video Matting and MatAnyone keep a row with **no provider at all**: their
licences (GPL-3.0-or-later; S-Lab 1.0 non-commercial) are why this build never
drives them, and the matrix says so instead of hiding them.

Why every cell says CPU:

* **DirectML is not a PyTorch execution provider.** It is reached through ONNX
  Runtime or Windows ML. The official checkpoints these models publish are
  PyTorch (`.pt`/`.pth`). ROCm has no consumer Windows build, so the honest
  PyTorch route on this laptop is the CPU.
* **No publisher ONNX export is verified for these models.** Community
  conversions exist for SAM 2, but "an export exists" is not "these operators
  run on DirectML and produce the same mask". A conversion would have to be
  produced, validated numerically and tested per model before any DirectML row
  could say anything but *unverified*.
* **Windows ML is an ONNX Runtime wrapper.** In principle it could host a
  validated ONNX export with the DirectML provider; in practice that inherits
  the same open question.

Detection, not probing: `compat::detect_files` looks for `onnxruntime.dll`,
`onnxruntime_providers_dml.dll`, `DirectML.dll`,
`Microsoft.AI.MachineLearning.dll` and `torch_cpu.dll` beside the app and on
`PATH`. It loads none of them, starts no process and queries no driver. A found
file produces the sentence "presence is not verification" — never "available".

This is the answer to "investigate ONNX Runtime, DirectML and Windows ML": the
investigation is represented as a matrix with every unverified cell marked, and
as a detection routine the user can run without side effects. Filling a cell in
requires a worker that reports the provider it used and a comparison of its mask
against the PyTorch reference, on the user's machine.

## Speed without lying

`runtime::PerformanceSettings` carries the knobs, and the panel must show what
each one costs:

* **Working resolution** (`max_width`, `max_height`, clamped to 64–3840 per
  side and ≤ 1920×1080 in area). Lower resolution is the single biggest lever on
  integrated graphics. It is a real trade, so the source resolution, the working
  resolution and the export resolution remain separate values.
* **Frame stride** (1–8): propagate every Nth frame and repeat the last mask in
  between. The cache still contains every frame and the temporal QC pass sees
  the repeats, so a stride that breaks the mask shows up in review rather than
  disappearing.
* **Memory budget** (512–65536 MB) with an **estimated** per-frame cost and a
  batch ceiling. The estimate is arithmetic on the working size plus a
  conservative overhead; it is labelled an estimate everywhere, because this
  project does not print fabricated measurements. The PyTorch workers process
  one frame at a time; the ceiling becomes real when an ONNX adapter batches.
* **Background processing**: the run path already spawns a thread and the panel
  polls it, so the UI stays responsive while inference runs. The setting records
  the queue's preference.

First-run policy, when the user has not chosen: 480×270 (the size the local XMem
probe used), stride 1, 2048 MB budget, background on. These are starting points,
not claims about speed.

## What exists now, and what is still missing

Shipped and tested in the crate: the request/reply contract, the compatibility
matrix, the runtime-file detection and the speed policy.

Shipped in the workspace: the Brush / Add point / Remove point / Box tools on the
mask canvas with visible, undoable prompts; the compatibility route and the
no-load runtime scan in Roto AI; the speed/quality controls; and a *Plan
selection* button that builds the dormant plan and prints the exact command line.

Shipped as a script (written, never executed): `tools/segment_worker.py`, the
SAM 2.1 CPU adapter over the protocol, with an EdgeTAM refusal and a
`--self-test` that runs without torch. Its checkpoint-construction call against
Meta's package is the one unproven step, and it fails loudly.

Still missing, and said out loud by the UI:

* The **selection runner**: export the requested frame, write the plan's
  request JSON, spawn the worker, validate the reply, adopt the mask into the
  canvas. No plan can run without it, and the panel says so rather than showing a
  runnable-looking button.
* No ONNX export has been produced or validated for any model here.
* No inference, weight load or benchmark has been run for this document, by
  design. The only measured numbers in the project remain the local XMem CPU
  probe, and they are not extrapolated to other models.

## Rules this design keeps

* SAM 3 / SAM 3.1 are absent; the catalogue test fails if they appear.
* RVM (GPL-3.0) and MatAnyone / MatAnyone 2 (non-commercial) are catalogued but
  prohibited: never downloaded, bundled or mirrored.
* Weights are never committed, never downloaded by the app at launch, and never
  hidden inside the binary.
* Nothing claims DirectML, CUDA, Vulkan, ROCm or Windows ML support without a
  verified operator path and a worker-reported provider.

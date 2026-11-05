#!/usr/bin/env python3
"""FrameForge's segmentation worker: one frame of prompts in, one mask out.

This is the process `frameforge_roto::segment::plan_segment` describes. It is
deliberately small and deliberately paranoid:

* It never downloads anything. If a checkpoint is not where the caller said it
  is, it stops and says so.
* It imports torch only after the request, the checkpoint and the option values
  have been checked, so an argument mistake costs nothing.
* It writes the reply atomically and writes an error reply, not a plausible
  mask, when anything is missing.
* It runs on the CPU only. DirectML / CUDA are not execution providers for these
  PyTorch checkpoints on Windows, and no ONNX export has been validated, so
  `--device` refuses anything but `cpu`. That is a statement about this build,
  not about the hardware.

Protocol `segment-1` (see `crates/roto/src/segment.rs`):

    python -u segment_worker.py --model sam2.1-tiny \
        --request request.json --reply reply.json --max-side 480 \
        --weights C:\\models\\sam2.1\\sam2.1_hiera_tiny.pt

The request is the JSON `SegmentRequest` writes: `version`, `frame`, `width`,
`height`, `prompts` (a list of `Point`/`Box`/`Mask` objects), `mask` (the PNG a
`Mask` prompt refers to, or null) and `image` (the frame to segment, added by the
runner before it spawns this process).

Supported adapters, and what "supported" means here:

    sam2.1-tiny / sam2.1-small   Meta's SAM 2.1 image predictor, CPU.
    edgetam                      Not wired: Meta publishes PyTorch code whose
                                 image-predictor API this worker has not been
                                 able to verify. It refuses rather than guess.

Exit status: 0 when a mask and a reply were written, 1 otherwise. On failure the
reply file contains `{"version": 1, "error": "..."}` so a runner can show the
reason without scraping stderr.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import time
from pathlib import Path

PROTOCOL_VERSION = 1

# Config names inside Meta's `sam2` package, per catalogue id. Kept here so the
# caller passes a model id, not a package-internal path.
SAM2_CONFIGS = {
    "sam2.1-tiny": "configs/sam2.1/sam2.1_hiera_t.yaml",
    "sam2.1-small": "configs/sam2.1/sam2.1_hiera_s.yaml",
}


class WorkerError(Exception):
    """A reason the run cannot proceed, phrased for the panel."""


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="FrameForge segment-1 worker")
    parser.add_argument("--model", required=False, help="catalogue model id")
    parser.add_argument("--request", help="segment-1 request JSON")
    parser.add_argument("--reply", help="where to write the segment-1 reply JSON")
    parser.add_argument("--max-side", type=int, default=0, help="longest side the model may use")
    parser.add_argument("--weights", action="append", default=[], help="checkpoint path (repeatable)")
    parser.add_argument("--weights-sha256", default="", help="expected SHA-256 of the first checkpoint")
    parser.add_argument("--device", default="cpu", help="execution device; only 'cpu' is accepted")
    parser.add_argument("--self-test", action="store_true", help="check argument and JSON handling without torch")
    return parser.parse_args(argv)


def load_request(path: Path) -> dict:
    if not path.is_file():
        raise WorkerError(f"the request file does not exist: {path}")
    try:
        request = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise WorkerError(f"the request is not valid JSON: {error}") from error
    if request.get("version") != PROTOCOL_VERSION:
        raise WorkerError(f"protocol version {request.get('version')} is not supported (expected {PROTOCOL_VERSION})")
    width = int(request.get("width") or 0)
    height = int(request.get("height") or 0)
    if width <= 0 or height <= 0:
        raise WorkerError("the request has an empty image")
    prompts = request.get("prompts") or []
    if not prompts:
        raise WorkerError("the request has no prompts")
    for prompt in prompts:
        if not isinstance(prompt, dict) or len(prompt) != 1:
            raise WorkerError(f"a prompt is not a tagged object: {prompt!r}")
        kind, body = next(iter(prompt.items()))
        if kind == "Point":
            if not (0 <= float(body["x"]) < width and 0 <= float(body["y"]) < height):
                raise WorkerError(f"a point lands outside the image: {body}")
        elif kind == "Box":
            rect = body["rect"]
            if float(rect["x1"]) <= float(rect["x0"]) or float(rect["y1"]) <= float(rect["y0"]):
                raise WorkerError(f"a box has no area: {rect}")
        elif kind == "Mask":
            if not request.get("mask"):
                raise WorkerError("a mask prompt arrived without the mask file")
        else:
            raise WorkerError(f"unknown prompt kind: {kind}")
    return request


def check_checkpoint(args: argparse.Namespace) -> Path:
    if not args.weights:
        raise WorkerError("no checkpoint was passed; install the model in the manager first")
    checkpoint = Path(args.weights[0])
    if not checkpoint.is_file():
        raise WorkerError(f"the checkpoint does not exist: {checkpoint}")
    if args.weights_sha256:
        import hashlib

        digest = hashlib.sha256()
        with checkpoint.open("rb") as handle:
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(block)
        measured = digest.hexdigest()
        if measured.lower() != args.weights_sha256.lower():
            raise WorkerError(f"the checkpoint's SHA-256 is {measured}, not the expected {args.weights_sha256}")
    return checkpoint


def split_prompts(request: dict) -> tuple[list[list[float]], list[int], list[float] | None, str | None]:
    """Return (points, labels, box, mask path) in the request's image pixels."""
    points: list[list[float]] = []
    labels: list[int] = []
    box: list[float] | None = None
    mask_path: str | None = None
    for prompt in request["prompts"]:
        kind, body = next(iter(prompt.items()))
        if kind == "Point":
            points.append([float(body["x"]), float(body["y"])])
            labels.append(1 if body.get("positive") else 0)
        elif kind == "Box":
            rect = body["rect"]
            box = [float(rect["x0"]), float(rect["y0"]), float(rect["x1"]), float(rect["y1"])]
        elif kind == "Mask":
            mask_path = request.get("mask")
    return points, labels, box, mask_path


def run_sam2(args: argparse.Namespace, request: dict, checkpoint: Path) -> tuple[bytes, str, float | None, int]:
    """Run Meta's SAM 2.1 image predictor on the CPU and return PNG bytes."""
    config = SAM2_CONFIGS.get(args.model or "")
    if config is None:
        raise WorkerError(f"{args.model} has no SAM 2.1 config in this worker")

    try:
        import numpy as np
        from PIL import Image
        from sam2.build_sam import build_sam2
        from sam2.sam2_image_predictor import SAM2ImagePredictor
    except Exception as error:  # noqa: BLE001 - the message is the point
        raise WorkerError(
            "the SAM 2.1 Python packages are not importable: install Meta's `sam2` package and PyTorch "
            f"for this interpreter, then try again ({error})"
        ) from error

    width = int(request["width"])
    height = int(request["height"])

    # The mask prompt: FrameForge draws the mask at the working size, so load it
    # at that size and use it as a hard intersection. It is not turned into
    # model logits, because that conversion would be a guess.
    mask_prompt = None
    mask_path = request.get("mask")
    if mask_path:
        path = Path(mask_path)
        if not path.is_file():
            raise WorkerError(f"the mask prompt does not exist: {path}")
        mask_prompt = np.asarray(Image.open(path).convert("L"), dtype=np.uint8)
        if mask_prompt.shape != (height, width):
            raise WorkerError(f"the mask prompt is {mask_prompt.shape[1]}x{mask_prompt.shape[0]}, but the request is {width}x{height}")

    # The image itself is supplied by the runner in a later increment; refuse
    # clearly rather than inventing one.
    image_path = request.get("image")
    if not image_path:
        raise WorkerError("the request carries no image; the selection runner must add one before this worker can run")
    image_file = Path(image_path)
    if not image_file.is_file():
        raise WorkerError(f"the image does not exist: {image_file}")
    image = np.asarray(Image.open(image_file).convert("RGB"))
    if image.shape[0] != height or image.shape[1] != width:
        raise WorkerError(f"the image is {image.shape[1]}x{image.shape[0]}, but the request is {width}x{height}")

    points, labels, box, _ = split_prompts(request)
    if not points and box is None and mask_prompt is not None:
        # Derive a box from the painted mask so the model has something to score.
        ys, xs = np.nonzero(mask_prompt >= 128)
        if len(xs) == 0:
            raise WorkerError("the mask prompt is empty")
        box = [float(xs.min()), float(ys.min()), float(xs.max()), float(ys.max())]

    started = time.monotonic()
    predictor = SAM2ImagePredictor(build_sam2(config, str(checkpoint), device="cpu"))
    predictor.set_image(image)
    kwargs = {"multimask_output": True}
    if points:
        kwargs["point_coords"] = np.asarray(points, dtype=np.float32)
        kwargs["point_labels"] = np.asarray(labels, dtype=np.int32)
    if box is not None:
        kwargs["box"] = np.asarray(box, dtype=np.float32)
    masks, scores, _ = predictor.predict(**kwargs)

    best = int(np.argmax(scores)) if len(scores) else 0
    mask = (masks[best] > 0.0).astype(np.uint8) * 255
    if mask_prompt is not None:
        # "Keep only what I painted": a hard intersection, not fabricated logits.
        mask = np.where(mask_prompt >= 128, mask, 0).astype(np.uint8)
    elapsed_ms = int((time.monotonic() - started) * 1000)
    score = float(scores[best]) if len(scores) else None

    # The reply must be the request's size, so this is never downscaled here;
    # the caller decides the working size when it exports the frame.
    out = Image.fromarray(mask, mode="L")
    buffer = tempfile.SpooledTemporaryFile()
    out.save(buffer, format="PNG")
    buffer.seek(0)
    return buffer.read(), "cpu", score, elapsed_ms


def write_reply(path: Path, reply: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(reply), encoding="utf-8")
    os.replace(temporary, path)


def self_test() -> int:
    """Exercise the pure parts. No torch, no weights, no image."""
    import copy

    sample = {
        "version": 1,
        "frame": 0,
        "width": 64,
        "height": 32,
        "prompts": [
            {"Point": {"x": 10.0, "y": 10.0, "positive": True, "frame": 0}},
            {"Box": {"rect": {"x0": 4.0, "y0": 4.0, "x1": 40.0, "y1": 24.0}, "frame": 0}},
        ],
        "mask": None,
        "max_side": 64,
    }
    with tempfile.TemporaryDirectory() as directory:
        request_path = Path(directory) / "request.json"
        request_path.write_text(json.dumps(sample), encoding="utf-8")
        assert load_request(request_path)["version"] == 1

        bad = copy.deepcopy(sample)
        bad["version"] = 2
        request_path.write_text(json.dumps(bad), encoding="utf-8")
        try:
            load_request(request_path)
        except WorkerError:
            pass
        else:
            print("self-test: a wrong version was accepted", file=sys.stderr)
            return 1

        reply_path = Path(directory) / "reply.json"
        write_reply(reply_path, {"version": 1, "error": "self-test"})
        assert json.loads(reply_path.read_text(encoding="utf-8"))["error"] == "self-test"

    print("segment_worker self-test: request validation and atomic replies work; no model was loaded")
    return 0


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    if args.self_test:
        return self_test()

    reply_path = Path(args.reply) if args.reply else None
    try:
        if args.device != "cpu":
            raise WorkerError(f"this build only runs on the CPU for these checkpoints; '{args.device}' is refused")
        if not args.request or not reply_path:
            raise WorkerError("--request and --reply are required")
        request = load_request(Path(args.request))
        checkpoint = check_checkpoint(args)

        if args.model in SAM2_CONFIGS:
            png, provider, score, elapsed_ms = run_sam2(args, request, checkpoint)
        elif args.model == "edgetam":
            raise WorkerError(
                "the EdgeTAM adapter is not wired yet: its image-predictor API has not been verified in this build. "
                "The prompts and the checkpoint are fine; the adapter is the missing piece."
            )
        else:
            raise WorkerError(f"{args.model} is not a selection model this worker can run")

        mask_path = reply_path.with_suffix(".mask.png")
        mask_path.write_bytes(png)
        write_reply(
            reply_path,
            {
                "version": PROTOCOL_VERSION,
                "frame": int(request["frame"]),
                "width": int(request["width"]),
                "height": int(request["height"]),
                "mask": str(mask_path),
                "score": score,
                "provider": provider,
                "elapsed_ms": elapsed_ms,
            },
        )
        return 0
    except WorkerError as error:
        if reply_path is not None:
            write_reply(reply_path, {"version": PROTOCOL_VERSION, "error": str(error)})
        print(f"segment_worker: {error}", file=sys.stderr)
        return 1
    except Exception as error:  # noqa: BLE001 - report, never fabricate a mask
        if reply_path is not None:
            write_reply(reply_path, {"version": PROTOCOL_VERSION, "error": f"unexpected failure: {error}"})
        print(f"segment_worker: unexpected failure: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

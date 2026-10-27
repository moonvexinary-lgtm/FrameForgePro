#!/usr/bin/env python3
"""Local R3 probe: drive the real XMem worker over protocol 1.

The probe builds a short synthetic clip with a moving rectangle, propagates the
first mask forward and the last mask backward through the real model, and
reports per-frame IoU plus measured timing. It requires a local XMem checkout
(MIT) and XMem.pth; neither is downloaded by this script or committed here.

This is local evidence, not a CI gate. It does not exercise cancellation,
resume, memory ceilings, or the final renderer's mask equality.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import numpy as np
from PIL import Image

WORKER = Path(__file__).with_name("xmem_worker.py")


class MemorySampler:
    """Sample the worker's peak RSS without requiring psutil at import time."""

    def __init__(self, pid: int):
        self.pid = pid
        self.peak = 0
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        try:
            import psutil
        except ImportError:
            return
        process = psutil.Process(self.pid)

        def sample() -> None:
            while not self._stop.is_set():
                try:
                    self.peak = max(self.peak, process.memory_info().rss)
                except Exception:
                    return
                self._stop.wait(0.1)

        self._thread = threading.Thread(target=sample, name="xmem-probe-memory", daemon=True)
        self._thread.start()

    def stop(self) -> float | None:
        self._stop.set()
        if self._thread is None:
            return None
        self._thread.join(timeout=2)
        return round(self.peak / (1024 * 1024), 1)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_clip(root: Path, width: int, height: int, frames: int) -> dict:
    frames_dir = root / "frames"
    truth_dir = root / "truth"
    frames_dir.mkdir(parents=True)
    truth_dir.mkdir(parents=True)
    rng = np.random.default_rng(7)
    x_axis = np.linspace(24, 96, width, dtype=np.float32)
    y_axis = np.linspace(32, 80, height, dtype=np.float32)
    background = (x_axis[None, :] + y_axis[:, None]) / 2.0
    box_w = max(22, width // 10)
    box_h = max(18, height // 8)
    margin_x = max(8, width // 20)
    top_base = max(8, height // 4)
    step = max(4, (width - 2 * margin_x - box_w) // max(1, frames - 1))
    boxes = []
    for index in range(frames):
        left = margin_x + index * step
        top = top_base + (index % 2) * max(2, height // 20)
        rgb = np.repeat(background[:, :, None], 3, axis=2)
        rgb = np.clip(rgb + rng.integers(-5, 6, size=rgb.shape), 0, 255).astype(np.uint8)
        rgb[top : top + box_h, left : left + box_w] = np.array([230, 210, 120], dtype=np.uint8)
        truth = np.zeros((height, width), dtype=np.uint8)
        truth[top : top + box_h, left : left + box_w] = 255
        Image.fromarray(rgb, mode="RGB").save(frames_dir / f"frame_{index:08d}.png")
        Image.fromarray(truth, mode="L").save(truth_dir / f"frame_{index:08d}.png")
        boxes.append((left, top))
    return {"frames_dir": frames_dir, "truth_dir": truth_dir, "boxes": boxes}


class Protocol:
    def __init__(self, source: Path, version: str, log_path: Path):
        self.log = log_path.open("w", encoding="utf-8")
        self.process = subprocess.Popen(
            [sys.executable, "-u", str(WORKER), "--xmem-source", str(source), "--xmem-version", version],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.log,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.events: "queue.Queue[dict]" = queue.Queue()
        self.reader = threading.Thread(target=self._read, name="xmem-probe-reader", daemon=True)
        self.reader.start()

    def _read(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                self.events.put(json.loads(line))
            except json.JSONDecodeError as error:
                self.events.put({"event": "bad_json", "message": str(error), "line": line[:400]})
        self.events.put({"event": "__eof__"})

    def send(self, message: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def wait(self, wanted: set[str], timeout: float) -> dict:
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"worker did not emit {sorted(wanted)} within {timeout:.0f}s")
            try:
                event = self.events.get(timeout=min(remaining, 1.0))
            except queue.Empty:
                continue
            if event.get("event") in wanted:
                return event
            if event.get("event") in ("error", "bad_json", "__eof__"):
                raise RuntimeError(f"worker failed: {event}")
            # progress and ready lines are informational here

    def close(self) -> None:
        try:
            self.send({"command": "shutdown"})
            self.process.wait(timeout=15)
        except Exception:
            self.process.kill()
        finally:
            self.log.close()


def iou(predicted: Path, truth: Path) -> float:
    with Image.open(predicted) as image:
        a = np.asarray(image.convert("L"), dtype=np.uint8) > 127
    with Image.open(truth) as image:
        b = np.asarray(image.convert("L"), dtype=np.uint8) > 127
    if a.shape != b.shape:
        raise ValueError(f"shape mismatch {a.shape} vs {b.shape}")
    union = np.logical_or(a, b).sum()
    return float(np.logical_and(a, b).sum() / union) if union else 1.0


def run_direction(
    protocol: Protocol,
    direction: str,
    clip: dict,
    root: Path,
    width: int,
    height: int,
    frames: int,
    model: Path,
    model_sha: str,
    version: str,
    timeout: float,
) -> dict:
    truth_dir = clip["truth_dir"]
    initial_frame = 0 if direction == "forward" else frames - 1
    initial_mask = root / f"initial_{direction}.png"
    shutil.copyfile(truth_dir / f"frame_{initial_frame:08d}.png", initial_mask)
    output_dir = root / f"out_{direction}"
    started = time.perf_counter()
    protocol.send(
        {
            "command": "propagate",
            "frames_dir": str(clip["frames_dir"]),
            "first_frame": 0,
            "last_frame": frames - 1,
            "initial_mask": str(initial_mask),
            "output_dir": str(output_dir),
            "width": width,
            "height": height,
            "model": str(model),
            "model_sha256": model_sha,
            "direction": direction,
        }
    )
    done = protocol.wait({"done"}, timeout=timeout)
    elapsed = time.perf_counter() - started
    scores = []
    for index in range(frames):
        predicted = output_dir / f"frame_{index:08d}.png"
        if not predicted.is_file():
            raise RuntimeError(f"worker did not write {predicted}")
        scores.append(round(iou(predicted, truth_dir / f"frame_{index:08d}.png"), 4))
    return {
        "frames": done.get("frames"),
        "elapsed_s": round(elapsed, 3),
        "reported_elapsed_s": done.get("elapsed"),
        "fps": round(frames / elapsed, 3) if elapsed > 0 else None,
        "iou": scores,
        "iou_mean": round(float(np.mean(scores)), 4),
        "iou_min": round(float(np.min(scores)), 4),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xmem-source", required=True)
    parser.add_argument("--weights", required=True)
    parser.add_argument("--frames", type=int, default=12)
    parser.add_argument("--width", type=int, default=160)
    parser.add_argument("--height", type=int, default=96)
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--measure-memory", action="store_true", help="sample the worker's peak RSS with psutil")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    source = Path(args.xmem_source).resolve()
    weights = Path(args.weights).resolve()
    if not (source / "model" / "network.py").is_file():
        raise SystemExit(f"not an XMem checkout: {source}")
    if not weights.is_file():
        raise SystemExit(f"weights not found: {weights}")
    if args.width <= 0 or args.height <= 0:
        raise SystemExit("width and height must be positive")

    version = "external"
    try:
        version = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "--short", "HEAD"], text=True, stderr=subprocess.DEVNULL
        ).strip()
    except Exception:
        pass

    root = Path(tempfile.mkdtemp(prefix="frameforge-xmem-probe-"))
    print(f"workdir: {root}")
    try:
        clip = build_clip(root, args.width, args.height, args.frames)
        model_sha = sha256_file(weights)
        protocol = Protocol(source, version, root / "worker.stderr.log")
        try:
            protocol.send({"command": "hello", "protocol": 1})
            ready = protocol.wait({"ready"}, timeout=120)
            sampler = MemorySampler(protocol.process.pid) if args.measure_memory else None
            if sampler:
                sampler.start()
            forward = run_direction(
                protocol, "forward", clip, root, args.width, args.height, args.frames, weights, model_sha, version, args.timeout
            )
            if sampler:
                forward["peak_rss_mb"] = sampler.stop()
            sampler = MemorySampler(protocol.process.pid) if args.measure_memory else None
            if sampler:
                sampler.start()
            backward = run_direction(
                protocol, "backward", clip, root, args.width, args.height, args.frames, weights, model_sha, version, args.timeout
            )
            if sampler:
                backward["peak_rss_mb"] = sampler.stop()
        finally:
            protocol.close()
        summary = {
            "xmem_version": version,
            "weights": str(weights),
            "weights_bytes": weights.stat().st_size,
            "weights_sha256": model_sha,
            "ready": ready,
            "width": args.width,
            "height": args.height,
            "frames": args.frames,
            "forward": forward,
            "backward": backward,
        }
        print(json.dumps(summary, indent=2))
        return 0 if forward["iou_min"] >= 0.5 and backward["iou_min"] >= 0.5 else 1
    finally:
        if not args.keep:
            shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())

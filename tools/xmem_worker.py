#!/usr/bin/env python3
"""FrameForge Pro roto worker, protocol 1.

This is the protocol adapter, not a reimplementation of XMem. It imports the
official MIT-licensed XMem network and inference core from a checkout supplied
with ``--xmem-source`` and runs the real model on CPU. The checkout and the
weights are never downloaded by this script and are not part of the repository.

Only JSON-lines protocol messages are written to stdout; model/library chatter
is redirected to stderr so it cannot corrupt the protocol stream.

Windows note: stdin is polled with ``PeekNamedPipe`` instead of a reader
thread. A blocking stdin read in a background thread starves the main thread
during ``import torch`` on this Python/Windows build, so the worker imports
torch first and only then polls for ``cancel`` between frames.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
import sys
import time
from pathlib import Path

PROTOCOL = 1
CANCEL_REQUESTED = False
PENDING: list[dict] = []
_MODEL_CACHE: dict[tuple[str, str], object] = {}
# Keep the real protocol stream even while library chatter is redirected.
PROTOCOL_OUT = sys.stdout


def emit(event: dict) -> None:
    PROTOCOL_OUT.write(json.dumps(event, separators=(",", ":")) + "\n")
    PROTOCOL_OUT.flush()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stdin_available() -> bool:
    """True when at least one byte can be read without blocking."""
    if os.name == "nt":
        import ctypes
        import msvcrt

        try:
            handle = msvcrt.get_osfhandle(sys.stdin.fileno())
        except OSError:
            return False
        available = ctypes.c_ulong(0)
        ok = ctypes.windll.kernel32.PeekNamedPipe(
            handle, None, 0, None, ctypes.byref(available), None
        )
        return bool(ok) and available.value > 0
    import select

    return bool(select.select([sys.stdin], [], [], 0)[0])


def parse_line(line: str) -> dict | None:
    line = line.strip()
    if not line:
        return None
    try:
        message = json.loads(line)
    except json.JSONDecodeError as error:
        return {"command": "__bad__", "message": str(error)}
    if not isinstance(message, dict):
        return {"command": "__bad__", "message": "command is not an object"}
    return message


def poll_cancel() -> bool:
    """Consume any queued commands; return True when the job must stop."""
    global CANCEL_REQUESTED
    while stdin_available():
        line = sys.stdin.readline()
        if not line:
            CANCEL_REQUESTED = True
            break
        message = parse_line(line)
        if message is None:
            continue
        if message.get("command") == "cancel":
            CANCEL_REQUESTED = True
        elif message.get("command") == "shutdown":
            CANCEL_REQUESTED = True
            PENDING.append(message)
        else:
            PENDING.append(message)
    return CANCEL_REQUESTED


def next_message() -> dict | None:
    """Block for the next command when the worker is idle."""
    global CANCEL_REQUESTED
    while True:
        if PENDING:
            return PENDING.pop(0)
        line = sys.stdin.readline()
        if not line:
            return None
        message = parse_line(line)
        if message is None:
            continue
        if message.get("command") == "cancel":
            CANCEL_REQUESTED = True
            continue
        return message


def frame_index(path: Path) -> int | None:
    stem = path.stem
    if not stem.startswith("frame_"):
        return None
    try:
        return int(stem[len("frame_") :])
    except ValueError:
        return None


def load_model(source: str, model_path: Path, model_sha: str):
    key = (source, model_sha)
    cached = _MODEL_CACHE.get(key)
    if cached is not None:
        return cached

    sys.path.insert(0, source)
    # Imports and construction print to stdout; keep protocol stdout clean.
    with contextlib.redirect_stdout(sys.stderr):
        import torch
        from inference.inference_core import InferenceCore
        from model.network import XMem

        config = {
            "single_object": False,
            "mem_every": 5,
            "deep_update_every": -1,
            "enable_long_term": True,
            "enable_long_term_count_usage": True,
            "max_mid_term_frames": 10,
            "min_mid_term_frames": 5,
            "max_long_term_elements": 10000,
            "num_prototypes": 128,
            "top_k": 30,
        }
        network = XMem(config, str(model_path), map_location="cpu").eval()
        torch.set_grad_enabled(False)
    _MODEL_CACHE[key] = (network, InferenceCore, config)
    return _MODEL_CACHE[key]


def load_rgb(path: Path, width: int, height: int):
    from PIL import Image
    import numpy as np

    with Image.open(path) as image:
        rgb = image.convert("RGB")
        if rgb.size != (width, height):
            raise ValueError(f"{path.name} is {rgb.size[0]}x{rgb.size[1]}, expected {width}x{height}")
        array = np.asarray(rgb, dtype=np.float32) / 255.0
    return array.transpose(2, 0, 1)


def load_mask(path: Path, width: int, height: int):
    from PIL import Image
    import numpy as np

    with Image.open(path) as image:
        gray = image.convert("L")
        if gray.size != (width, height):
            raise ValueError(f"{path.name} mask is {gray.size[0]}x{gray.size[1]}, expected {width}x{height}")
        array = np.asarray(gray, dtype=np.uint8)
    return (array > 127).astype("float32")


def save_mask(path: Path, mask) -> None:
    from PIL import Image
    import numpy as np

    array = (mask > 0.5).astype(np.uint8) * 255
    Image.fromarray(array, mode="L").save(path)


def propagate(message: dict, source: str) -> None:
    global CANCEL_REQUESTED
    import numpy as np
    import torch

    frames_dir = Path(message["frames_dir"])
    output_dir = Path(message["output_dir"])
    width = int(message["width"])
    height = int(message["height"])
    first = int(message["first_frame"])
    last = int(message["last_frame"])
    direction = message.get("direction", "forward")
    model_path = Path(message["model"])
    model_sha = str(message.get("model_sha256", ""))
    initial_mask = Path(message["initial_mask"])

    if direction not in ("forward", "backward"):
        raise ValueError(f"unknown direction {direction!r}")
    if width <= 0 or height <= 0:
        raise ValueError("width and height must be positive")
    if last < first:
        raise ValueError("last_frame is before first_frame")
    if not model_path.is_file():
        raise ValueError(f"model weights not found: {model_path}")
    if model_sha and sha256_file(model_path).lower() != model_sha.lower():
        raise ValueError("model SHA-256 does not match the weights on disk")
    if not initial_mask.is_file():
        raise ValueError(f"initial mask not found: {initial_mask}")

    by_index: dict[int, Path] = {}
    for path in sorted(frames_dir.glob("frame_*.png")):
        index = frame_index(path)
        if index is not None:
            by_index[index] = path
    order = list(range(first, last + 1))
    if direction == "backward":
        order.reverse()
    missing = [index for index in order if index not in by_index]
    if missing:
        raise ValueError(f"missing exported frame(s): {missing[:8]}")
    output_dir.mkdir(parents=True, exist_ok=True)

    network, inference_core, config = load_model(source, model_path, model_sha)
    processor = inference_core(network, config=config)
    processor.set_all_labels([1])

    initial = load_mask(initial_mask, width, height)
    total = len(order)
    started = time.perf_counter()
    written = 0

    with contextlib.redirect_stdout(sys.stderr):
        for position, index in enumerate(order):
            if poll_cancel():
                emit({"event": "cancelled"})
                return
            image = load_rgb(by_index[index], width, height)
            image_tensor = torch.from_numpy(image)
            mask_tensor = None
            valid_labels = None
            if position == 0:
                mask_tensor = torch.from_numpy(initial).unsqueeze(0)
                valid_labels = [1]
            probability = processor.step(
                image_tensor,
                mask_tensor,
                valid_labels,
                end=(position == total - 1),
            )
            # Channel 0 is background; the single object is channel 1.
            save_mask(output_dir / f"frame_{index:08d}.png", probability[1].detach().cpu().numpy())
            written += 1
            elapsed = time.perf_counter() - started
            emit(
                {
                    "event": "progress",
                    "frame": index,
                    "total": total,
                    "elapsed": round(elapsed, 4),
                    "fps": round(written / elapsed, 4) if elapsed > 0 else 0.0,
                }
            )

    emit({"event": "done", "frames": written, "elapsed": round(time.perf_counter() - started, 4)})


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xmem-source", required=True, help="path to an official XMem checkout")
    parser.add_argument("--xmem-version", default="external", help="commit/version label for the handshake")
    args = parser.parse_args()

    source = str(Path(args.xmem_source).resolve())
    if not Path(source).is_dir():
        sys.stderr.write(f"xmem source is not a directory: {source}\n")
        return 2

    import torch  # imported before polling so stdin cannot starve the import

    while True:
        message = next_message()
        if message is None:
            return 0
        command = message.get("command")
        if command == "hello":
            emit(
                {
                    "event": "ready",
                    "protocol": PROTOCOL,
                    "backend": "cpu",
                    "torch": torch.__version__,
                    "xmem": args.xmem_version,
                }
            )
        elif command == "propagate":
            CANCEL_REQUESTED = False
            try:
                propagate(message, source)
            except Exception as error:  # keep the worker alive so the host can report it
                emit({"event": "error", "message": f"{type(error).__name__}: {error}"})
                import traceback

                traceback.print_exc(file=sys.stderr)
        elif command == "cancel":
            emit({"event": "cancelled"})
        elif command == "shutdown":
            return 0
        else:
            emit({"event": "error", "message": f"unknown command {command!r}"})


if __name__ == "__main__":
    raise SystemExit(main())

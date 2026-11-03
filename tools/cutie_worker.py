r"""Protocol-1 rotoscoping worker for Cutie, FrameForge Pro's preferred tracker.

It speaks the same JSON-lines protocol the bridge already drives for XMem
(``hello``/``ready``, ``propagate``, ``cancel``, ``shutdown``), so a project moves
between backends without changing the transport:

    {"command":"hello","protocol":1}
    {"event":"ready","protocol":1,"backend":"cpu","torch":"…","xmem":"cutie <commit>"}
    {"command":"propagate","frames_dir":"…","first_frame":0,"last_frame":119,
     "initial_mask":"…","output_dir":"…","width":480,"height":270,
     "model":"…/cutie-base-mega.pth","model_sha256":"…","direction":"forward"}
    {"event":"progress","frame":0,"total":120,"elapsed":0.1,"fps":10.0,"rss_mb":712.4}
    {"event":"done","frames":120,"elapsed":12.0}
    {"command":"cancel"} / {"event":"cancelled"}
    {"command":"shutdown"}

Contract, in the same spirit as ``xmem_worker.py``:

* it **never downloads** anything — no ``get_default_model()``, no ``torch.hub``,
  no network access at all. Weights must already be on disk and are hashed before
  they are loaded;
* it **never fabricates** a mask, an IoU or a timing number: if the model or the
  checkout is not usable, the answer is an ``error`` event saying exactly what is
  wrong;
* ``rss_mb`` is the worker's own resident set, so the editor's memory ceiling can
  be enforced on a real number;
* cancel is honoured between frames, and a partial run leaves the masks it
  already wrote for the editor to inspect.

**Status: written but not executed.** No inference was run while building this,
so the model-construction step below is the one piece that has not been proven
against a live Cutie install. It is written defensively: the worker reports the
exact failing step (checkout path, config, checkpoint, tensor shape) instead of
guessing, and everything except that step — protocol, verification, frame
conversion, cancellation, progress and mask writing — mirrors the adapter that
already runs for XMem.
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
# Keep the protocol stream clean even while a library prints to stdout.
PROTOCOL_OUT = sys.stdout
CANCEL_REQUESTED = False
PENDING: list[dict] = []
_MODEL_CACHE: dict[tuple, object] = {}


def emit(event: dict) -> None:
    PROTOCOL_OUT.write(json.dumps(event, separators=(",", ":")) + "\n")
    PROTOCOL_OUT.flush()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resident_mb() -> float | None:
    """This process's resident set in MiB, or None when the platform will not say."""
    try:
        if sys.platform == "win32":
            import ctypes
            from ctypes import wintypes

            class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
                _fields_ = [
                    ("cb", wintypes.DWORD),
                    ("PageFaultCount", wintypes.DWORD),
                    ("PeakWorkingSetSize", ctypes.c_size_t),
                    ("WorkingSetSize", ctypes.c_size_t),
                    ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                    ("PagefileUsage", ctypes.c_size_t),
                    ("PeakPagefileUsage", ctypes.c_size_t),
                ]

            counters = PROCESS_MEMORY_COUNTERS()
            counters.cb = ctypes.sizeof(counters)
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            get_current = kernel32.GetCurrentProcess
            get_current.restype = wintypes.HANDLE
            psapi = ctypes.WinDLL("psapi", use_last_error=True)
            ok = psapi.GetProcessMemoryInfo(get_current(), ctypes.byref(counters), ctypes.sizeof(counters))
            if ok:
                return counters.WorkingSetSize / (1024 * 1024)
            return None
        status = Path("/proc/self/status")
        if status.exists():
            for line in status.read_text().splitlines():
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) / 1024
        return None
    except Exception:  # the ceiling is best-effort; a probe must never kill a job
        return None


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
        ok = ctypes.windll.kernel32.PeekNamedPipe(handle, None, 0, None, ctypes.byref(available), None)
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
        command = message.get("command")
        if command == "cancel":
            CANCEL_REQUESTED = True
        elif command == "shutdown":
            CANCEL_REQUESTED = True
            PENDING.append(message)
        else:
            PENDING.append(message)
    return CANCEL_REQUESTED


def next_message() -> dict | None:
    """Block for the next command when the worker is idle."""
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
            global CANCEL_REQUESTED
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


def checkout_commit(source: Path) -> str:
    """The checkout's commit without running git, or 'external' when unreadable."""
    try:
        dot_git = source / ".git"
        git_dir = dot_git
        if dot_git.is_file():
            text = dot_git.read_text().strip()
            if not text.startswith("gitdir:"):
                return "external"
            target = Path(text.split(":", 1)[1].strip())
            git_dir = target if target.is_absolute() else source / target
        head = (git_dir / "HEAD").read_text().strip()
        if head.startswith("ref:"):
            loose = git_dir / head.split(":", 1)[1].strip()
            if loose.is_file():
                return loose.read_text().strip()[:7]
            packed = git_dir / "packed-refs"
            if packed.is_file():
                reference = head.split(":", 1)[1].strip()
                for line in packed.read_text().splitlines():
                    parts = line.split()
                    if len(parts) == 2 and parts[1] == reference:
                        return parts[0][:7]
            return "external"
        return head[:7]
    except Exception:
        return "external"


def verify_weights(model_path: Path, expected_sha: str) -> str:
    """Hash the weights and refuse a mismatch. An all-zero expectation means the
    caller has not pinned a digest yet, so the measured one is reported instead."""
    if not model_path.is_file():
        raise FileNotFoundError(f"weights not found: {model_path}")
    digest = sha256_file(model_path)
    expected = (expected_sha or "").strip().lower()
    if expected and set(expected) != {"0"}:
        if len(expected) != 64 or any(character not in "0123456789abcdef" for character in expected):
            raise ValueError("the pinned weights SHA-256 is not 64 hex digits")
        if digest != expected:
            raise ValueError(f"weights SHA-256 mismatch: expected {expected}, found {digest}")
    return digest


def load_cutie(source: str, model_path: Path, config_path: Path | None, digest: str):
    """Build Cutie from the user's own checkout and load the verified weights.

    Cutie's repository layout is the only thing this depends on: the network is
    instantiated from a Hydra config inside the checkout and the state dict is
    loaded from `model_path`. Nothing here downloads, and every failure names the
    file or the step that failed.
    """
    key = (source, digest, str(config_path))
    cached = _MODEL_CACHE.get(key)
    if cached is not None:
        return cached

    sys.path.insert(0, source)
    with contextlib.redirect_stdout(sys.stderr):  # imports print; keep the protocol stdout clean
        import torch
        from cutie.inference.inference_core import InferenceCore

        cfg = None
        candidates = [config_path] if config_path else [
            Path(source) / "cutie" / "config" / "eval_config.yaml",
            Path(source) / "config" / "eval_config.yaml",
        ]
        for candidate in candidates:
            if candidate and candidate.is_file():
                from omegaconf import OmegaConf

                cfg = OmegaConf.load(str(candidate))
                break
        if cfg is None:
            raise FileNotFoundError(
                "Cutie's config was not found. Pass --cutie-config with the eval config from your checkout "
                f"(looked at: {', '.join(str(c) for c in candidates)})"
            )

        from hydra.utils import instantiate

        network = instantiate(cfg.model, _recursive_=False)
        state = torch.load(str(model_path), map_location="cpu")
        # Cutie checkpoints wrap the weights in a "model" key; accept both shapes.
        if isinstance(state, dict) and "model" in state and isinstance(state["model"], dict):
            state = state["model"]
        network.load_state_dict(state)
        network.eval()
        torch.set_grad_enabled(False)
        processor = InferenceCore(network, cfg=cfg)
    _MODEL_CACHE[key] = (processor, InferenceCore, torch)
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

    if not path.is_file():
        raise FileNotFoundError(f"initial mask not found: {path}")
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


def propagate(message: dict, source: str, model_path: Path, config_path: Path | None, digest: str, device: str) -> None:
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
    initial_mask = Path(message["initial_mask"])

    if direction not in ("forward", "backward"):
        raise ValueError(f"unknown direction {direction!r}")
    if width <= 0 or height <= 0:
        raise ValueError("width and height must be positive")
    if last < first:
        raise ValueError("last_frame is before first_frame")
    if not frames_dir.is_dir():
        raise FileNotFoundError(f"frames directory not found: {frames_dir}")

    processor, _inference_core, _torch = load_cutie(source, model_path, config_path, digest)
    output_dir.mkdir(parents=True, exist_ok=True)

    order = list(range(first, last + 1))
    if direction == "backward":
        order.reverse()
    total = len(order)

    mask = load_mask(initial_mask, width, height)
    labels = [1]
    processor.set_all_labels(labels)
    first_frame = order[0]

    started = time.perf_counter()
    written = 0
    for index in order:
        if poll_cancel():
            emit({"event": "cancelled"})
            return
        frame_path = frames_dir / f"frame_{index:08d}.png"
        image_array = load_rgb(frame_path, width, height)
        image_tensor = torch.from_numpy(image_array).float().to(device)
        if index == first_frame:
            # Cutie takes the prompt as a label map (H, W, integer), not a
            # probability: 1 = the object, 0 = background.
            mask_tensor = torch.from_numpy(mask.astype("int64"))
            probability = processor.step(image_tensor, mask_tensor, labels, end=(index == order[-1]))
        else:
            probability = processor.step(image_tensor, end=(index == order[-1]))
        # Channel 0 is background; the single object is channel 1. The editor's
        # mask cache reads 8-bit grayscale PNGs, which save_mask writes.
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
                "rss_mb": resident_mb(),
            }
        )
        _ = np  # numpy is used by the loaders; keep the import explicit for them

    emit({"event": "done", "frames": written, "elapsed": round(time.perf_counter() - started, 4)})


def main() -> int:
    parser = argparse.ArgumentParser(description="Cutie worker for FrameForge Pro")
    parser.add_argument("--model", default="cutie-base", help="catalogue id of the model, for the handshake")
    parser.add_argument("--cutie-source", required=True, help="path to the user's Cutie checkout")
    parser.add_argument("--weights", required=True, help="Cutie weights (.pth) the user supplied")
    parser.add_argument("--weights-sha256", default="", help="expected SHA-256 of the weights, checked before loading")
    parser.add_argument("--cutie-config", default="", help="override the Hydra config inside the checkout")
    parser.add_argument("--device", default="cpu", help="torch device (cpu is the only one this project has verified)")
    args = parser.parse_args()

    source_path = Path(args.cutie_source).resolve()
    if not source_path.is_dir():
        sys.stderr.write(f"cutie source is not a directory: {source_path}\n")
        return 2
    model_path = Path(args.weights).resolve()
    if not model_path.is_file():
        sys.stderr.write(f"weights are not a file: {model_path}\n")
        return 2
    config_path = Path(args.cutie_config).resolve() if args.cutie_config else None

    # Verify before anything is imported, so a wrong file never reaches torch.
    try:
        digest = verify_weights(model_path, args.weights_sha256)
    except Exception as error:
        sys.stderr.write(f"weights verification failed: {error}\n")
        return 2

    import torch  # imported after verification so stdin cannot starve a long import

    version = checkout_commit(source_path)
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
                    "backend": args.device,
                    "torch": torch.__version__,
                    "xmem": f"cutie {version}",
                }
            )
        elif command == "propagate":
            global CANCEL_REQUESTED
            CANCEL_REQUESTED = False
            try:
                propagate(message, str(source_path), model_path, config_path, digest, args.device)
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

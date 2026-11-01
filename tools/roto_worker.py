"""Protocol-1 rotoscoping worker for FrameForge Pro's model-backed backends.

This worker is deliberately a *stub*: it speaks the same JSON-lines protocol the
bridge already drives for XMem (``hello``/``ready``, ``propagate``, ``cancel``,
``shutdown``) and it refuses to produce masks. It exists so the engine adapters
have a real worker path to plan against, and so the protocol can be exercised
without any model, runtime, weights or inference.

What it never does:

* it never downloads anything,
* it never imports torch, a model repository or a vision library,
* it never fabricates a mask, an IoU or a timing number,
* it never claims a backend works.

When a backend is genuinely wired, its real worker is a separate, user-installed
piece: the app points at it, verifies the weights, and only then runs it. Until
then every ``propagate`` is answered with an ``error`` event that says exactly
what is missing.
"""

from __future__ import annotations

import argparse
import json
import sys

PROTOCOL = 1
# Keep the protocol stream even if a library later writes to stdout.
PROTOCOL_OUT = sys.stdout


def emit(event: dict) -> None:
    PROTOCOL_OUT.write(json.dumps(event, separators=(",", ":")) + "\n")
    PROTOCOL_OUT.flush()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", default="", help="the catalogue model id this worker would run")
    parser.add_argument("--weights", action="append", default=[], help="a weights file the caller verified")
    parser.add_argument("--xmem-source", default="", help="unused by the stub; accepted for symmetry")
    args = parser.parse_args()

    backend = f"stub:{args.model or 'unknown'}"
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            emit({"event": "error", "message": f"invalid command: {error}"})
            continue
        command = message.get("command")
        if command == "hello":
            emit({"event": "ready", "protocol": PROTOCOL, "backend": backend, "torch": "none", "xmem": "none"})
        elif command == "propagate":
            missing = "no runtime is configured for this backend"
            weights = args.weights or []
            if not args.model:
                missing = "no model id was passed with --model"
            elif not weights:
                missing = "no weights were passed with --weights"
            emit(
                {
                    "event": "error",
                    "message": (
                        f"{backend} cannot propagate: {missing}. This worker is the honest protocol stub: "
                        "install the backend's runtime and weights, then point FrameForge at the real worker."
                    ),
                }
            )
        elif command == "cancel":
            emit({"event": "cancelled"})
        elif command == "shutdown":
            break
        else:
            emit({"event": "error", "message": f"unknown command {command!r}"})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

# Local XMem probe

These files are the protocol-1 adapter and a local R3 evidence harness. They are
not part of the Rust build and are not run by CI: CI has no PyTorch runtime and
no XMem weights.

## Requirements

- Python 3.11 with `torch`, `torchvision`, `numpy`, `Pillow` (the local machine
  used `torch 2.14.1+cpu`); `psutil` is optional and only needed for
  `--measure-memory`.
- An **official XMem checkout** (MIT, e.g. `https://github.com/hkchengrex/XMem`)
  supplied with `--xmem-source`. Do not copy it into this repository.
- `XMem.pth` supplied with `--weights`; it is never downloaded or committed.

## Run

```powershell
py -3.11 tools/xmem_r3_probe.py `
  --xmem-source C:\Users\you\Downloads\XMem-official `
  --weights C:\Users\you\Downloads\XMem.pth `
  --frames 12 --width 160 --height 96
```

Add `--width 480 --height 270 --measure-memory` for the target-laptop 480p
measurement. The probe builds a moving-rectangle clip, drives the real worker
over pipes, checks forward and backward IoU, and exits non-zero if either
direction's minimum IoU is below 0.5.

The probe covers accuracy, forward/backward propagation, cancellation and peak
RSS. It does **not** cover resume, a memory ceiling, the app job runner or proof
that the masks reach the renderer; R3 is not complete until those exist.

## Protocol

`xmem_worker.py` implements the JSON lines in
[`docs/ROTO_AI_PLAN.md`](../docs/ROTO_AI_PLAN.md): `hello`, `propagate`,
`cancel`, `shutdown`, with `ready`, `progress`, `done`, `error` and `cancelled`
events. Model/library prints are redirected to stderr; only protocol JSON is
written to stdout. The model is loaded once per worker and the weights SHA-256
is checked before loading.

The worker polls stdin with `PeekNamedPipe` instead of using a blocking reader
thread: on this Windows/Python build a blocked background stdin read starves
`import torch`.

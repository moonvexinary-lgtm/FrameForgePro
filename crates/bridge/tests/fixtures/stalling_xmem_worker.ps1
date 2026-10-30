# Stall fixture for the roto job runner: answers hello, emits one progress
# event for the requested range, then goes silent and ignores stdin. The runner
# must treat the silence as a stall (or a killable cancel), never as a pass.
$ErrorActionPreference = "Stop"

function Send-Json($object) {
    [Console]::Out.WriteLine(($object | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
}

$line = [Console]::In.ReadLine()
$message = $line | ConvertFrom-Json
if ($message.command -eq "hello") {
    Send-Json @{ event = "ready"; protocol = 1; backend = "fake-stall"; torch = "none"; xmem = "fake" }
}

$line = [Console]::In.ReadLine()
$message = $line | ConvertFrom-Json
if ($message.command -eq "propagate") {
    Send-Json @{ event = "progress"; frame = [int]$message.first_frame; total = 1; elapsed = 0.1; fps = 1.0 }
}

Start-Sleep -Seconds 60

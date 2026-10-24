# Fake XMem worker for the R2 protocol test. It models the transport and event
# shapes only: no PyTorch, no XMem, no weights, no image processing.
$ErrorActionPreference = "Stop"

function Send-Json($object) {
    [Console]::Out.WriteLine(($object | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
}

while ($true) {
    $line = [Console]::In.ReadLine()
    if ($null -eq $line) { break }
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $message = $line | ConvertFrom-Json
    switch ($message.command) {
        "hello" {
            Send-Json @{ event = "ready"; protocol = 1; backend = "fake-cpu"; torch = "none"; xmem = "fake" }
        }
        "propagate" {
            $total = ([int]$message.last_frame - [int]$message.first_frame) + 1
            for ($i = 0; $i -lt $total; $i++) {
                $frame = [int]$message.first_frame + $i
                Send-Json @{ event = "progress"; frame = $frame; total = $total; elapsed = 0.1; fps = 10.0 }
            }
            Send-Json @{ event = "done"; frames = $total; elapsed = 0.2 }
        }
        "cancel" {
            Send-Json @{ event = "cancelled" }
        }
        "shutdown" {
            break
        }
        default {
            Send-Json @{ event = "error"; message = "unknown command" }
        }
    }
    if ($message.command -eq "shutdown") { break }
}

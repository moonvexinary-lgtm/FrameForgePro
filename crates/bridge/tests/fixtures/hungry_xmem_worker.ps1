# Memory-ceiling fixture: a worker that reports an enormous resident set on its
# first progress event, then claims to finish. The runner must kill it rather
# than accept the result, and must finish normally when no ceiling is set.
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
            Send-Json @{ event = "ready"; protocol = 1; backend = "fake-hungry"; torch = "none"; xmem = "fake" }
        }
        "propagate" {
            $total = ([int]$message.last_frame - [int]$message.first_frame) + 1
            Send-Json @{ event = "progress"; frame = [int]$message.first_frame; total = $total; elapsed = 0.1; fps = 10.0; rss_mb = 999999.0 }
            Send-Json @{ event = "done"; frames = $total; elapsed = 0.2 }
        }
        "cancel" {
            Send-Json @{ event = "cancelled" }
        }
        "shutdown" {
            break
        }
    }
    if ($message.command -eq "shutdown") { break }
}

# Timeout fixture for the R2 worker test: starts, then never speaks. The test
# proves handshake_timeout returns a timeout and kill() ends the process.
$ErrorActionPreference = "Stop"
Start-Sleep -Seconds 60

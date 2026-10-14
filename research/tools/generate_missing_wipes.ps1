# FrameForge Pro - luma wipe generator (PowerShell)
#
# Generates PGM luma wipe files for Premiere-documented transitions that were
# missing from Kdenlive's shipped set (see PREMIERE_FEATURE_PARITY.md section C).
#
# Generated wipes (5, each in HD 1920x1080 and PAL 720x576):
#   barn_doors.pgm    - Premiere "Barn Doors": reveals from the center outward
#                       toward the left and right edges.
#   band_wipe.pgm     - Premiere "Band Wipe": 8 horizontal bands sweeping
#                       simultaneously in alternating directions.
#   random_blocks.pgm - Premiere "Random Blocks": a 16x9 grid of blocks
#                       revealed in a deterministic pseudo-random order
#                       (fixed-seed LCG, reproducible output).
#   wedge.pgm         - Premiere "Wedge Wipe": a quarter-pie wedge sweeping
#                       clockwise around the top-left corner.
#   zigzag_blocks.pgm - Premiere "Zig-Zag Blocks": a zigzag leading edge that
#                       advances in quantized block steps.
#
# Format matches the existing shipped lumas: binary P5 grayscale, minimal
# header "P5\n<W> <H>\n255\n", one byte per pixel, row-major from the top.
# Convention (verified against data/lumas/HD/linear_x.pgm): dark pixels (0)
# are revealed first, bright pixels (255) last. Kdenlive's luma transition
# auto-discovers these files (%lumaPaths), so no code changes are needed;
# new files must be added to data/lumas/CMakeLists.txt for installation.

param(
    [string]$Root = "data/lumas",
    [switch]$Verify
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Pgm {
    param([int]$W, [int]$H, [byte[]]$Gray, [string]$Path)
    $header = [System.Text.Encoding]::ASCII.GetBytes("P5`n$W $H`n255`n")
    $fs = [System.IO.File]::Create($Path)
    try {
        $fs.Write($header, 0, $header.Length)
        $fs.Write($Gray, 0, $Gray.Length)
    } finally { $fs.Dispose() }
    Write-Host "wrote $Path  ($W x $H, $($Gray.Length) pixels)"
}

function New-BarnDoors {
    param([int]$W, [int]$H)
    $col = [byte[]]::new($W)
    $half = $W / 2.0
    for ($x = 0; $x -lt $W; $x++) {
        $v = [Math]::Abs($x - $half) / $half * 255.0
        $col[$x] = [byte][Math]::Max(0, [Math]::Min(255, [int]$v))
    }
    $gray = [byte[]]::new($W * $H)
    for ($y = 0; $y -lt $H; $y++) {
        [Array]::Copy($col, 0, $gray, $y * $W, $W)
    }
    return $gray
}

function New-BandWipe {
    param([int]$W, [int]$H)
    $bands = 8
    $bandCol = [byte[][]]::new($bands)
    for ($b = 0; $b -lt $bands; $b++) {
        $row = [byte[]]::new($W)
        for ($x = 0; $x -lt $W; $x++) {
            $frac = if ($W -gt 1) { $x / ($W - 1.0) } else { 0.0 }
            if ($b % 2 -eq 1) { $frac = 1.0 - $frac }
            $row[$x] = [byte][Math]::Max(0, [Math]::Min(255, [int]($frac * 255.0)))
        }
        $bandCol[$b] = $row
    }
    $gray = [byte[]]::new($W * $H)
    for ($y = 0; $y -lt $H; $y++) {
        $b = [Math]::Min($bands - 1, [int]([Math]::Floor($y * $bands / $H)))
        [Array]::Copy($bandCol[$b], 0, $gray, $y * $W, $W)
    }
    return $gray
}

function New-RandomBlocks {
    param([int]$W, [int]$H)
    $cols = 16
    $rows = 9
    # Deterministic LCG (Park-Miller style constants, fixed seed) so the output
    # is reproducible on every machine and auditable in the repo.
    $seed = 42
    $state = $seed
    $lcg = {
        $script:state = ($state * 1103515245 + 12345) % 2147483648
        return $state
    }
    # Fisher-Yates shuffle of cell reveal order.
    $cells = 0..($cols * $rows - 1)
    for ($i = $cells.Count - 1; $i -gt 0; $i--) {
        $state = ($state * 1103515245 + 12345) % 2147483648
        $j = $state % ($i + 1)
        $tmp = $cells[$i]; $cells[$i] = $cells[$j]; $cells[$j] = $tmp
    }
    # $cells[k] = cell index revealed at step k; rank of cell c = position of c in $cells
    $rank = [int[]]::new($cells.Count)
    for ($k = 0; $k -lt $cells.Count; $k++) { $rank[$cells[$k]] = $k }
    $cellGray = [byte[]]::new($cells.Count)
    for ($c = 0; $c -lt $cells.Count; $c++) {
        $cellGray[$c] = [byte][Math]::Round($rank[$c] * 255.0 / ($cells.Count - 1))
    }
    $gray = [byte[]]::new($W * $H)
    $i = 0
    for ($y = 0; $y -lt $H; $y++) {
        $cy = [Math]::Min($rows - 1, [int]([Math]::Floor($y * $rows / $H)))
        for ($x = 0; $x -lt $W; $x++) {
            $cx = [Math]::Min($cols - 1, [int]([Math]::Floor($x * $cols / $W)))
            $gray[$i++] = $cellGray[$cy * $cols + $cx]
        }
    }
    return $gray
}

function New-Wedge {
    param([int]$W, [int]$H)
    # Quarter-pie wedge anchored at the top-left corner, sweeping clockwise
    # from the top edge (+x axis) down to the left edge (+y axis).
    $gray = [byte[]]::new($W * $H)
    $i = 0
    $q = [Math]::PI / 2.0
    for ($y = 0; $y -lt $H; $y++) {
        for ($x = 0; $x -lt $W; $x++) {
            $t = [Math]::Atan2($y, $x) / $q
            $gray[$i++] = [byte][Math]::Max(0, [Math]::Min(255, [int]($t * 255.0)))
        }
    }
    return $gray
}

function New-ZigzagBlocks {
    param([int]$W, [int]$H)
    # Zigzag leading edge (6 periods), advancing in 16 quantized steps.
    $gray = [byte[]]::new($W * $H)
    $i = 0
    $periods = 6.0
    $amp = 0.09
    $steps = 16.0
    for ($y = 0; $y -lt $H; $y++) {
        $ph = ($y / $H) * $periods
        $tri = [Math]::Abs(($ph % 2.0) - 1.0)   # triangle wave 0..1..0
        $offset = (1.0 - $tri) * $amp           # shift edge per row band
        for ($x = 0; $x -lt $W; $x++) {
            $t = ($x / [double]$W) + $offset
            $tq = [Math]::Round($t * $steps) / $steps
            $v = [int]($tq * 255.0)
            $gray[$i++] = [byte][Math]::Max(0, [Math]::Min(255, $v))
        }
    }
    return $gray
}

foreach ($set in @(
        @{ Dir = "HD";  W = 1920; H = 1080 },
        @{ Dir = "PAL"; W = 720;  H = 576 }
    )) {
    $W = $set.W; $H = $set.H
    $dir = Join-Path $Root $set.Dir
    if (-not (Test-Path $dir)) { throw "missing directory: $dir" }
    Write-Pgm $W $H (New-BarnDoors $W $H) (Join-Path $dir "barn_doors.pgm")
    Write-Pgm $W $H (New-BandWipe $W $H) (Join-Path $dir "band_wipe.pgm")
    Write-Pgm $W $H (New-RandomBlocks $W $H) (Join-Path $dir "random_blocks.pgm")
    Write-Pgm $W $H (New-Wedge $W $H) (Join-Path $dir "wedge.pgm")
    Write-Pgm $W $H (New-ZigzagBlocks $W $H) (Join-Path $dir "zigzag_blocks.pgm")
}

if ($Verify) {
    # Re-read each generated file, validate header and dimensions, report stats.
    foreach ($f in (Get-ChildItem -Path $Root -Recurse -Filter *.pgm |
            Where-Object { $_.Name -in @("barn_doors.pgm", "band_wipe.pgm", "random_blocks.pgm", "wedge.pgm", "zigzag_blocks.pgm") })) {
        $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
        # Parse minimal P5 header: "P5\n<W> <H>\n255\n"
        $text = [System.Text.Encoding]::ASCII.GetString($bytes[0..30])
        if ($text -notmatch "^P5\s+(\d+)\s+(\d+)\s+255\s") { Write-Host "BAD HEADER: $($f.FullName)"; continue }
        $W = [int]$Matches[1]; $H = [int]$Matches[2]
        $hdrLen = ($Matches[0]).Length
        $rasterLen = $bytes.Length - $hdrLen
        $ok = ($rasterLen -eq $W * $H)
        $raster = $bytes[$hdrLen..($bytes.Length - 1)]
        $min = ($raster | Measure-Object -Minimum).Minimum
        $max = ($raster | Measure-Object -Maximum).Maximum
        Write-Host ("verify {0}: {1}x{2} raster={3} ok={4} min={5} max={6} center={7} corner-tl={8}" -f `
                $f.Name, $W, $H, $rasterLen, $ok, $min, $max, $raster[[int]($W * $H / 2 + $W / 2)], $raster[0])
    }
}

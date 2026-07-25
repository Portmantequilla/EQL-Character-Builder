# Release build with build-path privacy.
#
# WHY THIS EXISTS: rustc bakes ABSOLUTE source paths into the binary for panic
# messages (<cargo-home>\registry\src\..., plus the workspace dir). The release
# profile's `strip = true` removes symbols but NOT those strings, so a plain
# `npm run tauri build` ships an exe containing the build machine's user name
# (verified: 168 occurrences of C:\Users\<name>\ before this was added).
#
# The stable fix is rustc's --remap-path-prefix. The paths are read from the
# ENVIRONMENT so no user name is ever hardcoded in this repo. (The cleaner
# `[profile.release] trim-paths` is still unstable on cargo 1.97.)
#
# Usage:   pwsh -File scripts/build_release.ps1
# Verify:  python scripts/scan_release.py

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$app  = Join-Path $repo "app"
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }

# NOTE: rustc applies the LAST matching remapping, so order least-specific first.
# Paths containing spaces would break RUSTFLAGS word-splitting — none of these
# normally do, but bail loudly rather than ship a half-remapped binary.
$prefixes = @(
    @{ From = $env:USERPROFILE; To = "/home"  },
    @{ From = $repo;            To = "/build" },
    @{ From = $cargoHome;       To = "/cargo" }
)
foreach ($p in $prefixes) {
    if ([string]::IsNullOrWhiteSpace($p.From)) { throw "empty remap source for $($p.To)" }
    if ($p.From -match '\s') { throw "path contains a space, cannot pass via RUSTFLAGS: $($p.From)" }
}

$env:RUSTFLAGS = ($prefixes | ForEach-Object { "--remap-path-prefix=$($_.From)=$($_.To)" }) -join " "
Write-Host "RUSTFLAGS = $env:RUSTFLAGS" -ForegroundColor Cyan
Write-Host "Building release (this recompiles everything when RUSTFLAGS change)..." -ForegroundColor Cyan

Push-Location $app
try {
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

Write-Host "`nDone. Now verify with:  python scripts/scan_release.py" -ForegroundColor Green

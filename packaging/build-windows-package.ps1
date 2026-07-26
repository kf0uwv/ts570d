# Build a Windows distribution package for ts570d-radio-control.
#
# Usage: pwsh ./packaging/build-windows-package.ps1
#
# Mirrors packaging/build-deb.sh's structure (version-from-Cargo.toml,
# stage-then-package), but for Windows: a plain zip of the release binaries
# plus docs, no installer. Per radio-cat-rs's docs/adr/0008-shared-release-
# workflow.md §3, this script takes NO arguments and assumes
# `cargo build --release` has already produced
# `target/release/ts570d.exe` (and any extra binaries, e.g. `pin-test.exe`)
# -- there is no `--skip-build`-style flag here because the shared release
# workflow always builds first, then calls this script.
#
# Output: ts570d-radio-control_<version>_windows-x86_64.zip in the project
# root.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Resolve-Path (Join-Path $ScriptDir "..")

# ── 1. Read the version from Cargo.toml ─────────────────────────────────────
$CargoToml = Get-Content (Join-Path $Root "Cargo.toml")
$VersionLine = $CargoToml | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1
if (-not $VersionLine) {
    throw "Could not find a 'version = ...' line in Cargo.toml"
}
$Version = ($VersionLine -replace '^version\s*=\s*"(.*)"\s*$', '$1')

$PkgName = "ts570d-radio-control_${Version}_windows-x86_64"
$Staging = Join-Path $Root "target/windows/$PkgName"
$Release = Join-Path $Root "target/release"

Write-Host "==> Staging into $Staging"
if (Test-Path $Staging) {
    Remove-Item -Recurse -Force $Staging
}
New-Item -ItemType Directory -Path $Staging | Out-Null

# ── 2. Copy binaries ─────────────────────────────────────────────────────────
# Main control application. Built by `cargo build --release` (the `ts570d`
# workspace package's own `[[bin]]`).
$MainExe = Join-Path $Release "ts570d.exe"
if (-not (Test-Path $MainExe)) {
    throw "Missing $MainExe -- run 'cargo build --release' first."
}
Copy-Item $MainExe (Join-Path $Staging "ts570d.exe")

# Shared RS-232 pin-test tool, now a `cat-transport-serial` binary in
# radio-cat-rs (see docs/adr/0006-windows-network-transport.md there, and
# this repo's own docs/adr/0006-windows-concurrency-model.md). Built via
# `cargo build --release -p cat-transport-serial --bin pin-test`, landing at
# the same `target/release/pin-test.exe` path.
$PinTestExe = Join-Path $Release "pin-test.exe"
if (Test-Path $PinTestExe) {
    Copy-Item $PinTestExe (Join-Path $Staging "pin-test.exe")
} else {
    Write-Warning "pin-test.exe not found at $PinTestExe -- packaging without it. " +
        "Build it with: cargo build --release -p cat-transport-serial --bin pin-test"
}

# ── 3. Copy docs ─────────────────────────────────────────────────────────────
Copy-Item (Join-Path $Root "README.md") (Join-Path $Staging "README.md")
Copy-Item (Join-Path $Root "LICENSE.txt") (Join-Path $Staging "LICENSE.txt")

# ── 4. Zip it up ─────────────────────────────────────────────────────────────
$Out = Join-Path $Root "$PkgName.zip"
if (Test-Path $Out) {
    Remove-Item -Force $Out
}

Write-Host "==> Compress-Archive -> $Out"
Compress-Archive -Path (Join-Path $Staging "*") -DestinationPath $Out

Write-Host ""
Write-Host "Package built: $Out"
Write-Host ""
Write-Host "Contents:"
Get-ChildItem $Staging | ForEach-Object { Write-Host "  $($_.Name)" }

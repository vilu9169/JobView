# Optional: activate the project-local Rust installation for this PowerShell session.
# Run from the repository: . .\scripts\use-local-rust.ps1
$mailViewRoot = Split-Path -Parent $PSScriptRoot
$mailViewCargo = Join-Path $mailViewRoot '.tools\cargo'
$mailViewRustup = Join-Path $mailViewRoot '.tools\rustup'
$mailViewCargoBin = Join-Path $mailViewCargo 'bin'

if (-not (Test-Path -LiteralPath (Join-Path $mailViewCargoBin 'cargo.exe'))) {
    throw 'No project-local Rust installation found. Install Rust using the README, or use an existing Rust installation on PATH.'
}

$env:CARGO_HOME = $mailViewCargo
$env:RUSTUP_HOME = $mailViewRustup
if (($env:PATH -split ';') -notcontains $mailViewCargoBin) {
    $env:PATH = "$mailViewCargoBin;$env:PATH"
}

Write-Host 'Project-local Rust enabled for this PowerShell session.'
& cargo --version
& rustc --version

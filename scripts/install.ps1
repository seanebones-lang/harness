# harness install script (Windows) — builds from source and installs to ~\.local\bin
# Usage (PowerShell):  
#   Invoke-RestMethod https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.ps1 | Invoke-Expression  
# Or from a clone:  .\scripts\install.ps1
$ErrorActionPreference = "Stop"

$RepoUrl = "https://github.com/seanebones-lang/harness.git"
$Repo = "seanebones-lang/harness"
$Artifact = "harness-windows-x86_64.exe"
if ($env:HARNESS_INSTALL_DIR) {
    $InstallDir = $env:HARNESS_INSTALL_DIR
} else {
    $InstallDir = Join-Path $HOME ".local\bin"
}

function Info($msg) { Write-Host "[harness] $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "[harness] $msg" -ForegroundColor Yellow }

function Install-Prebuilt {
    param([string]$Version = "latest")
    if ($Version -eq "latest") {
        $url = "https://github.com/$Repo/releases/latest/download/$Artifact"
    } else {
        $url = "https://github.com/$Repo/releases/download/$Version/$Artifact"
    }
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("harness-dl-" + [Guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    Info "Downloading prebuilt binary ($Version)..."
    try {
        Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp "harness.exe") -UseBasicParsing
    } catch {
        Warn "No prebuilt binary found for $Version"
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        return $false
    }
    try {
        $checksumUrl = $url.Substring(0, $url.LastIndexOf('/')) + '/checksums.txt'
        $checksumPath = Join-Path $tmp 'checksums.txt'
        Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing
        $entries = @(Get-Content -LiteralPath $checksumPath | Where-Object { $_ -match ('^[a-fA-F0-9]{64}\s+' + [regex]::Escape($Artifact) + '$') })
        if ($entries.Count -ne 1) { throw "Expected exactly one SHA-256 checksum for $Artifact" }
        $expected = ($entries[0] -split '\s+')[0]
        $actual = (Get-FileHash -LiteralPath (Join-Path $tmp 'harness.exe') -Algorithm SHA256).Hash
        if ($actual -ne $expected) { throw "Checksum mismatch for $Artifact" }
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        Copy-Item -LiteralPath (Join-Path $tmp "harness.exe") -Destination (Join-Path $InstallDir "harness.exe") -Force
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
    Info "Checksum verified; installed prebuilt binary to $InstallDir"
    return $true
}

$Version = if ($args.Count -gt 0) { $args[0] } else { "latest" }

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

if (($env:HARNESS_INSTALL_SOURCE -ne "1") -and (Install-Prebuilt -Version $Version)) {
    $Dest = Join-Path $InstallDir "harness.exe"
    & $Dest --version
    if ($LASTEXITCODE -ne 0) { throw "Installed binary failed its version check" }
    Info "Run: harness setup"
    exit 0
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo not found. Install Rust from https://rustup.rs (MSVC toolchain recommended)."
}

Info ("Rust " + (rustc --version))

$CargoBin = Join-Path $HOME ".cargo\bin\harness.exe"
if ((Test-Path -LiteralPath $CargoBin) -and ($InstallDir -ne (Join-Path $HOME ".cargo\bin"))) {
    Warn "~\.cargo\bin\harness.exe also exists — cargo install and this script use different paths."
}

$Tmp = $null
if (($Version -eq "latest") -and (Test-Path -LiteralPath "Cargo.toml") -and (Test-Path -LiteralPath "crates/harness-provider-core/Cargo.toml")) {
    $SrcDir = (Get-Location).Path
    Info "Building from current directory"
} else {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-Error "git not found. Install Git for Windows (adds sh.exe on PATH; recommended for the shell tool)."
    }
    $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("harness-install-" + [Guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    Info "Cloning $RepoUrl ..."
    if ($Version -eq "latest") {
        git clone --depth 1 $RepoUrl (Join-Path $Tmp "harness")
    } else {
        git clone --depth 1 --branch $Version $RepoUrl (Join-Path $Tmp "harness")
    }
    if ($LASTEXITCODE -ne 0) { throw "Source clone failed for $Version" }
    $SrcDir = Join-Path $Tmp "harness"
}

Push-Location $SrcDir
try {
    cargo build --locked --profile release-lto
    if ($LASTEXITCODE -ne 0) { throw "Source build failed" }
} finally {
    Pop-Location
}
$Built = Join-Path $SrcDir "target\release-lto\harness.exe"
if (-not (Test-Path -LiteralPath $Built)) { throw "Build did not produce harness.exe" }

$Dest = Join-Path $InstallDir "harness.exe"
Copy-Item -LiteralPath $Built -Destination $Dest -Force
Info "Installed $Dest"

if ($Tmp) {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}

if ($env:Path -notlike "*${InstallDir}*") {
    Warn "$InstallDir is not on your PATH. Add it under User environment variable Path, or:"
    Warn "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$InstallDir', 'User')"
}

& $Dest --version
if ($LASTEXITCODE -ne 0) { throw "Installed binary failed its version check" }
Info "Run: harness setup"

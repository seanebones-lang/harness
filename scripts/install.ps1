# harness install script (Windows) — builds from source and installs to ~\.local\bin
# Usage (PowerShell):  
#   Invoke-RestMethod https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.ps1 | Invoke-Expression  
# Or from a clone:  .\scripts\install.ps1
$ErrorActionPreference = "Stop"

$RepoUrl = "https://github.com/seanebones-lang/harness.git"
if ($env:HARNESS_INSTALL_DIR) {
    $InstallDir = $env:HARNESS_INSTALL_DIR
} else {
    $InstallDir = Join-Path $HOME ".local\bin"
}

function Info($msg) { Write-Host "[harness] $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "[harness] $msg" -ForegroundColor Yellow }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo not found. Install Rust from https://rustup.rs (MSVC toolchain recommended)."
}

Info ("Rust " + (rustc --version))

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$Tmp = $null
if (Test-Path -LiteralPath "Cargo.toml") {
    $SrcDir = (Get-Location).Path
    Info "Building from current directory"
} else {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-Error "git not found. Install Git for Windows (adds sh.exe on PATH; recommended for the shell tool)."
    }
    $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("harness-install-" + [Guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    Info "Cloning $RepoUrl ..."
    git clone --depth 1 $RepoUrl (Join-Path $Tmp "harness")
    $SrcDir = Join-Path $Tmp "harness"
}

Push-Location $SrcDir
try {
    cargo build --profile release-lto
    if ($LASTEXITCODE -ne 0) {
        Warn "release-lto build failed; trying release profile..."
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            Write-Error "cargo build failed"
        }
    }
    $LtoBin = Join-Path $SrcDir "target\release-lto\harness.exe"
    $RelBin = Join-Path $SrcDir "target\release\harness.exe"
    if (-not (Test-Path -LiteralPath $LtoBin) -and -not (Test-Path -LiteralPath $RelBin)) {
        Warn "Expected binary missing; building release..."
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            Write-Error "cargo build --release failed"
        }
    }
} finally {
    Pop-Location
}

$ReleaseLto = Join-Path $SrcDir "target\release-lto\harness.exe"
$Release = Join-Path $SrcDir "target\release\harness.exe"
$Built = if (Test-Path -LiteralPath $ReleaseLto) { $ReleaseLto } else { $Release }
if (-not (Test-Path -LiteralPath $Built)) {
    Write-Error "Build did not produce harness.exe under target\release-lto or target\release"
}

$Dest = Join-Path $InstallDir "harness.exe"
Copy-Item -LiteralPath $Built -Destination $Dest -Force
Info "Installed $Dest"

if ($Tmp) {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$HarnessHome = Join-Path $HOME ".harness"
New-Item -ItemType Directory -Force -Path $HarnessHome | Out-Null
$ConfigPath = Join-Path $HarnessHome "config.toml"
if (-not (Test-Path -LiteralPath $ConfigPath)) {
    Info "Creating default config at $ConfigPath"
    $configBody = @'
[provider]
# api_key = "sk-ant-..."   # or set ANTHROPIC_API_KEY for the session
model = "claude-sonnet-4-6"
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "nomic-embed-text"

[agent]
system_prompt = """
You are a powerful coding assistant running in a terminal.

Available tools:
  read_file, write_file     — read or overwrite files
  patch_file                — surgical old→new text replacement (prefer this over write_file for edits)
  list_dir                  — list directory contents
  shell                     — run shell commands (build, test, git, etc.)
  search_code               — regex search across the codebase
  spawn_agent               — run a sub-agent with base tools for parallel tasks
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json

Guidelines:
  - Prefer patch_file over write_file for targeted edits.
  - Always run tests or build commands after changes to verify correctness.
  - Be concise. Prefer making changes over explaining them.
  - When editing multiple files, use spawn_agent for parallelism.
  - In plan mode (--plan flag), destructive calls pause for user approval.
"""
'@
    Set-Content -LiteralPath $ConfigPath -Value $configBody -Encoding utf8
}

if ($env:Path -notlike "*${InstallDir}*") {
    Warn "$InstallDir is not on your PATH. Add it under User environment variable Path, or:"
    Warn "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$InstallDir', 'User')"
}

& $Dest --version
Info "Run: harness"

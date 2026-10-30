<#PSScriptInfo
.VERSION 1.0.0
.GUID 8f3b2e1a-4c7d-4a9e-8b6f-1e2d3c4a5b6c
.AUTHOR ritchegerona
.COMPANYNAME nanomite
.COPYRIGHT (c) 2026 ritchegerona. MIT License.
.TAGS nanomite, installer, rust, ai, agent
.PROJECTURI https://github.com/ritchegerona/nanomite
.LICENSEURI https://github.com/ritchegerona/nanomite/blob/master/LICENSE
.ICONURI https://raw.githubusercontent.com/ritchegerona/nanomite/master/assets/icon.png
.EXTERNALMODULEDEPENDENCIES
.REQUIREDSCRIPTS
.EXTERNALSCRIPTDEPENDENCIES
.RELEASENOTES Initial release of Windows installer
#>

<#
.SYNOPSIS
    nanomite — unified Windows installer (irm | iex)
    irm https://raw.githubusercontent.com/ritchegerona/nanomite/master/install.ps1 | iex

.DESCRIPTION
    Installs: Rust toolchain (via rustup-init.exe), Visual Studio Build Tools,
    and nanomite binary. Edge (pre-installed) is used for browse tools.

.NOTES
    Requires: Windows 10/11, PowerShell 5.1+ or PowerShell 7+
    Run in an elevated (Admin) session for Build Tools installation.
#>

#requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Configuration ──────────────────────────────────────────────────────────
$REPO           = "ritchegerona/nanomite"
$INSTALL_DIR    = "$env:USERPROFILE\.cargo\bin"  # standard cargo location
$BUILD_DIR      = "$env:TEMP\nanomite-build-$PID"
$MIN_RUST_VER   = [version]"1.75.0"   # MSRV
$RUSTUP_URL     = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"

# ── Helpers ────────────────────────────────────────────────────────────────
function Log   { param($msg) Write-Host "[$(Get-Date -Format HH:mm:ss)] $msg" -ForegroundColor Green }
function Warn  { param($msg) Write-Host "[$(Get-Date -Format HH:mm:ss)] $msg" -ForegroundColor Yellow }
function Error { param($msg) Write-Host "[$(Get-Date -Format HH:mm:ss)] $msg" -ForegroundColor Red }
function Die   { param($msg) Error $msg; exit 1 }

function HasCmd($cmd) { (Get-Command $cmd -ErrorAction SilentlyContinue) -ne $null }

function Test-RustVersion {
    if (-not (HasCmd cargo)) { return $false }
    try {
        $ver = (cargo --version) -replace '^cargo\s+', ''
        [version]$ver -ge $MIN_RUST_VER
    } catch { $false }
}

# ── 1. Ensure Rust toolchain ───────────────────────────────────────────────
if (Test-RustVersion) {
    $v = (cargo --version) -replace '^cargo\s+', ''
    Log "Rust $v OK (≥ $MIN_RUST_VER)"
} else {
    Log "Installing Rust (rustup-init.exe)..."
    $rustupExe = "$env:TEMP\rustup-init.exe"
    try {
        Invoke-WebRequest -Uri $RUSTUP_URL -OutFile $rustupExe -UseBasicParsing
        & $rustupExe -y --no-modify-path --profile minimal
        Remove-Item -Force $rustupExe -ErrorAction SilentlyContinue
    } catch {
        Die "Failed to install Rust: $_"
    }
    # Refresh PATH for this session
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    if (-not (Test-RustVersion)) {
        Die "Rust installed but cargo not found in PATH. Restart your shell and re-run."
    }
}

# ── 2. Ensure Visual Studio Build Tools (MSVC) ─────────────────────────────
function Ensure-BuildTools {
    # Check if MSVC is already available (cl.exe in PATH or VS installed)
    if (HasCmd cl) {
        Log "MSVC (cl.exe) found in PATH"
        return
    }
    # Check for VS Build Tools / Community / Professional / Enterprise
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vcPath = & $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vcPath) {
            $vcVars = Join-Path $vcPath "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $vcVars) {
                Log "Found Visual Studio at $vcPath — setting up MSVC environment"
                # We can't persist vcvars in this session easily; cargo will find it via vswhere
                return
            }
        }
    }

    # Not found — offer to install Build Tools
    Warn "MSVC not found. nanomite requires the MSVC toolchain (link.exe, cl.exe)."
    if ($PSVersionTable.PSVersion.Major -ge 7) {
        $choice = Read-Host "Install Visual Studio Build Tools now? (requires Admin) [Y/n]"
    } else {
        $choice = Read-Host "Install Visual Studio Build Tools now? (requires Admin) [Y/n]"
    }
    if ($choice -match '^[yY]') {
        Log "Installing Visual Studio Build Tools via winget..."
        try {
            # Try winget first (Win10 1809+ / Win11)
            if (HasCmd winget) {
                winget install --id Microsoft.VisualStudio.2022.BuildTools --silent --accept-source-agreements --accept-package-agreements
            } elseif (HasCmd choco) {
                choco install -y visualstudio2022buildtools --package-parameters "--add Microsoft.VisualStudio.Workload.VCTools"
            } else {
                Die "No winget or chocolatey found. Please install Visual Studio Build Tools manually from https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022"
            }
            Log "Build Tools installed. You may need to restart your shell."
        } catch {
            Die "Failed to install Build Tools: $_"
        }
    } else {
        Die "Please install Visual Studio Build Tools manually from https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022"
    }
}
Ensure-BuildTools

# ── 3. Chromium/Edge check ─────────────────────────────────────────────────
# Edge is pre-installed on Windows 10/11 and auto-discovered by nanomite
if (Test-Path "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe") -or
   (Test-Path "${env:ProgramFiles}\Microsoft\Edge\Application\msedge.exe") {
    Log "Microsoft Edge found — browse tools will work."
} else {
    Warn "Edge not found in standard location. Browse tools may not work."
}

# ── 4. Build nanomite ──────────────────────────────────────────────────────
Log "Cloning $REPO..."
if (Test-Path $BUILD_DIR) { Remove-Item -Recurse -Force $BUILD_DIR -ErrorAction SilentlyContinue }
git clone --depth 1 "https://github.com/$REPO.git" $BUILD_DIR

Log "Building release binary (this takes ~2-3 min)..."
Push-Location $BUILD_DIR
try {
    cargo build --release --locked
} catch {
    Die "Build failed: $_"
}
Pop-Location

# ── 5. Install binary ──────────────────────────────────────────────────────
$binSrc = Join-Path $BUILD_DIR "target\release\nanomite.exe"
if (-not (Test-Path $binSrc)) { Die "Built binary not found at $binSrc" }

New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
Copy-Item -Force $binSrc -Destination (Join-Path $INSTALL_DIR "nanomite.exe")
Log "Installed to $INSTALL_DIR\nanomite.exe"

# ── 6. PATH check ──────────────────────────────────────────────────────────
$pathDirs = ($env:PATH -split ';').Trim()
if ($pathDirs -notcontains $INSTALL_DIR) {
    Warn "$INSTALL_DIR is not in your PATH."
    Warn "Add it permanently (PowerShell):"
    Warn '  [Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";' + $INSTALL_DIR + '", "User")'
    Warn "Or add to your shell profile (\$PROFILE):"
    Warn '  $env:PATH += ";' + $INSTALL_DIR + '"'
}

# ── 7. Cleanup ─────────────────────────────────────────────────────────────
Remove-Item -Recurse -Force $BUILD_DIR -ErrorAction SilentlyContinue

# ── 8. Verify ──────────────────────────────────────────────────────────────
$binDst = Join-Path $INSTALL_DIR "nanomite.exe"
if (Test-Path $binDst) {
    $ver = & $binDst --version
    Log "✅ $ver installed successfully!"
    Log "Run 'nanomite' to start. First run will guide you through /connect."
} else {
    Die "Binary installed but verification failed."
}

# ── 9. Next steps ──────────────────────────────────────────────────────────
@"
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
NEXT STEPS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1) Start the agent (may need to restart shell for PATH):
   nanomite

2) Configure a provider (first run):
   /connect        # pick provider, paste API key
   /models         # pick a model

   Keyless local options (no API key needed):
   - Ollama:       /provider → ollama (http://localhost:11434/v1)
   - LM Studio:    /provider → lmstudio (http://localhost:1234/v1)

3) Browse tools:
   Microsoft Edge is pre-installed on Windows 10/11 — auto-discovered.

4) Keep updated:
   nanomite update   # git-checkout installs only
   # Or re-run this installer for the latest master

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
"@
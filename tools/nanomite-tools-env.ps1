# nanomite-tools-env.ps1
#
# Dot-source this file to make all nanomite tools available from ANY directory
# on the current Windows machine.
#
# Usage (PowerShell):
#   . C:\path\to\nanomite\tools\nanomite-tools-env.ps1
#
# Or add to your PowerShell profile ($PROFILE):
#   . 'C:\path\to\nanomite\tools\nanomite-tools-env.ps1'
#
# After loading, run from anywhere:
#   whois_rdap example.com --json
#   spiderfoot -t example.com -m sfp_whois
#   nanomite whois_rdap example.com --json
#
# Authorized use only: run against assets you own or have explicit written
# permission to assess. Do not use to identify or dox individuals.

# --- locate python executable ---
function Find-Python {
    foreach ($candidate in @('python3', 'python', 'py')) {
        $path = Get-Command $candidate -ErrorAction SilentlyContinue
        if ($path) { return $candidate }
    }
    return $null
}

$pythonCmd = Find-Python
if (-not $pythonCmd) {
    Write-Warning "[!] No Python found on PATH. Install Python from https://www.python.org/"
}

# Resolve the tools directory (this file lives in nanomite/tools/)
if ($PSScriptRoot) {
    $ENV_DIR = Resolve-Path (Split-Path -Parent $PSScriptRoot)
} else {
    # Fallback when dot-sourced in contexts without $PSScriptRoot
    $ENV_DIR = Resolve-Path (Join-Path $PWD "tools")
}
$env:NANOMITE_TOOLS_HOME = $ENV_DIR

# Add shim directories to PATH (install.ps1 creates shims here)
$shimDirs = @(
    "$env:LOCALAPPDATA\nanomite-tools\bin",
    "C:\Program Files\nanomite-tools\bin",
    "$ENV_DIR\bin"
)

$added = $false
foreach ($dir in $shimDirs) {
    if (Test-Path $dir) {
        $env:PATH = "$dir;$env:PATH"
        $added = $true
        break
    }
}

# Convenience function: nanomite <tool> [args]
function nanomite {
    param([string]$Tool, [string[]]$Args)

    if (-not $Tool) {
        Write-Host "Usage: nanomite <tool-name> [args...]"
        Write-Host "  e.g. nanomite whois_rdap example.com --json"
        Write-Host "  e.g. nanomite spiderfoot -t example.com -m sfp_whois"
        return 1
    }

    # Search shim locations first
    $found = $null
    foreach ($dir in $shimDirs) {
        $candidatePs1 = Join-Path $dir "$Tool.ps1"
        $candidateBat = Join-Path $dir "$Tool.bat"
        if (Test-Path $candidatePs1) { $found = $candidatePs1; break }
        if (Test-Path $candidateBat) { $found = $candidateBat; break }
    }

    if (-not $found) {
        # Fall back to direct script under custom/
        $found = Get-ChildItem -Path "$ENV_DIR\custom" -Recurse -Filter "$Tool.ps1" -File | Select-Object -First 1
        if (-not $found) {
            $found = Get-ChildItem -Path "$ENV_DIR\custom" -Recurse -Filter "$Tool.py" -File | Select-Object -First 1
        }
    }

    if (-not $found) {
        Write-Error "[!] Tool '$Tool' not found. Run install.ps1 first."
        return 127
    }

    if ($found.Extension -eq '.ps1') {
        & $found @Args
    } elseif ($found.Extension -eq '.bat') {
        cmd /c "$found" $Args
    } else {
        & $pythonCmd $found.FullName @Args
    }
}

if ($added) {
    Write-Host "[+] nanomite-tools env loaded (NANOMITE_TOOLS_HOME=$ENV_DIR)"
    Write-Host "[+] Python: $pythonCmd"
    Write-Host "[+] Run 'nanomite <tool> [args]' or install shims via install.ps1"
} else {
    Write-Host "[+] nanomite-tools env loaded (NANOMITE_TOOLS_HOME=$ENV_DIR)"
    Write-Host "[+] Python: $pythonCmd"
    Write-Host "[!] No shim dir found. Run install.ps1 first, or call tools by direct path."
    Write-Host "[+] Run 'nanomite <tool> [args]' to use direct script fallback."
}

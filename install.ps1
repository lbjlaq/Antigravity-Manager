# Antigravity Tools Install Script for Windows
# Usage: irm https://raw.githubusercontent.com/lbjlaq/Antigravity-Manager/main/install.ps1 | iex
#
# Parameters (set before running):
#   $Version = "4.1.25"  # Install specific version
#   $DryRun = $true      # Preview commands without executing

if (-not $Version) { $Version = "" }
if (-not $DryRun) { $DryRun = $false }

$ErrorActionPreference = "Continue"

$Repo = "lbjlaq/Antigravity-Manager"
$AppName = "Antigravity Tools"
$GithubApi = "https://api.github.com/repos/$Repo/releases"
$script:ReleaseVersion = ""
$script:DownloadUrl = ""
$script:Filename = ""
$script:HasError = $false

# Colors helper
function Write-ColorOutput {
    param([string]$ForegroundColor, [string]$Message)
    Write-Host $Message -ForegroundColor $ForegroundColor
}

function Info { Write-ColorOutput "Cyan" "[INFO] $args" }
function Success { Write-ColorOutput "Green" "[OK] $args" }
function Warn { Write-ColorOutput "Yellow" "[WARN] $args" }
function Script-Error {
    Write-ColorOutput "Red" "[ERROR] $args"
    $script:HasError = $true
}

function Wait-AndExit {
    param([int]$ExitCode = 0)
    Write-Host ""
    Write-Host "Press any key to exit..." -ForegroundColor Gray
    $null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
    exit $ExitCode
}

function Get-ReleaseVersion {
    if ($Version) {
        $script:ReleaseVersion = $Version
        Info "Using specified version: v$($script:ReleaseVersion)"
        return $true
    }

    Info "Fetching latest version..."

    # Method 1: Try GitHub API
    try {
        $release = Invoke-RestMethod -Uri "$GithubApi/latest" -Headers @{
            "User-Agent" = "Antigravity-Installer"
            "Accept"     = "application/vnd.github.v3+json"
        } -TimeoutSec 10
        $script:ReleaseVersion = $release.tag_name -replace "^v", ""
        Info "Latest version: v$($script:ReleaseVersion)"
        return $true
    } catch {
        Warn "GitHub API failed (rate limit?), trying fallback..."
    }

    # Method 2: Fallback - parse updater.json from releases (no API rate limit)
    try {
        $updaterJson = Invoke-RestMethod -Uri "https://github.com/$Repo/releases/latest/download/updater.json" -TimeoutSec 10
        $script:ReleaseVersion = $updaterJson.version -replace "^v", ""
        Info "Latest version (from updater.json): v$($script:ReleaseVersion)"
        return $true
    } catch {
        Warn "Fallback failed, trying redirect method..."
    }

    # Method 3: Last resort - follow redirect from /releases/latest
    try {
        Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest" -MaximumRedirection 0 -ErrorAction SilentlyContinue -UseBasicParsing
    } catch {
        $redirectUrl = $_.Exception.Response.Headers.Location
        if ($redirectUrl -and $redirectUrl -match "/tag/v?(.+)$") {
            $script:ReleaseVersion = $Matches[1]
            Info "Latest version (from redirect): v$($script:ReleaseVersion)"
            return $true
        }
    }

    Script-Error "Failed to determine latest version. Try specifying version manually:"
    Write-Host '  $Version = "4.1.25"; irm https://raw.githubusercontent.com/lbjlaq/Antigravity-Manager/main/install.ps1 | iex' -ForegroundColor Yellow
    return $false
}

function Get-DownloadUrl {
    # NSIS installer: Antigravity.Tools_4.1.25_x64-setup.exe
    $script:DownloadUrl = "https://github.com/$Repo/releases/download/v$($script:ReleaseVersion)/Antigravity.Tools_$($script:ReleaseVersion)_x64-setup.exe"
    $script:Filename = "Antigravity.Tools_$($script:ReleaseVersion)_x64-setup.exe"

    Info "Download URL: $($script:DownloadUrl)"
}

function Install-App {
    $tempDir = [System.IO.Path]::GetTempPath()
    $downloadPath = Join-Path $tempDir $script:Filename

    Info "Downloading $AppName v$($script:ReleaseVersion)..."

    if ($DryRun) {
        Write-ColorOutput "Yellow" "[DRY-RUN] Invoke-WebRequest -Uri $($script:DownloadUrl) -OutFile $downloadPath"
    } else {
        try {
            $ProgressPreference = 'Continue'
            Invoke-WebRequest -Uri $script:DownloadUrl -OutFile $downloadPath -UseBasicParsing
        } catch {
            Script-Error "Download failed: $_"
            Script-Error "URL: $($script:DownloadUrl)"
            return $false
        }
    }

    # Verify download
    if (-not $DryRun -and -not (Test-Path $downloadPath)) {
        Script-Error "Downloaded file not found at $downloadPath"
        return $false
    }

    Success "Downloaded to $downloadPath"

    Info "Running installer..."

    if ($DryRun) {
        Write-ColorOutput "Yellow" "[DRY-RUN] Start-Process -FilePath $downloadPath -Wait"
    } else {
        try {
            Start-Process -FilePath $downloadPath -Wait
        } catch {
            Script-Error "Installation failed: $_"
            return $false
        }
    }

    # Cleanup
    if (-not $DryRun -and (Test-Path $downloadPath)) {
        Remove-Item $downloadPath -Force
        Info "Cleaned up installer file"
    }

    return $true
}

# Main
Write-Host ""
Write-ColorOutput "Cyan" "========================================"
Write-ColorOutput "Cyan" "    $AppName Installer"
Write-ColorOutput "Cyan" "========================================"
Write-Host ""

# Step 1: Get version
if (-not (Get-ReleaseVersion)) {
    Wait-AndExit 1
}

# Step 2: Build download URL
Get-DownloadUrl

# Step 3: Download and install
if (-not (Install-App)) {
    Wait-AndExit 1
}

if ($script:HasError) {
    Wait-AndExit 1
}

Write-Host ""
Success "Installation complete!"
Write-Host ""
Info "Launch '$AppName' from the Start Menu or desktop shortcut."
Write-Host ""

# Only wait if running interactively
if ($Host.Name -eq "ConsoleHost") {
    Wait-AndExit 0
}

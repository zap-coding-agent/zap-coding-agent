#Requires -Version 5.1
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repo      = "zap-coding-agent/zap-coding-agent"
$BinName   = "zap.exe"
$Artifact  = "zap-windows-x86_64.zip"
$InstallDir = if ($env:ZAP_INSTALL_DIR) { $env:ZAP_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }

function Info  { Write-Host "  -> $args" -ForegroundColor Cyan }
function Ok    { Write-Host "  v $args"  -ForegroundColor Green }
function Warn  { Write-Host "  ! $args"  -ForegroundColor Yellow }
function Die   { Write-Host "  x $args"  -ForegroundColor Red; exit 1 }

Write-Host "`nInstalling zap" -ForegroundColor White

# ── Arch check ────────────────────────────────────────────────────────────────
if ($env:PROCESSOR_ARCHITECTURE -notin @('AMD64','EM64T')) {
    Die "Only x86_64 Windows is supported. Got: $env:PROCESSOR_ARCHITECTURE"
}

# ── Local binary detection (extracted package) ────────────────────────────────
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$LocalBin  = Join-Path $ScriptDir $BinName
$Binary    = $null
$Version   = "unknown"

if (Test-Path $LocalBin) {
    $Binary  = $LocalBin
    try { $Version = (& $Binary --version 2>$null | Select-Object -First 1) } catch {}
    if (-not $Version) { $Version = "local" }
    Info "Using local binary: $BinName  ($Version)"
} else {
    # ── Download from GitHub releases ─────────────────────────────────────────
    Info "Detecting latest release..."

    try {
        $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -ErrorAction Stop
    } catch {
        Die "Failed to reach GitHub API — check your internet connection. ($_)"
    }

    $Version     = $Release.tag_name
    $DownloadUrl = ($Release.assets | Where-Object { $_.name -eq $Artifact } | Select-Object -First 1).browser_download_url

    if (-not $Version)     { Die "Could not determine latest release version" }
    if (-not $DownloadUrl) { Die "No download found for $Artifact in release $Version" }

    Info "Found $Version ($Artifact)"

    $Tmp     = Join-Path $env:TEMP "zap-install-$([System.IO.Path]::GetRandomFileName())"
    New-Item -ItemType Directory -Path $Tmp | Out-Null
    $Archive = Join-Path $Tmp $Artifact

    Info "Downloading..."
    try {
        Invoke-WebRequest $DownloadUrl -OutFile $Archive -UseBasicParsing -ErrorAction Stop
    } catch {
        Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
        Die "Download failed: $_"
    }
    Ok "Downloaded"

    Expand-Archive -Path $Archive -DestinationPath $Tmp -Force

    # Binary may be at root or inside a pkg/ subfolder depending on archive layout
    $Binary = Get-ChildItem -Path $Tmp -Filter $BinName -Recurse | Select-Object -First 1 -ExpandProperty FullName
    if (-not $Binary) {
        Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
        Die "Binary '$BinName' not found in archive"
    }
}

# ── Install ───────────────────────────────────────────────────────────────────
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

$Dest = Join-Path $InstallDir $BinName
Copy-Item $Binary $Dest -Force
Ok "Installed -> $Dest"

# ── PATH update (user scope, no admin needed) ─────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -split ';' | Where-Object { $_ -eq $InstallDir }) {
    Info "$InstallDir already in user PATH"
} else {
    $NewPath = ($UserPath.TrimEnd(';') + ";$InstallDir").TrimStart(';')
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Ok "Added $InstallDir to user PATH"
    Warn "Restart your terminal (or open a new window) for PATH to take effect"
}

# ── Done ──────────────────────────────────────────────────────────────────────
Write-Host "`nzap $Version installed." -ForegroundColor Green
Write-Host "Run " -NoNewline; Write-Host "zap" -ForegroundColor Cyan -NoNewline; Write-Host " to start (after restarting your terminal).`n"

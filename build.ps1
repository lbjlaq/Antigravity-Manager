# Antigravity Tools Web Server Build Script
$ErrorActionPreference = "Stop"

Write-Host "===================================================" -ForegroundColor Cyan
Write-Host "     Antigravity Tools Web Server Build Script     " -ForegroundColor Cyan
Write-Host "===================================================" -ForegroundColor Cyan

Set-Location $PSScriptRoot

Write-Host "[1/3] Checking frontend dependencies..." -ForegroundColor Yellow
if (-not (Test-Path "node_modules")) {
    Write-Host "Installing dependencies with npm install --legacy-peer-deps..." -ForegroundColor Gray
    npm install --legacy-peer-deps
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERROR] npm install failed!" -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "Dependencies already installed." -ForegroundColor Green
}

Write-Host "[2/3] Building frontend (Vite)..." -ForegroundColor Yellow
npm run build
if ($LASTEXITCODE -ne 0) {
    Write-Host "[ERROR] Frontend build failed!" -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host "[3/3] Building Rust Web Server..." -ForegroundColor Yellow
Set-Location (Join-Path $PSScriptRoot "src-tauri")
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "[ERROR] Rust build failed!" -ForegroundColor Red
    exit $LASTEXITCODE
}

Set-Location $PSScriptRoot
$releasePath = Join-Path $PSScriptRoot "src-tauri\target\release"
$distTarget = Join-Path $releasePath "dist"

# 将前端 dist 复制到 release 目录，确保 exe 双击即可直接运行并提供 Web UI
if (Test-Path "dist") {
    Write-Host "Syncing dist static assets to release folder..." -ForegroundColor Gray
    if (Test-Path $distTarget) {
        Remove-Item -Recurse -Force $distTarget
    }
    Copy-Item -Recurse -Force "dist" $distTarget
}

Write-Host "===================================================" -ForegroundColor Green
Write-Host "Build finished successfully!" -ForegroundColor Green
Write-Host "Binary: $releasePath\antigravity-tools.exe" -ForegroundColor Green
Write-Host "Web Assets: $distTarget" -ForegroundColor Green
Write-Host "===================================================" -ForegroundColor Green

if (Test-Path $releasePath) {
    explorer.exe $releasePath
}

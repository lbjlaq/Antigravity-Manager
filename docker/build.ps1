# 构建二改版 Antigravity Manager Docker 镜像
# 用法:
#   .\docker\build.ps1
#   .\docker\build.ps1 -Tag "antigravity-manager:4.6.9-fix"
#   .\docker\build.ps1 -UseMirror   # 国内加速
#   .\docker\build.ps1 -Push -Registry "yourname/antigravity-manager"

param(
    [string]$Tag = "antigravity-manager:local",
    [string]$VersionTag = "",
    [ValidateSet("auto", "true", "false")]
    [string]$Mirror = "auto",
    [switch]$UseMirror,
    [switch]$Push,
    [string]$Registry = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Host @"
[错误] 未检测到 docker 命令。

请先安装并启动 Docker Desktop:
  winget install -e --id Docker.DockerDesktop

安装完成后重启电脑，打开 Docker Desktop，再重新运行本脚本。
"@ -ForegroundColor Red
    exit 1
}

if ($UseMirror) { $Mirror = "true" }

$Version = (Get-Content "package.json" -Raw | ConvertFrom-Json).version
if (-not $VersionTag) {
    $VersionTag = "antigravity-manager:$Version-fix"
}

Write-Host "==> 项目根目录: $Root" -ForegroundColor Cyan
Write-Host "==> 构建标签:   $Tag" -ForegroundColor Cyan
Write-Host "==> 版本标签:   $VersionTag" -ForegroundColor Cyan
Write-Host "==> 镜像源模式: $Mirror" -ForegroundColor Cyan
Write-Host ""

$buildArgs = @(
    "build",
    "-f", "docker/Dockerfile",
    "--build-arg", "USE_MIRROR=$Mirror",
    "-t", $Tag,
    "-t", $VersionTag,
    "."
)

Write-Host "==> 执行: docker $($buildArgs -join ' ')" -ForegroundColor Yellow
docker @buildArgs
if ($LASTEXITCODE -ne 0) {
    Write-Host "[错误] 镜像构建失败" -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "==> 构建成功" -ForegroundColor Green
docker images $Tag.Split(":")[0]

if ($Push) {
    if (-not $Registry) {
        Write-Host "[错误] -Push 需要同时指定 -Registry，例如 yourname/antigravity-manager" -ForegroundColor Red
        exit 1
    }
    $remoteLatest = "${Registry}:latest"
    $remoteVersion = "${Registry}:$Version-fix"
    docker tag $Tag $remoteLatest
    docker tag $VersionTag $remoteVersion
    docker push $remoteLatest
    docker push $remoteVersion
    Write-Host "==> 已推送: $remoteLatest , $remoteVersion" -ForegroundColor Green
}

Write-Host @"

启动示例:

  docker run -d ``
    --name antigravity-manager ``
    -p 8045:8045 ``
    -e API_KEY=your-api-key ``
    -e WEB_PASSWORD=your-login-password ``
    -v `${HOME}/.antigravity_tools:/root/.antigravity_tools ``
    $Tag

或使用 compose:

  docker compose -f docker/docker-compose.yml -f docker/docker-compose.fork.yml up -d --build

"@

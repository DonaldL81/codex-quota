$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $projectRoot

Write-Host "正在发布 Codex 额度监控便携版..."
Write-Host "项目目录：$projectRoot"
Write-Host ""

& (Join-Path $PSScriptRoot "build-portable.ps1")

Write-Host ""
Write-Host "正在执行发布前验证..."
& (Join-Path $PSScriptRoot "verify-release.ps1")

$tauriConfig = Get-Content -LiteralPath "src-tauri\tauri.conf.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$portableExe = Join-Path $projectRoot ("dist-portable\{0} {1} Portable.exe" -f $tauriConfig.productName, $tauriConfig.version)
$releaseNote = Join-Path $projectRoot "发布说明.md"
$outDir = Join-Path $projectRoot "dist-portable"
if (Test-Path -LiteralPath $releaseNote) {
  Copy-Item -LiteralPath $releaseNote -Destination (Join-Path $outDir "发布说明.md") -Force
}

Write-Host ""
Write-Host "发布完成。" -ForegroundColor Green
Write-Host "可分享文件："
Write-Host $portableExe
Write-Host ""
Write-Host "说明文档已复制到："
Write-Host (Join-Path $outDir "发布说明.md")

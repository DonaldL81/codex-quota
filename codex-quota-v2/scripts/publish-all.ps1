$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$repoRoot = Resolve-Path (Join-Path $projectRoot "..")
Set-Location $projectRoot

Write-Host "正在发布正式安装包和便携版 EXE..."
Write-Host "项目目录：$projectRoot"
Write-Host "仓库目录：$repoRoot"
Write-Host ""

& (Join-Path $PSScriptRoot "build-all.ps1")

Write-Host ""
Write-Host "正在执行发布前验证..."
& (Join-Path $PSScriptRoot "verify-release.ps1")

$tauriConfig = Get-Content -LiteralPath "src-tauri\tauri.conf.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$productName = $tauriConfig.productName
$version = $tauriConfig.version

$repoPortable = Join-Path $repoRoot ("{0} {1} Portable.exe" -f $productName, $version)
$repoSetup = Join-Path $repoRoot ("{0} {1} Setup.exe" -f $productName, $version)
if (-not (Test-Path -LiteralPath $repoPortable)) {
  throw "仓库根目录便携版不存在：$repoPortable"
}
if (-not (Test-Path -LiteralPath $repoSetup)) {
  throw "仓库根目录正式安装包不存在：$repoSetup"
}

$oldPackages = Get-ChildItem -LiteralPath $repoRoot -Filter "$productName *.exe" -File -ErrorAction SilentlyContinue |
  Where-Object {
    $_.FullName -ne $repoPortable -and
    $_.FullName -ne $repoSetup -and
    ($_.Name -match ' Portable\.exe$' -or $_.Name -match ' Setup\.exe$')
  }
if ($oldPackages) {
  Write-Host ""
  Write-Host "正在从 Git 索引移除旧版本发布包，本地文件保留："
  foreach ($package in $oldPackages) {
    Write-Host $package.FullName
    git -C $repoRoot rm --cached --ignore-unmatch -- $package.Name | Out-Null
  }
}

Write-Host ""
Write-Host "正在加入当前版本发布包到 Git 索引："
Write-Host $repoPortable
Write-Host $repoSetup
git -C $repoRoot add -f -- (Split-Path -Leaf $repoPortable) (Split-Path -Leaf $repoSetup)

$releaseNote = Join-Path $projectRoot "发布说明.md"
$outDir = Join-Path $projectRoot "dist-portable"
if (Test-Path -LiteralPath $releaseNote) {
  Copy-Item -LiteralPath $releaseNote -Destination (Join-Path $outDir "发布说明.md") -Force
}

Write-Host ""
Write-Host "发布产物："
Write-Host $repoPortable
Write-Host $repoSetup
Write-Host ""
Write-Host "发布完成。"

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

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
  throw "未找到 GitHub CLI（gh）。请先安装并登录 gh，才能创建 GitHub Release。"
}

$ghStatus = gh auth status 2>&1
if ($LASTEXITCODE -ne 0) {
  throw "GitHub CLI 尚未登录。请先执行 gh auth login。"
}

$tagName = "v$version"
$releaseTitle = "$productName $version"
$releaseNotes = @"
$productName $version

- 单文件免安装包：$(Split-Path -Leaf $repoPortable)
- 安装包：$(Split-Path -Leaf $repoSetup)
"@

Write-Host ""
Write-Host "正在创建或更新 GitHub Release：$tagName"
$releaseExists = $false
try {
  $releaseViewOutput = gh release view $tagName --repo "DonaldL81/codex-quota" 2>$null
  if ($LASTEXITCODE -eq 0) {
    $releaseExists = $true
  }
} catch {
  $releaseExists = $false
}

if (-not $releaseExists) {
  gh release create $tagName `
    --repo "DonaldL81/codex-quota" `
    --title $releaseTitle `
    --notes $releaseNotes `
    --latest
  if ($LASTEXITCODE -ne 0) {
    throw "创建 GitHub Release 失败：$tagName"
  }
} else {
  gh release edit $tagName `
    --repo "DonaldL81/codex-quota" `
    --title $releaseTitle `
    --notes $releaseNotes `
    --latest
  if ($LASTEXITCODE -ne 0) {
    throw "更新 GitHub Release 失败：$tagName"
  }
}

gh release upload $tagName $repoPortable $repoSetup --repo "DonaldL81/codex-quota" --clobber
if ($LASTEXITCODE -ne 0) {
  throw "上传 GitHub Release 资源失败：$tagName"
}

Write-Host ""
Write-Host "发布产物："
Write-Host $repoPortable
Write-Host $repoSetup
Write-Host "GitHub Release：$tagName"
Write-Host ""
Write-Host "发布完成。"

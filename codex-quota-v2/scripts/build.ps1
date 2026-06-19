$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $projectRoot

Write-Host "项目目录：$projectRoot"

. (Join-Path $PSScriptRoot "release-version.ps1")
if ($env:CODEX_QUOTA_BUMP_VERSION -eq "1") {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot -BumpPatch
} elseif ($env:CODEX_QUOTA_KEEP_VERSION -eq "1") {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot -NoPrompt
} else {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot
}
Write-Host "正在打包 Codex 额度监控 $selectedVersion 正式安装包..."

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path $cargoBin) -and ($env:Path -notlike "*$cargoBin*")) {
  $env:Path = "$cargoBin;$env:Path"
}

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "未找到 npm。请先安装 Node.js。"
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "未找到 Rust/Cargo。Tauri 2 需要 Rust 环境。"
}

$rustcVersion = rustc -Vv
$rustHost = ($rustcVersion | Select-String "^host:").ToString().Replace("host:", "").Trim()
Write-Host "Rust target: $rustHost"

if ($rustHost -like "*windows-msvc") {
  $clInPath = Get-Command cl.exe -ErrorAction SilentlyContinue
  $clInVs = $null
  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path -LiteralPath $vswhere) {
    $vsInstallPath = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
    if ($vsInstallPath) {
      $clInVs = Get-ChildItem $vsInstallPath -Recurse -Filter cl.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    }
  }
  if (-not $clInVs) {
    $vsRoots = @(
      "C:\Program Files\Microsoft Visual Studio",
      "C:\Program Files (x86)\Microsoft Visual Studio"
    )
    foreach ($vsRoot in $vsRoots) {
      if (Test-Path -LiteralPath $vsRoot) {
        $clInVs = Get-ChildItem $vsRoot -Recurse -Filter cl.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($clInVs) { break }
      }
    }
  }
  if ((-not $clInPath) -and (-not $clInVs)) {
    throw "当前 Rust 是 MSVC 目标，但未找到 Visual Studio Build Tools/MSVC。请安装 VS Build Tools，勾选 使用 C++ 的桌面开发 和 Windows SDK。"
  }
} elseif ($rustHost -like "*windows-gnu") {
  Write-Host "当前使用 GNU Rust 目标，跳过 MSVC 检查。"
} else {
  Write-Host "未识别的 Rust Windows 目标，继续尝试构建。"
}

if (-not (Test-Path "node_modules")) {
  Write-Host "正在安装前端依赖..."
  npm install
}

npm run build

Write-Host ""
Write-Host "打包完成后请查看："
Write-Host (Join-Path $projectRoot "src-tauri\target\release\bundle")



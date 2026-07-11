$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$repoRoot = Resolve-Path (Join-Path $projectRoot "..")
Set-Location $projectRoot

Write-Host "项目目录：$projectRoot"
Write-Host "仓库目录：$repoRoot"

. (Join-Path $PSScriptRoot "release-version.ps1")
if ($env:CODEX_QUOTA_BUMP_VERSION -eq "1") {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot -BumpPatch
} elseif ($env:CODEX_QUOTA_KEEP_VERSION -eq "1") {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot -NoPrompt
} else {
  $selectedVersion = Invoke-ReleaseVersionPrompt -ProjectRoot $projectRoot
}
Write-Host "正在打包 Codex 额度监控 $selectedVersion 单文件版 EXE..."

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

$tauriConfig = Get-Content -LiteralPath "src-tauri\tauri.conf.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$productName = $tauriConfig.productName
$version = $tauriConfig.version

npm run build

$sourceExe = Join-Path $projectRoot "src-tauri\target\release\codex-quota-v2.exe"
if (-not (Test-Path -LiteralPath $sourceExe)) {
  throw "便携版源 EXE 不存在：$sourceExe"
}

$repoPortable = Join-Path $repoRoot ("{0} {1} Portable.exe" -f $productName, $version)
if (Test-Path -LiteralPath $repoPortable) {
  Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
    try {
      if ($_.Path -eq $repoPortable) {
        Write-Host "正在停止已运行的便携版：PID $($_.Id)"
        Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
      }
    } catch {
      # Some system processes deny Path access; ignore them.
    }
  }
  Start-Sleep -Milliseconds 500
}
Copy-Item -LiteralPath $sourceExe -Destination $repoPortable -Force

$portableOutDir = Join-Path $projectRoot "dist-portable"
if (Test-Path -LiteralPath $portableOutDir) {
  Remove-Item -LiteralPath $portableOutDir -Recurse -Force
}
$bundleDir = Join-Path $projectRoot "src-tauri\target\release\bundle"
if (Test-Path -LiteralPath $bundleDir) {
  Remove-Item -LiteralPath $bundleDir -Recurse -Force
}

Write-Host ""
Write-Host "已输出到仓库根目录："
Write-Host $repoPortable
Write-Host ""
Write-Host "提示：当前只维护单文件版；子目录打包产物已清理。"

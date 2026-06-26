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
Write-Host "正在打包 Codex 额度监控 $selectedVersion 正式安装包和便携版 EXE..."

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

$outDir = Join-Path $projectRoot "dist-portable"
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

$portableExe = Join-Path $outDir "$productName $version Portable.exe"
if (Test-Path -LiteralPath $portableExe) {
  Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
    try {
      if ($_.Path -eq $portableExe) {
        Write-Host "正在停止已运行的便携版：PID $($_.Id)"
        Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
      }
    } catch {
      # Some system processes deny Path access; ignore them.
    }
  }
  Start-Sleep -Milliseconds 500
}
Copy-Item -LiteralPath $sourceExe -Destination $portableExe -Force

$nsisDir = Join-Path $projectRoot "src-tauri\target\release\bundle\nsis"
$setupExe = Get-ChildItem -LiteralPath $nsisDir -Filter ("{0}_{1}_*_setup.exe" -f $productName, $version) -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $setupExe) {
  $setupExe = Get-ChildItem -LiteralPath $nsisDir -Filter ("{0}_{1}_*setup.exe" -f $productName, $version) -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
}
if (-not $setupExe) {
  throw "正式安装包不存在：$nsisDir"
}

$repoPortable = Join-Path $repoRoot ("{0} {1} Portable.exe" -f $productName, $version)
$repoSetup = Join-Path $repoRoot ("{0} {1} Setup.exe" -f $productName, $version)
Copy-Item -LiteralPath $portableExe -Destination $repoPortable -Force
Copy-Item -LiteralPath $setupExe.FullName -Destination $repoSetup -Force

Write-Host ""
Write-Host "正式安装包目录："
Write-Host (Join-Path $projectRoot "src-tauri\target\release\bundle")
Write-Host ""
Write-Host "便携版 EXE："
Write-Host $portableExe
Write-Host ""
Write-Host "已同步到仓库根目录："
Write-Host $repoPortable
Write-Host $repoSetup
Write-Host ""
Write-Host "提示：便携版复用本次正式版编译产物，不会重复执行 tauri build --no-bundle。"

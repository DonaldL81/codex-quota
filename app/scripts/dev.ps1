$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $projectRoot

Write-Host "Starting Codex Quota Monitor 2.1 dev mode..."
Write-Host "Project: $projectRoot"

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $cargoBin) {
  $env:Path = "$cargoBin;$env:Path"
}

$winlibsBin = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
if (Test-Path $winlibsBin) {
  $env:Path = "$winlibsBin;$env:Path"
}

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "npm was not found. Install Node.js first."
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "Rust/Cargo was not found. Tauri 2 requires Rust."
}

if (-not (Get-Command windres -ErrorAction SilentlyContinue)) {
  throw "WinLibs windres was not found. Install WinLibs POSIX UCRT with winget first."
}

if (-not (Get-Command x86_64-w64-mingw32-gcc -ErrorAction SilentlyContinue)) {
  throw "WinLibs GCC was not found. Install WinLibs POSIX UCRT with winget first."
}

if (-not (Test-Path "node_modules")) {
  Write-Host "Installing frontend dependencies..."
  npm install
}

$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "x86_64-w64-mingw32-gcc.exe"

$viteLog = Join-Path $projectRoot ("dev-vite-{0}.log" -f $PID)
$viteProcess = $null

try {
  $ready = $false

  try {
    $response = Invoke-WebRequest -Uri "http://127.0.0.1:1420" -UseBasicParsing -TimeoutSec 2
    if ($response.StatusCode -eq 200) {
      $ready = $true
      Write-Host "Reusing existing Vite dev server: http://127.0.0.1:1420"
    }
  } catch {
    $ready = $false
  }

  if (-not $ready) {
    $viteCommand = "Set-Location '$projectRoot'; npm run dev *> '$viteLog'"
    $viteProcess = Start-Process powershell -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $viteCommand) -WindowStyle Hidden -PassThru

    for ($i = 0; $i -lt 30; $i++) {
      try {
        $response = Invoke-WebRequest -Uri "http://127.0.0.1:1420" -UseBasicParsing -TimeoutSec 2
        if ($response.StatusCode -eq 200) {
          $ready = $true
          break
        }
      } catch {
        Start-Sleep -Seconds 1
      }
    }
  }

  if (-not $ready) {
    if (Test-Path $viteLog) {
      Get-Content $viteLog -Tail 80
    }
    throw "Vite dev server did not become ready on http://127.0.0.1:1420."
  }

  Write-Host "Vite is ready: http://127.0.0.1:1420"
  Set-Location (Join-Path $projectRoot "src-tauri")
  cargo +stable-x86_64-pc-windows-gnu run --target x86_64-pc-windows-gnu
} finally {
  if ($viteProcess -and -not $viteProcess.HasExited) {
    Stop-Process -Id $viteProcess.Id -Force -ErrorAction SilentlyContinue
  }
}

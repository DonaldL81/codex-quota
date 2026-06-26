$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $projectRoot

$results = New-Object System.Collections.Generic.List[object]

function Add-Result {
  param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][bool]$Ok,
    [string]$Detail = ""
  )
  $results.Add([pscustomobject]@{
    Name = $Name
    Ok = $Ok
    Detail = $Detail
  }) | Out-Null
  if ($Ok) {
    Write-Host "[通过] $Name" -ForegroundColor Green
  } else {
    Write-Host "[失败] $Name" -ForegroundColor Red
  }
  if ($Detail) {
    Write-Host "       $Detail"
  }
}

function Read-Json {
  param([Parameter(Mandatory = $true)][string]$Path)
  Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
}

function Get-CargoVersion {
  param([Parameter(Mandatory = $true)][string]$Path)
  $text = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
  $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
  if ($match.Success) {
    return $match.Groups[1].Value
  }
  return $null
}

function Find-CodexExe {
  if ($env:CODEX_QUOTA_CODEX_PATH -and (Test-Path -LiteralPath $env:CODEX_QUOTA_CODEX_PATH)) {
    return (Resolve-Path -LiteralPath $env:CODEX_QUOTA_CODEX_PATH).Path
  }

  if ($env:LOCALAPPDATA) {
    $binDir = Join-Path $env:LOCALAPPDATA "OpenAI\Codex\bin"
    $defaultPath = Join-Path $binDir "codex.exe"
    if (Test-Path -LiteralPath $defaultPath) {
      return $defaultPath
    }
    if (Test-Path -LiteralPath $binDir) {
      $nested = Get-ChildItem -LiteralPath $binDir -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "codex.exe" } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Sort-Object { (Get-Item -LiteralPath $_).LastWriteTimeUtc } -Descending |
        Select-Object -First 1
      if ($nested) {
        return $nested
      }
    }
  }

  $command = Get-Command codex.exe -ErrorAction SilentlyContinue
  if ($command) {
    return $command.Source
  }

  return $null
}

function Test-WebView2Runtime {
  $knownRoots = @(
    "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application",
    "$env:ProgramFiles\Microsoft\EdgeWebView\Application",
    "$env:LOCALAPPDATA\Microsoft\EdgeWebView\Application"
  )
  foreach ($root in $knownRoots) {
    if ($root -and (Test-Path -LiteralPath $root)) {
      $exe = Get-ChildItem -LiteralPath $root -Recurse -Filter msedgewebview2.exe -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($exe) {
        return $exe.FullName
      }
    }
  }

  $registryRoots = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients"
  )
  foreach ($root in $registryRoots) {
    if (-not (Test-Path -LiteralPath $root)) {
      continue
    }
    foreach ($child in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
      $item = Get-ItemProperty -LiteralPath $child.PSPath -ErrorAction SilentlyContinue
      if ($item -and (($item.name -like "*WebView2*") -or ($item.pv -and $child.PSChildName -match "F1E7"))) {
        return "$($item.name) $($item.pv)"
      }
    }
  }
  return $null
}

function Get-PortableProcesses {
  param([Parameter(Mandatory = $true)][string]$PortableExe)

  $portablePath = (Resolve-Path -LiteralPath $PortableExe).Path
  $matches = @()
  foreach ($process in Get-Process -ErrorAction SilentlyContinue) {
    try {
      if ($process.Path -eq $portablePath) {
        $matches += $process
      }
    } catch {
      # Some system processes deny Path access; ignore them.
    }
  }
  return $matches
}

function Test-PortableStartup {
  param([Parameter(Mandatory = $true)][string]$PortableExe)

  if (-not (Test-Path -LiteralPath $PortableExe)) {
    throw "便携版 EXE 不存在：$PortableExe"
  }

  $existing = @(Get-PortableProcesses -PortableExe $PortableExe)
  if ($existing.Count -gt 0) {
    $p = $existing | Select-Object -First 1
    if ($p.MainWindowHandle -ne 0) {
      return "已运行：PID $($p.Id)，窗口句柄 $($p.MainWindowHandle)"
    }
    return "已运行：PID $($p.Id)，托盘/无边框窗口未暴露主窗口句柄。"
  }

  $process = Start-Process -FilePath $PortableExe -PassThru
  $startedByScript = $true
  try {
    for ($i = 0; $i -lt 20; $i++) {
      Start-Sleep -Milliseconds 500
      $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
      if (-not $current) {
        throw "启动后进程已退出。"
      }
      if ($current.MainWindowHandle -ne 0) {
        return "启动成功：PID $($current.Id)，窗口句柄 $($current.MainWindowHandle)"
      }
    }
    $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
    if ($current) {
      return "启动成功：PID $($current.Id)，托盘/无边框窗口未暴露主窗口句柄。"
    }
    throw "启动后 10 秒内未检测到运行中的进程。"
  } finally {
    if ($startedByScript) {
      $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
      if ($current) {
        Stop-Process -Id $current.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
}

function Find-AppStateFile {
  param(
    [Parameter(Mandatory = $true)][object]$TauriConfig
  )

  $candidates = @()
  if ($env:APPDATA -and $TauriConfig.identifier) {
    $candidates += Join-Path (Join-Path $env:APPDATA $TauriConfig.identifier) "window-state.json"
  }
  if ($env:LOCALAPPDATA -and $TauriConfig.identifier) {
    $candidates += Join-Path (Join-Path $env:LOCALAPPDATA $TauriConfig.identifier) "window-state.json"
  }
  if ($env:APPDATA -and $TauriConfig.productName) {
    $candidates += Join-Path (Join-Path $env:APPDATA $TauriConfig.productName) "window-state.json"
  }
  if ($env:LOCALAPPDATA -and $TauriConfig.productName) {
    $candidates += Join-Path (Join-Path $env:LOCALAPPDATA $TauriConfig.productName) "window-state.json"
  }

  foreach ($path in $candidates) {
    if (Test-Path -LiteralPath $path) {
      return (Resolve-Path -LiteralPath $path).Path
    }
  }
  return $null
}

function Test-AppStateFile {
  param(
    [Parameter(Mandatory = $true)][object]$TauriConfig
  )

  $stateFile = Find-AppStateFile -TauriConfig $TauriConfig
  if (-not $stateFile) {
    throw "未找到 window-state.json。请先启动一次便携版。"
  }

  $state = Get-Content -LiteralPath $stateFile -Raw -Encoding UTF8 | ConvertFrom-Json
  if (($state.mode -ne "small") -and ($state.mode -ne "large")) {
    throw "mode 字段异常：$($state.mode)"
  }
  if ($null -eq $state.alwaysOnTop) {
    throw "缺少 alwaysOnTop 字段。"
  }
  if (($state.alwaysOnTop -isnot [bool])) {
    throw "alwaysOnTop 字段不是布尔值。"
  }
  if ($state.position) {
    if (($null -eq $state.position.x) -or ($null -eq $state.position.y)) {
      throw "position 字段缺少 x/y。"
    }
  }
  if ($state.largeSize) {
    if (($null -eq $state.largeSize.width) -or ($null -eq $state.largeSize.height)) {
      throw "largeSize 字段缺少 width/height。"
    }
    if (($state.largeSize.width -lt 200) -or ($state.largeSize.width -gt 380)) {
      throw "largeSize.width 超出范围：$($state.largeSize.width)"
    }
    if (($state.largeSize.height -lt 100) -or ($state.largeSize.height -gt 200)) {
      throw "largeSize.height 超出范围：$($state.largeSize.height)"
    }
  }

  return "$stateFile，mode=$($state.mode)，alwaysOnTop=$($state.alwaysOnTop)"
}

function Read-CodexResponse {
  param(
    [Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process,
    [Parameter(Mandatory = $true)][int]$Id,
    [int]$TimeoutMs = 15000
  )

  $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  while ([DateTime]::UtcNow -lt $deadline) {
    $remaining = [int][Math]::Max(1, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
    $task = $Process.StandardOutput.ReadLineAsync()
    if (-not $task.Wait($remaining)) {
      break
    }
    $line = $task.Result
    if ($null -eq $line) {
      throw "Codex app-server closed before responding."
    }
    try {
      $message = $line | ConvertFrom-Json -ErrorAction Stop
      if ($message.id -eq $Id) {
        return $message
      }
    } catch {
      continue
    }
  }

  throw "Timed out waiting for Codex app-server response id=$Id."
}

function Invoke-CodexQuotaProbe {
  param(
    [Parameter(Mandatory = $true)][string]$CodexPath,
    [Parameter(Mandatory = $true)][string]$Version
  )

  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $CodexPath
  $psi.Arguments = "app-server --listen stdio://"
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true

  $process = [System.Diagnostics.Process]::Start($psi)
  try {
    $initialize = @{
      id = 1
      method = "initialize"
      params = @{
        clientInfo = @{
          name = "codex-quota-monitor-v2-verify"
          version = $Version
        }
        capabilities = @{}
      }
    } | ConvertTo-Json -Depth 8 -Compress
    $process.StandardInput.WriteLine($initialize)
    $process.StandardInput.Flush()
    $null = Read-CodexResponse -Process $process -Id 1

    $request = @{
      id = 2
      method = "account/rateLimits/read"
    } | ConvertTo-Json -Depth 4 -Compress
    $process.StandardInput.WriteLine($request)
    $process.StandardInput.Flush()
    $response = Read-CodexResponse -Process $process -Id 2

    if ($response.error) {
      $message = if ($response.error.message) { $response.error.message } else { "Codex returned an error." }
      throw $message
    }

    $result = $response.result
    $snapshot = $null
    if ($result.rateLimitsByLimitId -and $result.rateLimitsByLimitId.codex) {
      $snapshot = $result.rateLimitsByLimitId.codex
    } elseif ($result.rateLimits) {
      $snapshot = $result.rateLimits
    }
    if (-not $snapshot) {
      throw "Codex returned no rate limit data."
    }

    $primaryUsed = if ($snapshot.primary.usedPercent -ne $null) { [double]$snapshot.primary.usedPercent } else { 0 }
    $secondaryUsed = if ($snapshot.secondary.usedPercent -ne $null) { [double]$snapshot.secondary.usedPercent } else { 0 }
    $primaryRemaining = [Math]::Round([Math]::Max(0, [Math]::Min(100, 100 - $primaryUsed)))
    $secondaryRemaining = [Math]::Round([Math]::Max(0, [Math]::Min(100, 100 - $secondaryUsed)))
    return "5小时剩余 $primaryRemaining% / 周剩余 $secondaryRemaining%"
  } finally {
    try {
      if ($process -and -not $process.HasExited) {
        $process.Kill()
      }
    } catch {
      # Ignore cleanup errors.
    }
  }
}

Write-Host "Codex 额度监控发布验证"
Write-Host "项目目录: $projectRoot"
Write-Host ""

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path $cargoBin) -and ($env:Path -notlike "*$cargoBin*")) {
  $env:Path = "$cargoBin;$env:Path"
}

try {
  $packageJson = Read-Json "package.json"
  $tauriConfig = Read-Json "src-tauri\tauri.conf.json"
  $cargoVersion = Get-CargoVersion "src-tauri\Cargo.toml"
  $versions = @(@($packageJson.version, $tauriConfig.version, $cargoVersion) | Select-Object -Unique)
  $versionOk = $versions.Count -eq 1
  $versionDetail = "package.json=$($packageJson.version), tauri.conf.json=$($tauriConfig.version), Cargo.toml=$cargoVersion"
  Add-Result -Name "版本号同步" -Ok $versionOk -Detail $versionDetail

  $repoRoot = Resolve-Path (Join-Path $projectRoot "..")
  $portableExe = Join-Path $repoRoot ("{0} {1} Portable.exe" -f $tauriConfig.productName, $tauriConfig.version)
  $setupExe = Join-Path $repoRoot ("{0} {1} Setup.exe" -f $tauriConfig.productName, $tauriConfig.version)
  if (Test-Path -LiteralPath $portableExe) {
    $portableItem = Get-Item -LiteralPath $portableExe
    $portableSizeMb = [Math]::Round($portableItem.Length / 1MB, 2)
    $portableOk = $portableItem.Length -gt (1024 * 1024)
    $portableDetail = "{0} ({1} MB)" -f $portableExe, $portableSizeMb
    Add-Result -Name "便携版 EXE 存在" -Ok $portableOk -Detail $portableDetail
    try {
      $startupDetail = Test-PortableStartup -PortableExe $portableExe
      Add-Result -Name "便携版启动冒烟" -Ok $true -Detail $startupDetail
    } catch {
      Add-Result -Name "便携版启动冒烟" -Ok $false -Detail $_.Exception.Message
    }
    try {
      $stateDetail = Test-AppStateFile -TauriConfig $tauriConfig
      Add-Result -Name "窗口状态文件" -Ok $true -Detail $stateDetail
    } catch {
      Add-Result -Name "窗口状态文件" -Ok $false -Detail $_.Exception.Message
    }
  } else {
    Add-Result -Name "便携版 EXE 存在" -Ok $false -Detail $portableExe
    Add-Result -Name "便携版启动冒烟" -Ok $false -Detail "便携版 EXE 不存在，跳过启动测试。"
    Add-Result -Name "窗口状态文件" -Ok $false -Detail "便携版 EXE 不存在，跳过状态文件测试。"
  }

  if (Test-Path -LiteralPath $setupExe) {
    $setupItem = Get-Item -LiteralPath $setupExe
    $setupSizeMb = [Math]::Round($setupItem.Length / 1MB, 2)
    Add-Result -Name "正式安装包存在" -Ok ($setupItem.Length -gt (1024 * 1024)) -Detail ("{0} ({1} MB)" -f $setupExe, $setupSizeMb)
  } else {
    Add-Result -Name "正式安装包存在" -Ok $false -Detail $setupExe
  }

  $webView2 = Test-WebView2Runtime
  $webView2Detail = if ($webView2) { $webView2 } else { "未检测到 WebView2 Runtime" }
  Add-Result "WebView2 Runtime" ([bool]$webView2) $webView2Detail

  $codexPath = Find-CodexExe
  $codexDetail = if ($codexPath) { $codexPath } else { "未找到 codex.exe" }
  Add-Result "Codex 可执行文件" ([bool]$codexPath) $codexDetail

  if ($codexPath) {
    try {
      $quotaDetail = Invoke-CodexQuotaProbe -CodexPath $codexPath -Version $tauriConfig.version
      Add-Result "Codex app-server 额度读取" $true $quotaDetail
    } catch {
      Add-Result "Codex app-server 额度读取" $false $_.Exception.Message
    }
  } else {
    Add-Result "Codex app-server 额度读取" $false "未找到 codex.exe，跳过读取。"
  }

  $npmCommand = Get-Command npm -ErrorAction SilentlyContinue
  $npmDetail = if ($npmCommand) { $npmCommand.Source } else { "未找到 npm" }
  Add-Result "Node/npm 环境" ([bool]$npmCommand) $npmDetail

  $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
  $cargoDetail = if ($cargoCommand) { $cargoCommand.Source } else { "未找到 cargo" }
  Add-Result "Rust/Cargo 环境" ([bool]$cargoCommand) $cargoDetail
} catch {
  Add-Result "验证脚本执行" $false $_.Exception.Message
}

Write-Host ""
Write-Host "验证结果汇总"
$results | Format-Table -AutoSize

$failed = @($results | Where-Object { -not $_.Ok })
if ($failed.Count -gt 0) {
  Write-Host ""
  Write-Host "存在 $($failed.Count) 个失败项，请根据上方提示处理。" -ForegroundColor Red
  exit 1
}

Write-Host ""
Write-Host "全部验证通过。" -ForegroundColor Green

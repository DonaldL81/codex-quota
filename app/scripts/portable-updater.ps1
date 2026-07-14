param(
  [Parameter(Mandatory = $true)][string]$Source,
  [Parameter(Mandatory = $true)][string]$Target,
  [int]$ProcessId = 0,
  [switch]$NoRun,
  [switch]$KeepRunning,
  [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"

$legacyPortableMarkerName = ".codex-quota-portable"
$noShortcutMarkerName = ".codex-quota-no-shortcut"
$installedVersionFileName = "installed-version.json"

function Get-CodexQuotaProcesses {
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object {
      $_.Id -ne $PID -and (
        $_.ProcessName -like "Codex Quota Monitor*" -or
        ($_.Path -and ([System.IO.Path]::GetFileName($_.Path) -like "Codex Quota Monitor*.exe"))
      )
    }
}

function Stop-CodexQuotaProcesses {
  param([int]$PreferredProcessId = 0)

  $processes = @()
  if ($PreferredProcessId -gt 0) {
    $preferred = Get-Process -Id $PreferredProcessId -ErrorAction SilentlyContinue
    if ($preferred) {
      $processes += $preferred
    }
  }
  $processes += @(Get-CodexQuotaProcesses)
  $processes = @($processes | Where-Object { $_ } | Sort-Object Id -Unique)

  foreach ($process in $processes) {
    try {
      if ($process.CloseMainWindow()) {
        $null = $process.WaitForExit(3000)
      }
      if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction Stop
      }
    } catch {
      Write-Host "Warning: failed to close process $($process.Id): $($_.Exception.Message)"
    }
  }
}

function Copy-WithRetry {
  param(
    [Parameter(Mandatory = $true)][string]$From,
    [Parameter(Mandatory = $true)][string]$To
  )

  $copied = $false
  for ($i = 0; $i -lt 30 -and -not $copied; $i++) {
    try {
      Copy-Item -LiteralPath $From -Destination $To -Force
      $copied = $true
    } catch {
      Start-Sleep -Milliseconds 500
    }
  }

  if (-not $copied) {
    throw "Failed to replace portable executable: $To"
  }
}

function Get-InstalledVersion {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Target
  )

  $fileVersion = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Target).FileVersion
  if ($fileVersion -match '^\d{1,2}\.\d\.\d$') {
    return $fileVersion
  }

  $match = [regex]::Match((Split-Path -Leaf $Source), '\d{1,2}\.\d\.\d')
  if ($match.Success) {
    return $match.Value
  }

  throw "Failed to determine installed version."
}

function Write-InstalledVersionRecord {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$TargetDir
  )

  $recordPath = Join-Path $TargetDir $installedVersionFileName
  $temporaryPath = "$recordPath.tmp"
  $record = [ordered]@{
    schemaVersion = 1
    version = Get-InstalledVersion -Source $Source -Target $Target
    installedAt = [DateTime]::UtcNow.ToString("o")
    sourceFileName = Split-Path -Leaf $Source
  }
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($temporaryPath, (($record | ConvertTo-Json) + [Environment]::NewLine), $encoding)
  if ([System.IO.File]::Exists($recordPath)) {
    [System.IO.File]::Replace($temporaryPath, $recordPath, $null)
  } else {
    [System.IO.File]::Move($temporaryPath, $recordPath)
  }
}

function Remove-VersionedPortablePackages {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$TargetDir
  )

  $packagePattern = '^Codex[ .]Quota[ .]Monitor[ .]\d{1,2}\.\d\.\d[ .]Portable\.exe$'
  $stablePath = [System.IO.Path]::GetFullPath($Target)
  $directories = @(
    (Split-Path -Parent $Source),
    $TargetDir
  ) | Where-Object { $_ } | Select-Object -Unique

  foreach ($directory in $directories) {
    Get-ChildItem -LiteralPath $directory -File -Filter "*.exe" -ErrorAction SilentlyContinue |
      Where-Object {
        $_.Name -match $packagePattern -and
        -not $_.FullName.Equals($stablePath, [System.StringComparison]::OrdinalIgnoreCase)
      } |
      ForEach-Object {
        Remove-Item -LiteralPath $_.FullName -Force
        Write-Host "RemovedPackage: $($_.FullName)"
      }
  }
}

function Start-FallbackTarget {
  param([Parameter(Mandatory = $true)][string]$Path)

  if ($NoRun -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return $false
  }

  try {
    Start-Process -FilePath $Path | Out-Null
    return $true
  } catch {
    Write-Host "Warning: failed to restart existing target: $($_.Exception.Message)"
    return $false
  }
}

if (-not (Test-Path -LiteralPath $Source)) {
  throw "Source file does not exist: $Source"
}

$targetDir = Split-Path -Parent $Target
if (-not (Test-Path -LiteralPath $targetDir)) {
  New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
}

if (-not $KeepRunning) {
  Stop-CodexQuotaProcesses -PreferredProcessId $ProcessId
}

try {
  Copy-WithRetry -From $Source -To $Target

  $noShortcutMarker = Join-Path $targetDir $noShortcutMarkerName
  if ($NoShortcut) {
    Set-Content -LiteralPath $noShortcutMarker -Value "no-shortcut" -Encoding ASCII
  } elseif (Test-Path -LiteralPath $noShortcutMarker) {
    Remove-Item -LiteralPath $noShortcutMarker -Force -ErrorAction SilentlyContinue
  }
} catch {
  Write-Host "Warning: portable update failed: $($_.Exception.Message)"
  $fallbackLaunched = Start-FallbackTarget -Path $Target
  Write-Host "FallbackLaunched: $fallbackLaunched"
  throw
}

$launched = $false
if (-not $NoRun) {
  Start-Process -FilePath $Target -ErrorAction Stop | Out-Null
  $launched = $true
  Write-InstalledVersionRecord -Source $Source -Target $Target -TargetDir $targetDir
  Remove-Item -LiteralPath (Join-Path $targetDir $legacyPortableMarkerName) -Force -ErrorAction SilentlyContinue
  Remove-VersionedPortablePackages -Source $Source -Target $Target -TargetDir $targetDir
}

Write-Host "Source: $Source"
Write-Host "Target: $Target"
Write-Host "Launched: $launched"

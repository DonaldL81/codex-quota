param(
  [Parameter(Mandatory = $true)][string]$Source,
  [Parameter(Mandatory = $true)][string]$Target,
  [int]$ProcessId = 0,
  [switch]$NoRun,
  [switch]$KeepRunning,
  [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"

$markerName = ".codex-quota-portable"
$noShortcutMarkerName = ".codex-quota-no-shortcut"

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

Copy-WithRetry -From $Source -To $Target
Set-Content -LiteralPath (Join-Path $targetDir $markerName) -Value "portable" -Encoding ASCII

$noShortcutMarker = Join-Path $targetDir $noShortcutMarkerName
if ($NoShortcut) {
  Set-Content -LiteralPath $noShortcutMarker -Value "no-shortcut" -Encoding ASCII
} elseif (Test-Path -LiteralPath $noShortcutMarker) {
  Remove-Item -LiteralPath $noShortcutMarker -Force -ErrorAction SilentlyContinue
}

$launched = $false
if (-not $NoRun) {
  Start-Process -FilePath $Target | Out-Null
  $launched = $true
}

Write-Host "Source: $Source"
Write-Host "Target: $Target"
Write-Host "Launched: $launched"

param(
  [ValidateSet("Portable", "Setup")]
  [string]$Kind = "Portable",
  [string]$OutputDir = "",
  [switch]$NoRun,
  [switch]$Force,
  [switch]$KeepRunning,
  [switch]$NoShortcut,
  [switch]$LaunchOnly
)

$ErrorActionPreference = "Stop"

$repo = "DonaldL81/codex-quota"
$branch = "main"
$rawBaseUrl = "https://raw.githubusercontent.com/$repo/$branch"
$readmeUrl = "$rawBaseUrl/README.md"
$userAgent = "codex-quota-skill"
$appDirName = "Codex Quota Monitor"

function Resolve-OutputDir {
  param([string]$Value)

  if ($Value) {
    if (Test-Path -LiteralPath $Value) {
      return (Resolve-Path -LiteralPath $Value).Path
    }
    return [System.IO.Path]::GetFullPath($Value)
  }

  $localPrograms = Join-Path $env:LOCALAPPDATA "Programs"
  if ($localPrograms) {
    return (Join-Path $localPrograms $appDirName)
  }

  $downloads = Join-Path $env:USERPROFILE "Downloads"
  if (Test-Path -LiteralPath $downloads) {
    return (Join-Path $downloads $appDirName)
  }

  return (Join-Path (Get-Location).Path $appDirName)
}

function Get-CurrentVersion {
  $readme = Invoke-WebRequest -Uri $readmeUrl -Headers @{ "User-Agent" = $userAgent } -UseBasicParsing
  $text = $readme.Content
  $match = [regex]::Match($text, '当前版本：`?(\d+\.\d+\.\d+)`?')
  if (-not $match.Success) {
    $match = [regex]::Match($text, 'Current version:\s*`?(\d+\.\d+\.\d+)`?', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
  }
  if (-not $match.Success) {
    $match = [regex]::Match($text, '(\d+\.\d+\.\d+)')
  }
  if (-not $match.Success) {
    throw "Cannot find current version in README.md."
  }
  return $match.Groups[1].Value
}

function New-PackageInfo {
  param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Kind
  )

  $suffix = if ($Kind -eq "Setup") { "Setup.exe" } else { "Portable.exe" }
  $name = "Codex Quota Monitor $Version $suffix"
  $url = "$rawBaseUrl/$([uri]::EscapeDataString($name))"

  [pscustomobject]@{
    Name = $name
    Url = $url
    Source = "Repository root"
    Version = $Version
  }
}

function Get-CodexQuotaProcesses {
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object {
      $_.ProcessName -like "Codex Quota Monitor*" -or
      ($_.Path -and ([System.IO.Path]::GetFileName($_.Path) -like "Codex Quota Monitor*.exe"))
    }
}

function Stop-CodexQuotaProcesses {
  param([string]$TargetPath)

  $closed = 0
  $processes = @(Get-CodexQuotaProcesses)
  foreach ($process in $processes) {
    try {
      if ($process.CloseMainWindow()) {
        $null = $process.WaitForExit(3000)
      }
      if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction Stop
      }
      $closed += 1
    } catch {
      Write-Host "Warning: failed to close running process $($process.Id): $($_.Exception.Message)"
    }
  }

  return $closed
}

function New-DesktopShortcut {
  param(
    [Parameter(Mandatory = $true)][string]$TargetPath,
    [Parameter(Mandatory = $true)][string]$ShortcutName
  )

  $desktop = [Environment]::GetFolderPath("Desktop")
  if (-not $desktop -or -not (Test-Path -LiteralPath $desktop)) {
    return $false
  }

  $shortcutPath = Join-Path $desktop $ShortcutName
  $shell = New-Object -ComObject WScript.Shell
  $shortcut = $shell.CreateShortcut($shortcutPath)
  $shortcut.TargetPath = $TargetPath
  $shortcut.WorkingDirectory = Split-Path -Parent $TargetPath
  $shortcut.IconLocation = $TargetPath
  $shortcut.Save()
  return $true
}

function Test-RunningTarget {
  param([string]$TargetPath)

  $resolvedTarget = [System.IO.Path]::GetFullPath($TargetPath)
  $processes = @(Get-CodexQuotaProcesses | Where-Object {
    $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -eq $resolvedTarget)
  })
  return ($processes.Count -gt 0)
}

$targetDir = Resolve-OutputDir -Value $OutputDir
if (-not (Test-Path -LiteralPath $targetDir)) {
  New-Item -ItemType Directory -Path $targetDir | Out-Null
}

$currentVersion = Get-CurrentVersion
$asset = New-PackageInfo -Version $currentVersion -Kind $Kind
$targetPath = Join-Path $targetDir $asset.Name
$existsBefore = Test-Path -LiteralPath $targetPath
$downloaded = $false
$closedProcesses = 0

if ($LaunchOnly -and (-not $existsBefore)) {
  throw "Target does not exist for launch-only mode: $targetPath"
}

if ((-not $existsBefore) -or $Force) {
  if ($Force -and $existsBefore -and (-not $NoRun) -and (-not $KeepRunning)) {
    $closedProcesses = Stop-CodexQuotaProcesses -TargetPath $targetPath
  }

  Write-Host "Version: $($asset.Version)"
  Write-Host "Source: $($asset.Source)"
  Write-Host "Package: $Kind"
  Write-Host "Downloading: $($asset.Name)"
  Write-Host "Target: $targetPath"

  Invoke-WebRequest -Uri $asset.Url -OutFile $targetPath -Headers @{ "User-Agent" = $userAgent }
  $downloaded = $true
} else {
  Write-Host "Version: $($asset.Version)"
  Write-Host "Package: $Kind"
  Write-Host "Using existing file: $targetPath"
}

$file = Get-Item -LiteralPath $targetPath
if ($file.Length -lt 102400) {
  throw "Downloaded file is unexpectedly small: $($file.Length) bytes."
}

$shortcutCreated = $false
if (($Kind -eq "Portable") -and (-not $NoShortcut)) {
  $shortcutCreated = New-DesktopShortcut -TargetPath $targetPath -ShortcutName "Codex Quota Monitor.lnk"
}

$launched = $false
$runningProcess = $false
if (-not $NoRun) {
  if (-not $KeepRunning) {
    $closedProcesses = Stop-CodexQuotaProcesses -TargetPath $targetPath
  }

  Start-Process -FilePath $targetPath | Out-Null
  $launched = $true
  Start-Sleep -Seconds 1
  $runningProcess = Test-RunningTarget -TargetPath $targetPath
}

Write-Host ""
Write-Host "Downloaded: $downloaded"
Write-Host "ExistingFileUsed: $($existsBefore -and (-not $Force))"
Write-Host "Path: $targetPath"
Write-Host "SizeBytes: $($file.Length)"
Write-Host "ShortcutCreated: $shortcutCreated"
Write-Host "ClosedRunningProcesses: $closedProcesses"
Write-Host "Launched: $launched"
Write-Host "RunningProcess: $runningProcess"
if (($Kind -eq "Setup") -and $launched) {
  Write-Host "Note: Windows may ask for administrator permission."
}

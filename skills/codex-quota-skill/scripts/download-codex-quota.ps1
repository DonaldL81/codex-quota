param(
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
$updaterUrl = "$rawBaseUrl/app/scripts/portable-updater.ps1"
$latestReleaseUrl = "https://api.github.com/repos/$repo/releases/latest"
$userAgent = "codex-quota-skill"
$appDirName = "Codex Quota Monitor"
$stablePortableName = "Codex Quota Monitor.exe"

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

function Get-LatestReleasePackageInfo {
  try {
    $release = Invoke-RestMethod -Uri $latestReleaseUrl -Headers @{
      "User-Agent" = $userAgent
      "Accept" = "application/vnd.github+json"
    }
    $version = ($release.tag_name -replace "^v", "")
    if (-not ($version -match "^\d+\.\d+\.\d+$")) {
      return $null
    }

    $asset = @($release.assets) |
      Where-Object { $_.name -match "Portable\.exe$" -and $_.browser_download_url } |
      Select-Object -First 1
    if (-not $asset) {
      return $null
    }

    [pscustomobject]@{
      Name = $asset.name
      Url = $asset.browser_download_url
      Source = "GitHub Release"
      Version = $version
    }
  } catch {
    return $null
  }
}

function New-PackageInfo {
  param([Parameter(Mandatory = $true)][string]$Version)

  $name = "Codex Quota Monitor $Version Portable.exe"
  $url = "$rawBaseUrl/$([uri]::EscapeDataString($name))"

  [pscustomobject]@{
    Name = $name
    Url = $url
    Source = "Repository root"
    Version = $Version
  }
}

function Get-PortableUpdaterPath {
  $candidates = @(
    (Join-Path $PSScriptRoot "portable-updater.ps1"),
    (Join-Path $PSScriptRoot "..\..\..\app\scripts\portable-updater.ps1")
  )

  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }

  $cacheDir = Join-Path $env:TEMP "codex-quota-skill"
  if (-not (Test-Path -LiteralPath $cacheDir)) {
    New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null
  }
  $updaterPath = Join-Path $cacheDir "portable-updater.ps1"
  Invoke-WebRequest -Uri $updaterUrl -OutFile $updaterPath -Headers @{ "User-Agent" = $userAgent } -UseBasicParsing
  return $updaterPath
}

function Test-RunningTarget {
  param([string]$TargetPath)

  $resolvedTarget = [System.IO.Path]::GetFullPath($TargetPath)
  $processes = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -eq $resolvedTarget)
  })
  return ($processes.Count -gt 0)
}

$targetDir = Resolve-OutputDir -Value $OutputDir
if (-not (Test-Path -LiteralPath $targetDir)) {
  New-Item -ItemType Directory -Path $targetDir | Out-Null
}

$asset = Get-LatestReleasePackageInfo
if (-not $asset) {
  $currentVersion = Get-CurrentVersion
  $asset = New-PackageInfo -Version $currentVersion
}
$packagePath = Join-Path $targetDir $asset.Name
$stablePortablePath = Join-Path $targetDir $stablePortableName
$existsBefore = Test-Path -LiteralPath $packagePath
$downloaded = $false
$launched = $false
$runningProcess = $false

if ($LaunchOnly) {
  if (-not (Test-Path -LiteralPath $stablePortablePath)) {
    throw "Target does not exist for launch-only mode: $stablePortablePath"
  }
  Start-Process -FilePath $stablePortablePath | Out-Null
  $launched = $true
  Start-Sleep -Seconds 1
  $runningProcess = Test-RunningTarget -TargetPath $stablePortablePath
} else {
  if ((-not $existsBefore) -or $Force) {
    Write-Host "Version: $($asset.Version)"
    Write-Host "Source: $($asset.Source)"
    Write-Host "Package: Portable"
    Write-Host "Downloading: $($asset.Name)"
    Write-Host "Target: $packagePath"

    Invoke-WebRequest -Uri $asset.Url -OutFile $packagePath -Headers @{ "User-Agent" = $userAgent } -UseBasicParsing
    $downloaded = $true
  } else {
    Write-Host "Version: $($asset.Version)"
    Write-Host "Package: Portable"
    Write-Host "Using existing file: $packagePath"
  }

  $packageFile = Get-Item -LiteralPath $packagePath
  if ($packageFile.Length -lt 102400) {
    throw "Downloaded file is unexpectedly small: $($packageFile.Length) bytes."
  }

  if (-not $NoRun) {
    $updaterPath = Get-PortableUpdaterPath
    $updaterArgs = @(
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      $updaterPath,
      "-Source",
      $packagePath,
      "-Target",
      $stablePortablePath
    )
    if ($KeepRunning) {
      $updaterArgs += "-KeepRunning"
    }
    if ($NoShortcut) {
      $updaterArgs += "-NoShortcut"
    }
    & powershell @updaterArgs
    if ($LASTEXITCODE -ne 0) {
      throw "Portable updater failed."
    }
    $launched = $true
    Start-Sleep -Seconds 1
    $runningProcess = Test-RunningTarget -TargetPath $stablePortablePath
  }
}

$reportedPath = if ($NoRun) { $packagePath } else { $stablePortablePath }
$reportedFile = Get-Item -LiteralPath $reportedPath

Write-Host ""
Write-Host "Downloaded: $downloaded"
Write-Host "ExistingFileUsed: $($existsBefore -and (-not $Force))"
Write-Host "PackagePath: $packagePath"
Write-Host "Path: $reportedPath"
Write-Host "SizeBytes: $($reportedFile.Length)"
Write-Host "NoShortcutRequested: $NoShortcut"
Write-Host "Launched: $launched"
Write-Host "RunningProcess: $runningProcess"

param(
  [ValidateSet("Portable", "Setup")]
  [string]$Kind = "Portable",
  [string]$OutputDir = "",
  [switch]$NoRun,
  [switch]$Force
)

$ErrorActionPreference = "Stop"

$repo = "DonaldL81/codex-quota"
$branch = "main"
$rawBaseUrl = "https://raw.githubusercontent.com/$repo/$branch"
$readmeUrl = "$rawBaseUrl/README.md"
$userAgent = "codex-quota-skill"

function Resolve-OutputDir {
  param([string]$Value)

  if ($Value) {
    if (Test-Path -LiteralPath $Value) {
      return (Resolve-Path -LiteralPath $Value).Path
    }
    return [System.IO.Path]::GetFullPath($Value)
  }

  $desktop = [Environment]::GetFolderPath("Desktop")
  if ($desktop -and (Test-Path -LiteralPath $desktop)) {
    return $desktop
  }

  $downloads = Join-Path $env:USERPROFILE "Downloads"
  if (Test-Path -LiteralPath $downloads) {
    return $downloads
  }

  return (Get-Location).Path
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

$targetDir = Resolve-OutputDir -Value $OutputDir
if (-not (Test-Path -LiteralPath $targetDir)) {
  New-Item -ItemType Directory -Path $targetDir | Out-Null
}

$currentVersion = Get-CurrentVersion
$asset = New-PackageInfo -Version $currentVersion -Kind $Kind
$targetPath = Join-Path $targetDir $asset.Name

if ((Test-Path -LiteralPath $targetPath) -and (-not $Force)) {
  throw "Target already exists: $targetPath. Re-run with -Force to overwrite it."
}

Write-Host "Version: $($asset.Version)"
Write-Host "Source: $($asset.Source)"
Write-Host "Package: $Kind"
Write-Host "Downloading: $($asset.Name)"
Write-Host "Target: $targetPath"

Invoke-WebRequest -Uri $asset.Url -OutFile $targetPath -Headers @{ "User-Agent" = $userAgent }

$file = Get-Item -LiteralPath $targetPath
if ($file.Length -lt 102400) {
  throw "Downloaded file is unexpectedly small: $($file.Length) bytes."
}

$launched = $false
if (-not $NoRun) {
  Start-Process -FilePath $targetPath | Out-Null
  $launched = $true
}

Write-Host ""
Write-Host "Downloaded: $targetPath"
Write-Host "SizeBytes: $($file.Length)"
Write-Host "Launched: $launched"
if (($Kind -eq "Setup") -and $launched) {
  Write-Host "Note: Windows may ask for administrator permission."
}

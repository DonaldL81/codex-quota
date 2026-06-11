$ErrorActionPreference = "Stop"

function Write-Utf8BomFile {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Text
  )
  $encoding = New-Object System.Text.UTF8Encoding($true)
  [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $Path), $Text, $encoding)
}

function Read-JsonFile {
  param([Parameter(Mandatory = $true)][string]$Path)
  Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
}

function Write-JsonFile {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)]$Value
  )
  $json = $Value | ConvertTo-Json -Depth 80
  Write-Utf8BomFile -Path $Path -Text ($json + [Environment]::NewLine)
}

function Get-CargoTomlVersion {
  param([Parameter(Mandatory = $true)][string]$Path)
  $text = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
  $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
  if (-not $match.Success) {
    throw "无法从 Cargo.toml 读取版本号。"
  }
  $match.Groups[1].Value
}

function Get-NextPatchVersion {
  param([Parameter(Mandatory = $true)][string]$Version)
  $parts = $Version.Split(".")
  if ($parts.Count -ne 3) {
    throw "版本号不是 major.minor.patch 格式：$Version"
  }
  $major = [int]$parts[0]
  $minor = [int]$parts[1]
  $patch = [int]$parts[2] + 1
  "$major.$minor.$patch"
}

function Assert-VersionSync {
  param([Parameter(Mandatory = $true)][string]$ProjectRoot)

  $package = Read-JsonFile -Path (Join-Path $ProjectRoot "package.json")
  $tauri = Read-JsonFile -Path (Join-Path $ProjectRoot "src-tauri\tauri.conf.json")
  $cargoVersion = Get-CargoTomlVersion -Path (Join-Path $ProjectRoot "src-tauri\Cargo.toml")
  $versions = @(@($package.version, $tauri.version, $cargoVersion) | Select-Object -Unique)
  if ($versions.Count -ne 1) {
    throw "版本号不同步：package.json=$($package.version), tauri.conf.json=$($tauri.version), Cargo.toml=$cargoVersion"
  }

  $package.version
}

function Set-ProjectVersion {
  param(
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [Parameter(Mandatory = $true)][string]$OldVersion,
    [Parameter(Mandatory = $true)][string]$NewVersion
  )

  $packagePath = Join-Path $ProjectRoot "package.json"
  $package = Read-JsonFile -Path $packagePath
  $package.version = $NewVersion
  Write-JsonFile -Path $packagePath -Value $package

  $lockPath = Join-Path $ProjectRoot "package-lock.json"
  if (Test-Path -LiteralPath $lockPath) {
    $lockText = Get-Content -LiteralPath $lockPath -Raw -Encoding UTF8
    $rootVersionRegex = [regex]::new('(^\s*"version"\s*:\s*")[^"]+(")', [System.Text.RegularExpressions.RegexOptions]::Multiline)
    $packageRootRegex = [regex]::new('("":\s*\{[\s\S]*?"version"\s*:\s*")[^"]+(")')
    $lockText = $rootVersionRegex.Replace($lockText, "`${1}$NewVersion`${2}", 1)
    $lockText = $packageRootRegex.Replace($lockText, "`${1}$NewVersion`${2}", 1)
    Write-Utf8BomFile -Path $lockPath -Text $lockText
  }

  $tauriPath = Join-Path $ProjectRoot "src-tauri\tauri.conf.json"
  $tauri = Read-JsonFile -Path $tauriPath
  $tauri.version = $NewVersion
  Write-JsonFile -Path $tauriPath -Value $tauri

  $cargoPath = Join-Path $ProjectRoot "src-tauri\Cargo.toml"
  $cargoText = Get-Content -LiteralPath $cargoPath -Raw -Encoding UTF8
  $cargoText = [regex]::Replace($cargoText, '(?m)^version\s*=\s*"[^"]+"', "version = `"$NewVersion`"", 1)
  Write-Utf8BomFile -Path $cargoPath -Text $cargoText

  $quotaPath = Join-Path $ProjectRoot "src-tauri\src\quota.rs"
  $quotaText = Get-Content -LiteralPath $quotaPath -Raw -Encoding UTF8
  $quotaText = [regex]::Replace($quotaText, 'const CLIENT_VERSION: &str = "[^"]+";', "const CLIENT_VERSION: &str = `"$NewVersion`";", 1)
  Write-Utf8BomFile -Path $quotaPath -Text $quotaText

  foreach ($docName in @("README.md", "发布说明.md")) {
    $docPath = Join-Path $ProjectRoot $docName
    if (Test-Path -LiteralPath $docPath) {
      $docText = Get-Content -LiteralPath $docPath -Raw -Encoding UTF8
      $docText = $docText.Replace($OldVersion, $NewVersion)
      Write-Utf8BomFile -Path $docPath -Text $docText
    }
  }
}

function Invoke-ReleaseVersionPrompt {
  param(
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [switch]$NoPrompt,
    [switch]$BumpPatch
  )

  $currentVersion = Assert-VersionSync -ProjectRoot $ProjectRoot
  Write-Host "当前版本号：$currentVersion"

  $shouldBump = $false
  if ($BumpPatch) {
    $shouldBump = $true
  } elseif (-not $NoPrompt) {
    $inputValue = Read-Host "输入 1：版本号升级为 patch +1；直接回车或输入其他字符：保持当前版本号"
    $shouldBump = $inputValue -eq "1"
  }

  if ($shouldBump) {
    $nextVersion = Get-NextPatchVersion -Version $currentVersion
    Set-ProjectVersion -ProjectRoot $ProjectRoot -OldVersion $currentVersion -NewVersion $nextVersion
    Write-Host "版本号已更新：$currentVersion -> $nextVersion"
    return $nextVersion
  }

  Write-Host "版本号保持：$currentVersion"
  return $currentVersion
}

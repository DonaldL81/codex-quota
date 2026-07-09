$ErrorActionPreference = "Stop"

function Write-Utf8File {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Text
  )

  $encoding = New-Object System.Text.UTF8Encoding($false)
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
  Write-Utf8File -Path $Path -Text ($json + [Environment]::NewLine)
}

function Get-CargoTomlVersion {
  param([Parameter(Mandatory = $true)][string]$Path)

  $text = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
  $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
  if (-not $match.Success) {
    throw "Cannot read version from Cargo.toml."
  }

  $match.Groups[1].Value
}

function Assert-ProjectVersionFormat {
  param([Parameter(Mandatory = $true)][string]$Version)

  if (-not ($Version -match '^\d{1,2}\.\d\.\d$')) {
    throw "Version must be N.n.n or NN.n.n, and the last two segments must be single digits: $Version"
  }
}

function Get-NextPatchVersion {
  param([Parameter(Mandatory = $true)][string]$Version)

  Assert-ProjectVersionFormat -Version $Version
  $parts = $Version.Split(".")
  $major = [int]$parts[0]
  $minor = [int]$parts[1]
  $patch = [int]$parts[2]
  if ($patch -lt 9) {
    $patch += 1
  } else {
    $patch = 0
    if ($minor -lt 9) {
      $minor += 1
    } else {
      $minor = 0
      $major += 1
    }
  }
  if ($major -gt 99) {
    throw "Version major segment cannot exceed two digits: $major"
  }
  "$major.$minor.$patch"
}

function Assert-VersionSync {
  param([Parameter(Mandatory = $true)][string]$ProjectRoot)

  $package = Read-JsonFile -Path (Join-Path $ProjectRoot "package.json")
  $tauri = Read-JsonFile -Path (Join-Path $ProjectRoot "src-tauri\tauri.conf.json")
  $cargoVersion = Get-CargoTomlVersion -Path (Join-Path $ProjectRoot "src-tauri\Cargo.toml")
  $versions = @(@($package.version, $tauri.version, $cargoVersion) | Select-Object -Unique)
  if ($versions.Count -ne 1) {
    throw "Version mismatch: package.json=$($package.version), tauri.conf.json=$($tauri.version), Cargo.toml=$cargoVersion"
  }

  Assert-ProjectVersionFormat -Version $package.version
  $package.version
}

function Set-ProjectVersion {
  param(
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [Parameter(Mandatory = $true)][string]$OldVersion,
    [Parameter(Mandatory = $true)][string]$NewVersion
  )

  Assert-ProjectVersionFormat -Version $OldVersion
  Assert-ProjectVersionFormat -Version $NewVersion

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
    Write-Utf8File -Path $lockPath -Text $lockText
  }

  $tauriPath = Join-Path $ProjectRoot "src-tauri\tauri.conf.json"
  $tauri = Read-JsonFile -Path $tauriPath
  $tauri.version = $NewVersion
  Write-JsonFile -Path $tauriPath -Value $tauri

  $cargoPath = Join-Path $ProjectRoot "src-tauri\Cargo.toml"
  $cargoText = Get-Content -LiteralPath $cargoPath -Raw -Encoding UTF8
  $cargoText = [regex]::Replace($cargoText, '(?m)^version\s*=\s*"[^"]+"', "version = `"$NewVersion`"", 1)
  Write-Utf8File -Path $cargoPath -Text $cargoText

  $quotaPath = Join-Path $ProjectRoot "src-tauri\src\quota.rs"
  $quotaText = Get-Content -LiteralPath $quotaPath -Raw -Encoding UTF8
  $quotaText = [regex]::Replace($quotaText, 'const CLIENT_VERSION: &str = "[^"]+";', "const CLIENT_VERSION: &str = `"$NewVersion`";", 1)
  Write-Utf8File -Path $quotaPath -Text $quotaText

  foreach ($docName in @("README.md", "发布说明.md")) {
    $docPath = Join-Path $ProjectRoot $docName
    if (Test-Path -LiteralPath $docPath) {
      $docText = Get-Content -LiteralPath $docPath -Raw -Encoding UTF8
      $docText = $docText.Replace($OldVersion, $NewVersion)
      Write-Utf8File -Path $docPath -Text $docText
    }
  }
}

function Read-VersionPromptWithTimeout {
  param(
    [Parameter(Mandatory = $true)][string]$Prompt,
    [int]$TimeoutSeconds = 10
  )

  Write-Host $Prompt
  Write-Host "请在 $TimeoutSeconds 秒内输入并回车；超时默认保持当前版本。"

  $buffer = ""
  $deadline = [DateTime]::Now.AddSeconds($TimeoutSeconds)
  while ([DateTime]::Now -lt $deadline) {
    $remaining = [Math]::Max(0, [int][Math]::Ceiling(($deadline - [DateTime]::Now).TotalSeconds))
    Write-Host -NoNewline ("`r剩余 {0}s > {1}" -f $remaining, $buffer)

    try {
      while ([Console]::KeyAvailable) {
        $key = [Console]::ReadKey($true)
        if ($key.Key -eq [ConsoleKey]::Enter) {
          Write-Host ""
          return $buffer
        }
        if ($key.Key -eq [ConsoleKey]::Backspace) {
          if ($buffer.Length -gt 0) {
            $buffer = $buffer.Substring(0, $buffer.Length - 1)
          }
          continue
        }
        if (-not [char]::IsControl($key.KeyChar)) {
          $buffer += $key.KeyChar
        }
      }
    } catch {
      Write-Host ""
      Write-Host "当前终端不支持倒计时输入，默认保持当前版本。"
      return ""
    }

    Start-Sleep -Milliseconds 100
  }

  Write-Host ""
  Write-Host "等待超时，保持当前版本。"
  return ""
}

function Invoke-ReleaseVersionPrompt {
  param(
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [switch]$NoPrompt,
    [switch]$BumpPatch
  )

  $currentVersion = Assert-VersionSync -ProjectRoot $ProjectRoot
  Write-Host "当前版本：$currentVersion"

  $shouldBump = $false
  if ($BumpPatch) {
    $shouldBump = $true
  } elseif (-not $NoPrompt) {
    $inputValue = Read-VersionPromptWithTimeout -Prompt "输入 1 自动升级到下一个版本；直接回车或输入其他内容保持当前版本" -TimeoutSeconds 10
    $shouldBump = $inputValue -eq "1"
  }

  if ($shouldBump) {
    $nextVersion = Get-NextPatchVersion -Version $currentVersion
    Set-ProjectVersion -ProjectRoot $ProjectRoot -OldVersion $currentVersion -NewVersion $nextVersion
    Write-Host "版本已更新：$currentVersion -> $nextVersion"
    return $nextVersion
  }

  Write-Host "版本保持不变：$currentVersion"
  return $currentVersion
}

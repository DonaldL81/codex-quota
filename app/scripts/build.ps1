$ErrorActionPreference = "Stop"

Write-Host "安装版已停止维护，正在改为打包单文件版..."
& (Join-Path $PSScriptRoot "build-all.ps1")

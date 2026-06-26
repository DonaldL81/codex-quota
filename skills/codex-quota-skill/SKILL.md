---
name: codex-quota-skill
description: 下载、安装或运行最新版 Codex Quota Monitor / Codex 额度监控 Windows 工具。Use when the user asks to download Codex Quota, install Codex quota monitor, get the portable version, get the setup version, download and run it, or install/update this Windows quota monitor on a new computer.
---

## Encoding

This skill uses UTF-8. Read and edit `SKILL.md`, scripts, and documentation as UTF-8. In Windows PowerShell, use `-Encoding UTF8` when reading text files.

Keep PowerShell script output in English/ASCII where possible to avoid Windows PowerShell 5 encoding issues.

# Codex Quota Skill

## 默认行为

使用本 skill 自带的 PowerShell 脚本，从 `DonaldL81/codex-quota` 下载最新版 Codex Quota Monitor。

- 用户没有明确要求安装版时，默认下载便携版 `Portable.exe`。
- 默认保存到用户桌面。
- 默认下载完成后自动打开。
- 只有用户要求“只下载”“不要打开”时，才使用 `-NoRun`。
- 只有用户明确要求“安装版”“安装包”“Setup”“下载安装”时，才使用 `-Kind Setup`。
- 用户要求覆盖、更新已有文件或重新下载时，使用 `-Force`。

脚本会读取公开 raw README 中的当前版本号，再下载仓库根目录中对应版本的 EXE 文件。这样可以避免 GitHub API 限流。

## 脚本用法

在 skill 目录中运行，或传入脚本完整路径：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\download-codex-quota.ps1
```

常用命令：

```powershell
# 默认：下载最新版便携版到桌面，并自动运行。
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\download-codex-quota.ps1

# 只下载便携版，不运行。
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\download-codex-quota.ps1 -NoRun

# 下载安装包，并启动安装程序。
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\download-codex-quota.ps1 -Kind Setup

# 保存到指定目录，并覆盖已有文件。
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\download-codex-quota.ps1 -OutputDir "D:\Downloads" -Force
```

## 回复用户

脚本运行后，向用户说明：

- 下载的版本号
- 下载的是便携版还是安装版
- 本地保存路径
- 是否已经启动

如果启动的是安装包，提醒用户 Windows 可能会弹出管理员权限确认。

如果 GitHub 无法访问、README 未解析到版本号、目标文件不存在或下载失败，简要说明错误，并建议稍后重试。

# Codex 额度监控

一个轻量的 Windows 托盘小工具，用来查看已登录 Codex 账号的剩余额度。

**Codex提示词：**
```text
安装skill下载软件
https://github.com/DonaldL81/codex-quota
```
## 界面预览

进度大窗：

<img src="assets/promo-large-window.png" alt="进度大窗宣传图" width="700">

环形大窗：

<img src="assets/promo-ring-window.png" alt="环形大窗宣传图" width="700">

小窗模式：

<img src="assets/promo-small-window.png" alt="小窗模式宣传图" width="700">

## 主要特点

- 便携版为约 8 MB 的单文件程序，适合长期放在托盘或桌面角落。
- 支持小窗、进度大窗和环形大窗三种显示方式。
- 九套配色在三种样式中统一使用渐变进度、轨道与发光层次。
- 无须打开 Codex 桌面窗口，也能读取本机已登录账号的额度。
- 支持自动检查更新；发现新版本时，托盘图标会显示提醒。

## 下载和运行

仓库根目录和对应 Release 提供单文件免安装包：

```text
Codex Quota Monitor 2.6.9 Portable.exe
```

推荐下载最新的 `Portable.exe` 后直接双击运行。首次运行会自动固定到当前用户程序目录并维护桌面快捷方式；稳定入口路径为 `%LOCALAPPDATA%\Programs\Codex Quota Monitor\Codex Quota Monitor.exe`。稳定入口启动成功后，会清理旧的带版本 Portable 包。

GitHub Release 会将文件名中的空格规范化为点号，下载附件时可能显示为 `Codex.Quota.Monitor.2.6.9.Portable.exe`。

使用前需要：

- Windows 10 或 Windows 11
- 已安装并登录 Codex
- WebView2 Runtime，多数 Windows 10/11 已自带

## 常见问题

### 关闭 Codex 后还能刷新吗？

可以。只要 Codex 已安装、账号登录状态有效且网络可用，即使没有手动打开 Codex 窗口，也可以读取额度。

### 打开后没有额度怎么办？

先确认 Codex 已安装并登录。工具会自动查找常见的 Codex 安装路径：

```text
%LOCALAPPDATA%\OpenAI\Codex\bin\codex.exe
%LOCALAPPDATA%\OpenAI\Codex\bin\<版本或哈希目录>\codex.exe
```

如果 Codex 安装在其他位置，可以设置环境变量：

```text
CODEX_QUOTA_CODEX_PATH
```

值填写 `codex.exe` 的完整路径。网络未连接、账号未登录或登录状态过期时，窗口会显示失败原因；短暂刷新失败但仍有上次额度时，会继续显示上次结果。处理后可以点击窗口刷新图标，或使用右键菜单中的“重启”。

### 窗口打不开或一闪而过怎么办？

优先检查系统是否安装 WebView2 Runtime。便携版不内置 WebView2。

### 开机自启动没有生效怎么办？

便携版记录的是当前 EXE 路径。移动过 EXE 文件后，请在右键菜单中关闭开机自启动，再重新开启。

## 近期更新

### 2.6.9

- 优化暗色主题下进度条未填充轨道，右侧槽位显示为实体暗橙底色。
- 放慢首次启动从 100% 降到当前额度的动画节奏，变化过程更容易看清。
- 保持后续额度变化的短动效，避免刷新时拖慢界面反馈。

历史版本请查看 [GitHub Releases](https://github.com/DonaldL81/codex-quota/releases)。

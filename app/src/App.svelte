<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { emit, listen, type Event as TauriEvent } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

  type QuotaSnapshot = {
    status: string;
    limitName: string;
    planType: string;
    updatedAt: string;
    quotaLabel: string;
    quotaRemaining: number;
    quotaReset: string;
    resetCreditsAvailable?: number | null;
  };

  type WindowMode = "small" | "large" | "ring";

  type WindowState = {
    mode: WindowMode;
    alwaysOnTop: boolean;
    visible: boolean;
  };

  type UpdateInfo = {
    available: boolean;
    currentVersion: string;
    latestVersion: string;
    releaseUrl: string;
    portableAssetUrl?: string | null;
    portableFileName?: string | null;
    portableAssetSize?: number | null;
    message: string;
  };

  type UpdateProgress = {
    phase: "downloading" | "finished" | "installing" | string;
    percent: number;
    downloaded: number;
    total?: number | null;
    message: string;
  };

  type Status = "loading" | "ready" | "stale" | "error";
  type ColorScheme = "red" | "orange" | "yellow" | "green" | "cyan" | "blue" | "purple" | "black" | "white";
  type ActiveThemeState = {
    colorScheme: ColorScheme;
    manualOverride: boolean;
  };
  type PanelOpacity = 100 | 90 | 80 | 70 | 60 | 50 | 40 | 30 | 20 | 10 | 0;
  type RingSegment = {
    index: number;
    length: number;
    offset: number;
    color: string;
  };
  type CurrentWindow = ReturnType<typeof getCurrentWindow>;
  type RawQuota = Partial<QuotaSnapshot> & Record<string, unknown>;
  type RefreshOptions = {
    retryOnFailure?: boolean;
  };

  const defaultAutoRefreshSeconds = 30;
  const autoRefreshPresets = [30, 60, 300, 600, 1200, 1800, 3600];
  const colorSchemes: ColorScheme[] = ["red", "orange", "yellow", "green", "cyan", "blue", "purple", "black", "white"];
  const opacityPresets: PanelOpacity[] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0];
  const visibleCacheSyncMs = 5_000;
  const invokeTimeoutMs = 35_000;
  const refreshRetryDelayMs = 1_500;
  const largeBaseWidth = 200;
  const largeBaseHeight = 50;
  const dragThresholdPx = 4;
  const ringSegmentCount = 120;
  const ringGradientMidpoint = 58;
  const quotaAnimationMinMs = 260;
  const quotaAnimationMaxMs = 520;
  const quotaAnimationPerPercentMs = 3;
  const quotaStartupAnimationMinMs = 760;
  const quotaStartupAnimationMaxMs = 1_180;
  const quotaStartupAnimationPerPercentMs = 6;
  const quotaCacheKey = "codex-quota-v2:last-quota";
  const autoRefreshCacheKey = "codex-quota-v2:auto-refresh-seconds";
  const colorSchemeCacheKey = "codex-quota-v2:color-scheme";
  const activeThemeEvent = "active-theme-changed";
  const darkModeCacheKey = "codex-quota-v2:dark-mode";
  const opacityCacheKey = "codex-quota-v2:opacity";
  const previewParams =
    typeof window === "undefined" ? new URLSearchParams() : new URLSearchParams(window.location.search);
  const previewEnabled = previewParams.has("mock");
  const previewInitialMode: WindowMode = normalizeMode(previewParams.get("mode"));

  let mode: WindowMode = previewInitialMode;
  let alwaysOnTop = true;
  let panelVisible = true;
  let autostart = false;
  let quota: QuotaSnapshot | null = previewEnabled ? sampleQuota() : readCachedQuota();
  let lastGoodQuota: QuotaSnapshot | null = quota;
  let displayQuota: QuotaSnapshot | null = quota;
  let lastRefreshText = quota?.updatedAt || "--:--:--";
  let blockedCacheRefreshText = "";
  let autoRefreshSeconds = readAutoRefreshSeconds();
  let preferredColorScheme = readColorScheme();
  let colorScheme = preferredColorScheme;
  let manualThemeOverride = false;
  let darkMode = readDarkMode();
  let panelOpacity = readPanelOpacity();
  let status: Status = previewEnabled ? "ready" : quota ? "stale" : "loading";
  let isRefreshing = false;
  let errorText = "";
  let updateInfo: UpdateInfo | null = null;
  let updateChecking = false;
  let updateDownloading = false;
  let updateProgress: UpdateProgress | null = null;
  let updateErrorText = "";
  let updatePanelOpen = false;
  let toast = "";
  let toastTimer: number | undefined;
  let interfaceReady = previewEnabled;
  let uiScale = 1;
  let widthScale = 1;
  let heightScale = 1;
  let largeHeaderVisible = false;
  let compactActionsVisible = false;
  let visualQuotaRemaining = hasUsableQuota(displayQuota) ? 100 : 0;
  let quotaVisualReady = false;
  let quotaAnimationFrame: number | undefined;
  let chromeRevealReady = false;
  let chromeRevealBlockedUntil = 0;
  let chromeRevealAnchor: { x: number; y: number } | null = null;
  let refreshTimer: number | undefined;
  let cacheSyncTimer: number | undefined;
  let appWindow: CurrentWindow | null = null;
  let windowLabel = "main";
  let pendingPointer:
    | {
        id: number;
        x: number;
        y: number;
      }
    | null = null;
  $: isSmall = mode === "small";
  $: isRing = mode === "ring";
  $: hasQuota = hasUsableQuota(displayQuota);
  $: subtitleText = makeSubtitle(displayQuota, lastRefreshText);
  $: statusMessage = makeStatusText(status, errorText, displayQuota, lastRefreshText);
  $: updateVisible = updatePanelOpen && (updateChecking || updateDownloading || !!updateErrorText || !!updateInfo?.available);
  $: updatePercent = clamp(Math.round(updateProgress?.percent ?? 0), 0, 100);
  $: updateNoticeText = makeUpdateNoticeText();
  $: bottomNoticeText = makeBottomNoticeText(status, hasQuota, updateNoticeText, errorText);
  $: visualQuotaWidth = formatPercent(visualQuotaRemaining);
  $: ringSegments = makeRingSegments(hasQuota ? visualQuotaRemaining : 0);
  $: ringReset = splitResetText(displayQuota?.quotaReset);
  $: isControllerWindow = windowLabel === "main";
  $: scaleStyle = `--ui-scale:${uiScale.toFixed(3)};--width-scale:${widthScale.toFixed(3)};--height-scale:${heightScale.toFixed(3)};--panel-opacity:${(panelOpacity / 100).toFixed(2)};`;
  $: shellClass = `shell ${mode}-mode color-${colorScheme} ${darkMode ? "dark-mode" : ""} ${interfaceReady ? "" : "interface-pending"} ${isSmall && !compactActionsVisible ? "small-actions-hidden" : ""} ${!isSmall && !largeHeaderVisible ? "topbar-hidden" : ""} ${panelOpacity < 100 ? "composite-refresh" : ""}`;

  onMount(() => {
    try {
      appWindow = getCurrentWindow();
      windowLabel = appWindow.label;
    } catch {
      appWindow = null;
    }

    updateScale();
    window.addEventListener("resize", updateScale);

    let cleanup = () => {};
    void init().then((nextCleanup) => {
      cleanup = nextCleanup;
      updateScale();
    });

    return () => {
      window.removeEventListener("resize", updateScale);
      cancelQuotaAnimation();
      cleanup();
    };
  });

  async function init() {
    if (previewEnabled) {
      updateScale();
      syncQuotaVisual(displayQuota);
      return () => {};
    }

    await hydrateQuotaCache();
    await hydrateWindowState();
    syncQuotaVisual(displayQuota);
    if (isControllerWindow) {
      void refreshQuota();
      void refreshAutostartState();
      void syncAutoRefreshMenu();
      void syncAppearanceMenu();
      void checkForUpdates(false);
    }
    scheduleRefreshTimer();
    scheduleCacheSyncTimer();

    const unlistenRefresh = await listenSafe("quota-refresh-requested", () => {
      if (isControllerWindow) void refreshQuota();
    });
    const unlistenUpdateCheck = await listenSafe("update-check-requested", () => {
      if (isControllerWindow) void checkForUpdates(true);
    });
    const unlistenUpdateDownload = await listenSafe("update-download-requested", () => {
      if (isControllerWindow) void startUpdateDownload();
    });
    const unlistenUpdateProgress = await listenSafe<UpdateProgress>("update-progress", (event) => {
      updateProgress = event.payload;
      updateDownloading = event.payload.phase === "downloading" || event.payload.phase === "installing";
      if (event.payload.phase === "finished") {
        updateDownloading = false;
      }
    });
    const unlistenAutoRefresh = await listenSafe<number>("auto-refresh-seconds-changed", (event) => {
      setAutoRefreshSeconds(event.payload);
    });
    const unlistenColorScheme = await listenSafe<ColorScheme>("color-scheme-changed", (event) => {
      void setColorScheme(event.payload, false);
    });
    const unlistenActiveTheme = await listenSafe<ActiveThemeState>(activeThemeEvent, (event) => {
      if (!isActiveThemeState(event.payload)) return;
      colorScheme = event.payload.colorScheme;
      manualThemeOverride = event.payload.manualOverride;
    });
    const unlistenDarkMode = await listenSafe<boolean>("dark-mode-changed", (event) => {
      setDarkMode(event.payload, false);
    });
    const unlistenOpacity = await listenSafe<PanelOpacity>("opacity-changed", (event) => {
      setPanelOpacity(event.payload, false);
    });
    const unlistenMode = await listenSafe<string>("mode-changed", (event) => {
      mode = normalizeMode(event.payload);
      resetChromeVisibility();
      updateScale();
      void hydrateQuotaCache().finally(() => {
        if (!hasUsableQuota(displayQuota) || status !== "ready") {
          void refreshQuota();
        }
      });
    });
    const unlistenTopmost = await listenSafe<boolean>("topmost-changed", (event) => {
      alwaysOnTop = event.payload;
    });
    const unlistenVisibility = await listenSafe<boolean>("panel-visibility-changed", (event) => {
      const wasVisible = panelVisible;
      panelVisible = event.payload;
      scheduleRefreshTimer();
      scheduleCacheSyncTimer();
      if (panelVisible && !wasVisible) {
        void syncDisplayFromBackendCache();
        void refreshQuota();
      }
    });
    const unlistenAutostart = await listenSafe("toggle-autostart-requested", () => {
      if (isControllerWindow) void toggleAutostart();
    });

    return () => {
      window.clearInterval(refreshTimer);
      window.clearInterval(cacheSyncTimer);
      unlistenRefresh();
      unlistenUpdateCheck();
      unlistenUpdateDownload();
      unlistenUpdateProgress();
      unlistenAutoRefresh();
      unlistenColorScheme();
      unlistenActiveTheme();
      unlistenDarkMode();
      unlistenOpacity();
      unlistenMode();
      unlistenTopmost();
      unlistenVisibility();
      unlistenAutostart();
    };
  }

  async function hydrateWindowState() {
    try {
      const state = await withTimeout(
        invoke<WindowState>("get_window_state"),
        2_000,
        "窗口状态读取超时"
      );
      const persistedMode = normalizeMode(state.mode);
      mode = windowLabel === "ring" ? "ring" : persistedMode === "ring" ? "small" : persistedMode;
      alwaysOnTop = state.alwaysOnTop;
      panelVisible = state.visible;
      resetChromeVisibility();
      updateScale();
    } catch {
      updateScale();
    } finally {
      interfaceReady = true;
    }
  }

  async function listenSafe<T = unknown>(
    event: string,
    handler: (event: TauriEvent<T>) => void | Promise<void>
  ) {
    try {
      return await listen<T>(event, handler);
    } catch {
      return () => {};
    }
  }

  function scheduleRefreshTimer() {
    window.clearInterval(refreshTimer);
    if (!isControllerWindow) return;
    refreshTimer = window.setInterval(refreshQuota, autoRefreshSeconds * 1000);
  }

  function scheduleCacheSyncTimer() {
    window.clearInterval(cacheSyncTimer);
    if (!panelVisible) return;
    cacheSyncTimer = window.setInterval(syncDisplayFromBackendCache, visibleCacheSyncMs);
  }

  async function refreshQuota(options: RefreshOptions = {}) {
    if (!previewEnabled && !isControllerWindow) return;
    if (isRefreshing) return;
    isRefreshing = true;
    if (!hasUsableQuota(displayQuota)) {
      status = "loading";
    }
    const maxAttempts = options.retryOnFailure === false ? 1 : 2;
    let lastError: unknown = null;
    try {
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        try {
          const normalized = await readLiveQuota();
          rememberQuota(normalized, "ready");
          errorText = "";
          await updateTrayQuota("ready", normalized);
          return;
        } catch (error) {
          lastError = error;
          if (attempt < maxAttempts) {
            await sleep(refreshRetryDelayMs);
          }
        }
      }

      errorText = friendlyError(lastError);
      blockedCacheRefreshText = lastRefreshText;
      const fallbackQuota = hasUsableQuota(displayQuota) ? displayQuota : null;
      status = fallbackQuota ? "stale" : "error";
      await updateTrayQuota(fallbackQuota ? "stale" : "error", fallbackQuota);
    } finally {
      isRefreshing = false;
    }
  }

  async function requestQuotaRefresh() {
    if (isControllerWindow) {
      await refreshQuota();
      return;
    }
    await emit("quota-refresh-requested");
  }

  async function readLiveQuota() {
    const nextQuota = await withTimeout(
      invoke<QuotaSnapshot>("read_quota"),
      invokeTimeoutMs,
      "Codex 响应超时，请稍后刷新"
    );
    return {
      ...normalizeQuota(nextQuota),
      updatedAt: currentTimeText()
    };
  }

  async function syncDisplayFromBackendCache() {
    if (previewEnabled || isRefreshing) return;
    try {
      const cached = await invoke<RawQuota | null>("read_cached_quota");
      if (!cached || !hasQuotaPercentages(cached)) return;
      const normalized = normalizeQuota(cached);
      if ((status === "error" || status === "stale") && normalized.updatedAt === blockedCacheRefreshText) {
        return;
      }
      if (quotaSnapshotKey(normalized) === quotaSnapshotKey(displayQuota) && normalized.updatedAt === lastRefreshText) {
        return;
      }
      rememberQuota(normalized, normalized.status === "ready" ? "ready" : "stale");
    } catch {
      // Live refresh remains the authoritative path; cache sync only keeps visible UI current.
    }
  }

  async function refreshAutostartState() {
    try {
      autostart = await withTimeout(isEnabled(), 2_000, "自启动状态读取超时");
    } catch {
      autostart = false;
    }
    await syncAutostartMenu();
  }

  function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
    let timer: number | undefined;
    return new Promise<T>((resolve, reject) => {
      timer = window.setTimeout(() => reject(new Error(message)), ms);
      promise.then(resolve, reject).finally(() => window.clearTimeout(timer));
    });
  }

  function sleep(ms: number) {
    return new Promise<void>((resolve) => window.setTimeout(resolve, ms));
  }

  async function updateTrayQuota(nextStatus: "ready" | "stale" | "error", currentQuota: QuotaSnapshot | null) {
    if (!isControllerWindow) return;
    try {
      await invoke("update_tray_quota", {
        state: {
          quotaLabel: currentQuota?.quotaLabel ?? null,
          quotaRemaining: currentQuota?.quotaRemaining ?? null,
          resetCreditsAvailable: currentQuota?.resetCreditsAvailable ?? null,
          status: nextStatus
        }
      });
    } catch {
      // The panel should keep working even if the tray icon cannot be updated.
    }
  }

  async function setTrayUpdateAvailable(available: boolean, latestVersion = "") {
    if (!isControllerWindow) return;
    try {
      await invoke("set_update_available", {
        available,
        latestVersion: latestVersion || null
      });
    } catch {
      // The in-window update state remains usable even if the tray badge cannot refresh.
    }
  }

  async function checkForUpdates(manual: boolean) {
    if (!isControllerWindow) return;
    if (updateChecking || updateDownloading) return;
    updateChecking = true;
    updateErrorText = "";
    try {
      const nextInfo = await withTimeout(
        invoke<UpdateInfo>("check_update"),
        15_000,
        "检查更新超时"
      );
      updateInfo = nextInfo;
      await setTrayUpdateAvailable(nextInfo.available, nextInfo.latestVersion);
      if (nextInfo.available) {
        if (manual) {
          showToast("发现新版本，开始更新");
          await switchMode("large");
        }
        void startUpdateDownload(nextInfo);
      } else if (manual) {
        showToast(nextInfo.message);
      }
    } catch (error) {
      updateErrorText = friendlyUpdateError(error);
      if (manual) showToast(updateErrorText);
    } finally {
      updateChecking = false;
    }
  }

  async function startUpdateDownload(nextInfo: UpdateInfo | null = updateInfo) {
    if (updateDownloading) {
      updatePanelOpen = true;
      return;
    }
    if (!nextInfo?.available) return;
    updateInfo = nextInfo;
    updatePanelOpen = true;
    updateDownloading = true;
    updateErrorText = "";
    updateProgress = {
      phase: "downloading",
      percent: 0,
      downloaded: 0,
      total: null,
      message: "准备下载更新"
    };

    try {
      if (!nextInfo.portableAssetUrl) throw new Error("未找到便携版更新文件");
      await invoke<string>("download_portable_update", {
        url: nextInfo.portableAssetUrl,
        fileName: nextInfo.portableFileName || null,
        expectedSize: nextInfo.portableAssetSize || null
      });
    } catch (error) {
      updateErrorText = friendlyUpdateError(error);
      showToast(updateErrorText);
    } finally {
      if (updateProgress?.phase !== "installing") {
        updateDownloading = false;
      }
    }
  }

  function closeUpdatePanel() {
    updatePanelOpen = false;
  }

  async function switchMode(nextMode: WindowMode) {
    resetChromeVisibility();
    try {
      await invoke("set_mode", { mode: nextMode });
    } catch {
      // Keep the preview usable outside Tauri.
      mode = nextMode;
      updateScale();
    }
  }

  async function toggleTopmost() {
    try {
      alwaysOnTop = await invoke<boolean>("toggle_topmost");
    } catch {
      showToast("置顶设置失败");
    }
  }

  async function toggleAutostart() {
    try {
      const wasEnabled = await withTimeout(isEnabled(), 2_000, "自启动状态读取超时");
      if (wasEnabled) {
        await withTimeout(disable(), 4_000, "关闭自启动超时");
        autostart = await withTimeout(isEnabled(), 2_000, "自启动状态复查超时");
        await syncAutostartMenu();
        showToast(autostart ? "关闭自启动失败" : "已关闭自启动");
      } else {
        await withTimeout(enable(), 4_000, "开启自启动超时");
        autostart = await withTimeout(isEnabled(), 2_000, "自启动状态复查超时");
        await syncAutostartMenu();
        showToast(autostart ? "已开启自启动" : "开启自启动失败");
      }
    } catch {
      await refreshAutostartState();
      showToast("自启动设置失败");
    }
  }

  async function syncAutostartMenu() {
    try {
      await invoke("set_autostart_menu_checked", { checked: autostart });
    } catch {
      // The menu check mark is a convenience; the actual autostart state remains authoritative.
    }
  }

  async function syncAutoRefreshMenu() {
    try {
      await invoke("set_auto_refresh_menu_seconds", { seconds: autoRefreshSeconds });
    } catch {
      // The menu label is a convenience; the frontend timer remains authoritative.
    }
  }

  async function syncAppearanceMenu() {
    try {
      await invoke("set_appearance_menu_state", {
        colorScheme,
        darkMode,
        opacity: panelOpacity
      });
    } catch {
      // The menu check marks are a convenience; local UI state remains authoritative.
    }
  }

  async function setAutoRefreshSeconds(nextSeconds: number) {
    if (!isAutoRefreshPreset(nextSeconds)) return;
    autoRefreshSeconds = nextSeconds;
    writeAutoRefreshSeconds(nextSeconds);
    scheduleRefreshTimer();
    if (isControllerWindow) {
      await syncAutoRefreshMenu();
      showToast(`自动刷新 ${autoRefreshLabel(nextSeconds)}`);
    }
  }

  async function setColorScheme(nextColorScheme: ColorScheme, syncMenu = true) {
    if (!isColorScheme(nextColorScheme)) return;
    preferredColorScheme = nextColorScheme;
    writeColorScheme(nextColorScheme);
    await applyActiveTheme(nextColorScheme, hasAutomaticTheme(displayQuota), isControllerWindow, syncMenu);
  }

  async function syncThemeForQuota(nextQuota: QuotaSnapshot) {
    const automaticTheme = automaticThemeForQuota(nextQuota.quotaRemaining);
    if (automaticTheme && manualThemeOverride) return;
    await applyActiveTheme(automaticTheme ?? preferredColorScheme, false, isControllerWindow);
  }

  async function applyActiveTheme(
    nextColorScheme: ColorScheme,
    nextManualOverride: boolean,
    notify = false,
    syncMenu = true
  ) {
    const changed = colorScheme !== nextColorScheme || manualThemeOverride !== nextManualOverride;
    colorScheme = nextColorScheme;
    manualThemeOverride = nextManualOverride;
    if (!changed || !notify) return;

    if (syncMenu) await syncAppearanceMenu();
    await emit(activeThemeEvent, {
      colorScheme: nextColorScheme,
      manualOverride: nextManualOverride
    } satisfies ActiveThemeState);
  }

  async function setDarkMode(nextDarkMode: boolean, syncMenu = true) {
    darkMode = nextDarkMode;
    writeDarkMode(nextDarkMode);
    if (syncMenu) await syncAppearanceMenu();
  }

  async function setPanelOpacity(nextOpacity: number, syncMenu = true) {
    if (!isPanelOpacity(nextOpacity)) return;
    panelOpacity = nextOpacity;
    writePanelOpacity(nextOpacity);
    if (syncMenu) await syncAppearanceMenu();
  }

  function showToast(text: string) {
    toast = text;
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => {
      toast = "";
    }, 3000);
  }

  function sampleQuota(): QuotaSnapshot {
    return normalizeQuota({
      status: "ready",
      limitName: "Codex",
      planType: "pro",
      updatedAt: "21:02:02",
      quotaLabel: "周额度",
      quotaRemaining: 96,
      quotaReset: "7/20 02:59",
      resetCreditsAvailable: 4
    });
  }

  function normalizeQuota(raw: RawQuota): QuotaSnapshot {
    const remaining = readQuotaRemaining(raw);
    return {
      status: readString(raw, "status") || "ready",
      limitName: readString(raw, "limitName", "limit_name") || "Codex",
      planType: readString(raw, "planType", "plan_type"),
      updatedAt: normalizeUpdatedAt(readString(raw, "updatedAt", "updated_at")),
      quotaLabel: readString(raw, "quotaLabel", "quota_label") || "周额度",
      quotaRemaining: normalizePercent(remaining),
      quotaReset: normalizeReset(readQuotaReset(raw, remaining)),
      resetCreditsAvailable: normalizeCount(readNumber(raw, "resetCreditsAvailable", "reset_credits_available"))
    };
  }

  function currentTimeText() {
    return new Date().toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false
    });
  }

  function rememberQuota(nextQuota: QuotaSnapshot, nextStatus: Status) {
    const refreshText = nextQuota.updatedAt || currentTimeText();
    quota = nextQuota;
    lastGoodQuota = nextQuota;
    displayQuota = nextQuota;
    lastRefreshText = refreshText;
    blockedCacheRefreshText = "";
    status = nextStatus;
    syncQuotaVisual(nextQuota);
    writeCachedQuota(nextQuota);
    void syncThemeForQuota(nextQuota);
  }

  function syncQuotaVisual(nextQuota: QuotaSnapshot | null) {
    if (!hasUsableQuota(nextQuota)) {
      cancelQuotaAnimation();
      quotaVisualReady = false;
      visualQuotaRemaining = 0;
      return;
    }

    const target = normalizePercent(nextQuota.quotaRemaining);
    if (!interfaceReady && !previewEnabled) {
      cancelQuotaAnimation();
      quotaVisualReady = false;
      visualQuotaRemaining = 100;
      return;
    }

    const startupAnimation = !quotaVisualReady;
    if (startupAnimation) {
      quotaVisualReady = true;
      visualQuotaRemaining = 100;
    }

    animateQuotaVisual(target, startupAnimation);
  }

  function animateQuotaVisual(targetValue: number, startupAnimation = false) {
    const target = normalizePercent(targetValue);
    const start = clamp(visualQuotaRemaining, 0, 100);
    cancelQuotaAnimation();

    if (Math.abs(start - target) < 0.1) {
      visualQuotaRemaining = target;
      return;
    }

    const startedAt = performance.now();
    const minDuration = startupAnimation ? quotaStartupAnimationMinMs : quotaAnimationMinMs;
    const maxDuration = startupAnimation ? quotaStartupAnimationMaxMs : quotaAnimationMaxMs;
    const perPercentMs = startupAnimation ? quotaStartupAnimationPerPercentMs : quotaAnimationPerPercentMs;
    const duration = clamp(minDuration + Math.abs(start - target) * perPercentMs, minDuration, maxDuration);

    const step = (now: number) => {
      const progress = clamp((now - startedAt) / duration, 0, 1);
      const eased = startupAnimation ? easeInOutCubic(progress) : easeOutCubic(progress);
      visualQuotaRemaining = start + (target - start) * eased;

      if (progress < 1) {
        quotaAnimationFrame = window.requestAnimationFrame(step);
      } else {
        visualQuotaRemaining = target;
        quotaAnimationFrame = undefined;
      }
    };

    quotaAnimationFrame = window.requestAnimationFrame(step);
  }

  function cancelQuotaAnimation() {
    if (quotaAnimationFrame !== undefined) {
      window.cancelAnimationFrame(quotaAnimationFrame);
      quotaAnimationFrame = undefined;
    }
  }

  function easeOutCubic(value: number) {
    const progress = clamp(value, 0, 1);
    return 1 - Math.pow(1 - progress, 3);
  }

  function easeInOutCubic(value: number) {
    const progress = clamp(value, 0, 1);
    return progress < 0.5 ? 4 * progress ** 3 : 1 - Math.pow(-2 * progress + 2, 3) / 2;
  }

  function normalizeUpdatedAt(value?: string) {
    const trimmed = typeof value === "string" ? value.trim() : "";
    if (trimmed && trimmed !== "--:--:--") {
      return trimmed;
    }
    return currentTimeText();
  }

  function normalizePercent(value?: number | null) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return 0;
    return Math.round(Math.min(100, Math.max(0, numeric)));
  }

  function normalizeCount(value?: number | null) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return null;
    return Math.max(0, Math.round(numeric));
  }

  function normalizeReset(value?: string) {
    if (typeof value !== "string" || !value.trim() || value === "unknown") return "--:--";
    return value.trim();
  }

  function readCachedQuota(): QuotaSnapshot | null {
    try {
      const text = window.localStorage.getItem(quotaCacheKey);
      if (!text) return null;
      const cached = JSON.parse(text) as RawQuota;
      if (!hasQuotaPercentages(cached)) return null;
      return normalizeQuota(cached);
    } catch {
      return null;
    }
  }

  function readAutoRefreshSeconds() {
    try {
      const value = window.localStorage.getItem(autoRefreshCacheKey);
      const seconds = Number(value);
      if (isAutoRefreshPreset(seconds)) {
        return seconds;
      }
    } catch {
      // Fall back to the default refresh interval when storage is unavailable.
    }
    return defaultAutoRefreshSeconds;
  }

  function readColorScheme(): ColorScheme {
    try {
      const value = window.localStorage.getItem(colorSchemeCacheKey);
      if (isColorScheme(value)) {
        return value;
      }
    } catch {
      // Fall back to the default color when storage is unavailable.
    }
    return "blue";
  }

  function readDarkMode() {
    try {
      return window.localStorage.getItem(darkModeCacheKey) === "1";
    } catch {
      return false;
    }
  }

  function readPanelOpacity(): PanelOpacity {
    try {
      const value = Number(window.localStorage.getItem(opacityCacheKey));
      if (isPanelOpacity(value)) {
        return value;
      }
    } catch {
      // Fall back to the default panel opacity when storage is unavailable.
    }
    return 90;
  }

  function writeAutoRefreshSeconds(seconds: number) {
    try {
      window.localStorage.setItem(autoRefreshCacheKey, String(seconds));
    } catch {
      // The in-memory value still applies for this session.
    }
  }

  function writeColorScheme(nextColorScheme: ColorScheme) {
    try {
      window.localStorage.setItem(colorSchemeCacheKey, nextColorScheme);
    } catch {
      // The in-memory value still applies for this session.
    }
  }

  function writeDarkMode(nextDarkMode: boolean) {
    try {
      window.localStorage.setItem(darkModeCacheKey, nextDarkMode ? "1" : "0");
    } catch {
      // The in-memory value still applies for this session.
    }
  }

  function writePanelOpacity(nextOpacity: PanelOpacity) {
    try {
      window.localStorage.setItem(opacityCacheKey, String(nextOpacity));
    } catch {
      // The in-memory value still applies for this session.
    }
  }

  function isAutoRefreshPreset(seconds: number) {
    return Number.isInteger(seconds) && autoRefreshPresets.includes(seconds);
  }

  function isColorScheme(value: unknown): value is ColorScheme {
    return typeof value === "string" && colorSchemes.includes(value as ColorScheme);
  }

  function isPanelOpacity(value: unknown): value is PanelOpacity {
    return typeof value === "number" && opacityPresets.includes(value as PanelOpacity);
  }

  function autoRefreshLabel(seconds: number) {
    if (seconds < 60) return `${seconds}s`;
    return `${Math.round(seconds / 60)}min`;
  }

  async function hydrateQuotaCache() {
    let hydrated = false;
    try {
      const cached = await invoke<RawQuota | null>("read_cached_quota");
      if (cached && hasQuotaPercentages(cached)) {
        const normalized = normalizeQuota(cached);
        if (!hasUsableQuota(lastGoodQuota)) {
          lastGoodQuota = normalized;
        }
        if (!hasUsableQuota(displayQuota) || status !== "ready") {
          rememberQuota(normalized, "stale");
          await updateTrayQuota("stale", normalized);
        }
        hydrated = true;
      }
    } catch {
      // Frontend localStorage remains available as a local cache.
    }

    if (hydrated) return;
    const cached = readCachedQuota();
    if (!cached) return;
    rememberQuota(cached, "stale");
    await updateTrayQuota("stale", cached);
  }

  function writeCachedQuota(nextQuota: QuotaSnapshot) {
    try {
      window.localStorage.setItem(quotaCacheKey, JSON.stringify(nextQuota));
    } catch {
      // Cache is a convenience only; live refresh remains authoritative.
    }
  }

  function automaticThemeForQuota(value?: number): ColorScheme | null {
    if (typeof value !== "number") return null;
    if (value <= 20) return "red";
    if (value <= 50) return "orange";
    return null;
  }

  function hasAutomaticTheme(currentQuota: QuotaSnapshot | null) {
    return automaticThemeForQuota(currentQuota?.quotaRemaining) !== null;
  }

  function isActiveThemeState(value: unknown): value is ActiveThemeState {
    if (!value || typeof value !== "object") return false;
    const candidate = value as Partial<ActiveThemeState>;
    return isColorScheme(candidate.colorScheme) && typeof candidate.manualOverride === "boolean";
  }

  function makeRingSegments(value: number): RingSegment[] {
    const progress = clamp(value, 0, 100);
    if (progress <= 0) return [];

    const step = 100 / ringSegmentCount;
    const segments: RingSegment[] = [];
    for (let index = 0; index < ringSegmentCount; index += 1) {
      const start = index * step;
      if (start >= progress) break;

      const length = Math.min(step, progress - start);
      const position = clamp(((start + length * 0.5) / progress) * 100, 0, 100);
      segments.push({
        index,
        length,
        offset: -start,
        color: ringSegmentColor(position)
      });
    }
    return segments;
  }

  function ringSegmentColor(position: number) {
    if (position <= ringGradientMidpoint) {
      const midMix = clamp((position / ringGradientMidpoint) * 100, 0, 100);
      return `color-mix(in srgb, var(--ring-start) ${formatPercent(100 - midMix)}, var(--ring-mid) ${formatPercent(midMix)})`;
    }

    const endMix = clamp(((position - ringGradientMidpoint) / (100 - ringGradientMidpoint)) * 100, 0, 100);
    return `color-mix(in srgb, var(--ring-mid) ${formatPercent(100 - endMix)}, var(--ring-end) ${formatPercent(endMix)})`;
  }

  function formatPercent(value: number) {
    return `${value.toFixed(2)}%`;
  }

  function resetText(value?: string) {
    if (!value || value === "unknown") return "--:--";
    return value;
  }

  function splitResetText(value?: string) {
    const normalized = resetText(value).trim();
    const [date, ...timeParts] = normalized.split(/\s+/);
    return {
      date,
      time: timeParts.join(" ")
    };
  }

  function quotaSnapshotKey(currentQuota: QuotaSnapshot | null) {
    if (!currentQuota) return "";
    return [
      currentQuota.status,
      currentQuota.limitName,
      currentQuota.planType,
      currentQuota.quotaLabel,
      currentQuota.quotaRemaining,
      currentQuota.quotaReset,
      currentQuota.resetCreditsAvailable
    ].join("|");
  }

  function normalizeMode(value: unknown): WindowMode {
    if (value === "large" || value === "ring") return value;
    return "small";
  }

  function nextMode(currentMode: WindowMode): WindowMode {
    if (currentMode === "small") return "large";
    if (currentMode === "large") return "ring";
    return "small";
  }

  function nextModeTitle(currentMode: WindowMode) {
    if (currentMode === "small") return "切换到进度大窗";
    if (currentMode === "large") return "切换到环形大窗";
    return "切换到小窗";
  }

  function hasQuotaPercentages(raw: RawQuota) {
    return readQuotaRemaining(raw) !== null;
  }

  function readQuotaRemaining(raw: RawQuota) {
    const current = readNumber(raw, "quotaRemaining", "quota_remaining");
    if (current !== null) return current;
    const weekly = readNumber(raw, "secondaryRemaining", "secondary_remaining");
    if (weekly !== null) return weekly;
    return readNumber(raw, "primaryRemaining", "primary_remaining");
  }

  function readQuotaReset(raw: RawQuota, remaining: number | null) {
    const current = readString(raw, "quotaReset", "quota_reset");
    if (current) return current;
    const weekly = readNumber(raw, "secondaryRemaining", "secondary_remaining");
    if (weekly !== null) return readString(raw, "secondaryReset", "secondary_reset");
    const primary = readNumber(raw, "primaryRemaining", "primary_remaining");
    if (primary !== null || remaining !== null) return readString(raw, "primaryReset", "primary_reset");
    return "";
  }

  function hasUsableQuota(currentQuota: QuotaSnapshot | null): currentQuota is QuotaSnapshot {
    return (
      !!currentQuota &&
      Number.isFinite(currentQuota.quotaRemaining)
    );
  }

  function readString(raw: RawQuota, camel: string, snake?: string) {
    const value = raw[camel] ?? (snake ? raw[snake] : undefined);
    return typeof value === "string" ? value.trim() : "";
  }

  function readNumber(raw: RawQuota, camel: string, snake?: string) {
    const value = raw[camel] ?? (snake ? raw[snake] : undefined);
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : null;
  }

  function compactSummaryText() {
    if (updateDownloading) return updateProgressLine();
    if (status === "error") return "Codex: 刷新失败";
    if (hasUsableQuota(displayQuota)) return null;
    return "Codex: 正在读取";
  }

  function updateProgressLine() {
    const percent = updatePercent;
    const width = 16;
    const arrowIndex = Math.min(width - 1, Math.max(0, Math.round((percent / 100) * (width - 1))));
    const chars = Array.from({ length: width }, (_, index) => (index === arrowIndex ? ">" : "-"));
    return `更新中：${chars.join("")} ${percent}%`;
  }

  function updatePanelText() {
    if (updateErrorText) return updateErrorText;
    if (updateDownloading) return `正在更新：${updatePercent}%`;
    if (updateInfo?.available) return `发现新版本 ${updateInfo.latestVersion}`;
    if (updateInfo) return updateInfo.message;
    if (updateChecking) return "正在检查更新";
    return "";
  }

  function makeUpdateNoticeText() {
    if (updateErrorText) return "更新失败，请稍后重试";
    if (!updateDownloading) return "";
    if (updateProgress?.phase === "installing") return "更新已准备，正在重启";
    return `正在更新：${updatePercent}%`;
  }

  function makeBottomNoticeText(
    currentStatus: Status,
    hasVisibleQuota: boolean,
    currentUpdateText: string,
    currentErrorText: string
  ) {
    const quotaNotice =
      (currentStatus === "error" || currentStatus === "stale") && hasVisibleQuota && currentErrorText
        ? "刷新失败，当前显示上次额度"
        : "";
    if (quotaNotice && currentUpdateText) return `${quotaNotice} · ${currentUpdateText}`;
    return quotaNotice || currentUpdateText;
  }

  function friendlyUpdateError(error: unknown) {
    const raw = String(error || "").trim();
    if (!raw) return "下载中断，请重新下载";
    const lower = raw.toLowerCase();
    if (lower.includes("未找到便携版更新文件")) {
      return "新版本暂不可下载";
    }
    if (lower.includes("无法启动便携版更新")) {
      return "更新程序启动失败，请手动下载";
    }
    if (lower.includes("检查更新") || lower.includes("timeout") || lower.includes("超时")) {
      return "无法检查更新，请稍后重试";
    }
    if (
      lower.includes("下载") ||
      lower.includes("network") ||
      lower.includes("dns") ||
      lower.includes("connection") ||
      lower.includes("request") ||
      lower.includes("fetch")
    ) {
      return "下载中断，请重新下载";
    }
    return raw;
  }

  function makeSubtitle(currentQuota: QuotaSnapshot | null, refreshText: string) {
    const nextRefreshText = refreshText || "--:--:--";
    if (!currentQuota) return `Codex ${nextRefreshText}`;
    const plan = currentQuota.planType ? ` / ${currentQuota.planType}` : "";
    return `${currentQuota.limitName || "Codex"}${plan} ${nextRefreshText}`;
  }

  function makeStatusText(
    currentStatus: Status,
    currentErrorText: string,
    currentQuota: QuotaSnapshot | null,
    refreshText: string
  ) {
    const nextRefreshText = refreshText || "--:--:--";
    if (currentStatus === "error" && currentQuota) {
      return `刷新失败，显示上次数据: ${currentErrorText || "未知原因"}`;
    }
    if (currentStatus === "error") return `暂时无法获取: ${currentErrorText || "未知原因"}`;
    if (currentStatus === "stale") {
      return currentErrorText ? `显示上次数据: ${currentErrorText}` : `显示上次数据 ${nextRefreshText}`;
    }
    if (!currentQuota) return "正在读取";
    return `上次刷新 ${nextRefreshText}`;
  }

  function friendlyError(error: unknown) {
    const raw = String(error || "").trim();
    if (!raw) return "未知原因";

    const lower = raw.toLowerCase();
    if (lower.includes("cannot find codex cli") || lower.includes("codex.exe")) {
      return "未找到 Codex，请先安装并登录 Codex，或设置 CODEX_QUOTA_CODEX_PATH";
    }
    if (lower.includes("cannot start codex app-server")) {
      return "无法启动 Codex app-server，请确认 Codex 已安装且可正常打开";
    }
    if (
      lower.includes("network") ||
      lower.includes("internet") ||
      lower.includes("dns") ||
      lower.includes("offline") ||
      lower.includes("connection") ||
      lower.includes("连接") ||
      lower.includes("网络")
    ) {
      return "网络未连接或连接不稳定，请检查网络后重试";
    }
    if (
      lower.includes("unauthorized") ||
      lower.includes("forbidden") ||
      lower.includes("sign in") ||
      lower.includes("login") ||
      lower.includes("logged in") ||
      lower.includes("auth") ||
      lower.includes("认证") ||
      lower.includes("登录")
    ) {
      return "Codex 账号未登录或登录状态已过期，请重新登录 Codex";
    }
    if (lower.includes("timed out") || lower.includes("timeout") || lower.includes("响应超时")) {
      return "Codex 响应超时，请稍后刷新";
    }
    if (lower.includes("no rate limit data") || lower.includes("returned no rate limit")) {
      return "未读取到额度数据，请确认 Codex 已登录账号";
    }
    if (lower.includes("temporary placeholder quota data")) {
      return "Codex 返回了临时额度数据，已保留上次额度";
    }
    if (lower.includes("closed before responding")) {
      return "Codex app-server 未返回数据，请确认 Codex 当前可用";
    }
    return raw;
  }

  function updateScale() {
    if (mode === "small" || mode === "ring") {
      uiScale = 1;
      widthScale = 1;
      heightScale = 1;
      return;
    }

    const nextWidthScale = clamp(window.innerWidth / largeBaseWidth, 1, 1.5);
    const nextHeightScale = clamp(window.innerHeight / largeBaseHeight, 1, 1.7);
    widthScale = nextWidthScale;
    heightScale = nextHeightScale;
    uiScale = clamp(nextWidthScale * 0.62 + nextHeightScale * 0.38, 1, 1.48);
  }

  function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
  }

  async function hidePanel() {
    try {
      await invoke("hide_panel");
    } catch {
      // Keep the preview usable outside Tauri.
    }
  }

  function startPointer(event: PointerEvent) {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button") || target.closest(".resize-handle")) return;
    event.preventDefault();
    pendingPointer = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function movePointer(event: PointerEvent) {
    if (!pendingPointer || pendingPointer.id !== event.pointerId) return;
    const moved =
      Math.abs(event.clientX - pendingPointer.x) > dragThresholdPx ||
      Math.abs(event.clientY - pendingPointer.y) > dragThresholdPx;
    if (!moved) return;

    pendingPointer = null;
    if (!appWindow) return;
    void appWindow.startDragging();
  }

  function finishPointer(event: PointerEvent) {
    if (!pendingPointer || pendingPointer.id !== event.pointerId) return;
    pendingPointer = null;
    void hidePanel();
  }

  function cancelPointer(event: PointerEvent) {
    if (!pendingPointer || pendingPointer.id !== event.pointerId) return;
    pendingPointer = null;
  }

  function startResize(event: PointerEvent) {
    if (event.button !== 0 || mode !== "large") return;
    if (!appWindow) return;
    event.preventDefault();
    event.stopPropagation();
    void appWindow.startResizeDragging("SouthEast");
  }

  function showLargeHeaderFromTarget(target: EventTarget | null) {
    if (!(target instanceof HTMLElement)) return;
    const interactiveContent =
      mode === "small"
        ? target.closest(".compact-summary, .compact-large, .compact-refresh")
        : mode === "ring"
          ? target.closest(".quota-ring, .topbar")
          : target.closest(".quota-item, .topbar");
    if (interactiveContent) {
      showLargeHeader();
    }
  }

  function handlePointerMove(event: PointerEvent) {
    if (canRevealChrome(event)) {
      showLargeHeaderFromTarget(event.target);
    }
    movePointer(event);
  }

  function canRevealChrome(event: PointerEvent) {
    if (chromeRevealReady) return true;
    if (Date.now() < chromeRevealBlockedUntil) {
      chromeRevealAnchor = { x: event.clientX, y: event.clientY };
      return false;
    }
    if (!chromeRevealAnchor) {
      chromeRevealAnchor = { x: event.clientX, y: event.clientY };
      return false;
    }
    const moved =
      Math.abs(event.clientX - chromeRevealAnchor.x) > dragThresholdPx ||
      Math.abs(event.clientY - chromeRevealAnchor.y) > dragThresholdPx;
    if (!moved) return false;
    chromeRevealReady = true;
    chromeRevealAnchor = null;
    return true;
  }

  function handlePointerLeave() {
    hideLargeHeader();
    chromeRevealReady = false;
    chromeRevealBlockedUntil = 0;
    chromeRevealAnchor = null;
  }

  function finishPointerActions(event: PointerEvent) {
    finishPointer(event);
  }

  function cancelPointerActions(event: PointerEvent) {
    cancelPointer(event);
  }

  function showContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    void invoke("show_context_menu").catch(() => {});
  }

  function showLargeHeader() {
    if (mode === "small") {
      setCompactActionsVisible(true);
    } else {
      largeHeaderVisible = true;
    }
  }

  function hideLargeHeader() {
    if (mode === "small") {
      setCompactActionsVisible(false);
    } else {
      largeHeaderVisible = false;
    }
  }

  function resetChromeVisibility() {
    largeHeaderVisible = false;
    compactActionsVisible = false;
    chromeRevealReady = false;
    chromeRevealBlockedUntil = Date.now() + 250;
    chromeRevealAnchor = null;
  }

  function setCompactActionsVisible(visible: boolean, force = false) {
    if (!force && compactActionsVisible === visible) return;
    compactActionsVisible = visible;
    if (mode === "small") {
      void invoke("set_small_actions_collapsed", { collapsed: !visible }).catch(() => {});
    }
  }

  function resetCreditsText(currentQuota: QuotaSnapshot) {
    const count = currentQuota.resetCreditsAvailable;
    return typeof count === "number" && Number.isFinite(count) ? `${count}次` : "--";
  }
</script>

<svelte:window on:contextmenu={showContextMenu} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<main
  class={shellClass}
  style={scaleStyle}
  on:pointerdown={startPointer}
  on:pointermove={handlePointerMove}
  on:pointerup={finishPointerActions}
  on:pointercancel={cancelPointerActions}
  on:pointerleave={handlePointerLeave}
>
  <section class="compact-row">
    <span class="compact-summary">
      {#if compactSummaryText()}
        {compactSummaryText()}
      {:else if hasQuota && displayQuota}
        <span class="compact-prefix">Codex:</span>
        <span class="compact-meter" aria-hidden="true">
          <span class="compact-meter-fill" style={`width:${visualQuotaWidth}`}></span>
        </span>
        <span class="compact-percent">{displayQuota.quotaRemaining}%</span>
      {/if}
    </span>
    <button class="compact-large" title={nextModeTitle(mode)} aria-label={nextModeTitle(mode)} on:click={() => switchMode(nextMode(mode))}>
      <svg class="icon action-icon window-icon" viewBox="0 0 24 24" aria-hidden="true">
        <rect x="7" y="7" width="10" height="10" rx="0.5"></rect>
      </svg>
    </button>
    <button class:spinning={isRefreshing} class="compact-refresh" title="立即刷新" aria-label="立即刷新" on:click={requestQuotaRefresh}>
      <svg class="icon action-icon refresh-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M21 12a9 9 0 1 1-2.64-6.36"></path>
        <path d="M21 3v6h-6"></path>
      </svg>
    </button>
  </section>

  <header class="topbar">
    <div class="title-area">
      <p>{subtitleText}</p>
    </div>
    <div class="window-actions">
      <button title={nextModeTitle(mode)} aria-label={nextModeTitle(mode)} on:click={() => switchMode(nextMode(mode))}>
        <svg class="icon action-icon window-icon" viewBox="0 0 24 24" aria-hidden="true">
          {#if mode === "large"}
            <circle cx="12" cy="12" r="7"></circle>
          {:else}
            <path d="M8 7h9v9H8z"></path>
            <path d="M5 10h9v9H5z"></path>
          {/if}
        </svg>
      </button>
      <button title={alwaysOnTop ? "取消置顶" : "置顶"} aria-label={alwaysOnTop ? "取消置顶" : "置顶"} on:click={toggleTopmost}>
        <svg class={`icon action-icon pin-icon ${alwaysOnTop ? "icon-solid" : ""}`} viewBox="0 0 24 24" aria-hidden="true">
          <path d="M14 3l7 7-2 2-2-2-4 4v5l-1 1-4-4-5 5-1-1 5-5-4-4 1-1h5l4-4-2-2 2-2z"></path>
        </svg>
      </button>
      <button class:spinning={isRefreshing} title="立即刷新" aria-label="立即刷新" on:click={requestQuotaRefresh}>
        <svg class="icon action-icon refresh-icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M21 12a9 9 0 1 1-2.64-6.36"></path>
          <path d="M21 3v6h-6"></path>
        </svg>
      </button>
    </div>
  </header>

  {#if updateVisible}
    <section class="update-panel">
      <button class="update-close" title="隐藏更新提示" aria-label="隐藏更新提示" on:click={closeUpdatePanel}>×</button>
      <div class="update-copy">
        <strong>{updatePanelText()}</strong>
      </div>
      <div class="update-meter">
        <div class="update-meter-fill" style={`width:${updatePercent}%`}></div>
      </div>
      {#if updateErrorText}
        <div class="update-actions">
          <button class="update-button" title="重试" aria-label="重试" on:click={() => startUpdateDownload()}>重试</button>
          <button class="update-button" title="隐藏" aria-label="隐藏" on:click={closeUpdatePanel}>隐藏</button>
        </div>
      {:else if updateInfo?.available && !updateDownloading}
        <button class="update-button" title="更新" aria-label="更新" on:click={() => startUpdateDownload()}>更新</button>
      {/if}
    </section>
  {/if}

  <section class="quota-list">
    {#if hasQuota && displayQuota}
      {#if isRing}
        <article class="quota-ring-card">
          <div class="quota-ring">
            <svg class="quota-ring-svg" viewBox="0 0 120 120" aria-hidden="true">
              <circle class="quota-ring-rail" cx="60" cy="60" r="52" pathLength="100"></circle>
              <g class="quota-ring-segments">
                {#each ringSegments as segment (segment.index)}
                  <circle
                    class="quota-ring-segment"
                    cx="60"
                    cy="60"
                    r="52"
                    pathLength="100"
                    style={`stroke-dasharray:${segment.length} ${100 - segment.length};stroke-dashoffset:${segment.offset};stroke:${segment.color}`}
                  ></circle>
                {/each}
              </g>
            </svg>
            <div class="quota-ring-content">
              <strong>{displayQuota.quotaRemaining}%</strong>
              <span class="quota-ring-reset">重置{ringReset.date}<span class="quota-ring-reset-time">{" "}{ringReset.time}</span></span>
              <span>{resetCreditsText(displayQuota)}</span>
            </div>
          </div>
        </article>
      {:else}
        <article class="quota-item">
          <div class="quota-line">
            <span class="quota-title">
              <strong class="quota-brand">Codex</strong><span class="quota-reset">: 重置{resetText(displayQuota.quotaReset)} | {resetCreditsText(displayQuota)}</span>
            </span>
            <span class="quota-facts">
              <strong>{displayQuota.quotaRemaining}%</strong>
            </span>
          </div>
          <div class="meter">
            <div class="meter-fill" style={`width:${visualQuotaWidth}`}></div>
          </div>
        </article>
      {/if}
    {:else}
      <div class="empty">
        {#if status === "error"}
          暂时无法获取：{errorText || "未知原因"}
        {:else}
          正在连接 Codex app-server
        {/if}
      </div>
    {/if}
  </section>

  {#if bottomNoticeText}
    <div class="status-overlay">{bottomNoticeText}</div>
  {/if}

  <footer class="footer">
    <span class={`dot ${status}`}></span>
    <span>{statusMessage}</span>
  </footer>
  <div class="resize-handle" aria-hidden="true" on:pointerdown={startResize}></div>
</main>

{#if toast}
  <div class="toast">{toast}</div>
{/if}

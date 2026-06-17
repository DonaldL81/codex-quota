<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type Event as TauriEvent } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

  type QuotaSnapshot = {
    status: string;
    limitName: string;
    planType: string;
    updatedAt: string;
    primaryRemaining: number;
    primaryReset: string;
    secondaryRemaining: number;
    secondaryReset: string;
  };

  type QuotaWindow = {
    label: string;
    remaining: number;
    reset: string;
  };

  type WindowState = {
    mode: "small" | "large";
    alwaysOnTop: boolean;
    visible: boolean;
  };

  type Status = "loading" | "ready" | "stale" | "error";
  type CurrentWindow = ReturnType<typeof getCurrentWindow>;
  type RawQuota = Partial<QuotaSnapshot> & Record<string, unknown>;

  const refreshMs = 10_000;
  const invokeTimeoutMs = 35_000;
  const largeBaseWidth = 200;
  const largeBaseHeight = 112;
  const quotaCacheKey = "codex-quota-v2:last-quota";
  const previewParams =
    typeof window === "undefined" ? new URLSearchParams() : new URLSearchParams(window.location.search);
  const previewEnabled = previewParams.has("mock");
  const previewInitialMode: "small" | "large" = previewParams.get("mode") === "large" ? "large" : "small";

  let mode: "small" | "large" = previewInitialMode;
  let alwaysOnTop = true;
  let autostart = false;
  let quota: QuotaSnapshot | null = previewEnabled ? sampleQuota() : readCachedQuota();
  let status: Status = previewEnabled ? "ready" : quota ? "stale" : "loading";
  let isRefreshing = false;
  let errorText = "";
  let toast = "";
  let toastTimer: number | undefined;
  let uiScale = 1;
  let widthScale = 1;
  let heightScale = 1;
  let appWindow: CurrentWindow | null = null;

  $: quotaWindows = makeQuotaWindows(quota);
  $: isSmall = mode === "small";
  $: hasQuota = hasUsableQuota(quota);
  $: scaleStyle = `--ui-scale:${uiScale.toFixed(3)};--width-scale:${widthScale.toFixed(3)};--height-scale:${heightScale.toFixed(3)};`;

  onMount(() => {
    try {
      appWindow = getCurrentWindow();
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
      cleanup();
    };
  });

  async function init() {
    if (previewEnabled) {
      updateScale();
      return () => {};
    }

    await hydrateQuotaCache();
    void refreshQuota();
    const timer = window.setInterval(refreshQuota, refreshMs);
    void hydrateWindowState();
    void refreshAutostartState();

    const unlistenRefresh = await listenSafe("quota-refresh-requested", refreshQuota);
    const unlistenMode = await listenSafe<string>("mode-changed", (event) => {
      mode = event.payload === "large" ? "large" : "small";
      updateScale();
      void hydrateQuotaCache().then(() => {
        if (!hasUsableQuota(quota) || status !== "ready") {
          void refreshQuota();
        }
      });
      if (!hasUsableQuota(quota) || status !== "ready") {
        void refreshQuota();
      }
    });
    const unlistenTopmost = await listenSafe<boolean>("topmost-changed", (event) => {
      alwaysOnTop = event.payload;
    });
    const unlistenAutostart = await listenSafe("toggle-autostart-requested", toggleAutostart);

    return () => {
      window.clearInterval(timer);
      unlistenRefresh();
      unlistenMode();
      unlistenTopmost();
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
      mode = state.mode;
      alwaysOnTop = state.alwaysOnTop;
      updateScale();
    } catch {
      updateScale();
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

  async function refreshQuota() {
    if (isRefreshing) return;
    isRefreshing = true;
    try {
      const nextQuota = await withTimeout(
        invoke<QuotaSnapshot>("read_quota"),
        invokeTimeoutMs,
        "Codex 响应超时，请稍后刷新"
      );
      quota = normalizeQuota(nextQuota);
      status = "ready";
      errorText = "";
      writeCachedQuota(quota);
      await updateTrayQuota("ready", quota);
    } catch (error) {
      errorText = friendlyError(error);
      status = quota ? "stale" : "error";
      await updateTrayQuota(status, quota);
    } finally {
      isRefreshing = false;
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

  async function updateTrayQuota(nextStatus: "ready" | "stale" | "error", currentQuota: QuotaSnapshot | null) {
    try {
      await invoke("update_tray_quota", {
        state: {
          primaryRemaining: currentQuota?.primaryRemaining ?? null,
          secondaryRemaining: currentQuota?.secondaryRemaining ?? null,
          status: nextStatus
        }
      });
    } catch {
      // The panel should keep working even if the tray icon cannot be updated.
    }
  }

  async function switchMode(nextMode: "small" | "large") {
    mode = nextMode;
    updateScale();
    try {
      await invoke("set_mode", { mode: nextMode });
    } catch {
      // Keep the preview usable outside Tauri.
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
      primaryRemaining: 88,
      primaryReset: "6/11 00:12",
      secondaryRemaining: 21,
      secondaryReset: "6/11 09:09"
    });
  }

  function normalizeQuota(raw: RawQuota): QuotaSnapshot {
    return {
      status: readString(raw, "status") || "ready",
      limitName: readString(raw, "limitName", "limit_name") || "Codex",
      planType: readString(raw, "planType", "plan_type"),
      updatedAt: normalizeUpdatedAt(readString(raw, "updatedAt", "updated_at")),
      primaryRemaining: normalizePercent(readNumber(raw, "primaryRemaining", "primary_remaining")),
      primaryReset: normalizeReset(readString(raw, "primaryReset", "primary_reset")),
      secondaryRemaining: normalizePercent(readNumber(raw, "secondaryRemaining", "secondary_remaining")),
      secondaryReset: normalizeReset(readString(raw, "secondaryReset", "secondary_reset"))
    };
  }

  function normalizeUpdatedAt(value?: string) {
    const trimmed = typeof value === "string" ? value.trim() : "";
    if (trimmed && trimmed !== "--:--:--") {
      return trimmed;
    }
    return new Date().toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false
    });
  }

  function normalizePercent(value?: number | null) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return 0;
    return Math.round(Math.min(100, Math.max(0, numeric)));
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

  async function hydrateQuotaCache() {
    try {
      const cached = await invoke<RawQuota | null>("read_cached_quota");
      if (!cached || !hasQuotaPercentages(cached)) return;
      if (quota && status === "ready") return;
      quota = normalizeQuota(cached);
      status = "stale";
      writeCachedQuota(quota);
      await updateTrayQuota("stale", quota);
    } catch {
      // Frontend localStorage remains available as a secondary cache.
    }
  }

  function writeCachedQuota(nextQuota: QuotaSnapshot) {
    try {
      window.localStorage.setItem(quotaCacheKey, JSON.stringify(nextQuota));
    } catch {
      // Cache is a convenience only; live refresh remains authoritative.
    }
  }

  function makeQuotaWindows(currentQuota: QuotaSnapshot | null): QuotaWindow[] {
    if (!hasUsableQuota(currentQuota)) return [];
    return [
      {
        label: "5小时额度",
        remaining: currentQuota.primaryRemaining,
        reset: resetText(currentQuota.primaryReset)
      },
      {
        label: "周额度",
        remaining: currentQuota.secondaryRemaining,
        reset: resetText(currentQuota.secondaryReset)
      }
    ];
  }

  function colorClass(value?: number) {
    if (typeof value !== "number") return "danger";
    if (value <= 20) return "danger";
    if (value <= 50) return "warning";
    return "ok";
  }

  function resetText(value?: string) {
    if (!value || value === "unknown") return "--:--";
    return value;
  }

  function hasQuotaPercentages(raw: RawQuota) {
    return (
      readNumber(raw, "primaryRemaining", "primary_remaining") !== null &&
      readNumber(raw, "secondaryRemaining", "secondary_remaining") !== null
    );
  }

  function hasUsableQuota(currentQuota: QuotaSnapshot | null): currentQuota is QuotaSnapshot {
    return (
      !!currentQuota &&
      Number.isFinite(currentQuota.primaryRemaining) &&
      Number.isFinite(currentQuota.secondaryRemaining)
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
    if (hasUsableQuota(quota)) return null;
    if (status === "error") return "Codex: 读取失败";
    return "Codex: 正在读取";
  }

  function subtitle() {
    if (!quota) return "Codex / pro --:--:--";
    const plan = quota.planType ? ` / ${quota.planType}` : "";
    return `${quota.limitName || "Codex"}${plan} ${quota.updatedAt || "--:--:--"}`;
  }

  function statusText() {
    if (status === "error") return `读取失败: ${errorText}`;
    if (status === "stale") return `显示上次数据: ${errorText}`;
    if (!quota) return "正在读取";
    return `上次刷新 ${quota.updatedAt}`;
  }

  function friendlyError(error: unknown) {
    const raw = String(error || "").trim();
    if (!raw) return "读取失败";

    const lower = raw.toLowerCase();
    if (lower.includes("cannot find codex cli") || lower.includes("codex.exe")) {
      return "未找到 Codex，请先安装并登录 Codex，或设置 CODEX_QUOTA_CODEX_PATH";
    }
    if (lower.includes("cannot start codex app-server")) {
      return "无法启动 Codex app-server，请确认 Codex 已安装且可正常打开";
    }
    if (lower.includes("timed out") || lower.includes("响应超时")) {
      return "Codex 响应超时，请稍后刷新";
    }
    if (lower.includes("no rate limit data") || lower.includes("returned no rate limit")) {
      return "未读取到额度数据，请确认 Codex 已登录账号";
    }
    if (lower.includes("closed before responding")) {
      return "Codex app-server 未返回数据，请确认 Codex 当前可用";
    }
    return raw;
  }

  function updateScale() {
    if (mode === "small") {
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

  function rememberWindowState() {
    void invoke("remember_window_state").catch(() => {});
  }

  async function hidePanel() {
    try {
      await invoke("hide_panel");
    } catch {
      // Keep the preview usable outside Tauri.
    }
  }

  function startDrag(event: MouseEvent) {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button") || target.closest(".resize-handle")) return;
    if (event.detail >= 2) {
      event.preventDefault();
      void hidePanel();
      return;
    }
    if (!appWindow) return;
    event.preventDefault();
    void appWindow.startDragging().finally(rememberWindowState);
  }

  function startResize(event: PointerEvent) {
    if (event.button !== 0 || mode !== "large") return;
    if (!appWindow) return;
    event.preventDefault();
    event.stopPropagation();
    void appWindow.startResizeDragging("SouthEast").finally(rememberWindowState);
  }

  function showContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    void invoke("show_context_menu").catch(() => {});
  }
</script>

<svelte:window on:contextmenu={showContextMenu} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<main class={`shell ${isSmall ? "small-mode" : "large-mode"}`} style={scaleStyle} on:mousedown={startDrag}>
  <section class="compact-row">
    <span class="compact-summary">
      {#if compactSummaryText()}
        {compactSummaryText()}
      {:else if hasQuota && quota}
        <span>Codex: 5小时</span><span class={`compact-percent ${colorClass(quota.primaryRemaining)}`}>{quota.primaryRemaining}%</span>
        <span> / 周</span><span class={`compact-percent ${colorClass(quota.secondaryRemaining)}`}>{quota.secondaryRemaining}%</span>
      {/if}
    </span>
    <button class="compact-large" title="打开大窗" aria-label="打开大窗" on:click={() => switchMode("large")}>
      <svg class="icon window-icon" viewBox="0 0 24 24" aria-hidden="true">
        <rect x="7" y="7" width="10" height="10" rx="0.5"></rect>
      </svg>
    </button>
    <button class:spinning={isRefreshing} class="compact-refresh" title="立即刷新" aria-label="立即刷新" on:click={refreshQuota}>
      <span>↻</span>
    </button>
  </section>

  <header class="topbar">
    <div class="title-area">
      <p>{subtitle()}</p>
    </div>
    <div class="window-actions">
      <button title="切换到小窗" aria-label="切换到小窗" on:click={() => switchMode("small")}>
        <svg class="icon window-icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M8 7h9v9H8z"></path>
          <path d="M5 10h9v9H5z"></path>
        </svg>
      </button>
      <button title={alwaysOnTop ? "取消置顶" : "置顶"} aria-label={alwaysOnTop ? "取消置顶" : "置顶"} on:click={toggleTopmost}>
        <svg class={`icon ${alwaysOnTop ? "icon-solid" : ""}`} viewBox="0 0 24 24" aria-hidden="true">
          <path d="M14 3l7 7-2 2-2-2-4 4v5l-1 1-4-4-5 5-1-1 5-5-4-4 1-1h5l4-4-2-2 2-2z"></path>
        </svg>
      </button>
      <button class:spinning={isRefreshing} title="立即刷新" aria-label="立即刷新" on:click={refreshQuota}>
        <span>↻</span>
      </button>
    </div>
  </header>

  <section class="quota-list">
    {#if quotaWindows.length}
      {#each quotaWindows as item}
        <article class="quota-item">
          <div class="quota-line">
            <span class="quota-title">
              <span class="quota-label">{item.label}</span>
              <span class="quota-reset">重置 {item.reset}</span>
            </span>
            <span class="quota-facts">
              <strong class={colorClass(item.remaining)}>{item.remaining}%</strong>
            </span>
          </div>
          <div class="meter">
            <div class={`meter-fill ${colorClass(item.remaining)}`} style={`width:${item.remaining}%`}></div>
          </div>
        </article>
      {/each}
    {:else}
      <div class="empty">{errorText || "正在连接 Codex app-server"}</div>
    {/if}
  </section>

  <footer class="footer">
    <span class={`dot ${status}`}></span>
    <span>{statusText()}</span>
  </footer>
  <div class="resize-handle" aria-hidden="true" on:pointerdown={startResize}></div>
</main>

{#if toast}
  <div class="toast">{toast}</div>
{/if}

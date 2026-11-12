<script setup lang="ts">
import { ref, onMounted, computed, provide } from "vue";
import { useRouter } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { auth } from "./api";
import LockScreen from "./components/LockScreen.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import TitleBar from "./components/TitleBar.vue";
import { useResizable } from "./useResizable";
import { initTheme } from "./theme";
import { setContentProtection, getAppVersion, openUrl, forceBackup } from "./api";
import { checkForUpdate } from "./updater";

/** 投屏保护：默认开启，共享屏幕/录屏时窗口不可见（设置页可关） */
async function applyContentProtection() {
  try {
    await setContentProtection((localStorage.getItem("sn_content_protection") ?? "1") === "1");
  } catch (e) {
    console.warn("投屏保护设置失败", e);
  }
}

const router = useRouter();
const initialized = ref<boolean | null>(null);
const unlocked = ref(false);
const loading = ref(true);

/** 左侧主导航：更窄 + 可拖拽调宽 */
const { width: navWidth, onDown: onResizeDown } = useResizable("sn_nav_w", 156, 132, 0.35);

/** 主导航折叠（折叠后只留图标；默认折叠） */
const navCollapsed = ref((localStorage.getItem("sn_nav_collapsed") ?? "1") === "1");
function toggleNav() {
  navCollapsed.value = !navCollapsed.value;
  localStorage.setItem("sn_nav_collapsed", navCollapsed.value ? "1" : "0");
}
const confirmRef = ref<InstanceType<typeof ConfirmDialog> | null>(null);
/** 全局确认弹窗（替代原生 confirm，WebView 下原生行为不可靠） */
provide("appConfirm", (msg: string, okText?: string) =>
  confirmRef.value?.ask(msg, okText) ?? Promise.resolve(false)
);

/** 顶部主导航 */
const mainNavItems = [
  { path: "/notes", label: "笔记", icon: "📓" },
  { path: "/todos", label: "待办", icon: "☑️" },
  { path: "/passwords", label: "密码库", icon: "🔑" },
  { path: "/toolbox", label: "工具箱", icon: "🧰" },
];
/** 底部导航：回收站 / 设置（锁定固定在最下） */
const footNavItems = [
  { path: "/trash", label: "回收站", icon: "🗑️" },
  { path: "/settings", label: "设置", icon: "⚙️" },
];

const currentPath = computed(() => router.currentRoute.value.path);

function navigate(path: string) {
  router.push(path);
}

async function boot() {
  loading.value = true;
  try {
    initialized.value = await auth.isInitialized();
    if (initialized.value) {
      unlocked.value = await auth.isUnlocked();
    }
  } catch (e) {
    console.error("启动失败", e);
  } finally {
    loading.value = false;
  }
}

async function handleUnlocked() {
  // 首次创建主密码后 initialized 仍为 false，需同步置 true，
  // 否则渲染条件 initialized === false 仍成立，锁屏不消失（表现为"点了没反应"）
  initialized.value = true;
  unlocked.value = true;
}

/** 启动后静默检查更新：发现新版则提示（含数据风险提示），确认后先备份再跳转下载 */
async function checkUpdates() {
  try {
    const current = await getAppVersion();
    const info = await checkForUpdate(current);
    if (!info) return;
    const ok = await (confirmRef.value?.ask(
      `发现新版本 v${info.version}（当前 v${current}）。\n\n` +
        `更新内容：\n${info.notes || "（无说明）"}\n\n` +
        `⚠️ 升级前会先自动备份本地数据（滚动保留最近 3 份）。\n` +
        `本应用为离线本地应用，升级不会主动删除您的数据，\n` +
        `但建议您确认「设置」中的备份已开启。是否前往下载？`,
      "前往下载"
    ) ?? Promise.resolve(false));
    if (!ok) return;
    // 升级前强制备份
    try {
      await forceBackup();
    } catch (e) {
      console.warn("升级前备份失败", e);
    }
    await openUrl(info.url);
  } catch (e) {
    console.warn("更新检测异常", e);
  }
}

async function setupTrayListeners() {
  try {
    // 托盘菜单"新建笔记/新建待办" → 导航到对应视图
    await listen<string>("tray-navigate", (e) => {
      router.push(e.payload);
    });
    // 托盘菜单"锁定"（Rust 端已锁定密钥）→ 切回锁屏
    await listen("vault-locked", () => {
      unlocked.value = false;
    });
    // 窗口已隐藏到托盘的提示事件（预留，可扩展 toast）
    await listen("tray-minimized", () => {
      console.info("SecureNote 已最小化到系统托盘");
    });
  } catch (e) {
    console.warn("托盘事件监听不可用", e);
  }
}

onMounted(() => {
  initTheme();
  boot();
  setupTrayListeners();
  applyContentProtection();
  // 延迟几秒静默检查更新，避免与启动/解锁流程抢资源
  setTimeout(checkUpdates, 3000);
});
</script>

<template>
  <div v-if="loading" class="splash">
    <div class="logo">🔐</div>
    <div>SecureNote</div>
  </div>

  <LockScreen
    v-else-if="initialized === false || (initialized === true && !unlocked)"
    :initialized="initialized === true"
    @unlocked="handleUnlocked"
  />

  <div v-else class="app-shell">
    <TitleBar />
    <div class="app">
      <aside
        class="sidebar"
        :class="{ collapsed: navCollapsed }"
        :style="navCollapsed ? { width: '52px' } : { width: navWidth + 'px' }"
      >
      <!-- 窄栏（<150px）时只显示 LOGO 并居中，保证左右对称 -->
      <div v-if="!navCollapsed" class="brand" :class="{ narrow: navWidth < 150 }">
        <svg class="brand-logo" viewBox="0 0 48 48" width="26" height="26" aria-hidden="true">
          <defs>
            <linearGradient id="sn-logo-g" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stop-color="#4f6ef7" />
              <stop offset="1" stop-color="#7c5cf0" />
            </linearGradient>
          </defs>
          <rect x="2" y="2" width="44" height="44" rx="11" fill="url(#sn-logo-g)" />
          <!-- 笔记本本体 -->
          <rect x="12" y="11" width="22" height="26" rx="3.5" fill="#ffffff" opacity="0.96" />
          <rect x="12" y="11" width="6.5" height="26" rx="3.25" fill="#3848c8" />
          <line x1="24" y1="19.5" x2="30" y2="19.5" stroke="#4f6ef7" stroke-width="2.4" stroke-linecap="round" />
          <line x1="24" y1="25" x2="30" y2="25" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
          <line x1="24" y1="30.5" x2="27.5" y2="30.5" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
          <!-- 右下角小锁 -->
          <circle cx="33.5" cy="33.5" r="7.5" fill="#ffffff" opacity="0.18" />
          <rect x="30.2" y="32.6" width="6.6" height="5.4" rx="1.4" fill="#ffd977" />
          <path d="M31.4 32.6 v-1.7 a2.1 2.1 0 0 1 4.2 0 v1.7" fill="none" stroke="#ffd977" stroke-width="1.5" />
        </svg>
        <span v-if="navWidth >= 150" class="brand-name">个人笔记本</span>
      </div>
      <button v-else class="expand-btn" title="个人笔记本" @click="toggleNav">
        <svg viewBox="0 0 48 48" width="26" height="26" aria-hidden="true">
          <rect x="2" y="2" width="44" height="44" rx="11" fill="url(#sn-logo-g)" />
          <rect x="12" y="11" width="22" height="26" rx="3.5" fill="#ffffff" opacity="0.96" />
          <rect x="12" y="11" width="6.5" height="26" rx="3.25" fill="#3848c8" />
          <line x1="24" y1="19.5" x2="30" y2="19.5" stroke="#4f6ef7" stroke-width="2.4" stroke-linecap="round" />
          <line x1="24" y1="25" x2="30" y2="25" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
          <line x1="24" y1="30.5" x2="27.5" y2="30.5" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
        </svg>
      </button>
      <nav class="nav">
        <button
          v-for="item in mainNavItems"
          :key="item.path"
          class="nav-item"
          :class="{ active: currentPath === item.path }"
          :title="navCollapsed ? item.label : undefined"
          @click="navigate(item.path)"
        >
          <span class="icon">{{ item.icon }}</span>
          <span v-if="!navCollapsed">{{ item.label }}</span>
        </button>
      </nav>
      <!-- 底部：回收站 / 设置，锁定在最下 -->
      <div class="sidebar-foot">
        <button
          v-for="item in footNavItems"
          :key="item.path"
          class="nav-item"
          :class="{ active: currentPath === item.path }"
          :title="navCollapsed ? item.label : undefined"
          @click="navigate(item.path)"
        >
          <span class="icon">{{ item.icon }}</span>
          <span v-if="!navCollapsed">{{ item.label }}</span>
        </button>
        <div class="foot-sep"></div>
        <button
          class="nav-item lock"
          :title="navCollapsed ? '锁定' : undefined"
          @click="auth.lock().then(() => (unlocked = false))"
        >
          <span class="icon">🔒</span>
          <span v-if="!navCollapsed">锁定</span>
        </button>
      </div>
    </aside>
    <!-- 垂直居中的细长折叠把手：折叠时主导航只留图标 -->
    <div
      class="edge-handle"
      :title="navCollapsed ? '展开侧边栏' : '折叠侧边栏'"
      @click="toggleNav"
    >
      <span class="handle-grip">{{ navCollapsed ? "⟩" : "⟨" }}</span>
    </div>
    <div class="col-resizer" v-show="!navCollapsed" title="拖动调整侧边栏宽度" @mousedown="onResizeDown"></div>
    <main class="main">
      <router-view />
    </main>
    </div>
  </div>

  <ConfirmDialog ref="confirmRef" />
</template>

<style scoped>
.splash {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--muted);
}
.splash .logo {
  font-size: 48px;
}
.app-shell {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.app {
  flex: 1;
  display: flex;
  min-height: 0;
  overflow: hidden;
}
.sidebar {
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  overflow: hidden;
  background: var(--panel);
}
.col-resizer {
  width: 5px;
  cursor: col-resize;
  background: transparent;
  transition: background 0.15s;
  flex-shrink: 0;
}
.col-resizer:hover,
.col-resizer:active {
  background: var(--accent-soft);
}
.brand {
  display: flex;
  align-items: center;
  justify-content: center; /* LOGO 与文字整体居中，窄栏依旧对称 */
  gap: 9px;
  padding: 18px 12px 14px;
  white-space: nowrap;
  min-height: 58px;
}
/* 窄栏：仅 LOGO，居中显示 */
.brand.narrow {
  padding: 18px 6px 14px;
}
.collapse-btn {
  margin-left: auto;
  border: none;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  font-size: 14px;
  padding: 4px 6px;
  border-radius: 6px;
}
.collapse-btn:hover {
  background: var(--hover-strong);
  color: var(--text);
}
/* 图标固定宽度：折叠/展开时图标列对齐 */
.nav-item .icon {
  width: 22px;
  text-align: center;
  font-size: 16px;
  flex-shrink: 0;
}
.expand-btn {
  margin: 16px auto 8px;
  border: none;
  background: transparent;
  font-size: 20px;
  cursor: pointer;
  padding: 6px;
  border-radius: 8px;
  display: block;
}
.expand-btn:hover {
  background: var(--hover-strong);
}
/* 折叠后只留图标：居中对齐 */
.sidebar.collapsed .nav {
  padding: 8px 6px;
}
.sidebar.collapsed .nav-item {
  justify-content: center;
  gap: 0;
  padding: 10px 0;
}
.sidebar.collapsed .sidebar-foot {
  padding: 12px 6px;
}
.sidebar.collapsed .sidebar-foot .nav-item {
  justify-content: center;
}
.brand-logo {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
}
.brand-name {
  font-weight: 700;
  font-size: 16px;
}
.nav {
  flex: 1;
  padding: 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 8px;
  border: none;
  background: transparent;
  cursor: pointer;
  color: var(--text);
  font-size: 14px;
  text-align: left;
  transition: 0.15s;
}
.nav-item:hover {
  background: var(--hover-strong);
}
.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.sidebar-foot {
  padding: 8px 12px 12px;
  border-top: 1px solid var(--border);
}
.foot-sep {
  border-top: 1px dashed var(--border);
  margin: 6px 2px;
}
.sidebar-foot .nav-item {
  width: 100%;
}
.lock {
  color: var(--muted);
}
.main {
  flex: 1;
  overflow: hidden;
  display: flex;
}
</style>

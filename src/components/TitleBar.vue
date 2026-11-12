<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { quitApp } from "../api";
import { exportRunning, exportPercent, exportMessage, exportKind, exportFlash } from "../exportProgress";

/** 顶栏任务文案：按任务类型区分 */
const taskLabel = computed(() => {
  if (exportKind.value === "migrate-data") return "数据迁移中";
  if (exportKind.value === "migrate-backup") return "备份迁移中";
  return "导出中";
});

const appWindow = getCurrentWindow();
const maximized = ref(false);

async function refreshMaximized() {
  try {
    maximized.value = await appWindow.isMaximized();
  } catch {
    /* 忽略 */
  }
}

function minimize() {
  appWindow.minimize().catch(() => {});
}
function toggleMaximize() {
  appWindow.toggleMaximize().catch(() => {});
}
function closeWin() {
  // 走 Rust 端退出命令：立即隐藏窗口并退出（关闭不再自动备份，备份由手动/闲时自动完成）
  quitApp().catch(() => appWindow.close().catch(() => {}));
}

let unlisten: (() => void) | null = null;
onMounted(async () => {
  await refreshMaximized();
  try {
    unlisten = await appWindow.onResized(refreshMaximized);
  } catch {
    /* 忽略 */
  }
});
onBeforeUnmount(() => {
  unlisten?.();
});
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="tb-left" data-tauri-drag-region>
      <svg class="tb-logo" viewBox="0 0 48 48" width="20" height="20" aria-hidden="true">
        <defs>
          <linearGradient id="sn-tb-logo-g" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="#4f6ef7" />
            <stop offset="1" stop-color="#7c5cf0" />
          </linearGradient>
        </defs>
        <rect x="2" y="2" width="44" height="44" rx="11" fill="url(#sn-tb-logo-g)" />
        <rect x="12" y="11" width="22" height="26" rx="3.5" fill="#ffffff" opacity="0.96" />
        <rect x="12" y="11" width="6.5" height="26" rx="3.25" fill="#3848c8" />
        <line x1="24" y1="19.5" x2="30" y2="19.5" stroke="#4f6ef7" stroke-width="2.4" stroke-linecap="round" />
        <line x1="24" y1="25" x2="30" y2="25" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
        <line x1="24" y1="30.5" x2="27.5" y2="30.5" stroke="#aab6ff" stroke-width="2.4" stroke-linecap="round" />
      </svg>
      <span class="tb-title">个人笔记本</span>
    </div>

    <!-- 后台任务进度胶囊（全局可见：导出 / 数据迁移 / 备份迁移） -->
    <div v-if="exportRunning" class="tb-export" :title="exportMessage">
      <span class="tb-export-spinner"></span>
      <span class="tb-export-text">{{ taskLabel }} {{ exportPercent }}% · {{ exportMessage }}</span>
    </div>
    <div v-else-if="exportFlash" class="tb-export" :class="{ done: exportFlash.startsWith('✓') }">
      <span class="tb-export-text">{{ exportFlash }}</span>
    </div>

    <div class="tb-right">
      <span class="tb-sep"></span>
      <button class="tb-btn tb-ctrl" title="最小化" @click="minimize">
        <svg viewBox="0 0 12 12" width="12" height="12"><line x1="2" y1="6" x2="10" y2="6" stroke="currentColor" stroke-width="1.2" /></svg>
      </button>
      <button class="tb-btn tb-ctrl" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
        <svg v-if="!maximized" viewBox="0 0 12 12" width="12" height="12"><rect x="2" y="2" width="8" height="8" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
        <svg v-else viewBox="0 0 12 12" width="12" height="12"><rect x="1.5" y="3.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2" /><path d="M3.5 3.5 V1.5 h7 v7 h-2" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
      </button>
      <button class="tb-btn tb-ctrl tb-close" title="关闭" @click="closeWin">
        <svg viewBox="0 0 12 12" width="12" height="12"><path d="M2 2 l8 8 M10 2 l-8 8" stroke="currentColor" stroke-width="1.2" /></svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  user-select: none;
  -webkit-app-region: drag;
}
.tb-left {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-left: 14px;
  height: 100%;
}
.tb-logo {
  flex-shrink: 0;
}
.tb-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
/* 后台导出进度胶囊 */
.tb-export {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 4px 12px;
  border-radius: 999px;
  background: var(--accent-soft, rgba(79, 110, 247, 0.12));
  color: var(--accent, #4f6ef7);
  font-size: 12px;
  -webkit-app-region: no-drag;
  max-width: 46%;
  overflow: hidden;
  user-select: none;
}
.tb-export.done {
  background: rgba(63, 185, 80, 0.14);
  color: #2da44e;
}
.tb-export-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tb-export-spinner {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: tb-spin 0.8s linear infinite;
}
@keyframes tb-spin {
  to {
    transform: rotate(360deg);
  }
}
.tb-right {
  display: flex;
  align-items: center;
  height: 100%;
  -webkit-app-region: no-drag;
}
.tb-btn {
  border: none;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  font-size: 12px;
  padding: 0 10px;
  height: 100%;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border-radius: 0;
  transition: 0.15s;
}
.tb-btn:hover {
  background: var(--hover-strong);
  color: var(--text);
}
.tb-sep {
  width: 1px;
  height: 18px;
  background: var(--border);
  margin: 0 2px;
}
.tb-ctrl {
  width: 46px;
  justify-content: center;
  padding: 0;
}
.tb-close:hover {
  background: #e5484d;
  color: #fff;
}
</style>

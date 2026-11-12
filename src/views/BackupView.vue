<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { inject } from "vue";
import {
  listBackupPoints,
  deleteBackupPoint,
  rollbackToBackup,
  forceBackup,
  type BackupPointInfo,
} from "../api";
import { exportRunning } from "../exportProgress";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const points = ref<BackupPointInfo[]>([]);
const toast = ref("");
const loading = ref(false);

function showToast(msg: string) {
  toast.value = msg;
  setTimeout(() => (toast.value = ""), 4000);
}

/** 按时间倒序显示（最新的在前） */
const sortedPoints = computed(() => [...points.value].sort((a, b) => b.created_at.localeCompare(a.created_at)));

/** 统计 */
const fullCount = computed(() => points.value.filter((p) => p.kind === "full").length);
const incCount = computed(() => points.value.filter((p) => p.kind === "incremental").length);
const totalSize = computed(() => points.value.reduce((s, p) => s + p.size, 0));

function fmtTime(iso: string) {
  const d = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function fmtSize(bytes: number) {
  if (bytes >= 1024 * 1024 * 1024) return (bytes / 1024 / 1024 / 1024).toFixed(2) + " GB";
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(1) + " MB";
  if (bytes >= 1024) return (bytes / 1024).toFixed(1) + " KB";
  return bytes + " B";
}

function kindLabel(p: BackupPointInfo) {
  return p.kind === "full" ? "全量" : "增量";
}

async function load() {
  loading.value = true;
  try {
    points.value = await listBackupPoints();
  } catch (e: any) {
    showToast("加载失败: " + String(e?.message || e));
  } finally {
    loading.value = false;
  }
}

/** 手动全量备份 */
async function doFullBackup() {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  try {
    const filename = await forceBackup();
    showToast(`✓ 全量备份完成：${filename}`);
    await load();
  } catch (e: any) {
    showToast("备份失败: " + String(e?.message || e));
  }
}

/** 回滚到指定备份点 */
async function doRollback(p: BackupPointInfo) {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  const ok = await appConfirm(
    `回滚到「${fmtTime(p.created_at)}」（${kindLabel(p)}备份）？\n\n` +
      `· 回滚前会自动全量备份当前状态（可再滚回来）\n` +
      `· 回滚会覆盖当前数据为所选时间点的状态\n` +
      `· 完成后需重新解锁查看\n\n是否继续？`,
    "回滚"
  );
  if (!ok) return;
  try {
    await rollbackToBackup(p.id);
    showToast("✓ 回滚已开始，可在顶部工具栏查看进度，完成后自动刷新");
    // 监听完成后刷新
    setTimeout(() => window.location.reload(), 3000);
  } catch (e: any) {
    showToast("回滚失败: " + String(e?.message || e));
  }
}

/** 删除备份点 */
async function doDelete(p: BackupPointInfo) {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  const cascadeNote =
    p.kind === "full"
      ? "\n\n⚠️ 删除全量备份会同时删除其后依赖它的所有增量备份。"
      : "";
  const ok = await appConfirm(
    `删除「${fmtTime(p.created_at)}」（${kindLabel(p)}备份，${fmtSize(p.size)}）？` +
      cascadeNote +
      "\n\n删除后无法恢复，请谨慎操作。",
    "删除"
  );
  if (!ok) return;
  try {
    await deleteBackupPoint(p.id);
    showToast("✓ 已删除");
    await load();
  } catch (e: any) {
    showToast("删除失败: " + String(e?.message || e));
  }
}

onMounted(load);
</script>

<template>
  <div class="backup-mgr">
    <div class="head">
      <h2>备份管理</h2>
      <button class="btn primary" :disabled="exportRunning" @click="doFullBackup">
        {{ exportRunning ? "⏳ 任务进行中…" : "💾 立即全量备份" }}
      </button>
    </div>

    <div class="stats">
      <div class="stat"><span class="n">{{ fullCount }}</span><span class="l">全量备份</span></div>
      <div class="stat"><span class="n">{{ incCount }}</span><span class="l">增量备份</span></div>
      <div class="stat"><span class="n">{{ fmtSize(totalSize) }}</span><span class="l">总占用</span></div>
    </div>

    <div class="hint">
      备份采用「全量 + 增量」滚动管理：全量是最多保留 3 份的完整快照，增量记录每次变化。
      增量累计到一定数量会自动建议全量备份。可随时回滚到任意备份点，或手动删除不再需要的备份。
    </div>

    <div v-if="loading" class="empty">加载中…</div>
    <div v-else-if="sortedPoints.length === 0" class="empty">
      还没有备份记录，点击右上角「立即全量备份」创建第一份。
    </div>
    <div v-else class="list">
      <div v-for="p in sortedPoints" :key="p.id" class="point" :class="p.kind">
        <div class="badge" :class="p.kind">{{ kindLabel(p) }}</div>
        <div class="meta">
          <div class="time">{{ fmtTime(p.created_at) }}</div>
          <div class="sub">
            {{ fmtSize(p.size) }}
            <span v-if="p.change_count > 0"> · 变更 {{ p.change_count }} 条</span>
          </div>
        </div>
        <div class="actions">
          <button class="btn" @click="doRollback(p)">↩ 回滚</button>
          <button class="btn danger" @click="doDelete(p)">删除</button>
        </div>
      </div>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.backup-mgr {
  flex: 1;
  height: 100%;
  overflow-y: auto;
  background: var(--panel);
  padding: 24px;
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}
.head h2 {
  margin: 0;
}
.btn {
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--text);
  border-radius: 8px;
  padding: 7px 14px;
  cursor: pointer;
  font-size: 13px;
  transition: 0.15s;
}
.btn:hover {
  border-color: var(--accent);
}
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn.primary {
  background: var(--accent, #4f6ef7);
  border-color: var(--accent, #4f6ef7);
  color: #fff;
}
.btn.danger {
  color: #e5484d;
  border-color: rgba(229, 72, 77, 0.4);
}
.btn.danger:hover {
  background: rgba(229, 72, 77, 0.1);
  border-color: #e5484d;
}
.stats {
  display: flex;
  gap: 12px;
  margin-bottom: 16px;
}
.stat {
  flex: 1;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.stat .n {
  font-size: 22px;
  font-weight: 700;
  color: var(--accent, #4f6ef7);
}
.stat .l {
  font-size: 12px;
  color: var(--muted);
}
.hint {
  font-size: 12px;
  color: var(--muted);
  line-height: 1.6;
  margin-bottom: 16px;
  padding: 10px 14px;
  background: var(--card);
  border: 1px dashed var(--border);
  border-radius: 10px;
}
.empty {
  text-align: center;
  color: var(--muted);
  padding: 48px 0;
  font-size: 14px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.point {
  display: flex;
  align-items: center;
  gap: 14px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 12px 16px;
}
.badge {
  flex-shrink: 0;
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 600;
}
.badge.full {
  background: rgba(79, 110, 247, 0.14);
  color: #4f6ef7;
}
.badge.incremental {
  background: rgba(63, 185, 80, 0.14);
  color: #2da44e;
}
.meta {
  flex: 1;
  min-width: 0;
}
.meta .time {
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
}
.meta .sub {
  font-size: 12px;
  color: var(--muted);
  margin-top: 2px;
}
.actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.toast {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  background: var(--text);
  color: var(--panel);
  padding: 10px 18px;
  border-radius: 8px;
  font-size: 13px;
  z-index: 1000;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.2);
}
</style>

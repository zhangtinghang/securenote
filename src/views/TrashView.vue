<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { trash } from "../api";
import { inject } from "vue";
import type { Note, Todo, PasswordEntry } from "../types";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const tab = ref<"note" | "todo" | "password">("note");
const notes = ref<Note[]>([]);
const todos = ref<Todo[]>([]);
const passwords = ref<PasswordEntry[]>([]);
const toast = ref("");

const items = computed(() => {
  if (tab.value === "note") return notes.value.map((n) => ({ id: n.id, title: n.title || "无标题", time: n.deleted_at }));
  if (tab.value === "todo") return todos.value.map((t) => ({ id: t.id, title: t.title, time: t.deleted_at }));
  return passwords.value.map((p) => ({ id: p.id, title: p.name, time: p.deleted_at }));
});

const totalCount = computed(() => notes.value.length + todos.value.length + passwords.value.length);

function showToast(msg: string) {
  toast.value = msg;
  setTimeout(() => (toast.value = ""), 2500);
}

async function load() {
  // 进入时顺带执行一次过期清理（15 天）
  try {
    const purged = await trash.purgeExpired();
    if (purged > 0) showToast(`已自动清理 ${purged} 条超过 15 天的条目`);
  } catch {
    /* 未解锁等场景忽略 */
  }
  const data = await trash.list();
  notes.value = data.notes;
  todos.value = data.todos;
  passwords.value = data.passwords;
}

async function restore(id: string) {
  await trash.restore(tab.value, id);
  await load();
  showToast("✓ 已恢复");
}

async function purge(id: string, title: string) {
  const ok = await appConfirm(
    `彻底删除「${title}」？\n物理删除后无法恢复，请谨慎操作`,
    "彻底删除"
  );
  if (!ok) return;
  await trash.purge(tab.value, id);
  await load();
  showToast("已彻底删除");
}

async function emptyAll() {
  if (!totalCount.value) return;
  const ok = await appConfirm(
    `清空回收站全部 ${totalCount.value} 条？\n物理删除后无法恢复`,
    "全部清除"
  );
  if (!ok) return;
  const n = await trash.empty();
  await load();
  showToast(`已清空 ${n} 条`);
}

function fmtDeleted(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const days = Math.floor((Date.now() - d.getTime()) / 86400000);
  const remain = Math.max(0, 15 - days);
  return `${d.toLocaleString("zh-CN")} · ${remain} 天后自动清除`;
}

onMounted(load);
</script>

<template>
  <div class="trash">
    <div class="head">
      <h2>🗑️ 回收站</h2>
      <span class="tip">删除的条目保留 15 天，到期自动物理清除</span>
      <span class="spacer"></span>
      <button class="btn danger" :disabled="!totalCount" @click="emptyAll">清空回收站</button>
    </div>

    <div class="tabs">
      <button :class="{ on: tab === 'note' }" @click="tab = 'note'">📓 笔记 ({{ notes.length }})</button>
      <button :class="{ on: tab === 'todo' }" @click="tab = 'todo'">☑️ 待办 ({{ todos.length }})</button>
      <button :class="{ on: tab === 'password' }" @click="tab = 'password'">🔑 密码 ({{ passwords.length }})</button>
    </div>

    <div class="list">
      <div v-for="item in items" :key="item.id" class="item">
        <div class="info">
          <div class="title">{{ item.title }}</div>
          <div class="meta">删除于 {{ fmtDeleted(item.time) }}</div>
        </div>
        <div class="actions">
          <button class="btn sm" @click="restore(item.id)">↩ 恢复</button>
          <button class="btn sm danger" @click="purge(item.id, item.title)">彻底删除</button>
        </div>
      </div>
      <div v-if="!items.length" class="empty">回收站是空的</div>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.trash {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
  overflow: hidden;
  background: var(--panel);
}
.head {
  display: flex;
  align-items: center;
  gap: 12px;
}
.head h2 {
  font-size: 18px;
}
.tip {
  font-size: 12px;
  color: var(--muted);
}
.spacer {
  flex: 1;
}
.btn.sm {
  padding: 5px 10px;
  font-size: 12px;
}
.tabs {
  display: flex;
  gap: 6px;
  margin: 16px 0;
}
.tabs button {
  padding: 7px 14px;
  border: 1px solid var(--border);
  background: transparent;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--muted);
  transition: 0.15s;
}
.tabs button.on {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent);
  font-weight: 600;
}
.list {
  flex: 1;
  overflow-y: auto;
}
.item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 8px;
}
.info {
  flex: 1;
  min-width: 0;
}
.title {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.meta {
  font-size: 12px;
  color: var(--muted);
  margin-top: 2px;
}
.actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.empty {
  text-align: center;
  color: var(--muted);
  padding: 60px;
}
.toast {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  background: #1f2329;
  color: #fff;
  padding: 10px 18px;
  border-radius: 8px;
  font-size: 13px;
  z-index: 200;
}
</style>

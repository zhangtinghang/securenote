<script setup lang="ts">
import { ref, onMounted, computed, inject, watch } from "vue";
import { passwords, openUrl, pickPasswordFile, saveTemplate } from "../api";
import type { PasswordEntry } from "../types";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { base64ToBytes, bytesToBase64 } from "../logic";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const entries = ref<PasswordEntry[]>([]);
const search = ref("");
const showForm = ref(false);
const editing = ref<PasswordEntry | null>(null);
const ctxMenu = ref<{ x: number; y: number; entry: PasswordEntry } | null>(null);
const toast = ref("");
const importingBatch = ref(false);
/** 当前筛选的标签，空串 = 全部（默认不分组，平铺展示） */
const activeTag = ref("");

const form = ref({
  name: "",
  username: "",
  password: "",
  url: "",
  note: "",
  tags: [] as string[],
});

/** chips 输入框的草稿文本（未回车确认的部分） */
const tagDraft = ref("");
const tagInputEl = ref<HTMLInputElement | null>(null);

/** 标签配色：按名称 hash 稳定取色（表单 chips / 筛选条 / 卡片共用） */
const CHIP_COLORS: [string, string][] = [
  ["#e3ecfe", "#2f5fd0"], // 蓝
  ["#fde5e3", "#bf3a30"], // 红
  ["#e2f3e6", "#1e7a44"], // 绿
  ["#fdf1dc", "#a86807"], // 橙
  ["#f1e6fd", "#7433c2"], // 紫
  ["#e0f4f8", "#0f7a8d"], // 青
  ["#fde7f1", "#b82d7d"], // 粉
  ["#edeffc", "#4f6ef7"], // 靛
  ["#e9f6e9", "#3aa76d"], // 草绿
  ["#fdeadd", "#c05e17"], // 棕
];

function tagColor(tag: string): [string, string] {
  let h = 0;
  for (let i = 0; i < tag.length; i++) h = (h * 31 + tag.charCodeAt(i)) >>> 0;
  return CHIP_COLORS[h % CHIP_COLORS.length];
}

/** 文本 → 标签数组：空格/逗号/分号（中英文）皆可拆分，去重 */
function splitTags(raw: string): string[] {
  return [...new Set(
    raw
      .split(/[,，;；\s]+/)
      .map((s) => s.trim())
      .filter(Boolean)
  )];
}

/** 添加标签（输入/粘贴统一入口，自动拆分、去重） */
function addTags(raw: string) {
  for (const t of splitTags(raw)) {
    if (!form.value.tags.includes(t)) form.value.tags.push(t);
  }
  tagDraft.value = "";
}

function removeTag(idx: number) {
  form.value.tags.splice(idx, 1);
}

/** 输入框键盘：回车/逗号生成 chip，退格删除最后一个 */
function onTagKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" || e.key === "," || e.key === "，") {
    e.preventDefault();
    if (tagDraft.value.trim()) addTags(tagDraft.value);
  } else if (e.key === "Backspace" && !tagDraft.value && form.value.tags.length) {
    form.value.tags.pop();
  }
}

/** 粘贴：含分隔符则整段拆分为多个标签 */
function onTagPaste(e: ClipboardEvent) {
  const text = e.clipboardData?.getData("text") || "";
  if (text && /[,，;；\s]/.test(text)) {
    e.preventDefault();
    addTags(text);
  }
}

function focusTagInput() {
  tagInputEl.value?.focus();
}

/** 点击标签 chip：加入/移出表单标签（点选式，不用手敲） */
function toggleFormTag(tag: string) {
  const idx = form.value.tags.indexOf(tag);
  if (idx >= 0) form.value.tags.splice(idx, 1);
  else form.value.tags.push(tag);
}

/** 所有已用标签（用于筛选条与输入建议） */
const allTags = computed(() => {
  const set = new Set<string>();
  entries.value.forEach((e) => e.tags?.forEach((t) => set.add(t)));
  return [...set].sort((a, b) => a.localeCompare(b, "zh"));
});

const filtered = computed(() => {
  let list = entries.value;
  if (activeTag.value) {
    list = list.filter((e) => e.tags?.includes(activeTag.value));
  }
  const q = search.value.toLowerCase();
  if (!q) return list;
  return list.filter(
    (e) =>
      e.name.toLowerCase().includes(q) ||
      e.username.toLowerCase().includes(q) ||
      e.url.toLowerCase().includes(q) ||
      e.tags?.some((t) => t.toLowerCase().includes(q))
  );
});

/* ---------- 卡片二维网格拖拽排序（横向 + 纵向） ---------- */

/** 用户手动拖拽的卡片顺序（localStorage 持久化，未记录的新卡片排最前） */
const ORDER_KEY = "sn_passwords_order";
const orderIds = ref<string[]>(JSON.parse(localStorage.getItem(ORDER_KEY) || "[]"));

/** 渲染源：先按 orderIds 重排 entries，再应用标签/搜索筛选 */
const pwdView = ref<PasswordEntry[]>([]);

function applyOrder() {
  const pos = new Map(orderIds.value.map((id, i) => [id, i]));
  // 未记录的新条目排最前（与笔记一致）
  entries.value = [...entries.value].sort(
    (a, b) => (pos.get(a.id) ?? -1) - (pos.get(b.id) ?? -1)
  );
}

function syncPwdView() {
  pwdView.value = filtered.value;
}

watch([entries, activeTag, search], () => {
  if (!pwdDragState) syncPwdView();
});

const pwdDragIdx = ref<number | null>(null);
interface PwdDragState {
  itemEl: HTMLElement;
  shadow: HTMLElement;
  offsetX: number;
  offsetY: number;
}
let pwdDragState: PwdDragState | null = null;

/** 卡片容器内可见项（排除影子），按 pwdView 数组顺序收集 */
function visiblePwdItems(): HTMLElement[] {
  const box = pwdDragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const e of pwdView.value) {
    const el = box.querySelector(`.card[data-id="${e.id}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

/** 按鼠标坐标计算二维网格中的目标索引（含被拖项），取值 0..n */
function pwdIndexFromPoint(x: number, y: number): number {
  const items = visiblePwdItems();
  if (!items.length) return 0;
  const rects = items.map((el) => el.getBoundingClientRect());

  // 按 Y 中心聚类成"行"：相邻卡片 Y 中心差 < 半卡高视为同一行
  const centers = rects.map((r) => ({ cy: r.top + r.height / 2, cx: r.left + r.width / 2 }));
  const rowOf = new Array(centers.length).fill(0);
  const rowCenters: number[] = [];
  for (let i = 0; i < centers.length; i++) {
    let assigned = -1;
    for (let k = 0; k < rowCenters.length; k++) {
      if (Math.abs(centers[i].cy - rowCenters[k]) < rects[i].height * 0.5) {
        assigned = k;
        break;
      }
    }
    if (assigned < 0) {
      assigned = rowCenters.length;
      rowCenters.push(centers[i].cy);
    }
    rowOf[i] = assigned;
  }
  // 每行按 X 排序 → 得到该行内卡片的全局索引顺序
  const rowCount = rowCenters.length;
  const rows: number[][] = Array.from({ length: rowCount }, () => []);
  for (let i = 0; i < centers.length; i++) rows[rowOf[i]].push(i);
  rows.forEach((r) => r.sort((a, b) => centers[a].cx - centers[b].cx));

  // 拍平为一维顺序（全局索引 → pwdView 索引）
  const flat: number[] = [];
  for (const r of rows) flat.push(...r);

  // 鼠标所在行：Y 中心最接近鼠标的行
  let mouseRow = 0;
  let md = Infinity;
  for (let k = 0; k < rowCount; k++) {
    const d = Math.abs(y - rowCenters[k]);
    if (d < md) {
      md = d;
      mouseRow = k;
    }
  }

  // 该行内：数 X 中心在鼠标左侧的卡片数 = 行内插入位
  const mouseRowItems = rows[mouseRow];
  let colBefore = 0;
  for (const idx of mouseRowItems) {
    if (x > centers[idx].cx) colBefore++;
  }

  // 全局插入点 = 所有行序在 mouseRow 之前的卡片数 + 行内 colBefore
  let before = 0;
  for (let k = 0; k < mouseRow; k++) before += rows[k].length;
  let target = before + colBefore;
  return Math.min(Math.max(target, 0), items.length);
}

let pwdPress: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onCardDown(e: PointerEvent, i: number) {
  if (pwdDragState) return;
  const t = e.target as HTMLElement;
  if (t.closest("input, button, a")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".card") as HTMLElement;
  if (!itemEl) return;
  pwdPress = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onPwdPressMove);
  window.addEventListener("pointerup", onPwdPressUp);
  window.addEventListener("pointercancel", onPwdPressUp);
}

function onPwdPressMove(e: PointerEvent) {
  if (!pwdPress) return;
  if (Math.hypot(e.clientX - pwdPress.x, e.clientY - pwdPress.y) < 6) return;
  const { i, itemEl } = pwdPress;
  releasePwdPress();
  startPwdDrag(e, i, itemEl);
}

function onPwdPressUp() {
  releasePwdPress();
}

function releasePwdPress() {
  pwdPress = null;
  window.removeEventListener("pointermove", onPwdPressMove);
  window.removeEventListener("pointerup", onPwdPressUp);
  window.removeEventListener("pointercancel", onPwdPressUp);
}

function startPwdDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (pwdDragState || pwdView.value.length < 2) return;
  document.body.style.userSelect = "none";
  document.body.classList.add("pwd-dragging");
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  // 用 transform 定位影子，规避重排/缩放带来的抖动
  shadow.style.left = "0px";
  shadow.style.top = "0px";
  shadow.style.transform = `translate3d(${rect.left}px, ${rect.top}px, 0) scale(1.03)`;
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  pwdDragState = {
    itemEl,
    shadow,
    offsetX: e.clientX - rect.left,
    offsetY: e.clientY - rect.top,
  };
  pwdDragIdx.value = i;
  window.addEventListener("pointermove", onPwdPointerMove);
  window.addEventListener("pointerup", onPwdPointerUp);
  window.addEventListener("pointercancel", onPwdPointerUp);
}

function onPwdPointerMove(e: PointerEvent) {
  if (!pwdDragState) return;
  // 影子跟随（含横向+纵向），transform 定位跟手不偏移
  const x = e.clientX - pwdDragState.offsetX;
  const y = e.clientY - pwdDragState.offsetY;
  pwdDragState.shadow.style.transform = `translate3d(${x}px, ${y}px, 0) scale(1.03)`;
  const from = pwdDragIdx.value!;
  const target = pwdIndexFromPoint(e.clientX, e.clientY);
  if (target === from) return;
  const arr = pwdView.value;
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  pwdDragIdx.value = to;
}

function onPwdPointerUp() {
  window.removeEventListener("pointermove", onPwdPointerMove);
  window.removeEventListener("pointerup", onPwdPointerUp);
  window.removeEventListener("pointercancel", onPwdPointerUp);
  document.body.style.userSelect = "";
  document.body.classList.remove("pwd-dragging");
  if (!pwdDragState) return;
  pwdDragState.shadow.remove();
  pwdDragState.itemEl.classList.remove("drag-placeholder");
  pwdDragState = null;
  if (pwdDragIdx.value === null) return;
  pwdDragIdx.value = null;
  // 把当前视图顺序写回全量 orderIds 并持久化
  const fIds = pwdView.value.map((e) => e.id);
  const fSet = new Set(fIds);
  const rest = orderIds.value.filter((id) => !fSet.has(id));
  orderIds.value = [...fIds, ...rest];
  localStorage.setItem(ORDER_KEY, JSON.stringify(orderIds.value));
}

async function load() {
  entries.value = await passwords.list();
  applyOrder();
  syncPwdView();
}

async function create() {
  if (!form.value.name.trim()) return;
  const entry = await passwords.create({
    name: form.value.name,
    username: form.value.username,
    password: form.value.password,
    url: form.value.url,
    note: form.value.note,
    tags: form.value.tags,
  });
  entries.value.unshift(entry);
  resetForm();
}

async function update() {
  if (!editing.value) return;
  const updated = await passwords.update({
    ...editing.value,
    tags: form.value.tags,
  });
  const idx = entries.value.findIndex((e) => e.id === updated.id);
  if (idx >= 0) entries.value[idx] = updated;
  editing.value = null;
  showForm.value = false;
}

async function remove(entry: PasswordEntry) {
  const ok = await appConfirm(
    `删除「${entry.name}」？\n删除后进入回收站，保留 15 天`,
    "删除"
  );
  if (!ok) return;
  await passwords.remove(entry.id);
  entries.value = entries.value.filter((e) => e.id !== entry.id);
  ctxMenu.value = null;
}

function resetForm() {
  form.value = { name: "", username: "", password: "", url: "", note: "", tags: [] };
  tagDraft.value = "";
  showForm.value = false;
  editing.value = null;
}

// 点击卡片：复制密码
async function copyPassword(entry: PasswordEntry) {
  ctxMenu.value = null; // 无论成功失败都收起右键菜单（修复 hover 残留）
  try {
    const pwd = await passwords.reveal(entry.id);
    await writeText(pwd);
    showToast(`✓ 已复制「${entry.name}」的密码，30 秒后自动清空剪贴板`);
    // 30 秒后清空剪贴板
    setTimeout(async () => {
      try {
        await writeText("");
      } catch {}
    }, 30000);
  } catch (e: any) {
    showToast("复制失败: " + String(e));
  }
}

async function copyUsername(entry: PasswordEntry) {
  await writeText(entry.username);
  showToast("✓ 已复制用户名");
  ctxMenu.value = null;
}

async function openUrlInBrowser(entry: PasswordEntry) {
  if (entry.url) {
    try {
      await openUrl(entry.url);
    } catch (e: any) {
      showToast("打开失败: " + String(e?.message || e));
    }
  }
  ctxMenu.value = null;
}

async function generate() {
  form.value.password = await passwords.generate(16, true, true, true, true);
}

function showToast(msg: string) {
  toast.value = msg;
  setTimeout(() => (toast.value = ""), 2500);
}

/** 下载 Excel 导入模板 */
async function downloadTemplate() {
  try {
    const XLSX = await import("xlsx");
    const aoa = [
      ["名称", "用户名", "密码", "网址", "备注", "标签（用分号分隔，可留空）"],
      ["示例-GitHub", "your@email.com", "p@ssw0rd", "https://github.com", "示例备注，导入后可删除此行", "工作;代码"],
    ];
    const ws = XLSX.utils.aoa_to_sheet(aoa);
    (ws as any)["!cols"] = [{ wch: 22 }, { wch: 26 }, { wch: 22 }, { wch: 32 }, { wch: 26 }, { wch: 20 }];
    const wb = XLSX.utils.book_new();
    XLSX.utils.book_append_sheet(wb, ws, "密码");
    const out = XLSX.write(wb, { bookType: "xlsx", type: "array" }) as ArrayBuffer;
    await saveTemplate(bytesToBase64(new Uint8Array(out)));
    showToast("✓ 模板已保存，填好后点「批量导入」");
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg !== "已取消保存") showToast("保存模板失败: " + msg);
  }
}

/** 批量导入（xlsx/csv）：所有字段自动剔除首尾空格，密码同样处理 */
async function importBatch() {
  try {
    const [, b64] = await pickPasswordFile();
    importingBatch.value = true;
    const XLSX = await import("xlsx");
    const wb = XLSX.read(base64ToBytes(b64), { type: "array" });
    const sheet = wb.Sheets[wb.SheetNames[0]];
    if (!sheet) {
      showToast("文件中没有工作表");
      return;
    }
    const rows = XLSX.utils.sheet_to_json<Record<string, unknown>>(sheet, { defval: "" });
    const str = (v: unknown) => String(v ?? "").trim();
    let count = 0;
    let skipped = 0;
    for (const row of rows) {
      const name = str(row["名称"] ?? row["名字"] ?? row["name"]);
      const password = str(row["密码"] ?? row["password"]);
      if (!name || !password) {
        skipped++;
        continue;
      }
      const username = str(row["用户名"] ?? row["账号"] ?? row["username"]);
      const url = str(row["网址"] ?? row["URL"] ?? row["url"]);
      const note = str(row["备注"] ?? row["note"]);
      const tags = str(row["标签"] ?? row["tags"])
        .split(/[;；,，]+/)
        .map((s) => s.trim())
        .filter(Boolean);
      await passwords.create({ name, username, password, url, note, tags });
      count++;
    }
    await load();
    showToast(
      `✓ 导入 ${count} 条${skipped ? `，跳过 ${skipped} 条（缺名称或密码）` : ""}（首尾空格已自动剔除）`
    );
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg !== "已取消导入") showToast("导入失败: " + msg);
  } finally {
    importingBatch.value = false;
  }
}

function openContextMenu(e: MouseEvent, entry: PasswordEntry) {
  e.preventDefault();
  ctxMenu.value = { x: e.clientX, y: e.clientY, entry };
}

function startEdit(entry: PasswordEntry) {
  editing.value = { ...entry };
  form.value = {
    name: entry.name,
    username: entry.username,
    password: entry.password,
    url: entry.url,
    note: entry.note,
    tags: [...(entry.tags || [])],
  };
  tagDraft.value = "";
  showForm.value = true;
  ctxMenu.value = null;
}

function closeCtx(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".ctx-menu")) {
    ctxMenu.value = null;
  }
}

onMounted(() => {
  load();
  document.addEventListener("click", closeCtx);
});
</script>

<template>
  <div class="passwords" @click="closeCtx">
    <div class="content">
      <div class="toolbar">
        <button class="btn primary" @click="showForm = !showForm">＋ 新建密码</button>
        <button class="btn" :disabled="importingBatch" title="从 Excel/CSV 批量导入" @click="importBatch">
          {{ importingBatch ? "导入中…" : "📥 批量导入" }}
        </button>
        <button class="btn" title="下载 Excel 模板（名称/用户名/密码/网址/备注/标签）" @click="downloadTemplate">📄 模板</button>
        <input v-model="search" class="search" placeholder="搜索名称 / 用户名 / 网址 / 标签..." />
      </div>

      <!-- 标签筛选：默认平铺，点标签即过滤 -->
      <div v-if="allTags.length" class="tag-bar">
        <button class="tag-chip" :class="{ on: !activeTag }" @click="activeTag = ''">全部</button>
        <button
          v-for="t in allTags"
          :key="t"
          class="tag-chip"
          :class="{ on: activeTag === t }"
          :style="activeTag === t ? {} : { background: tagColor(t)[0], color: tagColor(t)[1] }"
          @click="activeTag = activeTag === t ? '' : t"
        >
          #{{ t }}
        </button>
      </div>

      <div v-if="showForm" class="form">
        <h3>{{ editing ? "编辑密码" : "新建密码" }}</h3>
        <div class="row">
          <div>
            <label>名称</label>
            <input v-model="form.name" placeholder="如：GitHub" />
          </div>
          <div>
            <label>用户名</label>
            <input v-model="form.username" placeholder="账号 / 邮箱" />
          </div>
        </div>
        <div class="row">
          <div>
            <label>密码</label>
            <div class="pwd-row">
              <input v-model="form.password" type="text" placeholder="密码" />
              <button class="btn sm" @click="generate">⚙ 生成</button>
            </div>
          </div>
          <div>
            <label>标签（回车 / 逗号生成，粘贴自动拆分）</label>
            <div class="chip-box" @click="focusTagInput">
              <span
                v-for="(t, i) in form.tags"
                :key="t"
                class="chip"
                :style="{ background: tagColor(t)[0], color: tagColor(t)[1] }"
              >
                #{{ t }}
                <span class="chip-x" title="移除标签" @click.stop="removeTag(i)">×</span>
              </span>
              <input
                ref="tagInputEl"
                v-model="tagDraft"
                class="chip-input"
                placeholder="输入标签后回车"
                @keydown="onTagKeydown"
                @paste="onTagPaste"
              />
            </div>
            <div class="tag-picker">
              <button
                v-for="t in allTags"
                :key="t"
                type="button"
                class="tag-pick"
                :class="{ on: form.tags.includes(t) }"
                @click.prevent="toggleFormTag(t)"
              >
                #{{ t }}
              </button>
            </div>
          </div>
        </div>
        <label>网址</label>
        <input v-model="form.url" placeholder="https://..." />
        <label>备注</label>
        <textarea v-model="form.note" rows="2"></textarea>
        <div class="form-actions">
          <button class="btn" @click="resetForm">取消</button>
          <button class="btn primary" @click="editing ? update() : create()">
            {{ editing ? "保存" : "创建" }}
          </button>
        </div>
      </div>

      <div class="card-grid">
        <div
          v-for="(entry, i) in pwdView"
          :key="entry.id"
          :data-id="entry.id"
          class="card"
          :class="{ dragging: pwdDragIdx === i }"
          title="按住可直接拖动排序"
          @click="copyPassword(entry)"
          @contextmenu="openContextMenu($event, entry)"
          @pointerdown="onCardDown($event, i)"
        >
          <div class="logo">{{ entry.name.charAt(0).toUpperCase() }}</div>
          <div class="name">{{ entry.name }}</div>
          <div class="user">{{ entry.username || "—" }}</div>
          <div class="pwd">••••••••••</div>
          <div v-if="entry.tags?.length" class="tags">
            <span
              v-for="t in entry.tags"
              :key="t"
              class="tag-mini"
              :style="{ background: tagColor(t)[0], color: tagColor(t)[1] }"
              >#{{ t }}</span
            >
          </div>
        </div>
        <div v-if="!pwdView.length" class="empty">
          {{ activeTag ? `没有带「#${activeTag}」标签的条目` : "暂无密码条目" }}
        </div>
      </div>
    </div>

    <div
      v-if="ctxMenu"
      class="ctx-menu"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
    >
      <div class="item" @click="copyPassword(ctxMenu.entry)">📋 复制密码</div>
      <div class="item" @click="copyUsername(ctxMenu.entry)">👤 复制用户名</div>
      <div v-if="ctxMenu.entry.url" class="item" @click="openUrlInBrowser(ctxMenu.entry)">🔗 打开网址</div>
      <div class="sep"></div>
      <div class="item" @click="startEdit(ctxMenu.entry)">✏️ 编辑</div>
      <div class="item danger" @click="remove(ctxMenu.entry)">🗑️ 删除</div>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.passwords {
  flex: 1;
  height: 100%;
  overflow: hidden;
  position: relative;
  background: var(--panel);
}
.content {
  height: 100%;
  overflow-y: auto;
  padding: 20px;
}
.toolbar {
  display: flex;
  gap: 10px;
  align-items: center;
  margin-bottom: 20px;
}
.search {
  flex: 1;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  outline: none;
}
.tag-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 18px;
}
.tag-chip {
  border: 1px solid var(--border);
  background: var(--card);
  border-radius: 14px;
  padding: 4px 12px;
  font-size: 12px;
  cursor: pointer;
  color: var(--muted);
  transition: 0.15s;
}
.tag-chip:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.tag-chip.on {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
  font-weight: 600;
}
.tags {
  margin-top: 8px;
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.tag-mini {
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 1px 7px;
  border-radius: 9px;
}
.tag-picker {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}
/* chips 标签输入框 */
.chip-box {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  width: 100%;
  min-height: 40px;
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  cursor: text;
}
.chip-box:focus-within {
  border-color: var(--accent);
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 9px;
  border-radius: 13px;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  white-space: nowrap;
}
.chip-x {
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  opacity: 0.7;
  border-radius: 50%;
  padding: 0 2px;
}
.chip-x:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.12);
}
.chip-input {
  flex: 1;
  min-width: 90px;
  border: none !important;
  outline: none;
  background: transparent;
  padding: 4px 2px !important;
  color: var(--text);
}
.tag-pick {
  border: 1px solid var(--border);
  background: var(--card);
  border-radius: 13px;
  padding: 3px 11px;
  font-size: 12px;
  cursor: pointer;
  color: var(--muted);
  transition: 0.15s;
}
.tag-pick:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.tag-pick.on {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
  font-weight: 600;
}
.form {
  background: var(--hover);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 20px;
  margin-bottom: 20px;
}
.form h3 {
  margin-bottom: 12px;
}
.form label {
  display: block;
  font-size: 12px;
  color: var(--muted);
  margin: 10px 0 4px;
}
.form input,
.form textarea {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 13px;
  outline: none;
}
.form .row {
  display: flex;
  gap: 12px;
}
.form .row > div {
  flex: 1;
}
.pwd-row {
  display: flex;
  gap: 8px;
}
.pwd-row input {
  flex: 1;
}
.btn.sm {
  padding: 5px 10px;
  font-size: 12px;
}
.form-actions {
  display: flex;
  gap: 8px;
  margin-top: 16px;
  justify-content: flex-end;
}
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 16px;
}
.card.drag-placeholder {
  opacity: 0.4;
  transform: none;
}
.card.dragging {
  opacity: 0.4;
  transform: none;
}
/* 拖拽期间：禁用所有卡片过渡与 hover 上浮，重排即时跟手、无动画抖动 */
body.pwd-dragging .card-grid .card {
  transition: none;
  transform: none !important;
}
body.pwd-dragging .card-grid .card:hover {
  transform: none;
}
.drag-shadow {
  position: fixed;
  z-index: 1000;
  pointer-events: none;
  opacity: 0.94;
  box-shadow: 0 14px 34px rgba(0, 0, 0, 0.25);
  border-radius: 12px;
  background: var(--card);
  will-change: transform;
}
.card {
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px 16px;
  cursor: pointer;
  position: relative;
  background: linear-gradient(180deg, var(--card) 0%, var(--card) 100%);
  transition: transform 0.18s, box-shadow 0.18s, border-color 0.18s;
  overflow: hidden;
}
.card::after {
  content: "";
  position: absolute;
  inset: 0 0 auto 0;
  height: 3px;
  background: linear-gradient(90deg, var(--accent), #8fa5ff);
  opacity: 0;
  transition: opacity 0.18s;
}
.card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow);
  border-color: var(--accent);
}
.card:hover::after {
  opacity: 1;
}
.logo {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  background: linear-gradient(135deg, var(--accent) 0%, #8fa5ff 100%);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  font-size: 16px;
  margin-bottom: 10px;
  box-shadow: 0 2px 8px rgba(79, 110, 247, 0.32);
}
.name {
  font-weight: 600;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.user {
  color: var(--muted);
  font-size: 12px;
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pwd {
  color: var(--muted);
  font-size: 14px;
  margin-top: 8px;
  letter-spacing: 2px;
}
.ctx-menu {
  position: fixed;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--shadow);
  padding: 6px;
  z-index: 100;
  min-width: 160px;
}
.ctx-menu .item {
  padding: 9px 12px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
}
.ctx-menu .item:hover {
  background: var(--hover-strong);
}
.ctx-menu .item.danger {
  color: var(--red);
}
.ctx-menu .sep {
  border-top: 1px solid var(--border);
  margin: 4px 0;
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
.empty {
  color: var(--muted);
  text-align: center;
  padding: 60px;
}
</style>

<script setup lang="ts">
import { ref, onMounted, computed, nextTick, watch, inject } from "vue";
import { notes, attachments, pickDocFile, pickDocFiles, saveTextFile, saveDataFile } from "../api";
import type { Note } from "../types";
import {
  markdownToHtml,
  htmlToMarkdown,
  isHtmlContent,
  base64ToBytes,
  bytesToBase64,
  plainTextToHtml,
} from "../logic";
import { useResizable } from "../useResizable";

/** 笔记列表宽度拖拽调节 */
const { width: listWidth, onDown: onResizeDown } = useResizable(
  "sn_notes_list_w",
  260,
  220
);

/** 第一层笔记本栏宽度拖拽调节 */
const { width: nbWidth, onDown: onNbResizeDown } = useResizable(
  "sn_notes_nb_w",
  185,
  120
);

/** 全局确认弹窗（原生 confirm 在 WebView 下不可靠） */
const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

/* ---------- 笔记本分类 ---------- */

/** 当前选中的笔记本（空串 = 全部笔记） */
const activeNotebook = ref(localStorage.getItem("sn_active_notebook") || "");
const favoriteOnly = ref(false);

/** 额外创建的笔记本名（notebook 是笔记字段，空笔记本记录在这里） */
const NB_KEY = "sn_notebooks";
/** 用 ref 保存（computed 依赖它才能实时更新），变更时同步写 localStorage */
const extraNbs = ref<string[]>(JSON.parse(localStorage.getItem(NB_KEY) || "[]"));

/* ---------- 笔记本单独加密（查看门禁：SHA-256 哈希存本地，解锁状态仅存内存） ---------- */

const nbLocks = ref<Record<string, string>>(
  JSON.parse(localStorage.getItem("sn_nb_locks") || "{}")
);
/** 已解锁的笔记本（主库锁定重进后自动重新上锁：NotesView 随锁屏卸载重建） */
const unlockedNbs = ref<string[]>([]);

function isNbLocked(nb: string): boolean {
  return !!nbLocks.value[nb] && !unlockedNbs.value.includes(nb);
}

async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** 笔记本对话框：unlock=输入密码解锁 / set=设置密码 / remove=移除密码 */
const nbDialog = ref<{ mode: "unlock" | "set" | "remove"; nb: string } | null>(null);
const nbPwd = ref("");
const nbPwd2 = ref("");
const nbErr = ref("");

function openNbDialog(mode: "unlock" | "set" | "remove", nb: string) {
  nbDialog.value = { mode, nb };
  nbPwd.value = "";
  nbPwd2.value = "";
  nbErr.value = "";
}

async function confirmNbDialog() {
  const d = nbDialog.value;
  if (!d) return;
  const pwd = nbPwd.value;
  if (d.mode === "unlock") {
    const hash = await sha256Hex(pwd);
    if (hash === nbLocks.value[d.nb]) {
      unlockedNbs.value.push(d.nb);
      nbDialog.value = null;
      syncNoteView();
      refocusActive();
    } else {
      nbErr.value = "密码错误，请重试";
    }
    return;
  }
  if (d.mode === "set") {
    if (!pwd) {
      nbErr.value = "请输入密码";
      return;
    }
    if (pwd !== nbPwd2.value) {
      nbErr.value = "两次输入的密码不一致";
      return;
    }
    nbLocks.value = { ...nbLocks.value, [d.nb]: await sha256Hex(pwd) };
    localStorage.setItem("sn_nb_locks", JSON.stringify(nbLocks.value));
    nbDialog.value = null;
    return;
  }
  // remove：需验证现有密码
  const hash = await sha256Hex(pwd);
  if (hash !== nbLocks.value[d.nb]) {
    nbErr.value = "密码错误，无法移除";
    return;
  }
  const { [d.nb]: _, ...rest } = nbLocks.value;
  nbLocks.value = rest;
  localStorage.setItem("sn_nb_locks", JSON.stringify(rest));
  nbDialog.value = null;
}

function openNbCtx(e: MouseEvent, nb: string) {
  nbCtx.value = { x: e.clientX, y: e.clientY, nb };
}

const nbCtx = ref<{ x: number; y: number; nb: string } | null>(null);

function closeNbCtx(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".ctx-menu")) {
    nbCtx.value = null;
  }
}

/** 笔记本拖拽顺序（localStorage 持久化，未记录的排后面） */
const NB_ORDER_KEY = "sn_notebooks_order";
const nbOrderIds = ref<string[]>(JSON.parse(localStorage.getItem(NB_ORDER_KEY) || "[]"));

const notebooks = computed(() => {
  const set = new Set<string>(extraNbs.value);
  noteList.value.forEach((n) => set.add(n.notebook || "默认"));
  const pos = new Map(nbOrderIds.value.map((n, i) => [n, i]));
  return [...set].sort((a, b) => {
    const pa = pos.has(a) ? pos.get(a)! : Number.MAX_SAFE_INTEGER;
    const pb = pos.has(b) ? pos.get(b)! : Number.MAX_SAFE_INTEGER;
    return pa - pb;
  });
});

/* ---------- 笔记本栏拖拽排序（与列表同款：影子 + FLIP） ---------- */

const nbView = ref<string[]>([]);
function syncNbView() {
  nbView.value = notebooks.value;
}
// 注意：watch 与 syncNbView 的实际调用必须放在 noteList 声明之后（见 applyOrder 下方），
// 因为 watch 在 setup 阶段会立即求值 notebooks computed，其中引用了其后声明的 noteList（TDZ）

const nbDragIdx = ref<number | null>(null);
interface NbDragState { itemEl: HTMLElement; shadow: HTMLElement; offsetY: number }
let nbDragState: NbDragState | null = null;

function visibleNbItems(): HTMLElement[] {
  const box = nbDragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const nb of nbView.value) {
    const el = box.querySelector(`.nb-item[data-id="${nb}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

function nbIndexFromY(clientY: number): number {
  const items = visibleNbItems();
  if (!items.length) return 0;
  let idx = 0;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY > r.top + r.height / 2) idx++;
  }
  // idx 语义：插到渲染列表（含被拖项）第 idx 项之前，取值 0..n（=n 表示最后）
  return Math.min(idx, items.length);
}

let nbPress: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onNbItemDown(e: PointerEvent, i: number) {
  if (nbDragState) return;
  const t = e.target as HTMLElement;
  if (t.closest("input, button")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".nb-item") as HTMLElement;
  if (!itemEl) return;
  nbPress = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onNbPressMove);
  window.addEventListener("pointerup", onNbPressUp);
  window.addEventListener("pointercancel", onNbPressUp);
}

function onNbPressMove(e: PointerEvent) {
  if (!nbPress) return;
  if (Math.hypot(e.clientX - nbPress.x, e.clientY - nbPress.y) < 6) return;
  const { i, itemEl } = nbPress;
  releaseNbPress();
  startNbDrag(e, i, itemEl);
}

function onNbPressUp() {
  releaseNbPress();
}

function releaseNbPress() {
  nbPress = null;
  window.removeEventListener("pointermove", onNbPressMove);
  window.removeEventListener("pointerup", onNbPressUp);
  window.removeEventListener("pointercancel", onNbPressUp);
}

function startNbDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (nbDragState || nbView.value.length < 2) return;
  document.body.style.userSelect = "none";
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  shadow.style.left = rect.left + "px";
  shadow.style.top = rect.top + "px";
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  nbDragState = { itemEl, shadow, offsetY: e.clientY - rect.top };
  nbDragIdx.value = i;
  window.addEventListener("pointermove", onNbPointerMove);
  window.addEventListener("pointerup", onNbPointerUp);
  window.addEventListener("pointercancel", onNbPointerUp);
}

function onNbPointerMove(e: PointerEvent) {
  if (!nbDragState) return;
  nbDragState.shadow.style.top = e.clientY - nbDragState.offsetY + "px";
  const from = nbDragIdx.value!;
  const target = nbIndexFromY(e.clientY);
  if (target === from) return;
  const arr = nbView.value;
  // target 是"含被拖项列表"的插入点，移除被拖项后向下插入需 -1 修正
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  nbDragIdx.value = to;
}

function onNbPointerUp() {
  window.removeEventListener("pointermove", onNbPointerMove);
  window.removeEventListener("pointerup", onNbPointerUp);
  window.removeEventListener("pointercancel", onNbPointerUp);
  document.body.style.userSelect = "";
  if (!nbDragState) return;
  nbDragState.shadow.remove();
  nbDragState.itemEl.classList.remove("drag-placeholder");
  nbDragState = null;
  if (nbDragIdx.value === null) return;
  nbDragIdx.value = null;
  nbOrderIds.value = [...nbView.value];
  localStorage.setItem(NB_ORDER_KEY, JSON.stringify(nbOrderIds.value));
}

/** 新建笔记本（回车确认） */
const newNbName = ref("");
function addNotebook() {
  const name = newNbName.value.trim();
  if (!name) return;
  if (!extraNbs.value.includes(name)) {
    extraNbs.value = [...extraNbs.value, name];
    localStorage.setItem(NB_KEY, JSON.stringify(extraNbs.value));
  }
  newNbName.value = "";
  selectNotebook(name);
}

/** 删除笔记本（右键菜单触发）：其下笔记自动移入「默认」，不丢数据 */
async function deleteNotebook(nb: string) {
  if (nb === "默认") return;
  const count = countIn(nb);
  const ok = await appConfirm(
    `删除笔记本「${nb}」？\n其中 ${count} 篇笔记将自动移入「默认」笔记本，不会丢失。`,
    "删除"
  );
  if (!ok) return;
  const affected = noteList.value.filter((n) => (n.notebook || "默认") === nb);
  for (const n of affected) {
    const updated = await notes.update(n.id, n.title, n.content, "默认");
    const idx = noteList.value.findIndex((x) => x.id === n.id);
    if (idx >= 0) noteList.value[idx] = updated;
  }
  extraNbs.value = extraNbs.value.filter((x) => x !== nb);
  localStorage.setItem(NB_KEY, JSON.stringify(extraNbs.value));
  // 清理排序记录与独立加密锁
  nbOrderIds.value = nbOrderIds.value.filter((x) => x !== nb);
  localStorage.setItem(NB_ORDER_KEY, JSON.stringify(nbOrderIds.value));
  if (nbLocks.value[nb]) {
    const locks = { ...nbLocks.value };
    delete locks[nb];
    nbLocks.value = locks;
    localStorage.setItem("sn_nb_locks", JSON.stringify(locks));
  }
  if (activeNotebook.value === nb) selectNotebook("");
}

/* ---------- 全局模糊搜索（标题 + 内容，跨所有未锁定笔记本） ---------- */

const searchQuery = ref("");

/** HTML 转纯文本（搜索用）：剥标签 + 解常见实体 */
function htmlToSearchText(html: string): string {
  return html
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&quot;/gi, '"');
}

/** 预计算的搜索索引（noteList / 锁状态变化时重算），避免每次输入重复剥离大 HTML */
const searchIndex = computed(() =>
  noteList.value.map((n) => ({
    note: n,
    text: ((n.title || "") + " " + htmlToSearchText(n.content || "")).toLowerCase(),
  }))
);

const favCount = computed(() => noteList.value.filter((n) => n.favorite).length);

/** 第二层列表栏标题（随筛选变化） */
const currentNbLabel = computed(() =>
  searchQuery.value.trim()
    ? `🔍 搜索「${searchQuery.value.trim()}」`
    : favoriteOnly.value
      ? "⭐ 收藏"
      : activeNotebook.value || "全部笔记"
);

/** 左栏列表的过滤结果（全局搜索 / 收藏 / 某笔记本；锁定的笔记本内容不展示） */
const filteredNotes = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (q) {
    // 全局模糊搜索：标题 + 内容（不区分大小写），跨所有未锁定的笔记本
    return searchIndex.value
      .filter((it) => !isNbLocked(it.note.notebook || "默认") && it.text.includes(q))
      .map((it) => it.note);
  }
  if (favoriteOnly.value)
    return noteList.value.filter((n) => n.favorite && !isNbLocked(n.notebook || "默认"));
  if (activeNotebook.value) {
    if (isNbLocked(activeNotebook.value)) return [];
    return noteList.value.filter((n) => (n.notebook || "默认") === activeNotebook.value);
  }
  return noteList.value.filter((n) => !isNbLocked(n.notebook || "默认"));
});

function countIn(name: string) {
  return noteList.value.filter((n) => (n.notebook || "默认") === name).length;
}

/** 切换筛选后若当前笔记不在范围内，自动选中第一篇 */
function refocusActive() {
  if (activeId.value && !filteredNotes.value.some((n) => n.id === activeId.value)) {
    activeId.value = null;
    if (filteredNotes.value.length) selectNote(filteredNotes.value[0]);
  }
}

function selectNotebook(name: string) {
  favoriteOnly.value = false;
  activeNotebook.value = name;
  localStorage.setItem("sn_active_notebook", name);
  refocusActive();
}

function showFavorites() {
  favoriteOnly.value = true;
  activeNotebook.value = "";
  localStorage.setItem("sn_active_notebook", "");
  refocusActive();
}

/** 更改当前笔记所属笔记本（下拉选择，立即保存） */
async function changeNotebook(e: Event) {
  if (!activeNote.value) return;
  const nb = (e.target as HTMLSelectElement).value;
  const updated = await notes.update(
    activeNote.value.id,
    editTitle.value,
    collectContent(),
    nb
  );
  const idx = noteList.value.findIndex((n) => n.id === updated.id);
  if (idx >= 0) noteList.value[idx] = updated;
}

/* ---------- 侧边栏折叠（第一层笔记本栏 / 第二层列表栏均完全隐藏） ---------- */

const nbCollapsed = ref(localStorage.getItem("sn_notes_nb_collapsed") === "1");
function toggleNb() {
  nbCollapsed.value = !nbCollapsed.value;
  localStorage.setItem("sn_notes_nb_collapsed", nbCollapsed.value ? "1" : "0");
}

const listCollapsed = ref(localStorage.getItem("sn_notes_collapsed") === "1");
function toggleList() {
  listCollapsed.value = !listCollapsed.value;
  localStorage.setItem("sn_notes_collapsed", listCollapsed.value ? "1" : "0");
}

/* ---------- 列表右键菜单：导出 / 收藏 / 删除 ---------- */

const ctxMenu = ref<{ x: number; y: number; note: Note } | null>(null);

function openCtx(e: MouseEvent, note: Note) {
  e.preventDefault();
  ctxMenu.value = { x: e.clientX, y: e.clientY, note };
}

function closeCtx(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".ctx-menu")) {
    ctxMenu.value = null;
  }
}

/** 笔记内容 → Markdown 源码（HTML 内容转换，MD 源码原样） */
function noteToMd(note: Note): string {
  const c = note.content || "";
  return isHtmlContent(c) ? htmlToMarkdown(c) : c;
}

/** 笔记 → 标准 HTML（Word 导出用；MD 源码先转 HTML） */
function noteToHtml(note: Note): string {
  const c = note.content || "";
  return isHtmlContent(c) ? c : markdownToHtml(c);
}

function safeName(note: Note): string {
  return (note.title || "无标题").replace(/[\\/:*?"<>|]/g, "_");
}

/** 导出 Word（.doc，Office HTML 格式，Word/WPS 直接打开） */
/** 把内容中的 snatt:// 附件引用还原为 dataURL（导出 md/doc 用，保证导出文件自带图片） */
async function inlineSnattImages(text: string): Promise<string> {
  const ids = [
    ...new Set(Array.from(text.matchAll(/snatt:\/\/([\w-]+)/g)).map((m) => m[1])),
  ];
  let out = text;
  for (const id of ids) {
    try {
      const att = await attachments.get(id);
      out = out
        .split(`snatt://${id}`)
        .join(`data:${att.mime || "image/png"};base64,${att.data}`);
    } catch {
      /* 附件缺失时保留引用占位 */
    }
  }
  return out;
}

async function exportDoc(note: Note) {
  try {
    const bodyHtml = await inlineSnattImages(noteToHtml(note));
    const html = `<html xmlns:w="urn:schemas-microsoft-com:office:word">
<head><meta charset="utf-8"><title>${note.title || "无标题"}</title>
<style>body{font-family:"Microsoft YaHei",sans-serif;font-size:12pt;line-height:1.7}
table{border-collapse:collapse}th,td{border:1px solid #999;padding:6px 10px}th{background:#f0f0f0}
blockquote{border-left:3px solid #888;padding-left:10px;color:#555;margin:8px 0}</style></head>
<body>${bodyHtml}</body></html>`;
    await saveTextFile(`${safeName(note)}.doc`, html);
    ctxMenu.value = null;
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg !== "已取消导出") alert("导出失败: " + msg);
  }
}

/** 从笔记 HTML 提取表格为二维数组（无表格则按行返回全文） */
function noteToAoa(note: Note): string[][][] {
  const doc = new DOMParser().parseFromString(noteToHtml(note), "text/html");
  const tables = Array.from(doc.querySelectorAll("table"));
  if (tables.length) {
    return tables.map((t) =>
      Array.from(t.querySelectorAll("tr")).map((tr) =>
        Array.from(tr.querySelectorAll("th,td")).map((td) =>
          (td.textContent || "").trim()
        )
      )
    );
  }
  // 无表格：整篇按行一列，方便浏览
  return [
    (doc.body.textContent || "")
      .split(/\n+/)
      .map((line) => [line.trim()])
      .filter((row) => row[0]),
  ];
}

/** 导出 Excel（.xlsx）：表格逐 sheet 导出，无表格则全文按行 */
async function exportXlsx(note: Note) {
  try {
    const XLSX = await import("xlsx");
    const wb = XLSX.utils.book_new();
    const sheets = noteToAoa(note);
    sheets.forEach((aoa, i) => {
      const ws = XLSX.utils.aoa_to_sheet(aoa);
      const name = sheets.length > 1 ? `表格${i + 1}` : "笔记内容";
      XLSX.utils.book_append_sheet(wb, ws, name.slice(0, 30));
    });
    const out = XLSX.write(wb, { bookType: "xlsx", type: "array" }) as ArrayBuffer;
    await saveDataFile(`${safeName(note)}.xlsx`, bytesToBase64(new Uint8Array(out)));
    ctxMenu.value = null;
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg !== "已取消导出") alert("导出失败: " + msg);
  }
}

async function exportMd(note: Note) {
  try {
    const md = await inlineSnattImages(noteToMd(note));
    await saveTextFile(`${safeName(note)}.md`, md);
    ctxMenu.value = null;
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg !== "已取消导出") alert("导出失败: " + msg);
  }
}

async function removeNote(note: Note) {
  const ok = await appConfirm(
    `删除笔记「${note.title || "无标题"}」？\n删除后进入回收站，保留 15 天`,
    "删除"
  );
  ctxMenu.value = null;
  if (!ok) return;
  await notes.remove(note.id);
  noteList.value = noteList.value.filter((n) => n.id !== note.id);
  if (activeId.value === note.id) {
    activeId.value = null;
    if (noteList.value.length) selectNote(noteList.value[0]);
  }
}

/** 文本解码：UTF-8 优先，出现乱码（U+FFFD）时回退 GBK（兼容中文 txt/log/csv） */
function decodeText(bytes: Uint8Array): string {
  const utf8 = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
  if ((utf8.match(/\uFFFD/g) || []).length > 0) {
    try {
      return new TextDecoder("gbk").decode(bytes);
    } catch {
      /* 环境不支持 gbk 则用 utf-8 结果 */
    }
  }
  return utf8;
}

const noteList = ref<Note[]>([]);
/** 用户手动拖拽的笔记顺序（localStorage 持久化，未记录的新笔记排最前） */
const orderIds = ref<string[]>(JSON.parse(localStorage.getItem("sn_notes_order") || "[]"));

/** 按 orderIds 重排 noteList（稳定排序，未记录项相对顺序不变） */
function applyOrder() {
  const pos = new Map(orderIds.value.map((id, i) => [id, i]));
  noteList.value = [...noteList.value].sort(
    (a, b) => (pos.get(a.id) ?? -1) - (pos.get(b.id) ?? -1)
  );
}

// noteList 已声明，此处才能安全注册（watch 求值 notebooks computed 会读到 noteList）
watch(notebooks, () => {
  if (!nbDragState) syncNbView();
});
syncNbView();

const activeId = ref<string | null>(null);
const editTitle = ref("");
const saving = ref(false);
const saveTimer = ref<number | null>(null);
const importing = ref(false);

// 编辑器状态
const mode = ref<"rich" | "markdown">("rich");
const mdText = ref("");
const editorEl = ref<HTMLDivElement | null>(null);

/* ---------- 自定义撤销/重做（快照栈）：接管 Ctrl+Z/Y，
   使表格行/列删除等直接 DOM 操作也能撤销（原生 execCommand 撤销栈不含它们） ---------- */
const undoStack: string[] = [];
const redoStack: string[] = [];
const HISTORY_MAX = 120;
let historyPtr = -1; // 指向 undoStack 中当前状态
let restoring = false; // 恢复操作本身不再入栈

function pushHistory() {
  if (restoring || mode.value !== "rich" || !editorEl.value) return;
  const html = editorEl.value.innerHTML;
  if (undoStack[historyPtr] === html) return;
  undoStack.splice(historyPtr + 1); // 丢弃已撤销的分支
  undoStack.push(html);
  if (undoStack.length > HISTORY_MAX) undoStack.shift();
  historyPtr = undoStack.length - 1;
  redoStack.length = 0;
}

function resetHistory() {
  undoStack.length = 0;
  redoStack.length = 0;
  historyPtr = -1;
  pushHistory();
}

function undoEdit() {
  if (mode.value !== "rich" || !editorEl.value || historyPtr <= 0) return;
  historyPtr--;
  restoring = true;
  editorEl.value.innerHTML = undoStack[historyPtr];
  restoring = false;
  scheduleSave();
}

function redoEdit() {
  if (mode.value !== "rich" || !editorEl.value || historyPtr >= undoStack.length - 1) return;
  historyPtr++;
  restoring = true;
  editorEl.value.innerHTML = undoStack[historyPtr];
  restoring = false;
  scheduleSave();
}

/** 编辑器内 Ctrl+Z / Ctrl+Y（Ctrl+Shift+Z）拦截；以及选中图片的删除键处理 */
function onEditorKeydown(e: KeyboardEvent) {
  // 选区仅为图片时，可打印字符与 Tab 不替换图片（防误删；方向键等不受影响）
  const sel = window.getSelection();
  if (
    sel &&
    sel.rangeCount > 0 &&
    !sel.getRangeAt(0).collapsed &&
    selectionOnlyImages(sel.getRangeAt(0)) &&
    (e.key.length === 1 || e.key === "Tab") &&
    !e.ctrlKey &&
    !e.metaKey &&
    !e.altKey
  ) {
    e.preventDefault();
    return;
  }
  if (e.key === "Backspace" || e.key === "Delete") {
    // 选区含图片（单击选中或框选）时接管删除，避免浏览器行为不确定
    if (tryDeleteSelectedImages(e.key === "Backspace" ? "before" : "after")) {
      e.preventDefault();
      return;
    }
  }
  // Tab / Shift+Tab：插入 4 空格 / 删除光标前最多 4 个空格（写文档对齐用）
  if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey) {
    e.preventDefault();
    handleEditorTab(e.shiftKey);
    return;
  }
  if (!e.ctrlKey || e.altKey) return;
  const k = e.key.toLowerCase();
  if (k === "z" && !e.shiftKey) {
    e.preventDefault();
    undoEdit();
  } else if (k === "y" || (k === "z" && e.shiftKey)) {
    e.preventDefault();
    redoEdit();
  } else if (k === "x") {
    // Ctrl+X：选区仅含图片时直接删除（文字+图片混合剪切仍走浏览器默认）
    if (tryDeleteSelectedImages("before")) {
      e.preventDefault();
      return;
    }
  } else if (mode.value === "rich") {
    // 常用写作快捷键：B 粗体 / I 斜体 / U 下划线 / 1·2·3 标题 / 0 正文
    if (k === "b") {
      e.preventDefault();
      exec("bold");
    } else if (k === "i") {
      e.preventDefault();
      exec("italic");
    } else if (k === "u") {
      e.preventDefault();
      exec("underline");
    } else if (e.key === "1") {
      e.preventDefault();
      insertHeading("h1");
    } else if (e.key === "2") {
      e.preventDefault();
      insertHeading("h2");
    } else if (e.key === "3") {
      e.preventDefault();
      insertHeading("h3");
    } else if (e.key === "0") {
      e.preventDefault();
      insertHeading("p");
    }
  }
}

/** Tab → 4 空格；Shift+Tab → 删除光标前最多 4 个空格（走 execCommand，保留撤销栈） */
function handleEditorTab(shift: boolean) {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return;
  const range = sel.getRangeAt(0);
  if (!shift) {
    document.execCommand("insertText", false, "    ");
    pushHistory();
    scheduleSave();
    return;
  }
  // Shift+Tab：仅光标折叠时删除前面紧邻的空格
  if (!range.collapsed) return;
  const node = range.startContainer;
  const offset = range.startOffset;
  if (node.nodeType === Node.TEXT_NODE && offset > 0) {
    const text = node.textContent || "";
    let n = 0;
    while (n < 4 && offset - 1 - n >= 0 && text[offset - 1 - n] === " ") n++;
    if (n > 0) {
      const r = document.createRange();
      r.setStart(node, offset - n);
      r.setEnd(node, offset);
      sel.removeAllRanges();
      sel.addRange(r);
      document.execCommand("insertText", false, "");
      pushHistory();
      scheduleSave();
    }
  }
}

/** 拖选时图片自身不允许发起原生拖放（会中断文字拖选，导致无法全选） */
function onEditorDragStart(e: DragEvent) {
  if ((e.target as HTMLElement)?.closest?.("img")) e.preventDefault();
}

/** 选区变化：刷新图片选中态描边（框选图片时同样去掉默认蓝色遮罩） */
function onEditorSelectionChange() {
  refreshSelectedImgMark();
}

const activeNote = computed(() =>
  noteList.value.find((n) => n.id === activeId.value)
);

async function load() {
  noteList.value = await notes.list();
  applyOrder();
  syncNoteView();
  if (noteList.value.length && !activeId.value) {
    selectNote(noteList.value[0]);
  }
}

function selectNote(note: Note) {
  flushPendingSave();
  activeId.value = note.id;
  editTitle.value = note.title;
  loadContent(note.content);
}

/* ---------- 附件图片（snatt:// 外置存储）的显示与保存回写 ---------- */

/** blob URL → 附件 id 映射（编辑器显示用；保存时还原回 snatt:// 引用，避免巨大数据写进内容） */
const blobAttMap = new Map<string, string>();

function revokeAllBlobUrls() {
  for (const url of blobAttMap.keys()) {
    try {
      URL.revokeObjectURL(url);
    } catch {
      /* ignore */
    }
  }
  blobAttMap.clear();
}

/** 把编辑器内 snatt:// 引用的图片逐张解密加载为 blob URL（原图质量，不压缩） */
async function hydrateSnattImages() {
  const root = editorEl.value;
  if (!root) return;
  const imgs = Array.from(
    root.querySelectorAll<HTMLImageElement>('img[src^="snatt://"]')
  );
  for (const img of imgs) {
    const src = img.getAttribute("src") || "";
    const id = src.slice("snatt://".length);
    if (!id) continue;
    try {
      const att = await attachments.get(id);
      const bytes = base64ToBytes(att.data);
      const url = URL.createObjectURL(
        new Blob([bytes.buffer as ArrayBuffer], { type: att.mime || "image/png" })
      );
      blobAttMap.set(url, id);
      // 仅当该 img 仍引用同一附件时替换（期间编辑器可能已切换内容）
      if (img.isConnected && img.getAttribute("src") === src) img.src = url;
    } catch (err) {
      console.warn("附件加载失败:", id, err);
    }
  }
}

/** 按内容格式加载进编辑器（HTML 直接进富文本；纯文本视为 Markdown 转换后进富文本） */
function loadContent(content: string) {
  revokeAllBlobUrls();
  if (!content) {
    mode.value = "rich";
    mdText.value = "";
    nextTick(() => {
      if (editorEl.value) editorEl.value.innerHTML = "";
      resetHistory();
    });
    return;
  }
  if (isHtmlContent(content)) {
    mode.value = "rich";
    mdText.value = htmlToMarkdown(content);
    const clean = stripColorStyles(content);
    nextTick(() => {
      if (editorEl.value) editorEl.value.innerHTML = clean;
      resetHistory();
      hydrateSnattImages();
    });
  } else {
    // 兼容旧的纯 Markdown 笔记
    mode.value = "markdown";
    mdText.value = content;
    nextTick(() => {
      if (editorEl.value) editorEl.value.innerHTML = markdownToHtml(content);
      resetHistory();
    });
  }
}

/**
 * 剥离内容里的颜色/背景类内联样式（加载历史内容时调用）：
 * 移除 style 中的 background/background-color/color 及 bgcolor 属性，
 * 让底色/文字色完全跟随主题 CSS 变量，暗黑皮肤下不再出现固定白底。
 * 保留其他语义格式（粗体/斜体/对齐等）不破坏。
 */
function stripColorStyles(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.body.querySelectorAll("*").forEach((el) => {
    el.removeAttribute("bgcolor");
    el.removeAttribute("background");
    const style = el.getAttribute("style");
    if (!style) return;
    // 逐条拆分 style，剔除颜色/背景相关声明
    const kept = style
      .split(";")
      .map((s) => s.trim())
      .filter((s) => {
        if (!s) return false;
        const prop = s.split(":")[0].trim().toLowerCase();
        return !/^(background|background-color|color|bgcolor)$/.test(prop);
      });
    if (kept.length) el.setAttribute("style", kept.join("; ") + ";");
    else el.removeAttribute("style");
  });
  return doc.body.innerHTML;
}

/** 取当前编辑器内容（统一存 HTML 或 Markdown 源码） */
function collectContent(): string {
  if (mode.value === "markdown") {
    return mdText.value; // MD 模式直接存源码，加载时自动转换
  }
  if (!editorEl.value) return "";
  // 把 blob: URL 还原为 snatt:// 附件引用，避免把巨大图片数据写进笔记内容
  return editorEl.value.innerHTML.replace(
    /src="(blob:[^"]*)"/g,
    (m: string, url: string) => {
      const id = blobAttMap.get(url);
      return id ? `src="snatt://${id}"` : m;
    }
  );
}

function scheduleSave() {
  if (saveTimer.value) window.clearTimeout(saveTimer.value);
  saveTimer.value = window.setTimeout(saveNote, 800);
}

async function saveNote() {
  const id = activeId.value;
  if (!id) return;
  saving.value = true;
  try {
    // 参数在 await 前同步求值，避免飞行期间切换笔记导致内容串写
    const updated = await notes.update(
      id,
      editTitle.value,
      collectContent(),
      activeNote.value?.notebook || "默认"
    );
    const idx = noteList.value.findIndex((n) => n.id === id);
    if (idx >= 0 && noteList.value[idx].id === id) noteList.value[idx] = updated;
  } finally {
    saving.value = false;
  }
}

/** 切换笔记前调用：把 debounce 中尚未落库的旧笔记改动立即保存（内容仍取自切换前的编辑器） */
function flushPendingSave() {
  const oldId = activeId.value;
  if (!oldId || saveTimer.value == null) return;
  window.clearTimeout(saveTimer.value);
  saveTimer.value = null;
  const title = editTitle.value;
  const content = collectContent();
  const nb = activeNote.value?.notebook || "默认";
  void notes
    .update(oldId, title, content, nb)
    .then((updated) => {
      const idx = noteList.value.findIndex((n) => n.id === oldId);
      if (idx >= 0 && noteList.value[idx].id === oldId) noteList.value[idx] = updated;
    })
    .catch(() => {});
}

async function createNote() {
  if (activeNotebook.value && isNbLocked(activeNotebook.value)) {
    openNbDialog("unlock", activeNotebook.value);
    return;
  }
  const n = await notes.create("新建笔记", activeNotebook.value || "默认");
  noteList.value.unshift(n);
  // unshift 是就地变异，不会触发 watch 的引用侦听，手动同步渲染数组
  syncNoteView();
  selectNote(n);
}

/** 把文档字节转成 HTML（docx 走 mammoth，图片转附件外置存储；md/html/txt 等走对应转换） */
async function docBytesToHtml(ext: string, bytes: Uint8Array, noteId?: string): Promise<string> {
  if (ext === "docx") {
    const mod = await import("mammoth/mammoth.browser");
    const mammoth = (mod as any).default || mod;
    // 图片不压缩、不内联：上传为加密附件，HTML 中以 snatt://<id> 引用
    // （保证原图质量，且笔记内容本身只有几百 KB，超大文档也能正常保存/渲染）
    // 注意：mammoth.browser 的 API 是驼峰命名（imgElement / readAsBase64String），与 Node 版不同
    const result = await mammoth.convertToHtml(
      { arrayBuffer: bytes.slice().buffer },
      {
        convertImage: mammoth.images.imgElement((image: any) =>
          image
            .readAsBase64String()
            .then((b64: string) =>
              attachments.save(b64, image.contentType || "image/png", noteId)
            )
            .then((id: string) => ({ src: "snatt://" + id }))
        ),
      }
    );
    const html = result?.value || "";
    if (!html.trim() && result?.messages?.length) {
      console.warn("mammoth 提示:", result.messages);
    }
    return html;
  }
  if (ext === "md" || ext === "markdown") {
    return markdownToHtml(decodeText(bytes));
  }
  if (ext === "html" || ext === "htm") {
    return decodeText(bytes);
  }
  // txt / log / csv 等纯文本（自动识别 GBK/UTF-8）
  return plainTextToHtml(decodeText(bytes));
}

/** 导入文档为新笔记：docx 走 mammoth 转 HTML；md 走 Markdown 转换；txt/log/csv 按纯文本段落；html 直接导入 */
async function importWord() {
  try {
    const [name, ext, dataB64] = await pickDocFile();
    importing.value = true;
    /** docx 必须是 ZIP（PK 头），否则给出明确提示 */
    const bytes = base64ToBytes(dataB64);
    if (ext === "docx" && !(bytes[0] === 0x50 && bytes[1] === 0x4b)) {
      throw new Error(
        "这不是有效的 .docx 文件（可能是老版 .doc 或损坏文件）。请在 Word/WPS 中另存为 .docx 后重试"
      );
    }
    if (bytes.length > MAX_IMPORT_BYTES) {
      throw new Error(
        `文件超过 200MB（${(bytes.length / 1024 / 1024).toFixed(0)} MB），无法导入。` +
          `建议在 Word 中拆分文档后再试`
      );
    }
    // 先创建笔记占位（图片附件挂到该笔记名下，便于回收站清理），转换后写入内容
    const targetNb = activeNotebook.value || "默认";
    const n = await notes.create(name, targetNb);
    try {
      let html = await docBytesToHtml(ext, bytes, n.id);
      // 导入内容同样剥离背景/颜色类内联样式，避免 Word 文档带白底在暗黑皮肤下难看
      html = stripColorStyles(html);
      if (!html.trim()) {
        await notes.remove(n.id);
        alert(
          `未能从文档中提取到内容（文件大小 ${(bytes.length / 1024).toFixed(1)} KB）。` +
            `若为老版 .doc 格式，请先在 Word/WPS 中另存为 .docx 再导入`
        );
        return;
      }
      if (html.length > MAX_IMPORT_HTML) {
        await notes.remove(n.id);
        alert(
          `文档转换后文字内容过大（${(html.length / 1024 / 1024).toFixed(1)} MB），` +
            `无法可靠导入。建议在 Word 中拆分文档后再试`
        );
        return;
      }
      const saved = await notes.update(n.id, name, html, targetNb);
      noteList.value.unshift(saved);
      syncNoteView();
      selectNote(saved);
    } catch (err) {
      // 转换失败：清理占位笔记（进回收站）
      await notes.remove(n.id).catch(() => {});
      throw err;
    }
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg && msg !== "已取消导入") alert("导入失败: " + msg);
  } finally {
    importing.value = false;
  }
}

/** 导入文件大小上限（防呆；图片转附件外置后，超大文档主要消耗转换时间而非内存） */
const MAX_IMPORT_BYTES = 200 * 1024 * 1024;
/** 转换后 HTML 上限：图片已外置为附件，超限说明是超大体量的纯文字文档，渲染会不可靠 */
const MAX_IMPORT_HTML = 64 * 1024 * 1024;

/**
 * 批量导入（通用功能）：
 * 多选文档，每个文件整体作为一条笔记——标题 = 文件名（去扩展名），内容 = 全文。
 * 导入到当前选中的笔记本分类（未选中则进「默认」）。
 * 支持 docx（走 mammoth 转 HTML，大图自动压缩）、md/txt/html 等。
 */
async function importDocs() {
  try {
    const files = await pickDocFiles();
    importing.value = true;
    let created = 0;
    let skipped = 0;
    const tooBig: string[] = [];
    const nb = activeNotebook.value || "默认";

    for (const [name, ext, dataB64] of files) {
      const bytes = base64ToBytes(dataB64);
      if (ext === "docx" && !(bytes[0] === 0x50 && bytes[1] === 0x4b)) {
        skipped++;
        continue;
      }
      if (bytes.length > MAX_IMPORT_BYTES) {
        tooBig.push(name);
        continue;
      }
      // 整个文件作为一条笔记：标题 = 文件名（去扩展名），内容 = 全文。
      // 先创建笔记（占位），转换时图片附件挂到该笔记名下（便于回收站清理），再写入内容
      const noteTitle = name.replace(/\.[^.]+$/, "").trim() || "未命名";
      const n = await notes.create(noteTitle, nb);
      try {
        const rawHtml = await docBytesToHtml(ext, bytes, n.id);
        const html = stripColorStyles(rawHtml);
        if (!html.trim()) {
          skipped++;
          await notes.remove(n.id);
          continue;
        }
        if (html.length > MAX_IMPORT_HTML) {
          tooBig.push(name);
          await notes.remove(n.id);
          continue;
        }
        // 必须用 update 的返回值（create 返回的 content 为空），否则列表里笔记内容是空白
        const saved = await notes.update(n.id, noteTitle, html, nb);
        noteList.value.unshift(saved);
        created++;
      } catch (err) {
        // 转换失败：清理占位笔记（进回收站），继续下一个文件
        await notes.remove(n.id).catch(() => {});
        throw err;
      }
    }

    syncNoteView();
    if (created > 0 || tooBig.length) {
      let msg = created > 0 ? `批量导入完成：共导入 ${created} 条笔记` : "没有导入任何笔记";
      if (skipped) msg += `，跳过 ${skipped} 个无法识别/空内容的文件`;
      if (tooBig.length)
        msg += `。以下文件超过 64MB（转换后）已跳过，建议在 Word 中拆分后导入:\n${tooBig.join("\n")}`;
      alert(msg);
    } else {
      alert("没有成功导入任何笔记，请检查所选文件。");
    }
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg && msg !== "已取消导入") alert("批量导入失败: " + msg);
  } finally {
    importing.value = false;
  }
}

function toggleFavorite(note: Note) {
  note.favorite = !note.favorite;
  notes.toggleFavorite(note.id, note.favorite);
}

/* ---------- 笔记列表整行拖动排序（与待办同款：影子跟随 + FLIP 让位） ---------- */

/** 列表渲染源：由筛选结果同步，拖拽时直接在其上重排 */
const noteView = ref<Note[]>([]);

function syncNoteView() {
  noteView.value = filteredNotes.value;
}

watch([noteList, favoriteOnly, activeNotebook, searchQuery], () => {
  if (noteDragState) return; // 拖拽进行中不覆盖渲染数组
  syncNoteView();
}, { deep: true });

const nDragIndex = ref<number | null>(null);
interface NoteDragState {
  itemEl: HTMLElement;
  shadow: HTMLElement;
  offsetY: number;
}
let noteDragState: NoteDragState | null = null;

function visibleNoteItems(): HTMLElement[] {
  const box = noteDragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const n of noteView.value) {
    const el = box.querySelector(`.list-item[data-id="${n.id}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

function noteIndexFromY(clientY: number): number {
  const items = visibleNoteItems();
  if (!items.length) return 0;
  let idx = 0;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY > r.top + r.height / 2) idx++;
  }
  // idx 语义：插到渲染列表（含被拖项）第 idx 项之前，取值 0..n（=n 表示最后）
  return Math.min(idx, items.length);
}

/** 按下 → 移动超阈值才进入拖拽，否则仍是普通点击 */
let notePress: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onNoteItemDown(e: PointerEvent, i: number) {
  if (noteDragState) return;
  const t = e.target as HTMLElement;
  if (t.closest("input, button, .star")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".list-item") as HTMLElement;
  if (!itemEl) return;
  notePress = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onNotePressMove);
  window.addEventListener("pointerup", onNotePressUp);
  window.addEventListener("pointercancel", onNotePressUp);
}

function onNotePressMove(e: PointerEvent) {
  if (!notePress) return;
  if (Math.hypot(e.clientX - notePress.x, e.clientY - notePress.y) < 6) return;
  const { i, itemEl } = notePress;
  releaseNotePress();
  startNoteDrag(e, i, itemEl);
}

function onNotePressUp() {
  releaseNotePress();
}

function releaseNotePress() {
  notePress = null;
  window.removeEventListener("pointermove", onNotePressMove);
  window.removeEventListener("pointerup", onNotePressUp);
  window.removeEventListener("pointercancel", onNotePressUp);
}

function startNoteDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (noteDragState || noteView.value.length < 2) return;
  document.body.style.userSelect = "none";
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  shadow.style.left = rect.left + "px";
  shadow.style.top = rect.top + "px";
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  noteDragState = { itemEl, shadow, offsetY: e.clientY - rect.top };
  nDragIndex.value = i;
  window.addEventListener("pointermove", onNotePointerMove);
  window.addEventListener("pointerup", onNotePointerUp);
  window.addEventListener("pointercancel", onNotePointerUp);
}

function onNotePointerMove(e: PointerEvent) {
  if (!noteDragState) return;
  noteDragState.shadow.style.top = e.clientY - noteDragState.offsetY + "px";
  const from = nDragIndex.value!;
  const target = noteIndexFromY(e.clientY);
  if (target === from) return;
  const arr = noteView.value;
  // target 是"含被拖项列表"的插入点，移除被拖项后向下插入需 -1 修正
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  nDragIndex.value = to;
}

function onNotePointerUp() {
  window.removeEventListener("pointermove", onNotePointerMove);
  window.removeEventListener("pointerup", onNotePointerUp);
  window.removeEventListener("pointercancel", onNotePointerUp);
  document.body.style.userSelect = "";
  if (!noteDragState) return;
  noteDragState.shadow.remove();
  noteDragState.itemEl.classList.remove("drag-placeholder");
  noteDragState = null;
  if (nDragIndex.value === null) return;
  nDragIndex.value = null;
  commitNoteOrder();
}

/** 拖拽结束：把筛选子集的新顺序写回全量顺序并持久化 */
function commitNoteOrder() {
  const fIds = noteView.value.map((n) => n.id);
  const fSet = new Set(fIds);
  const oldOrder = orderIds.value.filter((id) =>
    noteList.value.some((n) => n.id === id)
  );
  const newOrder: string[] = [];
  let fi = 0;
  for (const id of oldOrder) {
    newOrder.push(fSet.has(id) ? fIds[fi++] : id);
  }
  const missing = fIds.filter((id) => !newOrder.includes(id));
  orderIds.value = [...missing, ...newOrder];
  localStorage.setItem("sn_notes_order", JSON.stringify(orderIds.value));
  applyOrder();
}

/* ---------- 富文本命令 ---------- */

function exec(cmd: string, value?: string) {
  if (mode.value !== "rich") return;
  // 注意：不调用 focus()——按钮已用 @mousedown.prevent 保持编辑器选区，
  // 调 focus() 会把光标重置到文档开头导致插入位置错乱
  document.execCommand(cmd, false, value);
  pushHistory();
  scheduleSave();
}

/** 编辑器输入：防抖保存 + 记录撤销快照 */
function onEditorInput() {
  pushHistory();
  scheduleSave();
}

/** MD 模式：Tab → 4 空格，Shift+Tab → 删除光标前最多 4 个空格（不把焦点切走） */
function onMdKeydown(e: KeyboardEvent) {
  if (e.key !== "Tab" || e.ctrlKey || e.metaKey || e.altKey) return;
  e.preventDefault();
  const ta = e.target as HTMLTextAreaElement;
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  const val = ta.value;
  if (e.shiftKey) {
    // 删除光标/选区前紧邻的最多 4 个空格
    let n = 0;
    while (n < 4 && start - 1 - n >= 0 && val[start - 1 - n] === " ") n++;
    if (n === 0) return;
    const next = val.slice(0, start - n) + val.slice(start);
    mdText.value = next;
    setMdSelection(start - n, end - n);
  } else {
    // 选区覆盖整行多行时按行缩进，否则插入 4 空格
    const multiline = end > start && val.slice(start, end).includes("\n");
    if (multiline) {
      const lineStart = val.lastIndexOf("\n", start - 1) + 1;
      const block = val.slice(lineStart, end);
      const indented = block
        .split("\n")
        .map((l) => (l.trim() ? "    " + l : l))
        .join("\n");
      mdText.value = val.slice(0, lineStart) + indented + val.slice(end);
      setMdSelection(lineStart, lineStart + indented.length);
    } else {
      mdText.value = val.slice(0, start) + "    " + val.slice(end);
      setMdSelection(start + 4, start + 4);
    }
  }
  scheduleSave();
}

/** 修改 mdText 后恢复 textarea 光标/选区（nextTick 等 v-model 同步完成） */
function setMdSelection(start: number, end: number) {
  nextTick(() => {
    const ta = document.querySelector(".md-editor") as HTMLTextAreaElement | null;
    if (!ta) return;
    ta.selectionStart = start;
    ta.selectionEnd = end;
  });
}

function insertHeading(level: string) {
  // Chromium 的 formatBlock 要求带尖括号：<h1> / <blockquote>
  exec("formatBlock", `<${level}>`);
}

function insertHr() {
  if (mode.value !== "rich") {
    mdText.value += "\n---\n";
    scheduleSave();
    return;
  }
  exec("insertHorizontalRule");
  // 分割线后补一个空段落，让光标有落脚点，否则下一行无法输入、光标回跳上一行
  exec("insertHTML", "<p><br></p>");
}

function insertTable() {
  if (mode.value !== "rich") {
    mdText.value += "\n| 列1 | 列2 | 列3 |\n| --- | --- | --- |\n| 内容 | 内容 | 内容 |\n";
    scheduleSave();
    return;
  }
  exec(
    "insertHTML",
    '<table><thead><tr><th>列1</th><th>列2</th><th>列3</th></tr></thead>' +
      "<tbody><tr><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td></tr>" +
      "<tr><td>&nbsp;</td><td>&nbsp;</td><td>&nbsp;</td></tr></tbody></table><p><br></p>"
  );
}

function insertImageFile() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "image/png,image/jpeg,image/gif,image/webp";
  input.onchange = () => {
    const file = input.files?.[0];
    if (file) insertImageBlob(file);
  };
  input.click();
}

async function insertImageBlob(file: Blob, filename?: string) {
  // 图片上传为加密附件（原图保存，不压缩），内容中以 snatt://<id> 引用
  const buf = new Uint8Array(await file.arrayBuffer());
  const mime = file.type || "image/png";
  const src =
    "snatt://" + (await attachments.save(bytesToBase64(buf), mime, activeId.value || undefined));
  if (mode.value === "markdown") {
    mdText.value += `\n![${filename || "图片"}](${src})\n`;
    scheduleSave();
    return;
  }
  editorEl.value?.focus();
  document.execCommand(
    "insertHTML",
    false,
    `<img src="${src}" alt="${filename || "图片"}" style="max-width:100%;">` +
      // 图片独立成行后补一个空段落：光标落在段落里，避免在图片右侧出现超高光标
      "<p><br></p>"
  );
  scheduleSave();
}

/* ---------- 表格 / 图片 上下文编辑 ---------- */

const tableCtx = ref(false);
const imageCtx = ref<HTMLImageElement | null>(null);
let lastEl: HTMLElement | null = null;

/** 双击放大预览 */
const previewSrc = ref("");
function onEditorDblclick(e: MouseEvent) {
  const img = e.target as HTMLElement;
  if (img instanceof HTMLImageElement) {
    previewSrc.value = img.src;
  }
}

/* ---------- 预览层：滚轮缩放 + 拖动平移 + 双击切换 ---------- */
const pvScale = ref(1); // 当前缩放倍数（1 = 适应窗口）
const pvX = ref(0);
const pvY = ref(0); // 平移偏移（px）
const pvImgStyle = computed(() => ({
  transform: `translate(${pvX.value}px, ${pvY.value}px) scale(${pvScale.value})`,
  cursor: pvScale.value > 1.01 ? "grab" : "zoom-in",
}));

// 打开新预览时复位
watch(previewSrc, (v) => {
  if (v) {
    pvScale.value = 1;
    pvX.value = 0;
    pvY.value = 0;
  }
});

/** 以光标为锚点缩放：保持光标下的内容点不动 */
function pvZoomAt(next: number, cx: number, cy: number) {
  const old = pvScale.value;
  if (next === old) return;
  const dx = cx - window.innerWidth / 2;
  const dy = cy - window.innerHeight / 2;
  pvX.value = dx - (next / old) * (dx - pvX.value);
  pvY.value = dy - (next / old) * (dy - pvY.value);
  pvScale.value = next;
}

const PV_SCALE_MIN = 0.2;
const PV_SCALE_MAX = 12;
function onPreviewWheel(e: WheelEvent) {
  const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
  pvZoomAt(
    Math.min(PV_SCALE_MAX, Math.max(PV_SCALE_MIN, pvScale.value * factor)),
    e.clientX,
    e.clientY,
  );
}

let pvDrag: { sx: number; sy: number; ox: number; oy: number; moved: boolean } | null = null;
let pvSuppressClick = false; // 拖动结束后的 click 事件不要触发关闭

function onPreviewMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  pvDrag = { sx: e.clientX, sy: e.clientY, ox: pvX.value, oy: pvY.value, moved: false };
}
function onPreviewMouseMove(e: MouseEvent) {
  if (!pvDrag) return;
  const dx = e.clientX - pvDrag.sx;
  const dy = e.clientY - pvDrag.sy;
  if (Math.abs(dx) + Math.abs(dy) > 3) pvDrag.moved = true;
  if (pvDrag.moved) {
    pvX.value = pvDrag.ox + dx;
    pvY.value = pvDrag.oy + dy;
  }
}
function onPreviewMouseUp() {
  const moved = pvDrag?.moved ?? false;
  pvDrag = null;
  if (moved) {
    pvSuppressClick = true;
    setTimeout(() => (pvSuppressClick = false), 350);
  }
}
/** 点击空白处关闭（拖动结束的点击不关闭） */
function onPreviewBgClick() {
  if (pvSuppressClick) {
    pvSuppressClick = false;
    return;
  }
  previewSrc.value = "";
}
/** 双击：适应窗口 ↔ 2.5x（以光标为中心） */
function onPreviewDblclick(e: MouseEvent) {
  if (pvScale.value > 1.01) {
    pvScale.value = 1;
    pvX.value = 0;
    pvY.value = 0;
  } else {
    pvZoomAt(2.5, e.clientX, e.clientY);
  }
}

/** 编辑器内点击：识别点中的表格单元格 / 图片，顶部钉住对应子工具栏 */
function onEditorClick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  lastEl = t;
  imageCtx.value = t instanceof HTMLImageElement ? t : null;
  tableCtx.value = !imageCtx.value && !!t.closest("td,th");
  // 单击图片：显式把整个 img 建入选区（原生点击选区不可靠），
  // 之后 Backspace / Ctrl+X / 打字替换都可作用于该图片。
  // 若是拖选结束时落点在图片上（已有非折叠选区），保留用户的"文字+图片"选区不覆盖
  if (imageCtx.value) {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || sel.getRangeAt(0).collapsed) {
      selectImageNode(imageCtx.value);
    }
  }
}

/** 把图片整体设为当前选区，并挂高亮 class（去掉浏览器默认蓝色遮罩） */
function selectImageNode(img: HTMLImageElement) {
  const sel = window.getSelection();
  if (!sel) return;
  const range = document.createRange();
  range.selectNode(img);
  sel.removeAllRanges();
  sel.addRange(range);
  refreshSelectedImgMark();
}

/** 根据当前选区刷新图片的选中态 class：仅"纯图片"选区加描边高亮 */
function refreshSelectedImgMark() {
  if (!editorEl.value) return;
  editorEl.value.querySelectorAll("img").forEach((i) => i.classList.remove("selected-img"));
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return;
  const range = sel.getRangeAt(0);
  if (range.collapsed) return;
  if (!selectionOnlyImages(range)) return; // 文字+图片混合选区时保留默认整体高亮
  editorEl.value.querySelectorAll("img").forEach((i) => {
    if (range.intersectsNode(i)) i.classList.add("selected-img");
  });
}

/** 取当前选区涉及的图片；dir：collapsed 光标时 Backspace 看前邻、Delete 看后邻 */
function selectedImages(dir: "before" | "after"): HTMLImageElement[] {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !editorEl.value) return [];
  const range = sel.getRangeAt(0);
  const out: HTMLImageElement[] = [];
  if (!range.collapsed) {
    editorEl.value.querySelectorAll("img").forEach((i) => {
      if (range.intersectsNode(i)) out.push(i);
    });
    return out;
  }
  const c = range.startContainer;
  if (c instanceof HTMLImageElement) {
    out.push(c);
    return out;
  }
  const off = range.startOffset;
  const node =
    dir === "before" ? c.childNodes[off - 1] : c.childNodes[off];
  if (node instanceof HTMLImageElement) out.push(node);
  return out;
}

/** 选区内容是否"只含图片、没有文字"（含文字时删除/剪切交给浏览器整体处理） */
function selectionOnlyImages(range: Range): boolean {
  const frag = range.cloneContents();
  if ((frag.textContent || "").trim() !== "") return false;
  return frag.querySelectorAll("img").length > 0;
}

/** 键盘删除选中的图片（Backspace / Delete / Ctrl+X）；返回是否已处理 */
function tryDeleteSelectedImages(dir: "before" | "after"): boolean {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return false;
  const range = sel.getRangeAt(0);
  // 非折叠选区：仅当选区只含图片时才手动接管；文字+图片混合交给浏览器整体删除/剪切
  if (!range.collapsed && !selectionOnlyImages(range)) return false;
  const imgs = selectedImages(dir);
  if (!imgs.length) return false;
  imgs.forEach((i) => i.remove());
  sel.removeAllRanges();
  imageCtx.value = null;
  pushHistory();
  scheduleSave();
  return true;
}

function getCell(): HTMLTableCellElement | null {
  return (lastEl?.closest("td,th") as HTMLTableCellElement) || null;
}
function getTable(): HTMLTableElement | null {
  return (lastEl?.closest("table") as HTMLTableElement) || null;
}

function tInsertRow(after: boolean) {
  const table = getTable();
  const cell = getCell();
  if (!table || !cell) return;
  const tr = cell.parentElement as HTMLTableRowElement;
  const newRow = table.insertRow(tr.rowIndex + (after ? 1 : 0));
  for (let c = 0; c < tr.cells.length; c++) newRow.insertCell().innerHTML = "<br>";
  pushHistory();
  scheduleSave();
}
function tDeleteRow() {
  const table = getTable();
  const cell = getCell();
  if (!table || !cell) return;
  if (table.rows.length <= 1) {
    tDeleteTable();
    return;
  }
  table.deleteRow((cell.parentElement as HTMLTableRowElement).rowIndex);
  tableCtx.value = false;
  pushHistory();
  scheduleSave();
}
function tInsertCol(after: boolean) {
  const table = getTable();
  const cell = getCell();
  if (!table || !cell) return;
  const idx = cell.cellIndex;
  for (const row of Array.from(table.rows)) {
    row.insertCell(idx + (after ? 1 : 0)).innerHTML = "<br>";
  }
  pushHistory();
  scheduleSave();
}
function tDeleteCol() {
  const table = getTable();
  const cell = getCell();
  if (!table || !cell) return;
  if (table.rows[0].cells.length <= 1) {
    tDeleteTable();
    return;
  }
  const idx = cell.cellIndex;
  for (const row of Array.from(table.rows)) row.deleteCell(idx);
  tableCtx.value = false;
  pushHistory();
  scheduleSave();
}
function tDeleteTable() {
  const table = getTable();
  if (!table) return;
  table.remove();
  tableCtx.value = false;
  pushHistory();
  scheduleSave();
}

function imgSetWidth(pct: number) {
  if (!imageCtx.value) return;
  imageCtx.value.style.width = pct + "%";
  imageCtx.value.style.height = "auto";
  pushHistory();
  scheduleSave();
}
function imgDelete() {
  imageCtx.value?.remove();
  imageCtx.value = null;
  pushHistory();
  scheduleSave();
}

/* ---------- 表格列宽 / 图片宽度 边缘拖拽 ---------- */

interface ColDragState {
  cells: HTMLElement[];
  startX: number;
  startWidth: number;
}
interface ImgDragState {
  img: HTMLImageElement;
  startX: number;
  startPct: number;
  baseWidth: number;
}
let colDrag: ColDragState | null = null;
let imgDrag: ImgDragState | null = null;
const EDGE = 6; // 边缘命中距离（px）

/** 命中单元格右边缘 → 该列 */
function hitCellEdge(t: HTMLElement, e: MouseEvent) {
  const cell = t.closest("td,th") as HTMLTableCellElement | null;
  if (!cell) return null;
  const rect = cell.getBoundingClientRect();
  if (e.clientX < rect.right - EDGE || e.clientX > rect.right + 3) return null;
  const row = cell.parentElement as HTMLTableRowElement;
  return { cell, colIdx: Array.from(row.cells).indexOf(cell) };
}

/** 命中图片右边缘 */
function hitImgEdge(t: HTMLElement, e: MouseEvent) {
  const img = t.closest("img") as HTMLImageElement | null;
  if (!img) return null;
  const rect = img.getBoundingClientRect();
  return e.clientX >= rect.right - EDGE && e.clientX <= rect.right + 3 ? img : null;
}

/** 编辑器内移动鼠标：靠近可拖拽边缘时显示双向箭头光标 */
function onEditorMouseMove(e: MouseEvent) {
  if (colDrag || imgDrag) return;
  if (mode.value !== "rich" || !editorEl.value) return;
  const t = e.target as HTMLElement;
  editorEl.value.style.cursor =
    hitImgEdge(t, e) || hitCellEdge(t, e) ? "col-resize" : "";
}

/** 编辑器内按下：命中边缘 → 开始拖动（列宽 / 图片宽） */
function onEditorMouseDown(e: MouseEvent) {
  if (mode.value !== "rich" || !editorEl.value) return;
  const t = e.target as HTMLElement;
  const img = hitImgEdge(t, e);
  if (img) {
    const editorW = editorEl.value.getBoundingClientRect().width;
    imgDrag = {
      img,
      startX: e.clientX,
      startPct:
        parseFloat(img.style.width) ||
        (img.getBoundingClientRect().width / editorW) * 100,
      baseWidth: editorW,
    };
    e.preventDefault();
    bindDragEnd();
    return;
  }
  const hit = hitCellEdge(t, e);
  if (hit) {
    const table = hit.cell.closest("table")!;
    const cells = Array.from(table.rows)
      .map((r) => r.cells[hit.colIdx] as HTMLElement | undefined)
      .filter((c): c is HTMLElement => !!c);
    colDrag = {
      cells,
      startX: e.clientX,
      startWidth: hit.cell.getBoundingClientRect().width,
    };
    e.preventDefault();
    bindDragEnd();
  }
}

/** 拖动期间挂 window 监听，松开即落盘（全程锁定光标与选择） */
function bindDragEnd() {
  const move = (ev: MouseEvent) => {
    if (colDrag) {
      const w = Math.max(28, colDrag.startWidth + ev.clientX - colDrag.startX);
      colDrag.cells.forEach((c) => (c.style.width = w + "px"));
    } else if (imgDrag) {
      const pct = Math.min(
        100,
        Math.max(8, imgDrag.startPct + ((ev.clientX - imgDrag.startX) / imgDrag.baseWidth) * 100)
      );
      imgDrag.img.style.width = pct.toFixed(1) + "%";
      imgDrag.img.style.height = "auto";
    }
  };
  const up = () => {
    if (colDrag || imgDrag) {
      pushHistory();
      scheduleSave();
    }
    colDrag = null;
    imgDrag = null;
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    if (editorEl.value) editorEl.value.style.cursor = "";
  };
  // 拖动全程保持双向箭头光标，并禁止选中文本
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

/* ---------- 粘贴：图片 / 表格 ---------- */

/**
 * 清洗外部粘贴的 HTML：彻底重建为纯文本 + 基础表格结构，
 * 剥离一切内联样式、颜色、背景、类名等，杜绝白底/错位/文字溢出。
 * 采用"提取式重建"——只保留文字与表格语义，不继承任何原始标签属性。
 */
function sanitizePastedHtml(raw: string): string {
  const doc = new DOMParser().parseFromString(raw, "text/html");
  const body = doc.body;
  // 移除 style / link / script 等非内容节点
  body.querySelectorAll("style, link, script, meta, title").forEach((n) => n.remove());

  // 递归重建：只保留文字 + 表格结构，剥离所有属性
  const ALLOWED_ATTR = new Set(["colspan", "rowspan"]);
  function rebuild(el: Element): string {
    const tag = el.tagName.toLowerCase();
    // 文本节点直接返回
    let inner = "";
    for (const child of Array.from(el.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        inner += (child.textContent || "").replace(/</g, "&lt;");
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        inner += rebuild(child as Element);
      }
    }

    switch (tag) {
      case "table":
        return `<table>${inner}</table>`;
      case "thead":
      case "tbody":
      case "tfoot":
      case "tr":
        return `<${tag}>${inner}</${tag}>`;
      case "th":
      case "td": {
        // 仅保留合并单元格属性
        let attrs = "";
        for (const a of ALLOWED_ATTR) {
          const v = el.getAttribute(a);
          if (v) attrs += ` ${a}="${v}"`;
        }
        return `<${tag}${attrs}>${inner || "<br>"}</${tag}>`;
      }
      case "img": {
        const src = el.getAttribute("src") || "";
        return src.startsWith("data:") ? `<img src="${src}">` : "";
      }
      case "br":
        return "<br>";
      case "p": {
        const t = inner.trim();
        return t ? `<p>${inner}</p>` : "";
      }
      case "div": {
        // div 是容器，透传内容；但若其直接文本无块级子元素，则补成段落
        const t = inner.trim();
        if (!t) return "";
        // 若包含块级元素（table/p/h/ul/ol/blockquote/div），直接透传，避免 <p><table></p> 非法嵌套
        if (/<(table|p|h[1-6]|ul|ol|blockquote|div)/i.test(inner)) return inner;
        return `<p>${inner}</p>`;
      }
      case "h1":
      case "h2":
      case "h3":
      case "h4":
      case "h5":
      case "h6":
        return inner.trim() ? `<${tag}>${inner}</${tag}>` : "";
      case "ul":
        return inner.trim() ? `<ul>${inner}</ul>` : "";
      case "ol":
        return inner.trim() ? `<ol>${inner}</ol>` : "";
      case "li":
        return inner.trim() ? `<li>${inner}</li>` : "";
      case "strong":
      case "b":
        return inner.trim() ? `<b>${inner}</b>` : "";
      case "em":
      case "i":
        return inner.trim() ? `<i>${inner}</i>` : "";
      case "u":
        return inner.trim() ? `<u>${inner}</u>` : "";
      case "code":
        return inner ? `<code>${inner}</code>` : "";
      case "blockquote":
        return inner.trim() ? `<blockquote>${inner}</blockquote>` : "";
      case "a": {
        const href = el.getAttribute("href") || "";
        return inner.trim() ? (href ? `<a href="${href}">${inner}</a>` : inner) : "";
      }
      case "span":
      case "font":
      case "section":
      case "article":
      case "header":
      case "footer":
      case "main":
        // 行内/容器：直接透传内容，丢弃自身标签与样式
        return inner;
      default:
        // 未知标签：透传内容（丢弃标签）
        return inner;
    }
  }

  const out = Array.from(body.childNodes)
    .map((n) => {
      if (n.nodeType === Node.TEXT_NODE) return n.textContent || "";
      // 注释(8)/CDATA(4) 等非元素节点直接跳过——否则 rebuild 里 el.tagName 为 undefined 会抛异常
      if (n.nodeType !== Node.ELEMENT_NODE) return "";
      return rebuild(n as Element);
    })
    .join("");
  // 去除空段落并规范化
  return out;
}

function onPaste(e: ClipboardEvent) {
  const items = e.clipboardData?.items;
  if (!items) return;
  // 1) 剪贴板里有图片 → 直接插入（截图、复制的图片文件）
  for (const item of items) {
    if (item.type.startsWith("image/")) {
      e.preventDefault();
      const blob = item.getAsFile();
      if (blob)
        insertImageBlob(blob, "粘贴图片").catch((err) =>
          alert("图片插入失败: " + (err?.message || err))
        );
      return;
    }
  }
  // 2) 富文本粘贴：清洗 HTML 后插入，完整保留文字 + 表格等结构
  const html = e.clipboardData?.getData("text/html");
  if (html && mode.value === "rich") {
    try {
      const clean = sanitizePastedHtml(html);
      if (clean.trim()) {
        e.preventDefault();
        document.execCommand("insertHTML", false, clean);
        scheduleSave();
        return;
      }
    } catch (err) {
      console.warn("粘贴清洗失败，回退默认粘贴", err);
    }
    // 清洗结果为空或出错 → 继续走默认粘贴（不 return，保证一定粘得进去）
  }
  // 其余情况走默认粘贴（纯文本、清洗失败回退等）
}

/* ---------- Markdown 模式切换 ---------- */

function switchMode(target: "rich" | "markdown") {
  if (target === mode.value) return;
  if (target === "markdown") {
    // 富文本 → Markdown
    mdText.value =
      mode.value === "rich" ? htmlToMarkdown(editorEl.value?.innerHTML || "") : mdText.value;
  } else {
    // Markdown → 富文本
    const html = markdownToHtml(mdText.value);
    nextTick(() => {
      if (editorEl.value) editorEl.value.innerHTML = html;
      resetHistory();
      hydrateSnattImages();
    });
  }
  mode.value = target;
  scheduleSave();
}

// Markdown 模式下编辑自动保存
watch(mdText, () => {
  if (mode.value === "markdown") scheduleSave();
});

onMounted(() => {
  load();
  document.addEventListener("click", closeCtx);
  document.addEventListener("click", closeNbCtx);
  // 预览层 Esc 关闭 + 拖动跟踪（挂 window，快速拖动出遮罩范围也能跟上）
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      previewSrc.value = "";
      nbCtx.value = null;
      if (nbDialog.value) nbDialog.value = null;
    }
  });
  window.addEventListener("mousemove", onPreviewMouseMove);
  window.addEventListener("mouseup", onPreviewMouseUp);
});
</script>

<template>
  <div class="notes">
    <!-- 第一层：笔记本分类栏（折叠为图标窄条） -->
    <aside v-if="nbCollapsed" class="nb-mini">
      <button class="mini-btn expand" title="展开笔记本栏" @click="toggleNb">
        <svg viewBox="0 0 24 24" width="15" height="15">
          <path
            d="M9 5.5 L15.5 12 L9 18.5"
            fill="none"
            stroke="currentColor"
            stroke-width="2.4"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <button
        class="mini-btn"
        :class="{ on: !activeNotebook && !favoriteOnly }"
        title="全部笔记"
        @click="selectNotebook('')"
      >📚</button>
      <button
        class="mini-btn"
        :class="{ on: favoriteOnly }"
        title="收藏"
        @click="showFavorites"
      >⭐</button>
      <button
        v-for="nb in notebooks"
        :key="nb"
        class="mini-btn"
        :class="{ on: activeNotebook === nb && !favoriteOnly }"
        :title="nb"
        @click="isNbLocked(nb) ? openNbDialog('unlock', nb) : selectNotebook(nb)"
      >{{ isNbLocked(nb) ? "🔒" : "📗" }}</button>
    </aside>
    <aside v-else class="nb-panel" :style="{ width: nbWidth + 'px' }">
      <div class="nb-search">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="🔍 全局搜索（标题 / 内容）"
          title="跨所有笔记本模糊搜索标题与正文，清空即恢复"
        />
        <button v-if="searchQuery" class="clear" title="清空搜索" @click="searchQuery = ''">×</button>
      </div>
      <div
        class="nb-item"
        :class="{ active: !activeNotebook && !favoriteOnly }"
        @click="selectNotebook('')"
      >
        <span>📚 全部笔记</span><span class="badge">{{ noteList.length }}</span>
      </div>
      <div class="nb-item" :class="{ active: favoriteOnly }" @click="showFavorites">
        <span>⭐ 收藏</span><span class="badge">{{ favCount }}</span>
      </div>
      <div class="nb-sep">我的笔记本</div>
      <div
        v-for="(nb, i) in nbView"
        :key="nb"
        :data-id="nb"
        class="nb-item"
        :class="{ active: activeNotebook === nb && !favoriteOnly, dragging: nbDragIdx === i }"
        title="按住可直接拖动排序"
        @click="isNbLocked(nb) ? openNbDialog('unlock', nb) : selectNotebook(nb)"
        @contextmenu.prevent="openNbCtx($event, nb)"
        @pointerdown="onNbItemDown($event, i)"
      >
        <span class="nb-name">{{ isNbLocked(nb) ? "🔒" : "📗" }} {{ nb }}</span>
        <span class="nb-item-right">
          <button
            class="nb-lock-btn"
            :title="nbLocks[nb] ? '密码设置（右键）' : '设置密码'"
            @click.stop="openNbDialog(nbLocks[nb] ? 'set' : 'set', nb)"
          >{{ nbLocks[nb] ? "🔒" : "🔓" }}</button>
          <span class="badge">{{ countIn(nb) }}</span>
        </span>
      </div>
      <div class="new-list">
        <input
          v-model="newNbName"
          placeholder="＋ 新建笔记本"
          title="输入名称后回车创建"
          @keyup.enter="addNotebook"
        />
      </div>
    </aside>
    <div
      v-if="!nbCollapsed"
      class="edge-handle"
      title="折叠笔记本栏"
      @click="toggleNb"
    >
      <span class="handle-grip">⟨</span>
    </div>
    <div class="col-resizer" v-show="!nbCollapsed" title="拖动调整笔记本栏宽度" @mousedown="onNbResizeDown"></div>

    <!-- 笔记本右键菜单：加密设置 -->
    <div
      v-if="nbCtx"
      class="ctx-menu"
      :style="{ left: nbCtx.x + 'px', top: nbCtx.y + 'px' }"
    >
      <div class="item" @click="openNbDialog('set', nbCtx.nb); nbCtx = null">
        {{ nbLocks[nbCtx.nb] ? "🔑 修改密码" : "🔒 设置密码" }}
      </div>
      <div v-if="nbLocks[nbCtx.nb]" class="item danger" @click="openNbDialog('remove', nbCtx.nb); nbCtx = null">
        🔓 移除密码
      </div>
      <div v-if="nbCtx.nb !== '默认'" class="item danger" @click="deleteNotebook(nbCtx.nb); nbCtx = null">
        🗑 删除笔记本
      </div>
    </div>

    <!-- 第二层：笔记列表 -->
    <aside v-show="!listCollapsed" class="list-panel" :style="{ width: listWidth + 'px' }">
      <div class="panel-head">
        <div class="head-title-row">
          {{ currentNbLabel }}
          <small class="head-count">{{ filteredNotes.length }} 篇</small>
        </div>
        <div class="head-actions-row">
          <button class="btn xs" :disabled="importing" title="把 Word / TXT / Log / MD / CSV / HTML 文档导入为新笔记" @click="importWord">
            {{ importing ? "导入中…" : "📥 导入" }}
          </button>
          <button class="btn xs" :disabled="importing" title="批量导入多个文档，每个文件导入为一条笔记（支持 docx / md / txt / html，导入到当前选中的分类）" @click="importDocs">
            {{ importing ? "导入中…" : "📚 批量" }}
          </button>
          <button class="btn xs primary" @click="createNote">＋ 新建</button>
        </div>
      </div>
      <div class="list">
        <div
          v-for="(note, i) in noteView"
          :key="note.id"
          :data-id="note.id"
          class="list-item"
          :class="{ active: note.id === activeId, dragging: nDragIndex === i }"
          title="按住可直接拖动排序"
          @click="selectNote(note)"
          @contextmenu="openCtx($event, note)"
          @pointerdown="onNoteItemDown($event, i)"
        >
          <div class="item-top">
            <span class="title" :title="note.title || '无标题'">{{ note.title || "无标题" }}</span>
            <span
              class="star"
              :class="{ on: note.favorite }"
              @click.stop="toggleFavorite(note)"
              >{{ note.favorite ? "★" : "☆" }}</span
            >
          </div>
          <div class="meta">
            <span v-if="searchQuery.trim()" class="nb-tag" :title="note.notebook || '默认'">📗 {{ note.notebook || "默认" }}</span>
            {{ new Date(note.updated_at).toLocaleString() }}
          </div>
        </div>
        <div v-if="!noteView.length" class="empty">
          {{
            searchQuery.trim()
              ? "没有匹配的笔记"
              : activeNotebook && isNbLocked(activeNotebook)
              ? "🔒 该笔记本已加密，点击左侧笔记本输入密码查看"
              : favoriteOnly
                ? "暂无收藏笔记"
                : activeNotebook
                  ? "该笔记本暂无笔记"
                  : "暂无笔记，点击「新建」开始（右键可导出/删除）"
          }}
        </div>
      </div>
    </aside>
    <div
      class="edge-handle"
      :title="listCollapsed ? '展开列表栏' : '折叠列表栏'"
      @click="toggleList"
    >
      <span class="handle-grip">{{ listCollapsed ? "⟩" : "⟨" }}</span>
    </div>
    <div class="col-resizer" v-show="!listCollapsed" title="拖动调整列表宽度" @mousedown="onResizeDown"></div>

    <!-- 右键菜单 -->
    <div
      v-if="ctxMenu"
      class="ctx-menu"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
    >
      <div class="item" @click="exportMd(ctxMenu.note)">📤 导出 Markdown (.md)</div>
      <div class="item" @click="exportDoc(ctxMenu.note)">📄 导出 Word (.doc)</div>
      <div class="item" @click="exportXlsx(ctxMenu.note)">📊 导出 Excel (.xlsx)</div>
      <div class="sep"></div>
      <div class="item" @click="toggleFavorite(ctxMenu.note); ctxMenu = null">
        {{ ctxMenu.note.favorite ? "☆ 取消收藏" : "⭐ 收藏" }}
      </div>
      <div class="item danger" @click="removeNote(ctxMenu.note)">🗑️ 删除（进回收站）</div>
    </div>

    <!-- 图片双击放大预览：滚轮缩放 / 拖动平移 / 双击切换 / 点空白或 Esc 关闭 -->
    <div
      v-if="previewSrc"
      class="img-preview"
      :title="pvScale > 1.01 ? '拖动移动 · 滚轮缩放 · 双击复原 · 点空白或 Esc 关闭' : '滚轮缩放 · 双击放大 · 点空白或 Esc 关闭'"
      @wheel.prevent="onPreviewWheel"
      @mousedown="onPreviewMouseDown"
      @click.self="onPreviewBgClick"
      @dblclick="onPreviewDblclick"
    >
      <img :src="previewSrc" alt="预览" :style="pvImgStyle" draggable="false" />
      <div v-if="pvScale > 1.01" class="pv-zoom-badge">{{ Math.round(pvScale * 100) }}%</div>
    </div>

    <!-- 笔记本密码对话框：解锁 / 设置 / 移除 -->
    <div v-if="nbDialog" class="nb-dialog-mask" @click.self="nbDialog = null">
      <div class="nb-dialog">
        <h3>
          {{
            nbDialog.mode === "unlock"
              ? `🔒 解锁「${nbDialog.nb}」`
              : nbDialog.mode === "set"
                ? `🔑 设置「${nbDialog.nb}」的密码`
                : `🔓 移除「${nbDialog.nb}」的密码`
          }}
        </h3>
        <label>{{ nbDialog.mode === "set" ? "设置密码" : "输入密码" }}</label>
        <input
          v-model="nbPwd"
          type="password"
          autofocus
          placeholder="笔记本密码"
          @keyup.enter="confirmNbDialog"
        />
        <template v-if="nbDialog.mode === 'set'">
          <label>确认密码</label>
          <input v-model="nbPwd2" type="password" placeholder="再次输入" @keyup.enter="confirmNbDialog" />
        </template>
        <div v-if="nbErr" class="nb-err">{{ nbErr }}</div>
        <div class="nb-dialog-actions">
          <button class="btn sm" @click="nbDialog = null">取消</button>
          <button class="btn sm primary" @click="confirmNbDialog">
            {{ nbDialog.mode === "unlock" ? "解锁" : nbDialog.mode === "set" ? "保存" : "移除" }}
          </button>
        </div>
        <div v-if="nbDialog.mode !== 'set'" class="nb-hint">
          加密笔记本的笔记在解锁前不会显示标题与内容
        </div>
      </div>
    </div>

    <section class="editor-panel">
      <template v-if="activeNote">
        <div class="title-row">
          <input
            v-model="editTitle"
            class="title-input"
            placeholder="笔记标题（点击此处直接修改）"
            @input="scheduleSave"
          />
          <select
            class="nb-select"
            title="所属笔记本"
            :value="activeNote.notebook || '默认'"
            @change="changeNotebook"
          >
            <option v-for="nb in notebooks" :key="nb" :value="nb">{{ nb }}</option>
          </select>
        </div>
        <div class="toolbar">
          <template v-if="mode === 'rich'">
            <button title="标题1" @mousedown.prevent @click="insertHeading('h1')">H1</button>
            <button title="标题2" @mousedown.prevent @click="insertHeading('h2')">H2</button>
            <button title="标题3" @mousedown.prevent @click="insertHeading('h3')">H3</button>
            <button title="加粗" @mousedown.prevent @click="exec('bold')"><b>B</b></button>
            <button title="斜体" @mousedown.prevent @click="exec('italic')"><i>I</i></button>
            <button title="下划线" @mousedown.prevent @click="exec('underline')"><u>U</u></button>
            <button title="删除线" @mousedown.prevent @click="exec('strikeThrough')">S̶</button>
            <span class="divider"></span>
            <button title="无序列表" @mousedown.prevent @click="exec('insertUnorderedList')">•≡</button>
            <button title="有序列表" @mousedown.prevent @click="exec('insertOrderedList')">1≡</button>
            <button title="引用" @mousedown.prevent @click="insertHeading('blockquote')">❝</button>
            <span class="divider"></span>
            <button title="插入表格" @mousedown.prevent @click="insertTable">▦</button>
            <button title="插入图片（也可直接粘贴截图）" @mousedown.prevent @click="insertImageFile">🖼️</button>
            <button title="插入分割线" @mousedown.prevent @click="insertHr">—</button>
            <button title="撤销（表格删除等操作也可撤销）" @mousedown.prevent @click="undoEdit">↩</button>
            <button title="重做" @mousedown.prevent @click="redoEdit">↪</button>
          </template>
          <template v-else>
            <span class="mode-tip">Markdown 源码模式 · # 标题 / **粗体** / | 表格 | / ![图片]()</span>
          </template>
          <span class="spacer"></span>
          <span class="paste-tip">💡 支持粘贴截图和表格</span>
          <button
            class="btn sm"
            :class="{ primary: mode === 'markdown' }"
            title="切换到 Markdown 源码"
            @click="switchMode('markdown')"
          >
            MD
          </button>
          <button
            class="btn sm"
            :class="{ primary: mode === 'rich' }"
            title="切换到富文本"
            @click="switchMode('rich')"
          >
            富文本
          </button>
        </div>

        <!-- 编辑区滚动容器：子工具栏 sticky 钉在顶部（固定位置、紧凑、不跟随鼠标） -->
        <div v-show="mode === 'rich'" class="editor-wrap">
          <div v-if="tableCtx" class="sub-toolbar">
            <span class="sub-label">表格</span>
            <button @mousedown.prevent @click="tInsertRow(false)">↑行</button>
            <button @mousedown.prevent @click="tInsertRow(true)">↓行</button>
            <button @mousedown.prevent @click="tInsertCol(false)">←列</button>
            <button @mousedown.prevent @click="tInsertCol(true)">→列</button>
            <button @mousedown.prevent @click="tDeleteRow()">删行</button>
            <button @mousedown.prevent @click="tDeleteCol()">删列</button>
            <button class="sub-danger" @mousedown.prevent @click="tDeleteTable">删表</button>
          </div>
          <div v-else-if="imageCtx" class="sub-toolbar">
            <span class="sub-label">图片</span>
            <button @mousedown.prevent @click="imgSetWidth(25)">25%</button>
            <button @mousedown.prevent @click="imgSetWidth(50)">50%</button>
            <button @mousedown.prevent @click="imgSetWidth(75)">75%</button>
            <button @mousedown.prevent @click="imgSetWidth(100)">100%</button>
            <button class="sub-danger" @mousedown.prevent @click="imgDelete">删除</button>
          </div>

          <!-- 富文本模式 -->
          <div
            v-show="mode === 'rich'"
            ref="editorEl"
            class="rich-editor"
            contenteditable="true"
            @input="onEditorInput"
            @paste="onPaste"
            @click="onEditorClick"
            @dblclick="onEditorDblclick"
            @keydown="onEditorKeydown"
            @dragstart="onEditorDragStart"
            @selectionchange="onEditorSelectionChange"
            @mousemove="onEditorMouseMove"
            @mousedown="onEditorMouseDown"
          ></div>
        </div>

        <!-- Markdown 模式 -->
        <textarea
          v-show="mode === 'markdown'"
          v-model="mdText"
          class="md-editor"
          placeholder="使用 Markdown 语法书写，例如：&#10;# 标题&#10;**加粗** *斜体*&#10;- 列表项&#10;| 表格 | 头 |&#10;| --- | --- |"
          @input="scheduleSave"
          @keydown="onMdKeydown"
        ></textarea>

        <div class="status-bar">{{ saving ? "保存中..." : "自动保存已开启" }}</div>
      </template>
      <div v-else class="empty">选择或新建一篇笔记</div>
    </section>
  </div>
</template>

<style scoped>
.notes {
  display: flex;
  flex: 1;
  height: 100%;
  /* 侧栏全展开窗口放不下时：内容区保底宽度，视图出现横向滚动（大屏不受影响） */
  overflow-x: auto;
  overflow-y: hidden;
}
.list-panel {
  min-width: 220px;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  background: var(--panel);
  flex-shrink: 0;
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
.panel-head {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 12px 10px;
  border-bottom: 1px solid var(--border);
}
.head-title-row {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 操作行：两个小按钮，窄栏也不换行 */
.head-actions-row {
  display: flex;
  gap: 6px;
}
.head-actions-row .btn.xs {
  flex: 1;
  justify-content: center;
  padding: 4px 6px;
  font-size: 11px;
  white-space: nowrap;
}
.head-count {
  font-size: 11px;
  color: var(--muted);
  font-weight: 400;
  margin-left: 6px;
}
/* ---------- 第一层：笔记本分类栏 ---------- */
.nb-panel {
  border-right: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  padding: 10px 8px;
  overflow-y: auto;
  overflow-x: hidden;
  flex-shrink: 0;
}
/* 折叠后的图标窄条 */
.nb-mini {
  width: 44px;
  border-right: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 8px 0;
  gap: 4px;
  overflow-y: auto;
  flex-shrink: 0;
}
.mini-btn {
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  border-radius: 8px;
  font-size: 15px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text);
  flex-shrink: 0;
}
.mini-btn:hover {
  background: var(--hover-strong);
}
.mini-btn.on {
  background: var(--accent-soft);
}
.mini-btn.expand {
  color: var(--accent);
  background: var(--accent-soft);
  border: 1px solid var(--accent);
}
.nb-item-right {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}
.nb-lock-btn {
  border: none;
  background: transparent;
  cursor: pointer;
  font-size: 12px;
  padding: 1px 2px;
  border-radius: 4px;
  opacity: 0;
  transition: 0.12s;
}
.nb-item:hover .nb-lock-btn {
  opacity: 1;
}
.nb-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  user-select: none;
  color: var(--text);
  white-space: nowrap;
}
.nb-item:hover {
  background: var(--hover);
}
.nb-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.nb-item .badge {
  font-size: 11px;
  color: var(--muted);
  background: var(--hover-strong);
  border-radius: 9px;
  padding: 1px 7px;
  flex-shrink: 0;
}
.nb-item.active .badge {
  color: var(--accent);
  background: var(--card);
}
.nb-name {
  overflow: hidden;
  text-overflow: ellipsis;
}
.nb-sep {
  font-size: 11px;
  color: var(--muted);
  padding: 10px 10px 4px;
}
/* 全局搜索框（笔记本栏顶部） */
.nb-search {
  position: relative;
  padding: 8px 10px 4px;
}
.nb-search input {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 26px 7px 10px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text);
  font-size: 12.5px;
  outline: none;
}
.nb-search input:focus {
  border-color: var(--accent);
}
.nb-search .clear {
  position: absolute;
  right: 14px;
  top: 50%;
  transform: translateY(calc(-50% - 2px));
  border: none;
  background: none;
  color: var(--muted);
  font-size: 15px;
  cursor: pointer;
  padding: 2px 4px;
}
/* 搜索结果中的笔记本归属标签 */
.nb-tag {
  display: inline-block;
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--muted);
  margin-right: 6px;
}
.new-list {
  margin-top: auto;
  padding-top: 8px;
}
.new-list input {
  width: 100%;
  padding: 8px 10px;
  border: 1px dashed var(--border);
  border-radius: 8px;
  background: transparent;
  outline: none;
  font-size: 12px;
}
.new-list input:focus {
  border-color: var(--accent);
  border-style: solid;
}
.head-actions {
  display: flex;
  gap: 6px;
  align-items: center;
}
.btn.sm {
  padding: 5px 10px;
  font-size: 12px;
}
.list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}
.list-item {
  padding: 10px 12px;
  border-radius: 8px;
  cursor: pointer;
}
.list-item:hover {
  background: var(--hover);
}
.list-item.active {
  background: var(--accent-soft);
}
/* 拖拽排序：占位半透明 + 影子跟随 */
.list-item.drag-placeholder,
.nb-item.drag-placeholder,
.nb-item.dragging {
  opacity: 0.5;
  background: var(--accent-soft);
}
.drag-shadow {
  position: fixed;
  z-index: 1000;
  pointer-events: none;
  opacity: 0.92;
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.22);
  border-radius: 8px;
  background: var(--card);
  transform: scale(1.02);
}
.item-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.title {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.star {
  color: #ccc;
  font-size: 16px;
}
.star.on {
  color: var(--amber);
}
.meta {
  color: var(--muted);
  font-size: 12px;
  margin-top: 2px;
}
.editor-panel {
  flex: 1 0 400px; /* 内容区保底 400px，不足时整个视图横向滚动 */
  display: flex;
  flex-direction: column;
  background: var(--panel);
  min-width: 0;
  position: relative; /* 子工具栏浮层的定位基准 */
}
.title-row {
  display: flex;
  gap: 8px;
  margin: 14px 18px 0;
}
.title-input {
  flex: 1;
  padding: 10px 14px;
  font-size: 18px;
  font-weight: 700;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  outline: none;
  color: var(--text);
  transition: 0.15s;
}
.title-input:hover {
  border-color: var(--border);
}
.title-input:focus {
  border-color: var(--accent);
  background: var(--card);
}
.nb-select {
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  outline: none;
  cursor: pointer;
  align-self: center;
  max-width: 130px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
  flex-wrap: wrap;
}
.toolbar button {
  min-width: 30px;
  height: 30px;
  border: none;
  background: transparent;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text); /* 显式指定：暗黑下避免使用系统默认黑色 */
}
/* 表格/图片上下文子工具栏：sticky 钉在编辑区顶部，紧凑省空间、不跟随鼠标 */
.sub-toolbar {
  position: sticky;
  top: 8px;
  z-index: 40;
  align-self: center;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: var(--shadow);
  max-width: calc(100% - 32px);
}
.sub-toolbar .sub-label {
  font-size: 11px;
  color: var(--accent);
  font-weight: 600;
  margin-right: 2px;
}
.sub-toolbar .sub-tip {
  font-size: 11px;
  color: var(--muted);
}
.sub-toolbar button {
  padding: 3px 8px;
  border: 1px solid var(--border);
  background: var(--card);
  border-radius: 5px;
  font-size: 11px;
  cursor: pointer;
  color: var(--text);
  transition: 0.15s;
}
.sub-toolbar button:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.sub-toolbar .sub-danger {
  color: var(--red);
}
.sub-toolbar .sub-danger:hover {
  border-color: var(--red);
  color: var(--red);
}
.toolbar button:hover {
  background: var(--hover-strong);
}
.mode-tip,
.paste-tip {
  font-size: 12px;
  color: var(--muted);
}
.divider {
  width: 1px;
  height: 20px;
  background: var(--border);
  margin: 0 4px;
}
.spacer {
  flex: 1;
}
/* 编辑区滚动容器：子工具栏 sticky 钉顶，滚动时内容从其下穿过 */
.editor-wrap {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.rich-editor {
  padding: 20px 24px 40px;
  outline: none;
  font-size: 15px;
  line-height: 1.8;
}
/* 注意：编辑器内容是动态插入的 DOM，scoped 属性不会附加，
   必须用 :deep() 才能命中（否则表格边框/标题样式全部失效） */
.rich-editor :deep(h1) {
  font-size: 24px;
  margin: 12px 0 8px;
}
.rich-editor :deep(h2) {
  font-size: 20px;
  margin: 10px 0 6px;
}
.rich-editor :deep(h3) {
  font-size: 17px;
  margin: 8px 0 4px;
}
.rich-editor :deep(ul),
.rich-editor :deep(ol) {
  padding-left: 26px;
  margin: 6px 0;
}
.rich-editor :deep(li) {
  margin: 2px 0;
}
.rich-editor :deep(blockquote) {
  border-left: 3px solid var(--accent);
  padding: 4px 12px;
  margin: 8px 0;
  color: var(--muted);
  background: var(--hover);
  border-radius: 0 6px 6px 0;
}
.rich-editor :deep(img) {
  /* 独立成行：避免行内图片把光标行撑高（回车后长光标问题） */
  display: block;
  max-width: 100%;
  border-radius: 8px;
  margin: 8px 0;
  cursor: pointer;
  /* 禁用图片原生拖拽：否则单击/拖选经过图片会触发浏览器拖放，
     选区建立不起来 → Backspace、Ctrl+X、鼠标全选全部失效 */
  -webkit-user-drag: none;
  user-select: auto;
}
/* 图片选中态：彻底去掉浏览器默认的蓝色遮罩，仅用一条细描边做轻提示 */
.rich-editor :deep(img)::selection,
.rich-editor :deep(img)::-moz-selection {
  background-color: transparent;
  color: transparent;
  -webkit-tap-highlight-color: transparent;
}
.rich-editor :deep(img.selected-img) {
  outline: 1.5px solid var(--accent);
  outline-offset: 1px;
}
/* 双击放大预览层 */
.img-preview {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.75);
  z-index: 500;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: zoom-out;
}
.img-preview img {
  max-width: 92vw;
  max-height: 92vh;
  border-radius: 8px;
  box-shadow: 0 8px 40px rgba(0, 0, 0, 0.5);
  will-change: transform;
  user-select: none;
  -webkit-user-drag: none;
  /* 透明 PNG 底衬：棋盘格背景（与图片编辑器一致），透明区域不再透出黑色遮罩 */
  background-color: #ffffff;
  background-image:
    linear-gradient(45deg, #e3e3e3 25%, transparent 25%, transparent 75%, #e3e3e3 75%),
    linear-gradient(45deg, #e3e3e3 25%, transparent 25%, transparent 75%, #e3e3e3 75%);
  background-size: 16px 16px;
  background-position: 0 0, 8px 8px;
}
/* 预览层缩放比例角标 */
.pv-zoom-badge {
  position: fixed;
  bottom: 18px;
  left: 50%;
  transform: translateX(-50%);
  background: rgba(0, 0, 0, 0.65);
  color: #fff;
  font-size: 12px;
  line-height: 1;
  padding: 5px 12px;
  border-radius: 999px;
  pointer-events: none;
  z-index: 501;
}
/* 表格边框（Word 导入的表格 mammoth 不带样式，统一显示边框） */
.rich-editor :deep(table),
.rich-editor :deep(tr),
.rich-editor :deep(tbody),
.rich-editor :deep(thead) {
  background: transparent;
}
.rich-editor :deep(table) {
  border-collapse: collapse;
  margin: 10px 0;
  min-width: 60%;
}
.rich-editor :deep(th),
.rich-editor :deep(td) {
  border: 1px solid var(--border);
  padding: 6px 12px;
  min-width: 32px;
  /* 兜底：剥离历史内容遗留的内联底色，让单元格跟随主题（暗黑下不再有固定白底） */
  background: transparent;
}
.rich-editor :deep(th) {
  background: var(--hover);
  font-weight: 600;
}
.rich-editor :deep(pre) {
  background: var(--hover);
  padding: 12px;
  border-radius: 8px;
  overflow-x: auto;
}
.rich-editor :deep(a) {
  color: var(--accent);
}
/* 右键菜单 */
.ctx-menu {
  position: fixed;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--shadow);
  padding: 6px;
  z-index: 300;
  min-width: 180px;
}
.ctx-menu .item {
  padding: 9px 12px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  white-space: nowrap;
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
.md-editor {
  flex: 1;
  border: none;
  outline: none;
  resize: none;
  padding: 20px 24px;
  font-size: 14px;
  line-height: 1.8;
  font-family: Consolas, "Courier New", monospace;
  background: var(--hover);
}
.status-bar {
  padding: 8px 20px;
  color: var(--muted);
  font-size: 12px;
  border-top: 1px solid var(--border);
}
.empty {
  color: var(--muted);
  text-align: center;
  padding: 40px;
}
/* 笔记本密码对话框 */
.nb-dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  z-index: 400;
  display: flex;
  align-items: center;
  justify-content: center;
}
.nb-dialog {
  width: 340px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  box-shadow: var(--shadow);
  padding: 20px;
}
.nb-dialog h3 {
  font-size: 15px;
  margin-bottom: 12px;
}
.nb-dialog label {
  display: block;
  font-size: 12px;
  color: var(--muted);
  margin: 10px 0 4px;
}
.nb-dialog input {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  outline: none;
  background: var(--panel);
  color: var(--text);
}
.nb-dialog input:focus {
  border-color: var(--accent);
}
.nb-err {
  color: var(--red);
  font-size: 12px;
  margin-top: 8px;
}
.nb-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.nb-hint {
  font-size: 11px;
  color: var(--muted);
  margin-top: 12px;
}
</style>

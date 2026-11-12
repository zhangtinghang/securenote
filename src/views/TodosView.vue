<script setup lang="ts">
import { ref, onMounted, computed, watch, inject } from "vue";
import { todos, notifications } from "../api";
import type { Todo } from "../types";
import { useResizable } from "../useResizable";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const todoList = ref<Todo[]>([]);const selected = ref<Todo | null>(null);
const panelMode = ref<"none" | "detail" | "create">("none");
const toast = ref("");

/** 分组模式：manual=手动(可拖拽) time=按时间 priority=按优先级（清单由左侧分类栏负责） */
const groupMode = ref<"manual" | "time" | "priority">("manual");

/** 中栏列表宽度拖拽调节 */
const { width: listWidth, onDown: onResizeDown } = useResizable(
  "sn_todos_list_w",
  310,
  250
);

/** 左栏清单分类宽度拖拽调节 */
const { width: navW, onDown: onNavResizeDown } = useResizable(
  "sn_todos_nav_w",
  185,
  120,
  0.35
);

/** 清单栏折叠 */
const navCollapsed = ref(localStorage.getItem("sn_todo_nav_collapsed") === "1");
function toggleNav() {
  navCollapsed.value = !navCollapsed.value;
  localStorage.setItem("sn_todo_nav_collapsed", navCollapsed.value ? "1" : "0");
}

/* ---------- 左侧清单分类栏（类 Microsoft To Do） ---------- */

const currentList = ref("__all"); // __all | __fav | __done | 清单名
const newListName = ref("");
/** 用户创建过的清单（localStorage 持久化，空清单也能保留） */
const createdLists = ref<string[]>(JSON.parse(localStorage.getItem("sn_lists") || "[]"));

const currentListLabel = computed(() => {
  if (currentList.value === "__all") return "全部待办";
  if (currentList.value === "__fav") return "⭐ 收藏";
  if (currentList.value === "__done") return "✅ 已完成";
  return currentList.value;
});

/** 当前分类过滤后的待办 */
const baseTodos = computed<Todo[]>(() => {
  if (currentList.value === "__all") return todoList.value;
  if (currentList.value === "__fav") return todoList.value.filter((t) => t.favorite);
  if (currentList.value === "__done") return todoList.value.filter((t) => t.completed);
  return todoList.value.filter((t) => (t.list || "默认") === currentList.value);
});

/** 清单列表（去重 + 未完成计数 + 用户新建但还没有待办的清单） */
const lists = computed(() => {
  const map = new Map<string, number>();
  todoList.value
    .filter((t) => !t.completed)
    .forEach((t) => {
      const k = t.list || "默认";
      map.set(k, (map.get(k) || 0) + 1);
    });
  createdLists.value.forEach((name) => {
    if (!map.has(name)) map.set(name, 0);
  });
  // 按用户拖拽的顺序显示，未记录的清单保持原序排在后面
  const pos = new Map(listOrderIds.value.map((n, i) => [n, i]));
  return [...map.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => {
      const pa = pos.has(a.name) ? pos.get(a.name)! : Number.MAX_SAFE_INTEGER;
      const pb = pos.has(b.name) ? pos.get(b.name)! : Number.MAX_SAFE_INTEGER;
      return pa - pb;
    });
});

/** 清单拖拽顺序（localStorage 持久化） */
const LIST_ORDER_KEY = "sn_todo_lists_order";
const listOrderIds = ref<string[]>(JSON.parse(localStorage.getItem(LIST_ORDER_KEY) || "[]"));

function commitListOrder() {
  listOrderIds.value = listView.value.map((l) => l.name);
  localStorage.setItem(LIST_ORDER_KEY, JSON.stringify(listOrderIds.value));
}

/* ---------- 清单栏拖拽排序（与待办列表同款：影子 + FLIP） ---------- */

const listView = ref<{ name: string; count: number }[]>([]);
function syncListView() {
  listView.value = lists.value;
}
watch(lists, () => {
  if (!listDragState) syncListView();
});
syncListView();

const listDragIdx = ref<number | null>(null);
interface ListDragState { itemEl: HTMLElement; shadow: HTMLElement; offsetY: number }
let listDragState: ListDragState | null = null;

function visibleListItems(): HTMLElement[] {
  const box = listDragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const l of listView.value) {
    const el = box.querySelector(`.cat-item[data-id="${l.name}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

function listIndexFromY(clientY: number): number {
  const items = visibleListItems();
  if (!items.length) return 0;
  let idx = 0;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY > r.top + r.height / 2) idx++;
  }
  return Math.min(idx, items.length);
}

let listPress: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onListItemDown(e: PointerEvent, i: number) {
  if (listDragState) return;
  const t = e.target as HTMLElement;
  if (t.closest("input, button")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".cat-item") as HTMLElement;
  if (!itemEl) return;
  listPress = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onListPressMove);
  window.addEventListener("pointerup", onListPressUp);
  window.addEventListener("pointercancel", onListPressUp);
}

function onListPressMove(e: PointerEvent) {
  if (!listPress) return;
  if (Math.hypot(e.clientX - listPress.x, e.clientY - listPress.y) < 6) return;
  const { i, itemEl } = listPress;
  releaseListPress();
  startListDrag(e, i, itemEl);
}

function onListPressUp() {
  releaseListPress();
}

function releaseListPress() {
  listPress = null;
  window.removeEventListener("pointermove", onListPressMove);
  window.removeEventListener("pointerup", onListPressUp);
  window.removeEventListener("pointercancel", onListPressUp);
}

function startListDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (listDragState || listView.value.length < 2) return;
  document.body.style.userSelect = "none";
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  shadow.style.left = rect.left + "px";
  shadow.style.top = rect.top + "px";
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  listDragState = { itemEl, shadow, offsetY: e.clientY - rect.top };
  listDragIdx.value = i;
  window.addEventListener("pointermove", onListPointerMove);
  window.addEventListener("pointerup", onListPointerUp);
  window.addEventListener("pointercancel", onListPointerUp);
}

function onListPointerMove(e: PointerEvent) {
  if (!listDragState) return;
  listDragState.shadow.style.top = e.clientY - listDragState.offsetY + "px";
  const from = listDragIdx.value!;
  const target = listIndexFromY(e.clientY);
  if (target === from) return;
  const arr = listView.value;
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  listDragIdx.value = to;
}

function onListPointerUp() {
  window.removeEventListener("pointermove", onListPointerMove);
  window.removeEventListener("pointerup", onListPointerUp);
  window.removeEventListener("pointercancel", onListPointerUp);
  document.body.style.userSelect = "";
  if (!listDragState) return;
  listDragState.shadow.remove();
  listDragState.itemEl.classList.remove("drag-placeholder");
  listDragState = null;
  if (listDragIdx.value === null) return;
  listDragIdx.value = null;
  commitListOrder();
}

function addList() {
  const name = newListName.value.trim();
  if (!name) return;
  if (!lists.value.some((l) => l.name === name)) {
    createdLists.value.push(name);
    localStorage.setItem("sn_lists", JSON.stringify(createdLists.value));
  }
  currentList.value = name;
  newListName.value = "";
}

/* ---------- 时间转换：本地输入格式 ↔ ISO（UTC）存储 ---------- */

function toLocalInput(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function toIso(local: string): string | null {
  if (!local) return null;
  const d = new Date(local);
  if (isNaN(d.getTime())) return null;
  return d.toISOString();
}

/* ---------- 详情编辑表单 ---------- */

const detail = ref({
  title: "",
  note: "",
  list: "默认",
  priority: "medium" as "high" | "medium" | "low",
  due_at: "",
  remind_at: "",
  repeat: "none" as "none" | "daily" | "weekly" | "monthly" | "weekday",
});

function fillDetail(todo: Todo) {
  detail.value = {
    title: todo.title,
    note: todo.note,
    list: todo.list,
    priority: todo.priority as "high" | "medium" | "low",
    due_at: toLocalInput(todo.due_at),
    remind_at: toLocalInput(todo.remind_at),
    repeat: todo.repeat as typeof detail.value.repeat,
  };
}

/* ---------- 排序与分组 ---------- */

/** 手动排序：置顶 > sort_order 升序 */
function byManual(a: Todo, b: Todo): number {
  if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
  return a.sort_order - b.sort_order;
}

/** 手动模式完整列表：未完成在前，已完成置底（基于当前分类过滤结果） */
const manualSorted = computed(() => {
  const pending = baseTodos.value.filter((t) => !t.completed).sort(byManual);
  const done = baseTodos.value.filter((t) => t.completed).sort(byManual);
  return [...pending, ...done];
});

/**
 * 拖拽排序：Pointer 事件自研拖拽
 * - 影子（克隆元素）实时跟随鼠标
 * - 其他项 FLIP 动画平滑让位（160ms transform 过渡）
 * - 松手持久化 sort_order
 */
const pendingDrag = ref<Todo[]>([]);
const dragIndex = ref<number | null>(null);

interface DragState {
  itemEl: HTMLElement; // 占位（原元素）
  shadow: HTMLElement; // 跟随鼠标的克隆
  offsetY: number;
  pointerId: number;
}
let dragState: DragState | null = null;

watch(
  [todoList, currentList, groupMode],
  () => {
    if (groupMode.value !== "manual") return;
    pendingDrag.value = baseTodos.value
      .filter((t) => !t.completed)
      .sort(byManual);
  },
  { deep: true }
);

/** 列表内可见项（排除影子）：严格按 pendingDrag 数组顺序收集元素引用 */
function visibleItems(): HTMLElement[] {
  const box = dragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const t of pendingDrag.value) {
    const el = box.querySelector(`[data-id="${t.id}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

/** 按鼠标 Y 计算应插入的数组索引（直接读元素视口矩形，无 transform 干扰） */
function indexFromY(clientY: number): number {
  const items = visibleItems();
  if (!items.length) return 0;
  let idx = 0;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY > r.top + r.height / 2) idx++;
  }
  // idx：插到数组第 idx 项之前，取值 0..n（=n 表示最后）
  return Math.min(idx, items.length);
}

function startDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (dragState) return;
  document.body.style.userSelect = "none";
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  shadow.style.left = rect.left + "px";
  shadow.style.top = rect.top + "px";
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  dragState = { itemEl, shadow, offsetY: e.clientY - rect.top, pointerId: e.pointerId };
  dragIndex.value = i;
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  window.addEventListener("pointercancel", onPointerUp);
}

/* 整行直接按住拖动：按下后移动超过阈值才判定为拖拽，否则仍是普通点击 */
const DRAG_THRESHOLD = 6;
let press: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onItemDown(e: PointerEvent, i: number) {
  if (dragState || groupMode.value !== "manual") return;
  const t = e.target as HTMLElement;
  // 表单控件/按钮上的按下不启动拖拽（勾选、快捷按钮等保持原行为）
  if (t.closest("input, button, textarea, select, a")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".todo-item") as HTMLElement;
  if (!itemEl) return;
  press = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onPressMove);
  window.addEventListener("pointerup", onPressUp);
  window.addEventListener("pointercancel", onPressUp);
}

function onPressMove(e: PointerEvent) {
  if (!press) return;
  if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_THRESHOLD) return;
  const { i, itemEl } = press;
  releasePress();
  startDrag(e, i, itemEl);
}

function onPressUp() {
  releasePress();
}

function releasePress() {
  press = null;
  window.removeEventListener("pointermove", onPressMove);
  window.removeEventListener("pointerup", onPressUp);
  window.removeEventListener("pointercancel", onPressUp);
}

function onPointerMove(e: PointerEvent) {
  if (!dragState) return;
  dragState.shadow.style.top = e.clientY - dragState.offsetY + "px";
  const from = dragIndex.value!;
  const target = indexFromY(e.clientY);
  if (target === from) return;
  // 重排数组：target 是"含被拖项列表"的插入点，移除被拖项后向下插入需 -1 修正
  const arr = pendingDrag.value;
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  dragIndex.value = to;
  // 不再做 FLIP 反向位移：直接重排，元素自然让位（靠 CSS transition 平滑）。
  // 此前 FLIP 用 getBoundingClientRect 读 transform 位移后的坐标，导致索引逐格偏后。
}

function onPointerUp() {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
  window.removeEventListener("pointercancel", onPointerUp);
  document.body.style.userSelect = "";
  if (!dragState) return;
  dragState.shadow.remove();
  dragState.itemEl.classList.remove("drag-placeholder");
  dragState = null;
  if (dragIndex.value === null) return;
  dragIndex.value = null;
  // 持久化新顺序
  const ids = pendingDrag.value.map((t) => t.id);
  ids.forEach((id, i) => {
    const t = todoList.value.find((x) => x.id === id);
    if (t) t.sort_order = i;
  });
  todos.reorder(ids).catch((err) => showToast(String(err)));
}

const doneItems = computed(() =>
  manualSorted.value.filter((t) => t.completed)
);

/* ---------- 已完成列表拖拽（手动模式下可拖动排序） ---------- */

const doneOrder = ref<Todo[]>([]);
const doneDragIdx = ref<number | null>(null);
interface DoneDragState { itemEl: HTMLElement; shadow: HTMLElement; offsetY: number }
let doneDragState: DoneDragState | null = null;

function syncDoneOrder() {
  doneOrder.value = baseTodos.value.filter((t) => t.completed).sort(byManual);
}
watch([todoList, currentList], () => {
  if (!doneDragState) syncDoneOrder();
}, { deep: true });
syncDoneOrder();

function visibleDoneItems(): HTMLElement[] {
  const box = doneDragState?.itemEl.parentElement;
  if (!box) return [];
  const refs: HTMLElement[] = [];
  for (const t of doneOrder.value) {
    const el = box.querySelector(`[data-id="${t.id}"]`) as HTMLElement | null;
    if (el) refs.push(el);
  }
  return refs;
}

function doneIndexFromY(clientY: number): number {
  const items = visibleDoneItems();
  if (!items.length) return 0;
  let idx = 0;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY > r.top + r.height / 2) idx++;
  }
  return Math.min(idx, items.length);
}

let donePress: { x: number; y: number; i: number; itemEl: HTMLElement } | null = null;

function onDoneItemDown(e: PointerEvent, i: number) {
  if (doneDragState || groupMode.value !== "manual") return;
  const t = e.target as HTMLElement;
  if (t.closest("input, button")) return;
  const itemEl = (e.currentTarget as HTMLElement).closest(".todo-item") as HTMLElement;
  if (!itemEl) return;
  donePress = { x: e.clientX, y: e.clientY, i, itemEl };
  window.addEventListener("pointermove", onDonePressMove);
  window.addEventListener("pointerup", onDonePressUp);
  window.addEventListener("pointercancel", onDonePressUp);
}

function onDonePressMove(e: PointerEvent) {
  if (!donePress) return;
  if (Math.hypot(e.clientX - donePress.x, e.clientY - donePress.y) < 6) return;
  const { i, itemEl } = donePress;
  releaseDonePress();
  startDoneDrag(e, i, itemEl);
}

function onDonePressUp() {
  releaseDonePress();
}

function releaseDonePress() {
  donePress = null;
  window.removeEventListener("pointermove", onDonePressMove);
  window.removeEventListener("pointerup", onDonePressUp);
  window.removeEventListener("pointercancel", onDonePressUp);
}

function startDoneDrag(e: PointerEvent, i: number, itemEl: HTMLElement) {
  if (doneDragState || doneOrder.value.length < 2) return;
  document.body.style.userSelect = "none";
  const rect = itemEl.getBoundingClientRect();
  const shadow = itemEl.cloneNode(true) as HTMLElement;
  shadow.classList.add("drag-shadow");
  shadow.style.width = rect.width + "px";
  shadow.style.left = rect.left + "px";
  shadow.style.top = rect.top + "px";
  document.body.appendChild(shadow);
  itemEl.classList.add("drag-placeholder");
  doneDragState = { itemEl, shadow, offsetY: e.clientY - rect.top };
  doneDragIdx.value = i;
  window.addEventListener("pointermove", onDonePointerMove);
  window.addEventListener("pointerup", onDonePointerUp);
  window.addEventListener("pointercancel", onDonePointerUp);
}

function onDonePointerMove(e: PointerEvent) {
  if (!doneDragState) return;
  doneDragState.shadow.style.top = e.clientY - doneDragState.offsetY + "px";
  const from = doneDragIdx.value!;
  const target = doneIndexFromY(e.clientY);
  if (target === from) return;
  const arr = doneOrder.value;
  const [moved] = arr.splice(from, 1);
  const to = target > from ? target - 1 : target;
  arr.splice(Math.max(0, Math.min(to, arr.length)), 0, moved);
  doneDragIdx.value = to;
}

function onDonePointerUp() {
  window.removeEventListener("pointermove", onDonePointerMove);
  window.removeEventListener("pointerup", onDonePointerUp);
  window.removeEventListener("pointercancel", onDonePointerUp);
  document.body.style.userSelect = "";
  if (!doneDragState) return;
  doneDragState.shadow.remove();
  doneDragState.itemEl.classList.remove("drag-placeholder");
  doneDragState = null;
  if (doneDragIdx.value === null) return;
  doneDragIdx.value = null;
  // 持久化已完成区新顺序（分区内重排，不影响未完成区）
  const ids = doneOrder.value.map((t) => t.id);
  ids.forEach((id, i) => {
    const t = todoList.value.find((x) => x.id === id);
    if (t) t.sort_order = i;
  });
  todos.reorder(ids).catch((err) => showToast(String(err)));
}

/** 时间分组 */
function timeGroupOf(todo: Todo): string {
  if (!todo.due_at) return "无日期";
  const d = new Date(todo.due_at);
  if (isNaN(d.getTime())) return "无日期";
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const day = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const diff = Math.round((day.getTime() - today.getTime()) / 86400000);
  if (diff < 0) return "已逾期";
  if (diff === 0) return "今天";
  if (diff === 1) return "明天";
  if (diff <= 7) return "本周内";
  return "以后";
}

interface Group {
  key: string;
  items: Todo[];
}

const grouped = computed<Group[]>(() => {
  const pending = baseTodos.value.filter((t) => !t.completed);
  if (groupMode.value === "time") {
    const order = ["已逾期", "今天", "明天", "本周内", "以后", "无日期"];
    return order
      .map((key) => ({ key, items: pending.filter((t) => timeGroupOf(t) === key).sort(byManual) }))
      .filter((g) => g.items.length);
  }
  if (groupMode.value === "priority") {
    const order: Array<[string, string]> = [["high", "高优先级"], ["medium", "中优先级"], ["low", "低优先级"]];
    return order
      .map(([key, label]) => ({ key: label, items: pending.filter((t) => t.priority === key).sort(byManual) }))
      .filter((g) => g.items.length);
  }
  return [];
});

const pendingCount = computed(() => todoList.value.filter((t) => !t.completed).length);
const doneCount = computed(() => todoList.value.filter((t) => t.completed).length);

/** 当前分类下的待办数（列表标题右侧） */
const currentListCount = computed(
  () => baseTodos.value.filter((t) => !t.completed).length
);

/** 常用清单建议 */
const listSuggestions = computed(() => {
  const base = ["默认", "工作", "个人", "学习"];
  const used = todoList.value.map((t) => t.list);
  return [...new Set([...base, ...used, ...createdLists.value])];
});

function selectTodo(todo: Todo) {
  // 已完成的待办：点击列表项只打开详情；恢复需点击左侧勾选框
  selected.value = todo;
  panelMode.value = "detail";
  fillDetail(todo);
}

/** 恢复已完成 → 待办（独立于 toggle，直接置 false，不依赖取反） */
async function restoreTodo(todo: Todo) {
  try {
    const res = await todos.toggle(todo.id, false);
    const saved: Todo = res ?? {
      ...todo,
      completed: false,
      completed_at: null,
    };
    const idx = todoList.value.findIndex((t) => t.id === todo.id);
    if (idx >= 0) todoList.value[idx] = saved;
    if (selected.value?.id === saved.id) selected.value = saved;
    if (saved.remind_at) {
      notifications.schedule(saved.id, saved.remind_at, saved.title);
    }
    showToast("已恢复为待办");
  } catch (e) {
    showToast(String(e));
  }
}

function openCreate() {
  selected.value = null;
  panelMode.value = "create";
  // 在某个清单分类下新建时，自动归属该清单
  const list = ["__all", "__fav", "__done"].includes(currentList.value)
    ? "默认"
    : currentList.value;
  detail.value = {
    title: "",
    note: "",
    list,
    priority: "medium",
    due_at: "",
    remind_at: "",
    repeat: "none",
  };
}

function showToast(msg: string) {
  toast.value = msg;
  setTimeout(() => (toast.value = ""), 2500);
}

async function load() {
  todoList.value = await todos.list();
}

async function createTodo() {
  if (!detail.value.title.trim()) {
    showToast("请输入标题");
    return;
  }
  const todo = await todos.create({
    title: detail.value.title,
    note: detail.value.note,
    list: detail.value.list,
    priority: detail.value.priority,
    due_at: toIso(detail.value.due_at),
    remind_at: toIso(detail.value.remind_at),
    repeat: detail.value.repeat,
  });
  todoList.value.unshift(todo);
  if (todo.remind_at) notifications.schedule(todo.id, todo.remind_at, todo.title);
  showToast("✓ 已创建");
  selectTodo(todo);
}

async function saveDetail() {
  if (!selected.value) return;
  if (!detail.value.title.trim()) {
    showToast("请输入标题");
    return;
  }
  const updated: Todo = {
    ...selected.value,
    title: detail.value.title,
    note: detail.value.note,
    list: detail.value.list,
    priority: detail.value.priority,
    due_at: toIso(detail.value.due_at),
    remind_at: toIso(detail.value.remind_at),
    repeat: detail.value.repeat,
  };
  const saved = await todos.update(updated);
  const idx = todoList.value.findIndex((t) => t.id === saved.id);
  if (idx >= 0) todoList.value[idx] = saved;
  selected.value = saved;
  if (saved.remind_at) {
    notifications.schedule(saved.id, saved.remind_at, saved.title);
  } else {
    notifications.cancel(saved.id);
  }
  showToast("✓ 已保存");
}

async function toggle(todo: Todo, ev?: Event) {
  ev?.stopPropagation();
  todo.completed = !todo.completed;
  const res = await todos.toggle(todo.id, todo.completed);
  // 防御：后端旧版本返回 null 时本地构造，避免 null 进入列表导致白屏
  const saved: Todo = res ?? {
    ...todo,
    completed: todo.completed,
    completed_at: todo.completed ? new Date().toISOString() : null,
  };
  const idx = todoList.value.findIndex((t) => t.id === todo.id);
  if (idx >= 0) todoList.value[idx] = saved;
  if (selected.value?.id === saved.id) selected.value = saved;
  if (saved.completed) {
    notifications.cancel(saved.id);
  } else if (saved.remind_at) {
    notifications.schedule(saved.id, saved.remind_at, saved.title);
  }
}

async function togglePinned(todo: Todo, ev?: Event) {
  ev?.stopPropagation();
  todo.pinned = !todo.pinned;
  await todos.togglePinned(todo.id, todo.pinned);
  const idx = todoList.value.findIndex((t) => t.id === todo.id);
  if (idx >= 0) todoList.value[idx] = { ...todoList.value[idx], pinned: todo.pinned };
  showToast(todo.pinned ? "📌 已置顶" : "已取消置顶");
}

async function toggleFavorite(todo: Todo, ev?: Event) {
  ev?.stopPropagation();
  todo.favorite = !todo.favorite;
  await todos.toggleFavorite(todo.id, todo.favorite);
  const idx = todoList.value.findIndex((t) => t.id === todo.id);
  if (idx >= 0) todoList.value[idx] = { ...todoList.value[idx], favorite: todo.favorite };
  showToast(todo.favorite ? "⭐ 已收藏" : "已取消收藏");
}

async function removeTodo() {
  if (!selected.value) return;
  const ok = await appConfirm(
    `删除待办「${selected.value.title}」？\n删除后进入回收站，保留 15 天`,
    "删除"
  );
  if (!ok) return;
  await todos.remove(selected.value.id);
  todoList.value = todoList.value.filter((t) => t.id !== selected.value!.id);
  selected.value = null;
  panelMode.value = "none";
  showToast("已移入回收站");
}

/** 点击输入框直接弹出日期时间选择器（无需点右侧图标） */
function showPicker(e: Event) {
  const el = e.currentTarget as HTMLInputElement;
  try {
    el.showPicker?.();
  } catch {
    /* 部分环境不支持则忽略 */
  }
}

function fmtTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return d.toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

const repeatLabel: Record<string, string> = {
  none: "",
  daily: "🔁 每天",
  weekday: "🔁 工作日",
  weekly: "🔁 每周",
  monthly: "🔁 每月",
};

onMounted(load);
</script>

<template>
  <div class="todos">
    <!-- 左栏：清单分类（折叠为图标窄条） -->
    <aside v-if="navCollapsed" class="nav-mini">
      <button class="mini-btn expand" title="展开清单栏" @click="toggleNav">
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
        :class="{ on: currentList === '__all' }"
        title="全部待办"
        @click="currentList = '__all'"
      >☑️</button>
      <button
        class="mini-btn"
        :class="{ on: currentList === '__fav' }"
        title="收藏"
        @click="currentList = '__fav'"
      >⭐</button>
      <button
        class="mini-btn"
        :class="{ on: currentList === '__done' }"
        title="已完成"
        @click="currentList = '__done'"
      >✅</button>
      <button
        v-for="l in lists"
        :key="l.name"
        class="mini-btn"
        :class="{ on: currentList === l.name }"
        :title="l.name"
        @click="currentList = l.name"
      >📋</button>
    </aside>
    <aside v-else class="nav-lists" :style="{ width: navW + 'px' }">
      <div
        class="cat-item"
        :class="{ active: currentList === '__all' }"
        @click="currentList = '__all'"
      >
        <span>☑️ 全部待办</span><span class="badge">{{ pendingCount }}</span>
      </div>
      <div
        class="cat-item"
        :class="{ active: currentList === '__fav' }"
        @click="currentList = '__fav'"
      >
        <span>⭐ 收藏</span>
      </div>
      <div
        class="cat-item"
        :class="{ active: currentList === '__done' }"
        @click="currentList = '__done'"
      >
        <span>✅ 已完成</span><span class="badge">{{ doneCount }}</span>
      </div>
      <div class="cat-sep">我的清单</div>
      <div
        v-for="(l, i) in listView"
        :key="l.name"
        :data-id="l.name"
        class="cat-item"
        :class="{ active: currentList === l.name, dragging: listDragIdx === i }"
        title="按住可直接拖动排序"
        @click="currentList = l.name"
        @pointerdown="onListItemDown($event, i)"
      >
        <span>📋 {{ l.name }}</span><span class="badge">{{ l.count }}</span>
      </div>
      <div class="new-list">
        <input
          v-model="newListName"
          placeholder="＋ 新建清单"
          @keyup.enter="addList"
        />
      </div>
    </aside>
    <div
      v-if="!navCollapsed"
      class="edge-handle"
      title="折叠清单栏"
      @click="toggleNav"
    >
      <span class="handle-grip">⟨</span>
    </div>
    <div class="col-resizer" v-show="!navCollapsed" title="拖动调整清单栏宽度" @mousedown="onNavResizeDown"></div>

    <!-- 中栏：待办列表 -->
    <aside class="list-panel" :style="{ width: listWidth + 'px' }">
      <div class="panel-head">
        <span>{{ currentListLabel }} <small class="count">未完成 {{ currentListCount }}</small></span>
        <button class="btn primary sm" @click="openCreate">＋ 新建</button>
      </div>
      <div class="group-switch">
        <button :class="{ on: groupMode === 'manual' }" @click="groupMode = 'manual'" title="手动排序（可拖拽）">手动</button>
        <button :class="{ on: groupMode === 'time' }" @click="groupMode = 'time'" title="按截止时间分组">时间</button>
        <button :class="{ on: groupMode === 'priority' }" @click="groupMode = 'priority'" title="按优先级分组">优先级</button>
      </div>

      <div class="list">
        <!-- 手动模式：整列可拖拽（Pointer 拖拽，影子跟随 + FLIP 平滑让位） -->
        <template v-if="groupMode === 'manual'">
          <div
            v-for="(todo, i) in pendingDrag"
            :key="todo.id"
            :data-id="todo.id"
            class="todo-item"
            :class="{ active: selected?.id === todo.id, dragging: dragIndex === i }"
            title="按住可直接拖动排序"
            @click="selectTodo(todo)"
            @pointerdown="onItemDown($event, i)"
          >
            <input
              type="checkbox"
              :checked="todo.completed"
              @click="toggle(todo, $event)"
            />
            <div class="todo-body">
              <div class="todo-title" :title="todo.title">
                <span v-if="todo.pinned" class="pin-flag">📌</span>
                <span v-if="todo.favorite" class="fav-flag">⭐</span>
                {{ todo.title }}
              </div>
              <div class="todo-meta">
                <span v-if="todo.list && todo.list !== '默认'" class="meta-list">{{ todo.list }}</span>
                <span v-if="todo.due_at">📅 {{ fmtTime(todo.due_at) }}</span>
                <span v-if="todo.remind_at && !todo.completed">⏰ {{ fmtTime(todo.remind_at) }}</span>
                <span v-if="repeatLabel[todo.repeat]">{{ repeatLabel[todo.repeat] }}</span>
              </div>
            </div>
            <span class="priority" :class="todo.priority">
              {{ todo.priority === "high" ? "高" : todo.priority === "medium" ? "中" : "低" }}
            </span>
            <span class="quick-actions">
              <button class="qa" :class="{ on: todo.pinned }" :title="todo.pinned ? '取消置顶' : '置顶'" @click="togglePinned(todo, $event)">📌</button>
              <button class="qa" :class="{ on: todo.favorite }" :title="todo.favorite ? '取消收藏' : '收藏'" @click="toggleFavorite(todo, $event)">⭐</button>
            </span>
          </div>

          <div v-if="doneOrder.length" class="done-section">
            <div class="done-label">已完成 · 点勾选恢复，点击查看详情</div>
            <div
              v-for="(todo, i) in doneOrder"
              :key="todo.id"
              :data-id="todo.id"
              class="todo-item done"
              :class="{ active: selected?.id === todo.id, dragging: doneDragIdx === i }"
              title="按住可直接拖动排序"
              @click="selectTodo(todo)"
              @pointerdown="onDoneItemDown($event, i)"
            >
              <input
                type="checkbox"
                :checked="todo.completed"
                @click="toggle(todo, $event)"
              />
              <div class="todo-body">
                <div class="todo-title" :title="todo.title">{{ todo.title }}</div>
              </div>
              <span class="restore-hint">恢复</span>
            </div>
          </div>

          <div v-if="!pendingDrag.length && !doneOrder.length" class="empty">暂无待办，点击「＋ 新建」开始</div>
        </template>

        <!-- 分组模式 -->
        <template v-else>
          <div v-for="group in grouped" :key="group.key" class="group">
            <div class="group-title">{{ group.key }} <small>{{ group.items.length }}</small></div>
            <div
              v-for="todo in group.items"
              :key="todo.id"
              class="todo-item"
              :class="{ active: selected?.id === todo.id }"
              @click="selectTodo(todo)"
            >
              <input
                type="checkbox"
                :checked="todo.completed"
                @click="toggle(todo, $event)"
              />
                <div class="todo-body">
                  <div class="todo-title" :title="todo.title">
                    <span v-if="todo.pinned" class="pin-flag">📌</span>
                    <span v-if="todo.favorite" class="fav-flag">⭐</span>
                    {{ todo.title }}
                  </div>
                  <div class="todo-meta">
                    <span v-if="todo.list && todo.list !== '默认'" class="meta-list">{{ todo.list }}</span>
                    <span v-if="groupMode !== 'time' && todo.due_at">📅 {{ fmtTime(todo.due_at) }}</span>
                    <span v-if="groupMode !== 'priority'" class="priority" :class="todo.priority">
                      {{ todo.priority === "high" ? "高" : todo.priority === "medium" ? "中" : "低" }}
                    </span>
                  </div>
                </div>
              <span class="quick-actions">
                <button class="qa" :class="{ on: todo.pinned }" @click="togglePinned(todo, $event)">📌</button>
                <button class="qa" :class="{ on: todo.favorite }" @click="toggleFavorite(todo, $event)">⭐</button>
              </span>
            </div>
          </div>
          <div v-if="!grouped.length" class="empty">暂无未完成待办</div>

          <div v-if="doneCount" class="done-section">
            <div class="done-label">已完成 · 点勾选恢复，点击查看详情</div>
            <div
              v-for="todo in doneItems"
              :key="todo.id"
              class="todo-item done"
              @click="selectTodo(todo)"
            >
              <input type="checkbox" :checked="true" @click="toggle(todo, $event)" />
              <div class="todo-body">
                <div class="todo-title" :title="todo.title">{{ todo.title }}</div>
              </div>
              <span class="restore-hint">恢复</span>
            </div>
          </div>
        </template>
      </div>
    </aside>
    <div class="col-resizer" title="拖动调整列表宽度" @mousedown="onResizeDown"></div>

    <section class="detail-panel">
      <div v-if="panelMode === 'detail' && selected" class="form">
        <h3>待办详情</h3>
        <label>标题</label>
        <input v-model="detail.title" placeholder="要做什么？" />
        <div class="row">
          <div>
            <label>清单</label>
            <input v-model="detail.list" list="list-suggestions" placeholder="清单名称" />
            <datalist id="list-suggestions">
              <option v-for="l in listSuggestions" :key="l" :value="l" />
            </datalist>
          </div>
        </div>
        <div class="row">
          <div>
            <label>截止时间（点击直接选择）</label>
            <input v-model="detail.due_at" type="datetime-local" step="60" @click="showPicker" />
          </div>
          <div>
            <label>提醒时间（点击直接选择）</label>
            <input v-model="detail.remind_at" type="datetime-local" step="60" @click="showPicker" />
          </div>
        </div>
        <div class="row">
          <div>
            <label>优先级</label>
            <select v-model="detail.priority">
              <option value="high">高</option>
              <option value="medium">中</option>
              <option value="low">低</option>
            </select>
          </div>
          <div>
            <label>重复</label>
            <select v-model="detail.repeat">
              <option value="none">不重复</option>
              <option value="daily">每天</option>
              <option value="weekday">工作日</option>
              <option value="weekly">每周</option>
              <option value="monthly">每月</option>
            </select>
          </div>
        </div>
        <label>备注</label>
        <textarea v-model="detail.note" rows="3"></textarea>
        <div class="form-actions">
          <button class="btn danger" @click="removeTodo">删除</button>
          <button
            v-if="selected"
            class="btn"
            @click="togglePinned(selected); selected = { ...selected, pinned: !selected.pinned }"
          >{{ selected.pinned ? "取消置顶" : "置顶" }}</button>
          <button
            v-if="selected"
            class="btn"
            @click="toggleFavorite(selected); selected = { ...selected, favorite: !selected.favorite }"
          >{{ selected.favorite ? "取消收藏" : "收藏" }}</button>
          <span class="spacer"></span>
          <button class="btn primary" @click="saveDetail">保存修改</button>
        </div>
        <div v-if="selected?.completed" class="done-badge">
          ✓ 此待办已完成，点击列表勾选框或
          <button class="restore-btn" @click="restoreTodo(selected)">恢复为待办</button>
        </div>
      </div>

      <div v-else-if="panelMode === 'create'" class="form">
        <h3>新建待办</h3>
        <label>标题</label>
        <input v-model="detail.title" placeholder="要做什么？" />
        <div class="row">
          <div>
            <label>清单</label>
            <input v-model="detail.list" list="list-suggestions" placeholder="清单名称" />
            <datalist id="list-suggestions">
              <option v-for="l in listSuggestions" :key="l" :value="l" />
            </datalist>
          </div>
        </div>
        <div class="row">
          <div>
            <label>截止时间（点击直接选择）</label>
            <input v-model="detail.due_at" type="datetime-local" step="60" @click="showPicker" />
          </div>
          <div>
            <label>提醒时间（点击直接选择）</label>
            <input v-model="detail.remind_at" type="datetime-local" step="60" @click="showPicker" />
          </div>
        </div>
        <div class="row">
          <div>
            <label>优先级</label>
            <select v-model="detail.priority">
              <option value="high">高</option>
              <option value="medium">中</option>
              <option value="low">低</option>
            </select>
          </div>
          <div>
            <label>重复</label>
            <select v-model="detail.repeat">
              <option value="none">不重复</option>
              <option value="daily">每天</option>
              <option value="weekday">工作日</option>
              <option value="weekly">每周</option>
              <option value="monthly">每月</option>
            </select>
          </div>
        </div>
        <label>备注</label>
        <textarea v-model="detail.note" rows="3"></textarea>
        <div class="form-actions">
          <button class="btn" @click="panelMode = 'none'">取消</button>
          <button class="btn primary" @click="createTodo">创建</button>
        </div>
      </div>

      <div v-else class="placeholder">
        <div class="icon">☑️</div>
        <p>点击左侧待办查看/编辑详情，或新建待办</p>
        <p class="hint">拖动 ⋮⋮ 图标可排序 · 📌 置顶 · ⭐ 收藏 · 已完成点击即恢复</p>
      </div>
    </section>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.todos {
  display: flex;
  flex: 1;
  height: 100%;
  /* 侧栏全展开窗口放不下时：详情区保底宽度，视图出现横向滚动 */
  overflow-x: auto;
  overflow-y: hidden;
}
/* 左栏：清单分类 */
.nav-lists {
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
.nav-mini {
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
.cat-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  user-select: none;
  transition: 0.15s;
  overflow: hidden;
  white-space: nowrap;
}
.cat-item:hover {
  background: var(--hover);
}
.cat-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.cat-item .badge {
  font-size: 11px;
  color: var(--muted);
  font-weight: 400;
}
.cat-sep {
  font-size: 11px;
  color: var(--muted);
  padding: 12px 10px 4px;
}
.new-list {
  margin-top: 8px;
}
.new-list input {
  width: 100%;
  padding: 7px 10px;
  border: 1px dashed var(--border);
  border-radius: 8px;
  font-size: 12px;
  outline: none;
  background: transparent;
}
.new-list input:focus {
  border-color: var(--accent);
  border-style: solid;
  background: var(--card);
}
.list-panel {
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
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--border);
  font-weight: 600;
}
.count {
  font-size: 12px;
  font-weight: 400;
  color: var(--muted);
  margin-left: 6px;
}
.btn.sm {
  padding: 5px 10px;
  font-size: 12px;
}
.group-switch {
  display: flex;
  gap: 4px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}
.group-switch button {
  flex: 1;
  padding: 5px 0;
  font-size: 12px;
  border: 1px solid var(--border);
  background: transparent;
  border-radius: 6px;
  cursor: pointer;
  color: var(--muted);
  transition: 0.15s;
}
.group-switch button.on {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent);
  font-weight: 600;
}
.list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}
.todo-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  border-radius: 8px;
  cursor: pointer;
  user-select: none;
  -webkit-user-select: none;
}
.todo-item:hover {
  background: var(--hover);
}
.todo-item.active {
  background: var(--accent-soft);
}
.todo-item input[type="checkbox"] {
  width: 18px;
  height: 18px;
  cursor: pointer;
  flex-shrink: 0;
}
.todo-item.dragging {
  opacity: 0.35;
  background: var(--accent-soft);
}
/* 清单项拖拽占位 */
.cat-item.dragging,
.cat-item.drag-placeholder {
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
  list-style: none;
}
.todo-body {
  flex: 1;
  min-width: 0;
}
.todo-title {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pin-flag,
.fav-flag {
  font-size: 11px;
  margin-right: 2px;
}
.todo-item.done {
  opacity: 0.55;
}
.todo-item.done .todo-title {
  text-decoration: line-through;
}
.todo-meta {
  font-size: 12px;
  color: var(--muted);
  margin-top: 2px;
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
.meta-list {
  background: var(--hover-strong);
  border-radius: 4px;
  padding: 0 5px;
}
.priority {
  font-size: 12px;
  padding: 1px 6px;
  border-radius: 4px;
  flex-shrink: 0;
}
.priority.high {
  color: var(--red);
  background: var(--red-bg);
}
.priority.medium {
  color: var(--amber);
  background: var(--amber-bg);
}
.priority.low {
  color: var(--green);
  background: var(--green-bg);
}
/* hover 快捷按钮：常驻占位 + 淡入淡出，hover 不改变布局（避免列表项跳动） */
.quick-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.12s;
}
.todo-item:hover .quick-actions {
  opacity: 1;
}
.qa {
  border: none;
  background: transparent;
  cursor: pointer;
  font-size: 13px;
  padding: 2px;
  opacity: 0.45;
  filter: grayscale(1);
  transition: 0.15s;
}
.qa:hover {
  opacity: 1;
  filter: none;
}
.qa.on {
  opacity: 1;
  filter: none;
}
.done-section {
  margin-top: 14px;
  border-top: 1px dashed var(--border);
  padding-top: 8px;
}
.done-label {
  font-size: 12px;
  color: var(--muted);
  padding: 4px 10px;
}
.restore-hint {
  font-size: 11px;
  color: var(--accent);
  flex-shrink: 0;
  opacity: 0;
  transition: 0.15s;
}
.todo-item.done:hover .restore-hint {
  opacity: 1;
}
.group {
  margin-bottom: 10px;
}
.group-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--muted);
  padding: 6px 10px;
  position: sticky;
  top: 0;
  background: var(--panel);
  z-index: 1;
}
.group-title small {
  font-weight: 400;
  margin-left: 4px;
}
.detail-panel {
  flex: 1 0 380px; /* 内容区保底宽度，不足时整个视图横向滚动 */
  padding: 24px;
  overflow-y: auto;
  background: var(--panel);
  min-width: 0;
}
.form h3 {
  margin-bottom: 16px;
}
.form label {
  display: block;
  font-size: 12px;
  color: var(--muted);
  margin: 12px 0 4px;
}
.form input,
.form select,
.form textarea {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 13px;
  outline: none;
  cursor: pointer;
}
.form input:focus,
.form select:focus,
.form textarea:focus {
  border-color: var(--accent);
}
.form input[placeholder="要做什么？"],
.form textarea {
  cursor: text;
}
.form .row {
  display: flex;
  gap: 12px;
}
.form .row > div {
  flex: 1;
  min-width: 0;
}
.form-actions {
  display: flex;
  gap: 8px;
  margin-top: 20px;
  align-items: center;
  flex-wrap: wrap;
}
/* 动态文本按钮固定最小宽：置顶↔取消置顶等切换时不挤压同行按钮跳动 */
.form-actions .btn {
  min-width: 84px;
  justify-content: center;
}
.form-actions .spacer {
  flex: 1;
}
.done-badge {
  margin-top: 14px;
  padding: 10px 14px;
  background: var(--green-bg);
  color: var(--green);
  border-radius: 8px;
  font-size: 13px;
}
.restore-btn {
  border: none;
  background: transparent;
  color: var(--green);
  font-weight: 600;
  cursor: pointer;
  text-decoration: underline;
  font-size: 13px;
  padding: 0;
}
.placeholder {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--muted);
}
.placeholder .icon {
  font-size: 48px;
  margin-bottom: 12px;
}
.hint {
  font-size: 12px;
  margin-top: 8px;
}
.empty {
  color: var(--muted);
  text-align: center;
  padding: 40px;
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

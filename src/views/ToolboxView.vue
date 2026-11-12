<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import {
  tsToDate,
  dateToTs,
  formatLocal,
  calcIp,
  cidrToMask,
  maskToCidr,
  intToIp,
  commonPrefixCidr,
  hostsToCidr,
  ipToHex,
  hexToIp,
  completeIp,
} from "../logic";
import type { IpCalcResult } from "../logic";

/* ---------- 工具列表 ---------- */

const activeTool = ref<"timestamp" | "ip">("timestamp");
const tools = [
  { id: "timestamp", name: "时间戳转换", icon: "⏱️", desc: "时间戳 ↔ 日期时间" },
  { id: "ip", name: "掩码 / IP 转换", icon: "🌐", desc: "网段 / 掩码计算" },
] as const;

/* ---------- 工具 1：时间戳转换 ---------- */

const tsInput = ref("");
const tsResult = computed(() => tsToDate(tsInput.value));
/** 实时当前时间戳 */
const nowSec = ref(0);
const nowMs = ref(0);
const nowLocal = ref("");
let nowTimer: number | null = null;

function tickNow() {
  const d = new Date();
  nowSec.value = Math.floor(d.getTime() / 1000);
  nowMs.value = d.getTime();
  nowLocal.value = formatLocal(d);
}

const dtInput = ref("");
const dtResult = computed(() => dateToTs(dtInput.value));

function fillNow() {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  dtInput.value = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(
    d.getHours()
  )}:${p(d.getMinutes())}`;
}

async function copyText(text: string | number) {
  try {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(String(text));
  } catch {
    /* 忽略复制失败 */
  }
}

/* ---------- 工具 2：掩码 / IP 转换（单页铺开，宽屏两列自适应） ---------- */

// 网段计算（原有）
const ipInput = ref("");
const maskInput = ref("24");
const ipResult = computed<IpCalcResult | null>(() => {
  if (!ipInput.value.trim()) return null;
  return calcIp(ipInput.value, maskInput.value);
});
const ipError = computed(() => {
  if (!ipInput.value.trim()) return "";
  if (!ipResult.value) return "IP 或掩码格式不正确（掩码支持 /24 或 255.255.255.0）";
  return "";
});

// 掩码转换：CIDR ↔ 点分 ↔ 通配符 ↔ 二进制/十六进制 + 可用地址数
const maskTabInput = ref("24");
const maskConv = computed(() => {
  const t = maskTabInput.value.trim().replace(/^\//, "");
  let cidr: number | null = null;
  if (/^\d{1,2}$/.test(t)) {
    const n = Number(t);
    if (n >= 0 && n <= 32) cidr = n;
  } else {
    cidr = maskToCidr(t);
  }
  if (cidr === null) return null;
  const maskInt = cidr === 0 ? 0 : (0xffffffff << (32 - cidr)) >>> 0;
  const mask = cidrToMask(cidr);
  const total = Math.pow(2, 32 - cidr);
  const usable = cidr >= 32 ? 1 : cidr === 31 ? 2 : total - 2;
  const binary = [0, 1, 2, 3]
    .map((i) => ((maskInt >>> (24 - i * 8)) & 255).toString(2).padStart(8, "0"))
    .join(".");
  return { cidr, mask, wildcard: intToIp(~maskInt >>> 0), binary, hex: ipToHex(mask), total, usable };
});
const maskTabErr = computed(() =>
  maskTabInput.value.trim() && !maskConv.value ? "掩码格式不正确（支持 /24 或 255.255.255.0）" : ""
);

// IP 段 → 最小掩码
const rangeStart = ref("");
const rangeEnd = ref("");
const rangeResult = computed(() => {
  if (!rangeStart.value.trim() || !rangeEnd.value.trim()) return null;
  const cidr = commonPrefixCidr(rangeStart.value, rangeEnd.value);
  if (cidr === null) return null;
  // calcIp 以最小 CIDR 计算网段，其 cidr 字段即最小掩码
  return calcIp(rangeStart.value, String(cidr));
});
const rangeErr = computed(() => {
  if (!rangeStart.value.trim() || !rangeEnd.value.trim()) return "";
  return rangeResult.value ? "" : "IP 地址格式不正确";
});

// IP 数量 → 掩码
const needHosts = ref<number | "">("");
const hostsResult = computed(() => {
  const n = Number(needHosts.value);
  if (!needHosts.value || !Number.isFinite(n)) return null;
  return hostsToCidr(n);
});
const hostsErr = computed(() =>
  needHosts.value && !hostsResult.value ? "请输入 1 ~ 4294967294 之间的数量" : ""
);

// 十六进制 ↔ 点分（自动识别输入类型）
const hexInput = ref("");
const hexConv = computed(() => {
  const s = hexInput.value.trim();
  if (!s) return null;
  if (s.includes(".")) {
    const hex = ipToHex(s);
    return hex ? { dir: "hex" as const, hex, ip: null } : null;
  }
  const ip = hexToIp(s);
  return ip ? { dir: "ip" as const, ip, hex: null } : null;
});
const hexErr = computed(() =>
  hexInput.value.trim() && !hexConv.value
    ? "格式不正确（支持点分 IP 或十六进制，如 C0A80164 / 0xFFFFFF00）"
    : ""
);

// IP 补全
const fillInput = ref("");
const fillResult = computed(() => {
  const s = fillInput.value.trim();
  if (!s) return null;
  const full = completeIp(s);
  if (!full) return null;
  return { full, isComplete: s.split(".").length === 4 };
});
const fillErr = computed(() =>
  fillInput.value.trim() && !fillResult.value ? "IP 格式不正确（如 192.168 / 10.1.2）" : ""
);

onMounted(() => {
  tickNow();
  nowTimer = window.setInterval(tickNow, 1000);
});
onUnmounted(() => {
  if (nowTimer) window.clearInterval(nowTimer);
});
</script>

<template>
  <div class="toolbox">
    <!-- 中间侧边栏：工具列表 -->
    <aside class="tool-nav">
      <div class="tool-nav-head">工具箱</div>
      <div
        v-for="t in tools"
        :key="t.id"
        class="tool-item"
        :class="{ active: activeTool === t.id }"
        @click="activeTool = t.id"
      >
        <span class="tool-icon">{{ t.icon }}</span>
        <span class="tool-text">
          <span class="tool-name">{{ t.name }}</span>
          <span class="tool-desc">{{ t.desc }}</span>
        </span>
      </div>
    </aside>

    <!-- 右侧：工具详情页 -->
    <section class="tool-panel">
      <!-- 时间戳转换 -->
      <template v-if="activeTool === 'timestamp'">
        <h2 class="tool-title">⏱️ 时间戳转换</h2>

        <div class="card-block">
          <div class="block-title">当前时间戳（实时，点击复制）</div>
          <div class="now-row">
            <div class="now-item" @click="copyText(nowSec)">
              <label>秒（10 位）</label>
              <span class="mono">{{ nowSec }}</span>
            </div>
            <div class="now-item" @click="copyText(nowMs)">
              <label>毫秒（13 位）</label>
              <span class="mono">{{ nowMs }}</span>
            </div>
          </div>
          <div class="now-local">{{ nowLocal }}</div>
        </div>

        <div class="card-block">
          <div class="block-title">时间戳 → 日期时间</div>
          <input
            v-model="tsInput"
            class="tool-input"
            placeholder="输入 10 位秒或 13 位毫秒时间戳，如 1762752000"
          />
          <div v-if="tsResult" class="result-grid">
            <div class="result-item"><label>秒</label><span class="mono">{{ tsResult.sec }}</span></div>
            <div class="result-item"><label>毫秒</label><span class="mono">{{ tsResult.ms }}</span></div>
            <div class="result-item"><label>本地时间</label><span class="mono strong">{{ tsResult.local }}</span></div>
            <div class="result-item"><label>UTC 时间</label><span class="mono">{{ tsResult.utc }}</span></div>
            <div class="result-item wide"><label>ISO 8601</label><span class="mono">{{ tsResult.iso }}</span></div>
          </div>
          <div v-else-if="tsInput.trim()" class="tool-err">时间戳格式不正确（应为 10 位秒或 13 位毫秒数字）</div>
        </div>

        <div class="card-block">
          <div class="block-title">日期时间 → 时间戳</div>
          <div class="dt-row">
            <input v-model="dtInput" type="datetime-local" class="tool-input" />
            <button class="btn sm" @click="fillNow">现在</button>
          </div>
          <div v-if="dtResult" class="result-grid">
            <div class="result-item"><label>秒</label><span class="mono strong">{{ dtResult.sec }}</span></div>
            <div class="result-item"><label>毫秒</label><span class="mono strong">{{ dtResult.ms }}</span></div>
          </div>
        </div>
      </template>

      <!-- 掩码 / IP 转换：单页铺开，宽屏两列 -->
      <template v-else>
        <h2 class="tool-title">🌐 掩码 / IP 转换</h2>

        <div class="ip-blocks">
          <!-- 网段计算 -->
          <div class="card-block">
            <div class="block-title">① 网段计算（IP + 掩码）</div>
            <div class="ip-row">
              <input v-model="ipInput" class="tool-input" placeholder="IP 地址，如 192.168.1.100" />
              <input v-model="maskInput" class="tool-input mask" placeholder="掩码：24 或 255.255.255.0" />
            </div>
            <div v-if="ipError" class="tool-err">{{ ipError }}</div>
            <div v-if="ipResult" class="result-grid">
              <div class="result-item"><label>IP 地址</label><span class="mono strong">{{ ipResult.ip }}/{{ ipResult.cidr }}</span></div>
              <div class="result-item"><label>子网掩码</label><span class="mono strong">{{ ipResult.mask }}</span></div>
              <div class="result-item"><label>通配符掩码</label><span class="mono">{{ ipResult.wildcard }}</span></div>
              <div class="result-item"><label>网络地址</label><span class="mono strong">{{ ipResult.network }}</span></div>
              <div class="result-item"><label>广播地址</label><span class="mono">{{ ipResult.broadcast }}</span></div>
              <div class="result-item"><label>可用 IP 范围</label><span class="mono strong">{{ ipResult.firstHost }} ~ {{ ipResult.lastHost }}</span></div>
              <div class="result-item"><label>可用主机数</label><span class="mono">{{ ipResult.hostCount.toLocaleString() }}</span></div>
              <div class="result-item"><label>地址块总数</label><span class="mono">{{ ipResult.total.toLocaleString() }}</span></div>
              <div class="result-item"><label>IP 类别</label><span class="mono">{{ ipResult.ipClass }}{{ ipResult.isPrivate ? " · 私网/环回" : " · 公网" }}</span></div>
              <div class="result-item wide"><label>IP 二进制</label><span class="mono">{{ ipResult.binary }}</span></div>
            </div>
          </div>

          <!-- 掩码转换 -->
          <div class="card-block">
            <div class="block-title">② 子网掩码转换器（CIDR ↔ 点分十进制）</div>
            <input v-model="maskTabInput" class="tool-input" placeholder="如 24 或 255.255.255.0" />
            <div v-if="maskTabErr" class="tool-err">{{ maskTabErr }}</div>
            <div v-if="maskConv" class="result-grid">
              <div class="result-item"><label>CIDR 前缀</label><span class="mono strong">/{{ maskConv.cidr }}</span></div>
              <div class="result-item"><label>点分子网掩码</label><span class="mono strong">{{ maskConv.mask }}</span></div>
              <div class="result-item"><label>通配符掩码</label><span class="mono">{{ maskConv.wildcard }}</span></div>
              <div class="result-item"><label>掩码二进制</label><span class="mono">{{ maskConv.binary }}</span></div>
              <div class="result-item"><label>掩码十六进制</label><span class="mono">0x{{ maskConv.hex }}</span></div>
              <div class="result-item"><label>网络可用地址数</label><span class="mono strong">{{ maskConv.usable.toLocaleString() }}</span></div>
              <div class="result-item"><label>地址块总数</label><span class="mono">{{ maskConv.total.toLocaleString() }}</span></div>
            </div>
          </div>

          <!-- IP 段 → 最小掩码 -->
          <div class="card-block">
            <div class="block-title">③ IP 段 → 最小掩码（输入起止地址）</div>
            <div class="ip-row">
              <input v-model="rangeStart" class="tool-input" placeholder="起始 IP，如 192.168.1.10" />
              <input v-model="rangeEnd" class="tool-input" placeholder="结束 IP，如 192.168.1.250" />
            </div>
            <div v-if="rangeErr" class="tool-err">{{ rangeErr }}</div>
            <div v-if="rangeResult" class="result-grid">
              <div class="result-item"><label>最小掩码</label><span class="mono strong">/{{ rangeResult.cidr }}（{{ rangeResult.mask }}）</span></div>
              <div class="result-item"><label>网络地址</label><span class="mono strong">{{ rangeResult.network }}</span></div>
              <div class="result-item"><label>广播地址</label><span class="mono">{{ rangeResult.broadcast }}</span></div>
              <div class="result-item"><label>可用 IP 范围</label><span class="mono">{{ rangeResult.firstHost }} ~ {{ rangeResult.lastHost }}</span></div>
              <div class="result-item"><label>可用主机数</label><span class="mono">{{ rangeResult.hostCount.toLocaleString() }}</span></div>
            </div>
          </div>

          <!-- IP 数量 → 掩码 -->
          <div class="card-block">
            <div class="block-title">④ 所需 IP 数量 → 掩码地址</div>
            <input v-model="needHosts" type="number" min="1" class="tool-input" placeholder="如 50" />
            <div v-if="hostsErr" class="tool-err">{{ hostsErr }}</div>
            <div v-if="hostsResult" class="result-grid">
              <div class="result-item"><label>CIDR 前缀</label><span class="mono strong">/{{ hostsResult.cidr }}</span></div>
              <div class="result-item"><label>子网掩码</label><span class="mono strong">{{ cidrToMask(hostsResult.cidr) }}</span></div>
              <div class="result-item"><label>地址块总数</label><span class="mono">{{ hostsResult.total.toLocaleString() }}</span></div>
              <div class="result-item"><label>可用主机数</label><span class="mono">{{ hostsResult.usable.toLocaleString() }}</span></div>
            </div>
          </div>

          <!-- 十六进制 ↔ 点分 -->
          <div class="card-block">
            <div class="block-title">⑤ 点分 IP / 掩码 ↔ 二进制 / 十六进制（自动识别）</div>
            <input v-model="hexInput" class="tool-input" placeholder="如 192.168.1.1 或 C0A80164 / 0xFFFFFF00" />
            <div v-if="hexErr" class="tool-err">{{ hexErr }}</div>
            <div v-if="hexConv" class="result-grid">
              <div v-if="hexConv.hex" class="result-item"><label>十六进制</label><span class="mono strong">0x{{ hexConv.hex }}</span></div>
              <div v-if="hexConv.ip" class="result-item"><label>点分十进制 IP</label><span class="mono strong">{{ hexConv.ip }}</span></div>
            </div>
          </div>

          <!-- IP 补全 -->
          <div class="card-block">
            <div class="block-title">⑥ IP 地址补全（缺失段补 0，计算位取反）</div>
            <input v-model="fillInput" class="tool-input" placeholder="如 192.168 → 192.168.0.0；10.1.2 → 10.1.2.0" />
            <div v-if="fillErr" class="tool-err">{{ fillErr }}</div>
            <div v-if="fillResult" class="result-grid">
              <div class="result-item"><label>完整 IP 地址</label><span class="mono strong">{{ fillResult.full }}</span></div>
              <div class="result-item"><label>说明</label><span class="mono">{{ fillResult.isComplete ? "输入已是完整 IP" : "已为缺失段补 0" }}</span></div>
            </div>
          </div>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.toolbox {
  display: flex;
  flex: 1;
  height: 100%;
}
.tool-nav {
  width: 200px;
  border-right: 1px solid var(--border);
  background: var(--panel);
  padding: 12px 8px;
  flex-shrink: 0;
  overflow-y: auto;
}
.tool-nav-head {
  font-weight: 600;
  padding: 4px 10px 12px;
}
.tool-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  border-radius: 8px;
  cursor: pointer;
  user-select: none;
}
.tool-item:hover {
  background: var(--hover);
}
.tool-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.tool-icon {
  font-size: 17px;
}
.tool-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.tool-name {
  font-size: 13px;
}
.tool-desc {
  font-size: 11px;
  color: var(--muted);
  font-weight: 400;
}
.tool-panel {
  flex: 1;
  overflow-y: auto;
  padding: 20px 24px;
  background: var(--panel);
}
.tool-title {
  margin-bottom: 16px;
  font-size: 18px;
}
/* IP 工具：单页铺开，每个功能块独占一行 */
.ip-blocks {
  display: grid;
  grid-template-columns: 1fr; /* 单列：每个功能块独占一行，宽度随窗口自适应 */
  gap: 16px;
  align-items: start;
}
.card-block {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  margin-bottom: 16px;
}
.block-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 10px;
  color: var(--text);
}
.tool-input {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  outline: none;
  background: var(--panel);
  color: var(--text);
  font-size: 13px;
}
.tool-input:focus {
  border-color: var(--accent);
}
.now-row {
  display: flex;
  gap: 12px;
}
.now-item {
  flex: 1;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 14px;
  cursor: pointer;
  transition: 0.15s;
}
.now-item:hover {
  border-color: var(--accent);
}
.now-item label {
  display: block;
  font-size: 11px;
  color: var(--muted);
  margin-bottom: 4px;
}
.now-local {
  margin-top: 8px;
  font-size: 13px;
  color: var(--muted);
}
.mono {
  font-family: Consolas, "Courier New", monospace;
  font-size: 14px;
  word-break: break-all;
}
.mono.strong {
  font-weight: 700;
  color: var(--accent);
}
.result-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 10px;
  margin-top: 10px;
}
.result-grid .wide {
  grid-column: 1 / -1;
}
.result-item {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 12px;
}
.result-item label {
  display: block;
  font-size: 11px;
  color: var(--muted);
  margin-bottom: 3px;
}
.dt-row {
  display: flex;
  gap: 8px;
}
.ip-row {
  display: flex;
  gap: 8px;
}
.ip-row .mask {
  max-width: 260px;
}
.tool-err {
  color: var(--red);
  font-size: 12px;
  margin-top: 8px;
}
.btn.sm {
  padding: 6px 14px;
  font-size: 12px;
  align-self: stretch;
}
</style>

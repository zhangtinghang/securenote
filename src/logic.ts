// 前端可测的纯函数：同步冲突解析与数据校验
// 这些逻辑与 Rust 后端 upsert 的冲突策略保持一致，用于单元测试验证

export interface SyncItem {
  id: string;
  updated_at: string;
  deleted_at: string | null;
}

/**
 * 合并同步策略：最后修改时间优先
 * 返回应保留本地、应覆盖本地、应新增的条目
 */
export function resolveMerge<T extends SyncItem>(
  local: T[],
  remote: T[]
): { upsert: T[]; keepLocal: T[] } {
  const localMap = new Map(local.map((i) => [i.id, i]));
  const upsert: T[] = [];
  const keepLocal: T[] = [];

  for (const remoteItem of remote) {
    const localItem = localMap.get(remoteItem.id);
    if (!localItem) {
      upsert.push(remoteItem);
    } else if (remoteItem.updated_at > localItem.updated_at) {
      upsert.push(remoteItem);
    } else {
      keepLocal.push(localItem);
    }
  }
  return { upsert, keepLocal };
}

/** 校验导出包格式 */
export function validatePayloadFormat(payload: any): boolean {
  return (
    payload &&
    payload.format === "securenote-vault" &&
    typeof payload.version === "number" &&
    payload.payload &&
    Array.isArray(payload.payload.notes) &&
    Array.isArray(payload.payload.todos) &&
    Array.isArray(payload.payload.passwords)
  );
}

/** 密码强度评分（与前端 LockScreen 一致） */
export function passwordStrength(pwd: string): number {
  if (!pwd) return 0;
  let score = 0;
  if (pwd.length >= 8) score++;
  if (/[a-z]/.test(pwd) && /[A-Z]/.test(pwd)) score++;
  if (/\d/.test(pwd)) score++;
  if (/[^a-zA-Z0-9]/.test(pwd)) score++;
  if (pwd.length >= 12) score++;
  return Math.min(score, 4);
}

/** 生成密码的字符集校验 */
export function validateGeneratedPassword(
  pwd: string,
  useUpper: boolean,
  useLower: boolean,
  useDigits: boolean,
  useSymbols: boolean
): boolean {
  if (pwd.length < 4) return false;
  if (useUpper && !/[A-Z]/.test(pwd)) return false;
  if (useLower && !/[a-z]/.test(pwd)) return false;
  if (useDigits && !/\d/.test(pwd)) return false;
  if (useSymbols && !/[^a-zA-Z0-9]/.test(pwd)) return false;
  return true;
}

/** 判断内容是否为 HTML（富文本） */
export function isHtmlContent(content: string): boolean {
  return /^\s*</.test(content);
}

/* ============ Markdown ↔ HTML 轻量转换器 ============ */

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Markdown → HTML（轻量实现，覆盖：h1-h3、粗斜体、删除线、
 * 行内代码、代码块、引用、有序/无序/任务列表、表格、图片、链接、分割线）
 */
export function markdownToHtml(md: string): string {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let inCode = false;
  let listType: "ul" | "ol" | null = null;
  let tableBuf: string[] = [];

  const closeList = () => {
    if (listType) {
      out.push(`</${listType}>`);
      listType = null;
    }
  };

  const flushTable = () => {
    if (!tableBuf.length) return;
    const rows = tableBuf.filter((r) => !/^\s*\|?[\s:|-]+\|?\s*$/.test(r));
    const parseCells = (r: string) =>
      r
        .trim()
        .replace(/^\||\|$/g, "")
        .split("|")
        .map((c) => inline(c.trim()));
    if (rows.length) {
      const [head, ...body] = rows;
      out.push("<table><thead><tr>");
      parseCells(head).forEach((c) => out.push(`<th>${c}</th>`));
      out.push("</tr></thead><tbody>");
      body.forEach((r) => {
        out.push("<tr>");
        parseCells(r).forEach((c) => out.push(`<td>${c}</td>`));
        out.push("</tr>");
      });
      out.push("</tbody></table>");
    }
    tableBuf = [];
  };

  const inline = (s: string): string => {
    let t = escapeHtml(s);
    t = t.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, '<img src="$2" alt="$1">');
    t = t.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');
    t = t.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    t = t.replace(/__([^_]+)__/g, "<strong>$1</strong>");
    t = t.replace(/\*([^*]+)\*/g, "<em>$1</em>");
    t = t.replace(/~~([^~]+)~~/g, "<del>$1</del>");
    t = t.replace(/`([^`]+)`/g, "<code>$1</code>");
    return t;
  };

  for (const raw of lines) {
    // 代码块
    if (/^```/.test(raw.trim())) {
      flushTable();
      closeList();
      if (inCode) {
        out.push("</code></pre>");
        inCode = false;
      } else {
        out.push("<pre><code>");
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      out.push(escapeHtml(raw) + "\n");
      continue;
    }
    // 表格行
    if (/^\s*\|.*\|\s*$/.test(raw)) {
      closeList();
      tableBuf.push(raw);
      continue;
    } else {
      flushTable();
    }
    // 标题
    const h = raw.match(/^(#{1,3})\s+(.*)/);
    if (h) {
      closeList();
      out.push(`<h${h[1].length}>${inline(h[2])}</h${h[1].length}>`);
      continue;
    }
    // 分割线
    if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(raw)) {
      closeList();
      out.push("<hr>");
      continue;
    }
    // 引用
    const q = raw.match(/^>\s?(.*)/);
    if (q) {
      closeList();
      out.push(`<blockquote>${inline(q[1])}</blockquote>`);
      continue;
    }
    // 任务列表
    const task = raw.match(/^\s*[-*]\s+\[([ xX])\]\s+(.*)/);
    if (task) {
      if (listType !== "ul") {
        closeList();
        out.push("<ul>");
        listType = "ul";
      }
      out.push(
        `<li><input type="checkbox" disabled ${task[1] !== " " ? "checked" : ""}> ${inline(task[2])}</li>`
      );
      continue;
    }
    // 无序/有序列表
    const ul = raw.match(/^\s*[-*]\s+(.*)/);
    const ol = raw.match(/^\s*\d+\.\s+(.*)/);
    if (ul || ol) {
      const want: "ul" | "ol" = ul ? "ul" : "ol";
      if (listType !== want) {
        closeList();
        out.push(`<${want}>`);
        listType = want;
      }
      out.push(`<li>${inline((ul || ol)![1])}</li>`);
      continue;
    }
    closeList();
    // 空行
    if (!raw.trim()) {
      continue;
    }
    out.push(`<p>${inline(raw)}</p>`);
  }
  flushTable();
  closeList();
  if (inCode) out.push("</code></pre>");
  return out.join("\n");
}

/**
 * HTML → Markdown（轻量实现，与 markdownToHtml 对称）
 * 用 DOMParser 解析，富文本粘贴的表格/图片也能转换
 */
export function htmlToMarkdown(html: string): string {
  if (typeof DOMParser === "undefined") {
    return html; // 非浏览器环境（测试）降级
  }
  const doc = new DOMParser().parseFromString(html, "text/html");
  const conv = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) {
      return node.textContent || "";
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return "";
    const el = node as Element;
    const children = Array.from(el.childNodes).map(conv).join("");
    switch (el.tagName) {
      case "H1":
        return `# ${children}\n\n`;
      case "H2":
        return `## ${children}\n\n`;
      case "H3":
        return `### ${children}\n\n`;
      case "STRONG":
      case "B":
        return `**${children}**`;
      case "EM":
      case "I":
        return `*${children}*`;
      case "DEL":
      case "S":
        return `~~${children}~~`;
      case "CODE": {
        if (el.parentElement?.tagName === "PRE") return children;
        return `\`${children}\``;
      }
      case "PRE":
        return `\`\`\`\n${children}\n\`\`\`\n\n`;
      case "BLOCKQUOTE":
        return children
          .split("\n")
          .map((l) => `> ${l}`)
          .join("\n");
      case "A":
        return `[${children}](${el.getAttribute("href") || ""})`;
      case "IMG": {
        const src = el.getAttribute("src") || "";
        const alt = el.getAttribute("alt") || "图片";
        return `![${alt}](${src})`;
      }
      case "HR":
        return `---\n\n`;
      case "BR":
        return "\n";
      case "UL":
        return (
          Array.from(el.children)
            .map((li) => `- ${liToMd(li)}`)
            .join("\n") + "\n\n"
        );
      case "OL":
        return (
          Array.from(el.children)
            .map((li, i) => `${i + 1}. ${liToMd(li)}`)
            .join("\n") + "\n\n"
        );
      case "TABLE": {
        const rows = Array.from(el.querySelectorAll("tr"));
        if (!rows.length) return "";
        const cellsOf = (tr: Element) =>
          Array.from(tr.querySelectorAll("th,td")).map(
            (td) => (td.textContent || "").trim()
          );
        const head = cellsOf(rows[0]);
        const body = rows.slice(1).map(cellsOf);
        const line = (cs: string[]) => `| ${cs.join(" | ")} |`;
        const sep = `| ${head.map(() => "---").join(" | ")} |`;
        return [line(head), sep, ...body.map(line)].join("\n") + "\n\n";
      }
      case "P":
      case "DIV":
        return children + "\n\n";
      default:
        return children;
    }
  };
  const liToMd = (li: Element): string => {
    const checkbox = li.querySelector('input[type="checkbox"]');
    const text = Array.from(li.childNodes).map(conv).join("").trim();
    return checkbox ? `[${(checkbox as HTMLInputElement).checked ? "x" : " "}] ${text}` : text;
  };
  return conv(doc.body).replace(/\n{3,}/g, "\n\n").trim();
}

/** base64 → 字节（用于 IPC 文件传输解码） */
export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

/** 字节 → base64（用于 IPC 文件传输编码） */
export function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

/** 纯文本（txt/log/csv 等）→ 简单 HTML 段落 */
export function plainTextToHtml(text: string): string {
  return text
    .split(/\r?\n/)
    .map((line) => (line.trim() ? `<div>${escapeHtml(line)}</div>` : "<div><br></div>"))
    .join("");
}

/* ============ 工具箱：时间戳转换 ============ */

export interface TsInfo {
  sec: number;
  ms: number;
  /** 本地时间 "YYYY-MM-DD HH:mm:ss" */
  local: string;
  /** ISO 8601（UTC） */
  iso: string;
  /** UTC 时间 "YYYY-MM-DD HH:mm:ss" */
  utc: string;
}

/** 时间戳字符串（10 位秒 / 13 位毫秒，自动识别）→ 日期信息；非法返回 null */
export function tsToDate(input: string): TsInfo | null {
  const s = input.trim();
  if (!/^\d{10}$|^\d{13}$/.test(s)) return null;
  const ms = s.length === 13 ? Number(s) : Number(s) * 1000;
  const d = new Date(ms);
  if (isNaN(d.getTime())) return null;
  const p = (n: number) => String(n).padStart(2, "0");
  const local = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(
    d.getHours()
  )}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  const iso = d.toISOString();
  return {
    sec: Math.floor(ms / 1000),
    ms,
    local,
    iso,
    utc: iso.slice(0, 19).replace("T", " "),
  };
}

/** 本地时间（datetime-local 值 "YYYY-MM-DDTHH:mm"）→ 时间戳；非法返回 null */
export function dateToTs(v: string): { sec: number; ms: number } | null {
  if (!v) return null;
  const d = new Date(v);
  if (isNaN(d.getTime())) return null;
  return { sec: Math.floor(d.getTime() / 1000), ms: d.getTime() };
}

/** Date → 本地 "YYYY-MM-DD HH:mm:ss" */
export function formatLocal(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(
    d.getHours()
  )}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/* ============ 工具箱：掩码 / IPv4 转换 ============ */

export interface IpCalcResult {
  ip: string;
  cidr: number;
  /** 子网掩码（点分十进制） */
  mask: string;
  /** 通配符掩码 */
  wildcard: string;
  network: string;
  broadcast: string;
  firstHost: string;
  lastHost: string;
  /** 可用主机数（/31=2，/32=1，其余减网络与广播） */
  hostCount: number;
  /** 地址块总数 */
  total: number;
  /** IP 二进制（点分） */
  binary: string;
  /** IP 类别 */
  ipClass: string;
  /** 是否私网/环回 */
  isPrivate: boolean;
}

/** 点分 IPv4 → 无符号 32 位整数；非法返回 null */
export function ipToInt(ip: string): number | null {
  const m = ip.trim().match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (!m) return null;
  let v = 0;
  for (let i = 1; i <= 4; i++) {
    const o = Number(m[i]);
    if (o > 255) return null;
    v = v * 256 + o;
  }
  return v >>> 0;
}

/** 无符号 32 位整数 → 点分 IPv4 */
export function intToIp(v: number): string {
  return [(v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255].join(".");
}

/** CIDR 前缀长度（0-32）→ 点分子网掩码 */
export function cidrToMask(n: number): string {
  const m = n <= 0 ? 0 : (0xffffffff << (32 - n)) >>> 0;
  return intToIp(m);
}

/** 点分子网掩码 → CIDR 前缀长度（要求 1 位连续位于高位）；非法返回 null */
export function maskToCidr(mask: string): number | null {
  const v = ipToInt(mask);
  if (v === null) return null;
  const inv = ~v >>> 0;
  // 掩码合法 ⇔ 反码 + 1 为 2 的幂（即 1 连续在高位、0 连续在低位）
  if ((inv & (inv + 1)) !== 0) return null;
  let n = 0;
  let x = v;
  while (x) {
    n += x & 1;
    x >>>= 1;
  }
  return n;
}

/** IP + 掩码（CIDR "/24" 或点分 "255.255.255.0"）→ 完整网段信息；非法返回 null */
export function calcIp(ipStr: string, maskStr: string): IpCalcResult | null {
  const ip = ipToInt(ipStr);
  if (ip === null) return null;
  const t = maskStr.trim().replace(/^\//, "");
  let cidr: number | null = null;
  if (/^\d{1,2}$/.test(t)) {
    const n = Number(t);
    if (n >= 0 && n <= 32) cidr = n;
  } else {
    cidr = maskToCidr(t);
  }
  if (cidr === null) return null;
  const maskInt = cidr === 0 ? 0 : (0xffffffff << (32 - cidr)) >>> 0;
  const net = (ip & maskInt) >>> 0;
  const bcast = (net | (~maskInt >>> 0)) >>> 0;
  const total = Math.pow(2, 32 - cidr);
  const hostCount = cidr >= 32 ? 1 : cidr === 31 ? 2 : total - 2;
  const firstHost = cidr >= 31 ? net : net + 1;
  const lastHost = cidr >= 31 ? bcast : bcast - 1;
  const o1 = (ip >>> 24) & 255;
  const o2 = (ip >>> 16) & 255;
  const ipClass =
    o1 < 128 ? "A 类" : o1 < 192 ? "B 类" : o1 < 224 ? "C 类" : o1 < 240 ? "D 类（组播）" : "E 类（保留）";
  const isPrivate =
    o1 === 10 ||
    o1 === 127 ||
    (o1 === 172 && o2 >= 16 && o2 <= 31) ||
    (o1 === 192 && o2 === 168);
  const binary = [0, 1, 2, 3]
    .map((i) => ((ip >>> (24 - i * 8)) & 255).toString(2).padStart(8, "0"))
    .join(".");
  return {
    ip: intToIp(ip),
    cidr,
    mask: cidrToMask(cidr),
    wildcard: intToIp(~maskInt >>> 0),
    network: intToIp(net),
    broadcast: intToIp(bcast),
    firstHost: intToIp(firstHost),
    lastHost: intToIp(lastHost),
    hostCount,
    total,
    binary,
    ipClass,
    isPrivate,
  };
}

/** IP 段（起止地址）→ 覆盖两地址的最小 CIDR 前缀长度；任一地址非法返回 null */
export function commonPrefixCidr(startStr: string, endStr: string): number | null {
  const a = ipToInt(startStr);
  const b = ipToInt(endStr);
  if (a === null || b === null) return null;
  const [lo, hi] = a <= b ? [a, b] : [b, a];
  const diff = (lo ^ hi) >>> 0;
  let common = 0;
  for (let bit = 31; bit >= 0; bit--) {
    if (((diff >>> bit) & 1) === 0) common++;
    else break;
  }
  return common;
}

/** 容纳 n 个可用主机的最小 CIDR；n 非法返回 null */
export function hostsToCidr(n: number): { cidr: number; total: number; usable: number } | null {
  if (!Number.isFinite(n) || n < 1 || n > 4294967294) return null;
  // 找最小的块使 usable >= n：/32 usable=1，/31 usable=2，其余 2^(32-c)-2
  for (let c = 32; c >= 0; c--) {
    const total = Math.pow(2, 32 - c);
    const usable = c >= 32 ? 1 : c === 31 ? 2 : total - 2;
    if (usable >= n) return { cidr: c, total, usable };
  }
  return null;
}

/** 点分 IPv4 → 十六进制（8 位大写，如 C0A80164）；非法返回 null */
export function ipToHex(ip: string): string | null {
  const v = ipToInt(ip);
  if (v === null) return null;
  return v.toString(16).toUpperCase().padStart(8, "0");
}

/** 十六进制（支持 0x 前缀、大小写、6-8 位）→ 点分 IPv4；非法返回 null */
export function hexToIp(hex: string): string | null {
  let s = hex.trim().replace(/^0x/i, "");
  if (!/^[0-9a-fA-F]{6,8}$/.test(s)) return null;
  s = s.padStart(8, "0");
  const v = parseInt(s, 16) >>> 0;
  return intToIp(v);
}

/** 点分十进制补全：缺失的段补 0（"192.168" → "192.168.0.0"，"10.1" → "10.1.0.0"）；非法返回 null */
export function completeIp(input: string): string | null {
  const parts = input.trim().replace(/\.+$/, "").split(".");
  if (parts.length === 0 || parts.length > 4) return null;
  if (parts.some((p) => !/^\d{1,3}$/.test(p) || Number(p) > 255)) return null;
  while (parts.length < 4) parts.push("0");
  return parts.join(".");
}

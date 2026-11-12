<script setup lang="ts">
import { ref, inject } from "vue";
import { vault, auth, setContentProtection } from "../api";
import { applyTheme, themeOptions, type ThemeName } from "../theme";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const toast = ref("");
const oldPwd = ref("");
const newPwd = ref("");
const confirmPwd = ref("");

/** 投屏/录屏内容保护（默认开启） */
const contentProtection = ref((localStorage.getItem("sn_content_protection") ?? "1") === "1");

async function toggleContentProtection() {
  const next = !contentProtection.value;
  try {
    await setContentProtection(next);
    contentProtection.value = next;
    localStorage.setItem("sn_content_protection", next ? "1" : "0");
    showToast(next ? "✓ 已开启：共享屏幕/录屏时本应用不可见" : "已关闭：共享屏幕时将显示本应用内容");
  } catch (e: any) {
    showToast("设置失败: " + String(e?.message || e));
  }
}

/** 当前主题 */
const theme = ref<ThemeName>((localStorage.getItem("sn_theme") as ThemeName) || "light");

function setTheme(t: ThemeName) {
  theme.value = t;
  applyTheme(t);
  showToast(`✓ 已切换主题：${themeOptions.find((o) => o.value === t)?.label}`);
}

function showToast(msg: string) {
  toast.value = msg;
  setTimeout(() => (toast.value = ""), 4000);
}

// 一键导出（Rust 端弹保存框并写文件）
async function doExport() {
  try {
    const path = await vault.exportToFile();
    showToast(`✓ 导出成功：${path}（文件已加密，可上传云盘）`);
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    showToast("导出失败: " + msg);
  }
}

// 导入（Rust 端弹选择框并读取）
async function doImport() {
  const merge = await appConfirm(
    "选择导入模式：\n· 确认 = 合并导入（增量，冲突取最新）\n· 取消 = 覆盖导入（清空后还原）",
    "合并导入"
  );
  try {
    const [count, path] = await vault.importFromFile(merge ? "merge" : "replace");
    showToast(`✓ 导入成功，共处理 ${count} 条（${path}）`);
    // 导入后刷新页面数据
    setTimeout(() => window.location.reload(), 1200);
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    showToast("导入失败: " + msg);
  }
}

// 修改主密码
async function changePwd() {
  if (newPwd.value.length < 8) {
    showToast("新密码至少 8 位");
    return;
  }
  if (newPwd.value !== confirmPwd.value) {
    showToast("两次输入的新密码不一致");
    return;
  }
  try {
    await auth.changeMasterPassword(oldPwd.value, newPwd.value);
    showToast("✓ 主密码修改成功");
    oldPwd.value = newPwd.value = confirmPwd.value = "";
  } catch (e: any) {
    showToast("修改失败: " + String(e?.message || e));
  }
}
</script>

<template>
  <div class="settings">
    <div class="content">
      <h2>设置</h2>

      <section class="block">
        <h3>外观主题</h3>
        <div class="theme-grid">
          <button
            v-for="opt in themeOptions"
            :key="opt.value"
            class="theme-card"
            :class="{ on: theme === opt.value }"
            @click="setTheme(opt.value)"
          >
            <span class="swatches">
              <span v-for="c in opt.colors" :key="c" class="swatch" :style="{ background: c }"></span>
            </span>
            <span class="t">{{ opt.label }}</span>
            <span class="s">{{ opt.desc }}</span>
          </button>
        </div>
        <div class="s" style="margin-top:8px">主题即时生效并自动保存，覆盖侧边栏、列表、编辑器等全部界面</div>
      </section>

      <section class="block">
        <h3>备份与还原</h3>
        <div class="row-card">
          <div class="desc">
            <div class="t">一键导出全部数据</div>
            <div class="s">格式：加密 JSON（AES-256-GCM）· 含笔记 / 待办 / 密码库</div>
            <div class="s">导出文件不可直接读取，可上传云盘完成备份</div>
          </div>
          <button class="btn primary" @click="doExport">📦 导出加密包</button>
        </div>
        <div class="row-card">
          <div class="desc">
            <div class="t">从加密包导入（还原）</div>
            <div class="s">支持合并（增量）与覆盖两种模式，冲突取最新</div>
          </div>
          <button class="btn" @click="doImport">📂 选择文件</button>
        </div>
      </section>

      <section class="block">
        <h3>隐私保护</h3>
        <div class="row-card">
          <div class="desc">
            <div class="t">投屏 / 录屏时隐藏窗口内容（推荐开启）</div>
            <div class="s">开启后共享屏幕、直播、录屏软件中本应用完全不可见，本地显示不受影响</div>
            <div class="s">需要 Windows 10 2004 及以上版本，重启应用后保持</div>
          </div>
          <button class="switch" :class="{ on: contentProtection }" @click="toggleContentProtection">
            <span class="knob"></span>
          </button>
        </div>
      </section>

      <section class="block">
        <h3>安全</h3>
        <div class="field">
          <label>当前主密码</label>
          <input v-model="oldPwd" type="password" />
        </div>
        <div class="row">
          <div class="field">
            <label>新主密码</label>
            <input v-model="newPwd" type="password" />
          </div>
          <div class="field">
            <label>确认新密码</label>
            <input v-model="confirmPwd" type="password" />
          </div>
        </div>
        <button class="btn" @click="changePwd">修改主密码</button>
      </section>

      <section class="block about">
        <h3>关于</h3>
        <div class="about-line">
          <span class="app-name">个人笔记本 <small>v1.0.0</small></span>
          <span class="copy">Copyright © 2026 zth · 开源协议 MIT</span>
        </div>
        <div class="info">
          <p>✓ 当前版本支持离线加密存储与客户端间互导</p>
          <p>✓ 增量同步：基于 UUID + 时间戳，冲突取最后修改时间</p>
          <p class="future">🔌 云端实时同步已预留接口，后续可对接自建服务器</p>
        </div>
      </section>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.settings {
  flex: 1;
  height: 100%;
  overflow-y: auto;
  background: var(--panel);
}
.content {
  /* 两列布局：每个分区占一列，列宽随窗口大小自适应伸缩 */
  max-width: none;
  padding: 24px;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
  align-items: start;
}
h2 {
  grid-column: 1 / -1;
  margin-bottom: 0;
}
.block {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 16px;
  margin-bottom: 0;
}
.block.about {
  grid-column: 1 / -1;
}
.about-line {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 16px;
  margin-bottom: 10px;
}
.app-name {
  font-weight: 700;
  font-size: 14px;
}
.app-name small {
  color: var(--muted);
  font-weight: 400;
  margin-left: 4px;
}
.copy {
  font-size: 12px;
  color: var(--muted);
}
.block h3 {
  font-size: 15px;
  margin-bottom: 12px;
  color: var(--text);
}
.theme-grid {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.theme-card {
  flex: 1;
  min-width: 160px;
  border: 1px solid var(--border);
  background: var(--card);
  border-radius: 12px;
  padding: 14px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  transition: 0.15s;
  color: var(--text);
}
.theme-card:hover {
  border-color: var(--accent);
}
.theme-card.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-soft);
}
.swatches {
  display: flex;
  gap: 6px;
}
.swatch {
  width: 26px;
  height: 26px;
  border-radius: 8px;
  border: 1px solid rgba(0, 0, 0, 0.08);
}
.theme-card .t {
  font-weight: 600;
  font-size: 13px;
}
.theme-card .s {
  font-size: 11px;
  color: var(--muted);
}
.row-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap; /* 卡片较窄时按钮换到描述下方 */
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 12px;
}
.desc .t {
  font-weight: 500;
  margin-bottom: 4px;
}
.desc .s {
  font-size: 12px;
  color: var(--muted);
}
/* 拨动开关 */
.switch {
  width: 46px;
  height: 26px;
  border-radius: 13px;
  border: 1px solid var(--border);
  background: var(--hover-strong);
  position: relative;
  cursor: pointer;
  transition: 0.18s;
  flex-shrink: 0;
}
.switch .knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  transition: 0.18s;
}
.switch.on {
  background: var(--accent);
  border-color: var(--accent);
}
.switch.on .knob {
  left: 22px;
}
.field {
  margin-bottom: 12px;
}
.field label {
  display: block;
  font-size: 12px;
  color: var(--muted);
  margin-bottom: 4px;
}
.field input {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 13px;
  outline: none;
}
.row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap; /* 卡片较窄时两个输入框自动上下排布 */
}
.row .field {
  flex: 1;
}
.info {
  padding: 16px;
  background: var(--hover);
  border: 1px solid var(--border);
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.8;
}
.info .future {
  color: var(--amber);
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

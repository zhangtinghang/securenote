<script setup lang="ts">
import { ref, inject, onMounted } from "vue";
import { vault, auth, setContentProtection, getBackupDir, chooseBackupDir, resetBackupDir, getDataDir, chooseDataDir, resetDataDir, getAppVersion, forceBackup, getIdleBackup, setIdleBackup, openUrl } from "../api";
import { exportRunning } from "../exportProgress";
import { applyTheme, themeOptions, type ThemeName } from "../theme";
import { checkForUpdate, type UpdateInfo } from "../updater";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const toast = ref("");
const oldPwd = ref("");
const newPwd = ref("");
const confirmPwd = ref("");

/** 当前应用版本号（动态读取） */
const appVersion = ref("1.0.0");

/** 当前备份目录 */
const backupDir = ref("");
/** 当前数据存储目录 */
const dataDir = ref("");
/** 是否正在检查更新 */
const checkingUpdate = ref(false);

onMounted(async () => {
  // 读取当前版本号与备份目录
  try {
    appVersion.value = await getAppVersion();
  } catch (e) {
    /* 忽略 */
  }
  await refreshBackupDir();
  await refreshDataDir();
  // 读取闲时自动备份开关
  try {
    idleBackup.value = await getIdleBackup();
  } catch (e) {
    /* 忽略 */
  }
});

async function refreshBackupDir() {
  try {
    backupDir.value = await getBackupDir();
  } catch (e) {
    /* 忽略 */
  }
}

/** 选择备份目录 */
async function pickBackupDir() {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  try {
    const dir = await chooseBackupDir();
    backupDir.value = dir;
    showToast(`✓ 备份目录已更改为：${dir}（历史备份正在后台迁移，可在顶部查看进度）`);
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    showToast("设置失败: " + msg);
  }
}

/** 恢复默认备份目录 */
async function resetBackupDirToDefault() {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  try {
    const dir = await resetBackupDir();
    backupDir.value = dir;
    showToast(`✓ 已恢复默认备份目录：${dir}（历史备份正在后台迁移）`);
  } catch (e: any) {
    showToast("恢复失败: " + String(e?.message || e));
  }
}

/** 手动立即备份（滚动保留最近 3 份） */
async function doManualBackup() {
  try {
    const filename = await forceBackup();
    showToast(`✓ 全量备份完成：${filename}`);
  } catch (e: any) {
    showToast("备份失败: " + String(e?.message || e));
  }
}

/** 闲时自动备份开关 */
const idleBackup = ref(false);

async function toggleIdleBackup() {
  const next = !idleBackup.value;
  try {
    await setIdleBackup(next);
    idleBackup.value = next;
    showToast(
      next
        ? "✓ 已开启：空闲 3 分钟且数据有变化时自动后台备份"
        : "已关闭闲时自动备份"
    );
  } catch (e: any) {
    showToast("设置失败: " + String(e?.message || e));
  }
}

/** 读取当前数据存储目录 */
async function refreshDataDir() {
  try {
    dataDir.value = await getDataDir();
  } catch (e) {
    /* 忽略 */
  }
}

/** 更改数据存储目录：后台迁移历史数据，完成后自动重启应用 */
async function pickDataDir() {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  const ok = await appConfirm(
    "迁移分两步自动完成（期间会弹出进度遮罩，请勿关闭应用）：\n\n" +
      "· 第 1 步：快照备份当前数据到备份目录\n" +
      "· 第 2 步：把数据复制到新目录（原目录保留作兜底）\n\n" +
      "完成后应用自动重启，需要重新输入主密码解锁。\n是否选择新目录？",
    "选择目录并迁移"
  );
  if (!ok) return;
  try {
    await chooseDataDir();
    showToast("✓ 迁移任务已开始，可在顶部工具栏查看进度，完成后自动重启");
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    showToast("迁移失败: " + msg);
  }
}

/** 恢复默认数据存储目录：后台迁移回默认目录，完成后自动重启 */
async function resetDataDirToDefault() {
  if (exportRunning.value) {
    showToast("已有后台任务进行中，请等待完成");
    return;
  }
  const ok = await appConfirm(
    "将把数据迁移回默认目录（弹出进度遮罩，请勿关闭应用），\n完成后应用自动重启（需重新输入主密码）。是否继续？",
    "迁移并重启"
  );
  if (!ok) return;
  try {
    await resetDataDir();
    showToast("✓ 迁移任务已开始，可在顶部工具栏查看进度，完成后自动重启");
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    showToast("迁移失败: " + msg);
  }
}

/** 手动检查更新 */
async function manualCheckUpdate() {
  if (checkingUpdate.value) return;
  checkingUpdate.value = true;
  try {
    const current = await getAppVersion();
    const info: UpdateInfo | null = await checkForUpdate(current);
    if (!info) {
      showToast(`✓ 已是最新版本（v${current}）`);
      return;
    }
    const ok = await appConfirm(
      `发现新版本 v${info.version}（当前 v${current}）。\n\n` +
        `更新内容：\n${info.notes || "（无说明）"}\n\n` +
        `⚠️ 升级前会先自动备份本地数据（滚动保留最近 3 份），\n` +
        `本应用为离线本地应用，升级不会主动删除数据。是否前往下载？`,
      "前往下载"
    );
    if (!ok) return;
    try {
      await forceBackup();
    } catch (e) {
      /* 备份失败不阻塞跳转 */
    }
    await openUrl(info.url);
  } catch (e: any) {
    showToast("检查更新失败: " + String(e?.message || e));
  } finally {
    checkingUpdate.value = false;
  }
}

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
/** 后台导出加密包：弹保存框 → 后台线程执行，进度见顶部工具栏，完成系统通知 */
async function doExport() {
  if (exportRunning.value) {
    showToast("已有导出任务进行中，请在顶部工具栏查看进度");
    return;
  }
  const ok = await appConfirm(
    "当前数据量较大，导出将在后台执行，期间可正常使用其他功能。\n\n" +
      "· 先弹出保存位置选择框\n" +
      "· 顶部工具栏显示导出进度\n" +
      "· 完成后系统通知\n\n是否开始导出？",
    "开始导出"
  );
  if (!ok) return;
  try {
    const path = await vault.startExportToFile();
    showToast(`✓ 导出任务已开始（${path}），可在顶部查看进度，完成后会通知你`);
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
        <h3>外观</h3>
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
        <h3>数据与备份</h3>
        <div class="row-card">
          <div class="desc">
            <div class="t">数据存储位置</div>
            <div class="s">笔记 / 待办 / 密码 / 图片附件等全部加密数据所在目录</div>
            <div class="s path" :title="dataDir">{{ dataDir || "加载中…" }}</div>
          </div>
          <div class="btn-group">
            <button class="btn" :disabled="exportRunning" @click="pickDataDir">📁 更改目录</button>
            <button class="btn" :disabled="exportRunning" @click="resetDataDirToDefault">恢复默认</button>
          </div>
        </div>
        <div class="s" style="margin-bottom:10px">
          更改目录时数据在后台迁移（顶部工具栏显示进度，不阻塞操作），原目录数据保留作为兜底；
          迁移完成后应用自动重启；默认备份目录也会跟随数据存储目录。
        </div>
        <div class="row-card">
          <div class="desc">
            <div class="t">备份位置</div>
            <div class="s">手动备份 / 升级前备份 / 闲时自动备份保存到此目录，滚动保留最近 3 份；更改目录时历史备份文件会自动复制过去</div>
            <div class="s path" :title="backupDir">{{ backupDir || "加载中…" }}</div>
          </div>
          <div class="btn-group">
            <button class="btn" :disabled="exportRunning" @click="doManualBackup">💾 立即备份</button>
            <button class="btn" :disabled="exportRunning" @click="pickBackupDir">📁 更改目录</button>
            <button class="btn" :disabled="exportRunning" @click="resetBackupDirToDefault">恢复默认</button>
          </div>
        </div>
        <div class="row-card">
          <div class="desc">
            <div class="t">闲时自动后台备份</div>
            <div class="s">开启后：空闲 3 分钟且数据有变化时，自动在后台备份一次（每分钟检查一次）</div>
            <div class="s">关闭软件时不再自动备份（避免大数据量时等待），靠此开关与手动备份保障数据安全</div>
          </div>
          <button class="switch" :class="{ on: idleBackup }" @click="toggleIdleBackup">
            <span class="knob"></span>
          </button>
        </div>
        <div class="row-card">
          <div class="desc">
            <div class="t">一键导出全部数据</div>
            <div class="s">格式：加密 JSON（AES-256-GCM）· 含笔记 / 待办 / 密码库 / 图片附件</div>
            <div class="s">后台执行不卡界面：顶部工具栏显示进度，完成后系统通知；文件不可直接读取，可上传云盘完成备份</div>
          </div>
          <button class="btn primary" @click="doExport" :disabled="exportRunning">
            {{ exportRunning ? "⏳ 导出中…" : "📦 导出加密包" }}
          </button>
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
        <h3>安全与隐私</h3>
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
        <div class="field" style="margin-top:10px">
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
          <span class="app-name">个人笔记本 <small>v{{ appVersion }}</small></span>
          <span class="copy">Copyright © 2026 zth · 开源协议 MIT</span>
        </div>
        <div class="about-actions">
          <button class="btn" :disabled="checkingUpdate" @click="manualCheckUpdate">
            {{ checkingUpdate ? "检查中…" : "🔄 检查更新" }}
          </button>
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
  /* 单列全宽布局：每个设置区块独占一行铺满，避免左右区块高度悬殊不对称 */
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
h2 {
  margin-bottom: 0;
}
.block {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 16px;
  margin-bottom: 0;
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
  max-width: 300px;
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
.desc .s.path {
  color: var(--text);
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  word-break: break-all;
  margin-top: 4px;
}
.btn-group {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.about-actions {
  margin-bottom: 12px;
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

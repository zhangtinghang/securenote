/**
 * 后台任务进度：全局响应式状态（顶栏进度胶囊 / 设置页按钮共用）
 * 数据来源：Rust 端 export-progress / export-done 事件
 * kind：export（导出加密包）| migrate-data（数据目录迁移）| migrate-backup（备份目录迁移）
 */
import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { restartApp } from "./api";

/** 是否正在后台任务（导出/迁移共用单任务槽） */
export const exportRunning = ref(false);
/** 进度 0-100 */
export const exportPercent = ref(0);
/** 当前阶段描述 */
export const exportMessage = ref("");
/** 任务类型（顶栏文案区分） */
export const exportKind = ref("export");
/** 完成后的短暂提示（顶栏展示几秒），空串表示无 */
export const exportFlash = ref("");
/** 是否需要遮罩限制用户操作（数据迁移期间为 true，防止写入导致快照与迁移不一致） */
export const exportBlocking = ref(false);

let inited = false;
let flashTimer: ReturnType<typeof setTimeout> | null = null;

function showFlash(text: string, ms = 8000) {
  exportFlash.value = text;
  if (flashTimer) clearTimeout(flashTimer);
  flashTimer = setTimeout(() => (exportFlash.value = ""), ms);
}

/** 注册事件监听（App.vue onMounted 调用一次） */
export async function initExportProgressListener() {
  if (inited) return;
  inited = true;
  await listen<{
    kind: string;
    stage: string;
    percent: number;
    message: string;
    path: string;
    error: string;
  }>("export-progress", (e) => {
    const p = e.payload || ({} as any);
    exportRunning.value = p.stage === "run";
    exportKind.value = p.kind ?? "export";
    exportPercent.value = p.percent ?? 0;
    exportMessage.value = p.message ?? "";
    // 数据迁移期间需要遮罩限制操作（快照 + 迁移期间避免写入）
    exportBlocking.value = exportRunning.value && p.kind === "migrate-data";
  });
  await listen<{
    kind: string;
    stage: string;
    percent: number;
    message: string;
    path: string;
    error: string;
  }>("export-done", (e) => {
    const p = e.payload || ({} as any);
    exportRunning.value = false;
    exportKind.value = p.kind ?? "export";
    exportBlocking.value = false;
    if (p.kind === "migrate-data") {
      if (p.stage === "done") {
        showFlash("✓ 数据迁移完成，即将重启…");
        setTimeout(() => restartApp().catch(() => {}), 1000);
      } else {
        showFlash(`✗ 数据迁移失败：${p.error || "未知错误"}（原目录数据未受影响）`);
      }
      return;
    }
    if (p.kind === "rollback") {
      if (p.stage === "done") {
        showFlash("✓ 回滚完成，即将刷新…");
        setTimeout(() => window.location.reload(), 1000);
      } else {
        showFlash(`✗ 回滚失败：${p.error || "未知错误"}`);
      }
      return;
    }
    if (p.stage === "done") {
      if (p.kind === "migrate-backup") {
        showFlash(`✓ ${p.message || "备份迁移完成"}`);
      } else {
        exportPercent.value = 100;
        showFlash(`✓ 导出完成：${p.path || ""}`);
      }
    } else {
      exportPercent.value = 0;
      showFlash(`✗ ${p.message || "任务失败"}：${p.error || "未知错误"}`);
    }
  });
}

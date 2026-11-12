<script setup lang="ts">
import { ref } from "vue";

/**
 * 全局确认弹窗：替代原生 window.confirm
 * （Tauri WebView 下原生 confirm 可能被自动放行，导致"还没点确认就执行了删除"）
 * 用法：App.vue 挂载并通过 provide("appConfirm") 暴露
 */
const visible = ref(false);
const message = ref("");
const confirmText = ref("确认");
let resolver: ((v: boolean) => void) | null = null;

function ask(msg: string, okText = "确认删除"): Promise<boolean> {
  message.value = msg;
  confirmText.value = okText;
  visible.value = true;
  return new Promise((resolve) => {
    resolver = resolve;
  });
}

function answer(v: boolean) {
  if (!visible.value) return;
  visible.value = false;
  resolver?.(v);
  resolver = null;
}

defineExpose({ ask });
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="confirm-mask" @click.self="answer(false)" @keydown.esc="answer(false)">
      <div class="confirm-dialog">
        <div class="confirm-icon">⚠️</div>
        <div class="confirm-msg">{{ message }}</div>
        <div class="confirm-actions">
          <button class="btn cancel-btn" @click="answer(false)">取消</button>
          <button class="btn confirm-ok" @click="answer(true)">{{ confirmText }}</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.confirm-mask {
  position: fixed;
  inset: 0;
  background: rgba(15, 18, 25, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 500;
}
.confirm-dialog {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 24px;
  width: 340px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.18);
  text-align: center;
  animation: pop 0.16s ease-out;
}
@keyframes pop {
  from {
    transform: scale(0.92);
    opacity: 0;
  }
  to {
    transform: scale(1);
    opacity: 1;
  }
}
.confirm-icon {
  font-size: 34px;
  margin-bottom: 10px;
}
.confirm-msg {
  font-size: 14px;
  line-height: 1.6;
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-all;
}
.confirm-actions {
  display: flex;
  gap: 10px;
  justify-content: center;
  margin-top: 18px;
}
.confirm-actions .btn {
  min-width: 88px;
  justify-content: center;
}
/* 双类名提高优先级，确保不被全局 .btn.danger / UA 样式覆盖 */
.confirm-dialog .confirm-ok {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.confirm-dialog .confirm-ok:hover {
  filter: brightness(0.92);
  background: var(--accent);
}
</style>

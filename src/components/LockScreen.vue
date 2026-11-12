<script setup lang="ts">
import { ref, computed, inject } from "vue";
import { auth, vault, pickBackupFile } from "../api";

const appConfirm = inject<(m: string, okText?: string) => Promise<boolean>>("appConfirm")!;

const props = defineProps<{ initialized: boolean }>();
const emit = defineEmits<{ (e: "unlocked"): void }>();

const password = ref("");
const confirm = ref("");
const error = ref("");
const busy = ref(false);
const restoring = ref(false);
/** 已选择的备份文件内容（进入恢复流程后缓存） */
const backupContent = ref("");

const isSetup = computed(() => !props.initialized);

/** 从备份初始化：先选文件，再让用户输入原主密码 */
async function pickAndRestore() {
  if (busy.value || restoring.value) return;
  error.value = "";
  try {
    const [, data] = await pickBackupFile();
    backupContent.value = data;
    password.value = "";
    confirm.value = "";
    restoring.value = true; // 进入恢复输入状态
  } catch (e: any) {
    const msg = String(e?.message || e);
    if (msg.includes("取消")) return;
    error.value = "读取备份失败: " + msg;
  }
}

/** 执行恢复（输入原主密码后调用） */
async function doRestore() {
  if (!backupContent.value) {
    error.value = "请先选择备份文件";
    return;
  }
  if (!password.value) {
    error.value = "请输入该备份对应的主密码";
    return;
  }
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await vault.restoreFromBackup(backupContent.value, password.value);
    emit("unlocked");
  } catch (e: any) {
    error.value = "恢复失败: " + String(e?.message || e);
  } finally {
    busy.value = false;
  }
}

/** 取消恢复，回到创建主密码流程 */
function cancelRestore() {
  restoring.value = false;
  backupContent.value = "";
  password.value = "";
  confirm.value = "";
  error.value = "";
}

const strength = computed(() => {
  const p = password.value;
  if (!p) return { label: "", pct: 0, color: "" };
  let score = 0;
  if (p.length >= 8) score++;
  if (/[a-z]/.test(p) && /[A-Z]/.test(p)) score++;
  if (/\d/.test(p)) score++;
  if (/[^a-zA-Z0-9]/.test(p)) score++;
  if (p.length >= 12) score++;
  const levels = [
    { label: "弱", pct: 20, color: "var(--red)" },
    { label: "较弱", pct: 40, color: "var(--amber)" },
    { label: "中等", pct: 60, color: "var(--amber)" },
    { label: "强", pct: 80, color: "var(--green)" },
    { label: "很强", pct: 100, color: "var(--green)" },
  ];
  return levels[Math.min(score, 4)];
});

async function submit() {
  if (busy.value) return; // 防止重复提交（双击/回车连按）
  error.value = "";  if (isSetup.value) {
    if (password.value.length < 8) {
      error.value = "主密码至少 8 位";
      return;
    }
    if (password.value !== confirm.value) {
      error.value = "两次输入的密码不一致";
      return;
    }
  }
  busy.value = true;
  try {
    if (isSetup.value) {
      await auth.setup(password.value);
      emit("unlocked");
    } else {
      const ok = await auth.unlock(password.value);
      if (!ok) {
        error.value = "主密码错误";
        return;
      }
      emit("unlocked");
    }
  } catch (e: any) {
    error.value = String(e?.message || e);
  } finally {
    busy.value = false;
  }
}

/** 重置存储库（清空全部数据），确认后执行并刷新 */
async function resetAll() {
  const ok = await appConfirm(
    "重置将清空全部数据且不可恢复，确定继续？",
    "确认重置"
  );
  if (!ok) return;
  try {
    await auth.reset();
    window.location.reload();
  } catch (e: any) {
    error.value = String(e?.message || e);
  }
}
</script>

<template>
  <div class="lock-wrap">
    <div class="lock-box">
      <div class="lock-icon">{{ restoring ? "♻️" : isSetup ? "🔐" : "🔓" }}</div>
      <h1>{{ restoring ? "从备份恢复" : isSetup ? "创建主密码" : "欢迎回来" }}</h1>
      <p class="sub">
        {{
          restoring
            ? "输入备份对应的主密码，即可从备份文件重建全部数据"
            : isSetup
              ? "主密码用于加密您的所有数据，请妥善保管，遗忘后无法找回"
              : "输入主密码解锁您的加密存储库"
        }}
      </p>

      <!-- 恢复模式：输入原主密码 -->
      <template v-if="restoring">
        <input
          v-model="password"
          type="password"
          placeholder="输入备份对应的主密码"
          @keyup.enter="doRestore"
        />
        <div v-if="error" class="error">{{ error }}</div>
        <button class="btn primary submit" :disabled="busy" @click="doRestore">
          {{ busy ? "恢复中..." : "♻️ 恢复并进入" }}
        </button>
        <div class="hint">
          <a class="link" @click.prevent="cancelRestore">← 返回创建主密码</a>
        </div>
      </template>

      <!-- 创建/解锁模式 -->
      <template v-else>
        <input
          v-model="password"
          type="password"
          :placeholder="isSetup ? '输入主密码（至少 8 位）' : '主密码'"
          @keyup.enter="submit"
        />
        <input
          v-if="isSetup"
          v-model="confirm"
          type="password"
          placeholder="再次确认主密码"
          @keyup.enter="submit"
        />

        <div v-if="isSetup && password" class="strength">
          <div class="strength-bar" :style="{ width: strength.pct + '%', background: strength.color }"></div>
        </div>
        <div v-if="isSetup && password" class="strength-label" :style="{ color: strength.color }">
          密码强度：{{ strength.label }}
        </div>

        <div v-if="error" class="error">{{ error }}</div>

        <button class="btn primary submit" :disabled="busy" @click="submit">
          {{ busy ? "处理中..." : isSetup ? "创建并进入" : "解锁" }}
        </button>

        <div v-if="isSetup" class="hint">
          已有备份文件？
          <a class="link" @click.prevent="pickAndRestore">从备份恢复</a>
        </div>
        <div v-if="!isSetup" class="hint">
          忘记主密码？
          <a class="link" @click.prevent="resetAll">重置（将清空全部数据）</a>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.lock-wrap {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg);
}
.lock-box {
  width: 400px;
  max-width: 90%;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 14px;
  box-shadow: var(--shadow);
  padding: 40px 36px;
  text-align: center;
}
.lock-icon {
  font-size: 44px;
  margin-bottom: 12px;
}
h1 {
  font-size: 20px;
  margin-bottom: 8px;
}
.sub {
  color: var(--muted);
  font-size: 13px;
  margin-bottom: 24px;
  line-height: 1.6;
}
input {
  width: 100%;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 14px;
  margin-bottom: 12px;
  outline: none;
  /* 显式指定：暗黑下不依赖 UA 默认（否则文字很淡看不清） */
  color: var(--text);
  background: var(--card);
  caret-color: var(--text);
}
input::placeholder {
  color: var(--muted);
  opacity: 1;
}
input:focus {
  border-color: var(--accent);
}
.strength {
  height: 6px;
  border-radius: 3px;
  background: #eef0f3;
  overflow: hidden;
  margin: 4px 0 6px;
}
.strength-bar {
  height: 100%;
  border-radius: 3px;
  transition: width 0.3s;
}
.strength-label {
  font-size: 12px;
  text-align: left;
  margin-bottom: 16px;
}
.error {
  color: var(--red);
  font-size: 13px;
  margin-bottom: 12px;
}
.submit {
  width: 100%;
  justify-content: center;
  padding: 12px;
  font-size: 14px;
}
.hint {
  margin-top: 16px;
  font-size: 12px;
  color: var(--muted);
}
.link {
  color: var(--accent);
  cursor: pointer;
}
</style>

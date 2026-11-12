export type ThemeName = "light" | "eye" | "dark";

/** 应用主题：root data-theme 驱动 CSS 变量切换 */
export function applyTheme(t: ThemeName) {
  if (t === "light") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = t;
  }
  localStorage.setItem("sn_theme", t);
}

/** 启动时恢复上次选择的主题 */
export function initTheme() {
  const t = localStorage.getItem("sn_theme") as ThemeName | null;
  if (t && t !== "light") {
    document.documentElement.dataset.theme = t;
  }
}

export const themeOptions: Array<{ value: ThemeName; label: string; desc: string; colors: string[] }> = [
  { value: "light", label: "默认浅色", desc: "清爽明亮", colors: ["#f5f6f8", "#ffffff", "#4f6ef7"] },
  { value: "eye", label: "护眼绿", desc: "柔和米绿，长时间写作更舒适", colors: ["#e7ecd9", "#f0f4e4", "#5a8a4f"] },
  { value: "dark", label: "暗黑", desc: "深色背景，夜间使用不刺眼", colors: ["#14161a", "#1d2026", "#6c86ff"] },
];

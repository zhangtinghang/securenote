import { ref } from "vue";

/**
 * 列表面板宽度拖拽调节（按住分隔条左右拖动）
 * 宽度持久化到 localStorage，下次启动保持
 */
export function useResizable(key: string, initial: number, min = 220, maxRatio = 0.6) {
  const saved = Number(localStorage.getItem(key));
  const width = ref(Number.isFinite(saved) && saved > 0 ? saved : initial);
  let startX = 0;
  let startW = 0;

  function onDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    startX = e.clientX;
    startW = width.value;
    const move = (ev: MouseEvent) => {
      const maxW = window.innerWidth * maxRatio;
      width.value = Math.min(maxW, Math.max(min, startW + (ev.clientX - startX)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      localStorage.setItem(key, String(width.value));
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  return { width, onDown };
}

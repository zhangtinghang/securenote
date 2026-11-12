/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

// mammoth 浏览器构建（docx → HTML），无类型定义
declare module "mammoth/mammoth.browser" {
  const mammoth: any;
  export default mammoth;
}

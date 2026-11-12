import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import "./style.css";

const app = createApp(App);
// 全局错误兜底：渲染/事件异常记录日志而不是白屏
app.config.errorHandler = (err, _instance, info) => {
  console.error("[SecureNote]", info, err);
};
app.use(createPinia());
app.use(router);
app.mount("#app");

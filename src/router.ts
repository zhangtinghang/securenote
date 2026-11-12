import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", redirect: "/notes" },
    { path: "/notes", name: "notes", component: () => import("./views/NotesView.vue") },
    { path: "/todos", name: "todos", component: () => import("./views/TodosView.vue") },
    { path: "/passwords", name: "passwords", component: () => import("./views/PasswordsView.vue") },
    { path: "/toolbox", name: "toolbox", component: () => import("./views/ToolboxView.vue") },
    { path: "/trash", name: "trash", component: () => import("./views/TrashView.vue") },
    { path: "/backup", name: "backup", component: () => import("./views/BackupView.vue") },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue") },
  ],
});

export default router;

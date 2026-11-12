import { invoke } from "@tauri-apps/api/core";
import type { Note, Todo, PasswordEntry, SyncPayload, TrashData } from "./types";

// 认证与密钥管理
export const auth = {
  isInitialized: () => invoke<boolean>("is_initialized"),
  setup: (masterPassword: string) =>
    invoke<void>("setup_vault", { masterPassword }),
  unlock: (masterPassword: string) => invoke<boolean>("unlock_vault", { masterPassword }),
  lock: () => invoke<void>("lock_vault"),
  isUnlocked: () => invoke<boolean>("is_unlocked"),
  changeMasterPassword: (oldPwd: string, newPwd: string) =>
    invoke<void>("change_master_password", { oldPwd, newPwd }),
  reset: () => invoke<void>("reset_vault"),
};

// 笔记
export const notes = {
  list: () => invoke<Note[]>("list_notes"),
  get: (id: string) => invoke<Note>("get_note", { id }),
  create: (title: string, notebook: string) =>
    invoke<Note>("create_note", { title, notebook }),
  update: (id: string, title: string, content: string, notebook: string) =>
    invoke<Note>("update_note", { id, title, content, notebook }),
  remove: (id: string) => invoke<void>("delete_note", { id }),
  toggleFavorite: (id: string, favorite: boolean) =>
    invoke<void>("toggle_note_favorite", { id, favorite }),
};

// 待办
export const todos = {
  list: () => invoke<Todo[]>("list_todos"),
  create: (todo: Partial<Todo>) => invoke<Todo>("create_todo", { todo }),
  update: (todo: Todo) => invoke<Todo>("update_todo", { todo }),
  remove: (id: string) => invoke<void>("delete_todo", { id }),
  toggle: (id: string, completed: boolean) =>
    invoke<Todo>("toggle_todo", { id, completed }),
  /** 拖拽排序：按传入 id 顺序持久化 */
  reorder: (ids: string[]) => invoke<void>("reorder_todos", { ids }),
  togglePinned: (id: string, pinned: boolean) =>
    invoke<void>("toggle_todo_pinned", { id, pinned }),
  toggleFavorite: (id: string, favorite: boolean) =>
    invoke<void>("toggle_todo_favorite", { id, favorite }),
};

// 密码库
export const passwords = {
  list: () => invoke<PasswordEntry[]>("list_passwords"),
  create: (entry: Partial<PasswordEntry>) =>
    invoke<PasswordEntry>("create_password", { entry }),
  update: (entry: PasswordEntry) =>
    invoke<PasswordEntry>("update_password", { entry }),
  remove: (id: string) => invoke<void>("delete_password", { id }),
  reveal: (id: string) => invoke<string>("reveal_password", { id }),
  generate: (length: number, useUpper: boolean, useLower: boolean, useDigits: boolean, useSymbols: boolean) =>
    invoke<string>("generate_password", { length, useUpper, useLower, useDigits, useSymbols }),
};

// 导入导出与同步
export const vault = {
  /** 一键导出：弹出保存框并写入加密文件，返回保存路径 */
  exportToFile: () => invoke<string>("export_vault_to_file"),
  /** 一键导入：弹出选择框读取加密文件并还原，返回 [处理条数, 文件路径] */
  importFromFile: (mode: string) =>
    invoke<[number, string]>("import_vault_from_file", { mode }),
  /** 原始导出（内存中的加密串，调试用） */
  export: () => invoke<string>("export_vault"),
  import: (encoded: string, mode: string) =>
    invoke<number>("import_vault", { encoded, mode }),
};

/** 用系统默认浏览器打开链接 */
export const openUrl = (url: string) =>
  invoke<void>("open_external_url", { url });

/** 投屏/录屏内容保护（Windows）：开启后共享屏幕与录屏中窗口不可见 */
export const setContentProtection = (enable: boolean) =>
  invoke<void>("set_content_protection", { enable });

/** 退出应用（触发退出前自动备份询问） */
export const quitApp = () => invoke<void>("quit_app");

/** 强制备份（静默，升级前/设置页触发），返回备份文件路径 */
export const forceBackup = () => invoke<string>("force_backup");

/** 获取当前应用版本号 */
export const getAppVersion = () => invoke<string>("get_app_version");

/** 获取当前备份目录路径 */
export const getBackupDir = () => invoke<string>("get_backup_dir");

/** 弹出目录选择框，设置备份目录，返回新目录路径 */
export const chooseBackupDir = () => invoke<string>("choose_backup_dir");

/** 恢复默认备份目录 */
export const resetBackupDir = () => invoke<string>("reset_backup_dir");

/** 选择要导入的文档（docx/txt/log/md/csv/html），返回 [文件名, 扩展名, base64 内容] */
export const pickDocFile = () =>
  invoke<[string, string, string]>("import_doc_file");

/** 批量选择要导入的文档（多选），返回 [[文件名, 扩展名, base64 内容], ...] */
export const pickDocFiles = () =>
  invoke<[string, string, string][]>("import_doc_files");

/** 选择密码批量导入文件（xlsx/csv），返回 [文件名, base64 内容] */
export const pickPasswordFile = () =>
  invoke<[string, string]>("import_passwords_file");

/** 保存密码导入模板（xlsx 的 base64 字节），返回保存路径 */
export const saveTemplate = (dataBase64: string) =>
  invoke<string>("save_template_file", { dataBase64 });

/** 导出文本文件（Markdown 等），弹出保存框，返回保存路径 */
export const saveTextFile = (fileName: string, content: string) =>
  invoke<string>("save_text_file", { fileName, content });

/** 导出二进制文件（Excel 等，base64），弹出保存框，返回保存路径 */
export const saveDataFile = (fileName: string, dataBase64: string) =>
  invoke<string>("save_data_file", { fileName, dataBase64 });

// 回收站（软删除条目，15 天后自动物理清理）
export const trash = {
  list: () => invoke<TrashData>("list_trash"),
  restore: (kind: "note" | "todo" | "password", id: string) =>
    invoke<void>("restore_item", { kind, id }),
  /** 彻底删除单条（物理删除，不可恢复） */
  purge: (kind: "note" | "todo" | "password", id: string) =>
    invoke<void>("purge_item", { kind, id }),
  /** 清空回收站，返回清除条数 */
  empty: () => invoke<number>("empty_trash"),
  /** 立即清理 15 天前条目，返回清除条数 */
  purgeExpired: () => invoke<number>("purge_expired"),
};

// 通知
export const notifications = {
  schedule: (todoId: string, remindAt: string, title: string) =>
    invoke<void>("schedule_reminder", { todoId, remindAt, title }),
  cancel: (todoId: string) => invoke<void>("cancel_reminder", { todoId }),
};

export type { Note, Todo, PasswordEntry, SyncPayload };

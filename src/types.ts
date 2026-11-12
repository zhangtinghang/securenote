// 业务数据类型定义
export interface Note {
  id: string;
  title: string;
  content: string; // JSON 序列化的富文本内容
  notebook: string;
  favorite: boolean;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface Todo {
  id: string;
  title: string;
  note: string;
  list: string;
  priority: "high" | "medium" | "low";
  due_at: string | null;
  remind_at: string | null;
  repeat: "none" | "daily" | "weekly" | "monthly" | "weekday";
  completed: boolean;
  completed_at: string | null;
  sort_order: number;
  pinned: boolean;
  favorite: boolean;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface PasswordEntry {
  id: string;
  name: string;
  username: string;
  password: string;
  url: string;
  note: string;
  category: string;
  favorite: boolean;
  tags: string[];
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface SyncPayload {
  format: string;
  version: number;
  exported_at: string;
  payload: {
    notes: Note[];
    todos: Todo[];
    passwords: PasswordEntry[];
    meta: Record<string, unknown>;
  };
}

/** 回收站数据 */
export interface TrashData {
  notes: Note[];
  todos: Todo[];
  passwords: PasswordEntry[];
}

export interface SyncChange {
  entity: "note" | "todo" | "password";
  id: string;
  updated_at: string;
  deleted: boolean;
}

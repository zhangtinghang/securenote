//! SecureNote 主库：Tauri 命令层
mod crypto;
mod models;
mod reminder;
mod store;
mod vault;

use models::*;
use reminder::ReminderScheduler;
use rusqlite::params;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use vault::Vault;

/// 应用全局状态
pub struct AppState {
    vault: Arc<Vault>,
    scheduler: Arc<ReminderScheduler>,
}

/// 显示并聚焦主窗口（从托盘唤起）
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 退出应用（自绘标题栏"关闭"按钮调用）：备份询问在独立线程执行，完成后退出
#[tauri::command]
fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    std::thread::spawn(move || {
        let _ = auto_backup_on_quit(&handle);
        handle.exit(0);
    });
    Ok(())
}

// ============ 认证与密钥 ============

#[tauri::command]
fn is_initialized(state: tauri::State<AppState>) -> bool {
    state.vault.is_initialized()
}

#[tauri::command]
fn is_unlocked(state: tauri::State<AppState>) -> bool {
    state.vault.is_unlocked()
}

#[tauri::command]
fn setup_vault(master_password: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.vault.setup(&master_password).map_err(|e| e.to_string())
}

#[tauri::command]
fn unlock_vault(master_password: String, state: tauri::State<AppState>) -> Result<bool, String> {
    state.vault.unlock(&master_password).map_err(|e| e.to_string())
}

#[tauri::command]
fn lock_vault(state: tauri::State<AppState>) {
    state.vault.lock();
}

#[tauri::command]
fn change_master_password(
    old_pwd: String,
    new_pwd: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state
        .vault
        .change_master_password(&old_pwd, &new_pwd)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_vault(state: tauri::State<AppState>) -> Result<(), String> {
    state.vault.reset().map_err(|e| e.to_string())
}

// ============ 笔记 ============

#[tauri::command]
fn list_notes(state: tauri::State<AppState>) -> Result<Vec<Note>, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    vault_load_notes_filtered(&conn, dek.as_ref(), false).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_note(id: String, state: tauri::State<AppState>) -> Result<Note, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    vault_get_note(&conn, dek.as_ref(), &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_note(
    title: String,
    notebook: String,
    state: tauri::State<AppState>,
) -> Result<Note, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    let id = vault::new_id();
    let note = Note {
        id: id.clone(),
        title: title.clone(),
        content: String::new(),
        notebook: notebook.clone(),
        favorite: false,
        created_at: now.clone(),
        updated_at: now,
        deleted_at: None,
    };
    conn.execute(
        "INSERT INTO notes(id, title, content, notebook, favorite, created_at, updated_at, deleted_at)
         VALUES(?1,?2,?3,?4,0,?5,?6,NULL)",
        params![
            note.id,
            crypto::encrypt_str(&title, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str("", dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&notebook, dek.as_ref()).map_err(|e| e.to_string())?,
            note.created_at,
            note.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(note)
}

#[tauri::command]
fn update_note(
    id: String,
    title: String,
    content: String,
    notebook: String,
    state: tauri::State<AppState>,
) -> Result<Note, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE notes SET title=?2, content=?3, notebook=?4, updated_at=?5 WHERE id=?1",
        params![
            id,
            crypto::encrypt_str(&title, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&content, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&notebook, dek.as_ref()).map_err(|e| e.to_string())?,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    let note = vault_get_note(&conn, dek.as_ref(), &id).map_err(|e| e.to_string())?;
    Ok(note)
}

#[tauri::command]
fn delete_note(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE notes SET deleted_at=?2 WHERE id=?1",
        params![id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn toggle_note_favorite(id: String, favorite: bool, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        "UPDATE notes SET favorite=?2 WHERE id=?1",
        params![id, favorite as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ============ 待办 ============

#[tauri::command]
fn list_todos(state: tauri::State<AppState>) -> Result<Vec<Todo>, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    vault_load_todos_filtered(&conn, dek.as_ref(), false).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_todo(todo: TodoInput, state: tauri::State<AppState>) -> Result<Todo, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    let id = vault::new_id();
    // 排到手动排序末尾
    let max_order: i64 = conn
        .query_row("SELECT COALESCE(MAX(sort_order), -1) FROM todos", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let full = Todo {
        id: id.clone(),
        title: todo.title.clone(),
        note: todo.note.clone(),
        list: todo.list.clone(),
        priority: todo.priority.clone(),
        due_at: todo.due_at.clone(),
        remind_at: todo.remind_at.clone(),
        repeat: todo.repeat.clone(),
        completed: false,
        completed_at: None,
        sort_order: max_order + 1,
        pinned: false,
        favorite: false,
        created_at: now.clone(),
        updated_at: now,
        deleted_at: None,
    };
    conn.execute(
        "INSERT INTO todos(id, title, note, list, priority, due_at, remind_at, repeat, completed, completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,NULL,?9,0,0,?10,?11,NULL)",
        params![
            full.id,
            crypto::encrypt_str(&full.title, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.note, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.list, dek.as_ref()).map_err(|e| e.to_string())?,
            full.priority,
            full.due_at,
            full.remind_at,
            full.repeat,
            full.sort_order,
            full.created_at,
            full.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(full)
}

#[tauri::command]
fn update_todo(todo: Todo, state: tauri::State<AppState>) -> Result<Todo, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE todos SET title=?2, note=?3, list=?4, priority=?5, due_at=?6, remind_at=?7,
                repeat=?8, completed=?9, completed_at=?10, sort_order=?11, pinned=?12, favorite=?13, updated_at=?14 WHERE id=?1",
        params![
            todo.id,
            crypto::encrypt_str(&todo.title, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&todo.note, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&todo.list, dek.as_ref()).map_err(|e| e.to_string())?,
            todo.priority,
            todo.due_at,
            todo.remind_at,
            todo.repeat,
            todo.completed as i64,
            todo.completed_at,
            todo.sort_order,
            todo.pinned as i64,
            todo.favorite as i64,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(todo)
}

/// 拖拽排序：按传入 id 顺序重写 sort_order
#[tauri::command]
fn reorder_todos(ids: Vec<String>, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE todos SET sort_order=?2 WHERE id=?1",
            params![id, i as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 待办置顶
#[tauri::command]
fn toggle_todo_pinned(id: String, pinned: bool, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        "UPDATE todos SET pinned=?2 WHERE id=?1",
        params![id, pinned as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 待办收藏
#[tauri::command]
fn toggle_todo_favorite(id: String, favorite: bool, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        "UPDATE todos SET favorite=?2 WHERE id=?1",
        params![id, favorite as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_todo(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute("UPDATE todos SET deleted_at=?2 WHERE id=?1", params![id, now])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn toggle_todo(id: String, completed: bool, state: tauri::State<AppState>) -> Result<Todo, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    let completed_at = if completed { Some(now.clone()) } else { None };
    conn.execute(
        "UPDATE todos SET completed=?2, completed_at=?3, updated_at=?4 WHERE id=?1",
        params![id, completed as i64, completed_at, now],
    )
    .map_err(|e| e.to_string())?;
    // 返回更新后的完整待办（前端依赖返回值刷新列表）
    let todos = vault_load_todos_filtered(&conn, dek.as_ref(), false).map_err(|e| e.to_string())?;
    todos
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "待办不存在".to_string())
}

// ============ 密码库 ============

#[tauri::command]
fn list_passwords(state: tauri::State<AppState>) -> Result<Vec<PasswordEntry>, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    vault_load_passwords_filtered(&conn, dek.as_ref(), false).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_password(
    entry: PasswordInput,
    state: tauri::State<AppState>,
) -> Result<PasswordEntry, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    let id = vault::new_id();
    let full = PasswordEntry {
        id: id.clone(),
        name: entry.name.clone(),
        username: entry.username.clone(),
        password: entry.password.clone(),
        url: entry.url.clone(),
        note: entry.note.clone(),
        category: entry.category.clone(),
        favorite: false,
        tags: entry.tags.clone(),
        created_at: now.clone(),
        updated_at: now,
        deleted_at: None,
    };
    conn.execute(
        "INSERT INTO passwords(id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10,NULL)",
        params![
            full.id,
            crypto::encrypt_str(&full.name, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.username, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.password, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.url, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.note, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&full.category, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(
                &serde_json::to_string(&full.tags).unwrap_or_default(),
                dek.as_ref()
            )
            .map_err(|e| e.to_string())?,
            full.created_at,
            full.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(full)
}

#[tauri::command]
fn update_password(
    entry: PasswordEntry,
    state: tauri::State<AppState>,
) -> Result<PasswordEntry, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE passwords SET name=?2, username=?3, password=?4, url=?5, note=?6, category=?7, tags=?8, updated_at=?9 WHERE id=?1",
        params![
            entry.id,
            crypto::encrypt_str(&entry.name, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&entry.username, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&entry.password, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&entry.url, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&entry.note, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(&entry.category, dek.as_ref()).map_err(|e| e.to_string())?,
            crypto::encrypt_str(
                &serde_json::to_string(&entry.tags).unwrap_or_default(),
                dek.as_ref()
            )
            .map_err(|e| e.to_string())?,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(entry)
}

/// 选择文档文件（docx/txt/log/md/csv/html），返回文件名 + 扩展名 + base64 内容
/// 说明：IPC 传输二进制不可靠，统一用 base64 字符串
#[tauri::command]
fn import_doc_file() -> Result<(String, String, String), String> {
    let path = rfd::FileDialog::new()
        .set_title("选择要导入的文档")
        .add_filter(
            "支持的文档 (docx/txt/log/md/csv/html)",
            &["docx", "txt", "log", "md", "markdown", "csv", "html", "htm"],
        )
        .pick_file();
    let path = path.ok_or("已取消导入")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.is_empty() {
        return Err("文件内容为空".to_string());
    }
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("文件过大（超过 20MB）".to_string());
    }
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "导入的笔记".to_string());
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    use base64::Engine;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((file_name, ext, data_base64))
}

/// 批量选择文档（多选），返回每个文件的 [文件名, 扩展名, base64 内容]
#[tauri::command]
fn import_doc_files() -> Result<Vec<(String, String, String)>, String> {
    let paths = rfd::FileDialog::new()
        .set_title("选择要批量导入的文档（可多选）")
        .add_filter(
            "支持的文档 (docx/txt/log/md/csv/html)",
            &["docx", "txt", "log", "md", "markdown", "csv", "html", "htm"],
        )
        .pick_files();
    let paths = paths.ok_or("已取消导入")?;
    if paths.is_empty() {
        return Err("未选择任何文件".to_string());
    }
    use base64::Engine;
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
        if bytes.is_empty() {
            continue; // 跳过空文件
        }
        if bytes.len() > 20 * 1024 * 1024 {
            continue; // 跳过超大文件
        }
        let file_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "导入的笔记".to_string());
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let data_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        out.push((file_name, ext, data_base64));
    }
    if out.is_empty() {
        return Err("没有可导入的文件".to_string());
    }
    Ok(out)
}

/// 选择密码批量导入文件（xlsx/csv），返回文件名 + base64 内容
#[tauri::command]
fn import_passwords_file() -> Result<(String, String), String> {
    let path = rfd::FileDialog::new()
        .set_title("选择密码批量导入文件")
        .add_filter("表格文件 (xlsx/csv)", &["xlsx", "csv"])
        .pick_file();
    let path = path.ok_or("已取消导入")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.is_empty() {
        return Err("文件内容为空".to_string());
    }
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    use base64::Engine;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((file_name, data_base64))
}

/// 导出文本文件（Markdown 等），弹出保存框写入，返回保存路径
#[tauri::command]
fn save_text_file(file_name: String, content: String) -> Result<String, String> {
    let path = rfd::FileDialog::new()
        .set_title("导出文件")
        .set_file_name(&file_name)
        .add_filter("Markdown", &["md"])
        .add_filter("Word 文档", &["doc"])
        .add_filter("文本文件", &["txt"])
        .save_file();
    let path = path.ok_or("已取消导出")?;
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 导出二进制文件（Excel 等以 base64 传入），弹出保存框写入，返回保存路径
#[tauri::command]
fn save_data_file(file_name: String, data_base64: String) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data_base64)
        .map_err(|e| format!("数据解码失败: {e}"))?;
    let path = rfd::FileDialog::new()
        .set_title("导出文件")
        .set_file_name(&file_name)
        .add_filter("Excel 工作簿", &["xlsx"])
        .save_file();
    let path = path.ok_or("已取消导出")?;
    std::fs::write(&path, &bytes).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 保存密码导入模板（xlsx 字节以 base64 传入，弹出保存框写入）
#[tauri::command]
fn save_template_file(data_base64: String) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data_base64)
        .map_err(|e| format!("模板数据解码失败: {e}"))?;
    let path = rfd::FileDialog::new()
        .set_title("保存密码导入模板")
        .set_file_name("SecureNote-密码导入模板.xlsx")
        .add_filter("Excel 工作簿", &["xlsx"])
        .save_file();
    let path = path.ok_or("已取消保存")?;
    std::fs::write(&path, &bytes).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn delete_password(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE passwords SET deleted_at=?2 WHERE id=?1",
        params![id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reveal_password(id: String, state: tauri::State<AppState>) -> Result<String, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    let encrypted: String = conn
        .query_row(
            "SELECT password FROM passwords WHERE id=?1 AND deleted_at IS NULL",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    crypto::decrypt_str(&encrypted, dek.as_ref())
        .map(|s| s.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn generate_password(
    length: usize,
    use_upper: bool,
    use_lower: bool,
    use_digits: bool,
    use_symbols: bool,
) -> String {
    let mut chars = String::new();
    if use_upper {
        chars.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    }
    if use_lower {
        chars.push_str("abcdefghijklmnopqrstuvwxyz");
    }
    if use_digits {
        chars.push_str("0123456789");
    }
    if use_symbols {
        chars.push_str("!@#$%^&*()-_=+[]{};:,.<>?");
    }
    if chars.is_empty() {
        chars.push_str("abcdefghijklmnopqrstuvwxyz0123456789");
    }
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let len = length.clamp(4, 128);
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..chars.len());
            chars.as_bytes()[idx] as char
        })
        .collect()
}

// ============ 导入导出 ============

#[tauri::command]
fn export_vault(state: tauri::State<AppState>) -> Result<String, String> {
    state.vault.export().map_err(|e| e.to_string())
}

/// 一键导出：弹出保存对话框并写入加密文件，返回保存路径
#[tauri::command]
fn export_vault_to_file(state: tauri::State<AppState>) -> Result<String, String> {
    let encoded = state.vault.export().map_err(|e| e.to_string())?;
    let path = rfd::FileDialog::new()
        .set_title("导出加密备份")
        .set_file_name(
            format!("securenote-backup-{}.snvault", chrono::Local::now().format("%Y-%m-%d")),
        )
        .add_filter("SecureNote 加密备份", &["snvault"])
        .save_file();
    let path = path.ok_or("已取消导出")?;
    std::fs::write(&path, encoded).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 一键导入：弹出选择对话框并读取加密文件，返回 (处理条数, 文件路径)
#[tauri::command]
fn import_vault_from_file(
    mode: String,
    state: tauri::State<AppState>,
) -> Result<(usize, String), String> {
    let path = rfd::FileDialog::new()
        .set_title("选择加密备份文件")
        .add_filter("SecureNote 加密备份", &["snvault", "json", "txt"])
        .pick_file();
    let path = path.ok_or("已取消导入")?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    let count = state.vault.import(&content, &mode).map_err(|e| e.to_string())?;
    Ok((count, path.display().to_string()))
}

/// 投屏/录屏内容保护（Windows）：开启后共享屏幕、录屏、截图中窗口不可见
#[tauri::command]
#[cfg(target_os = "windows")]
fn set_content_protection(window: tauri::WebviewWindow, enable: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let affinity = if enable { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    let ok = unsafe { SetWindowDisplayAffinity(hwnd.0 as _, affinity) };
    if ok == 0 {
        Err("设置投屏保护失败（可能系统版本过低，需 Windows 10 2004+）".to_string())
    } else {
        Ok(())
    }
}

/// 非 Windows 平台暂不支持内容保护，直接返回成功
#[tauri::command]
#[cfg(not(target_os = "windows"))]
fn set_content_protection(_window: tauri::WebviewWindow, _enable: bool) -> Result<(), String> {
    Ok(())
}

/// 备份目录配置文件名（存于应用数据目录，与加密库分离，无需解锁即可读写）
const BACKUP_SETTINGS_FILE: &str = "backup_settings.json";

/// 读取用户配置的备份目录；未配置时回退到默认 数据目录/backup
fn backup_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let data_dir = app.path().app_data_dir().ok()?;
    let cfg_path = data_dir.join(BACKUP_SETTINGS_FILE);
    // 尝试读取配置的自定义目录
    if let Ok(text) = std::fs::read_to_string(&cfg_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(dir) = v.get("backup_dir").and_then(|d| d.as_str()) {
                if !dir.trim().is_empty() {
                    return Some(std::path::PathBuf::from(dir));
                }
            }
        }
    }
    Some(data_dir.join("backup"))
}

/// 写入自定义备份目录
fn set_backup_dir_inner(app: &tauri::AppHandle, dir: &str) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let cfg_path = data_dir.join(BACKUP_SETTINGS_FILE);
    let json = serde_json::json!({ "backup_dir": dir }).to_string();
    std::fs::write(&cfg_path, json).map_err(|e| format!("写入配置失败: {e}"))
}

/// 获取当前备份目录（供设置页展示）
#[tauri::command]
fn get_backup_dir(app: tauri::AppHandle) -> Result<String, String> {
    let dir = backup_dir(&app).ok_or("无法获取应用数据目录")?;
    Ok(dir.display().to_string())
}

/// 弹出目录选择框，让用户选择备份目录，返回新目录路径
#[tauri::command]
fn choose_backup_dir(app: tauri::AppHandle) -> Result<String, String> {
    let default = backup_dir(&app).unwrap_or_default();
    let dir = rfd::FileDialog::new()
        .set_title("选择备份存放目录")
        .set_directory(&default)
        .pick_folder();
    let dir = dir.ok_or("已取消选择")?;
    let path = dir.display().to_string();
    set_backup_dir_inner(&app, &path)?;
    Ok(path)
}

/// 恢复默认备份目录（数据目录/backup）
#[tauri::command]
fn reset_backup_dir(app: tauri::AppHandle) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let default = data_dir.join("backup");
    set_backup_dir_inner(&app, default.display().to_string().as_str())?;
    Ok(default.display().to_string())
}

/// 静默备份：导出加密库到配置的备份目录，滚动保留最近 3 份（不弹窗、不询问）
/// 返回 Some(备份路径) 表示本次完成备份；未解锁/无数据时返回 None
fn backup_vault_silent(app: &tauri::AppHandle) -> Option<String> {
    let state: tauri::State<AppState> = app.state();
    // 未解锁或导出失败（无数据）时静默跳过
    let encoded = match state.vault.export() {
        Ok(e) => e,
        Err(_) => return None,
    };
    let dir = backup_dir(app)?;
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let name = format!(
        "SecureNote_backup_{}.snvault",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(name);
    if let Err(e) = std::fs::write(&path, encoded) {
        eprintln!("写入备份失败: {e}");
        return None;
    }
    // 滚动清理：只保留最近 3 份（文件名含时间戳，字典序即时间序）
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "snvault"))
        .collect();
    files.sort();
    while files.len() > 3 {
        let oldest = files.remove(0);
        let _ = std::fs::remove_file(oldest);
    }
    Some(path.display().to_string())
}

/// 退出前自动备份：静默执行（每次关闭都备份，不询问），滚动保留最近 3 份
/// 返回 Some(备份路径) 表示本次完成了备份
fn auto_backup_on_quit(app: &tauri::AppHandle) -> Option<String> {
    backup_vault_silent(app)
}

/// 升级/手动触发强制备份（静默，供更新流程与设置页调用）
#[tauri::command]
fn force_backup(app: tauri::AppHandle) -> Result<String, String> {
    backup_vault_silent(&app).ok_or_else(|| "备份失败（可能尚未解锁或无数据）".to_string())
}

/// 返回当前应用版本号（单一来源：Cargo.toml，随每次迭代 bump）
#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 用系统默认浏览器打开网址
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("仅支持 http/https 链接".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn import_vault(
    encoded: String,
    mode: String,
    state: tauri::State<AppState>,
) -> Result<usize, String> {
    state
        .vault
        .import(&encoded, &mode)
        .map_err(|e| e.to_string())
}

// ============ 提醒 ============

#[tauri::command]
fn schedule_reminder(
    todo_id: String,
    remind_at: String,
    _title: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // 提醒时间直接存储到 todos.remind_at，由后台调度器轮询
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        "UPDATE todos SET remind_at=?2 WHERE id=?1",
        params![todo_id, remind_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn cancel_reminder(todo_id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        "UPDATE todos SET remind_at=NULL WHERE id=?1",
        params![todo_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ============ 回收站 ============

/// 回收站数据（三类已删除条目）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrashData {
    notes: Vec<Note>,
    todos: Vec<Todo>,
    passwords: Vec<PasswordEntry>,
}

fn table_of(kind: &str) -> Result<&'static str, String> {
    match kind {
        "note" => Ok("notes"),
        "todo" => Ok("todos"),
        "password" => Ok("passwords"),
        _ => Err(format!("未知的条目类型: {kind}")),
    }
}

#[tauri::command]
fn list_trash(state: tauri::State<AppState>) -> Result<TrashData, String> {
    let dek = state.vault.dek().map_err(|e| e.to_string())?;
    let store = state.vault.store();
    let conn = store.conn();
    Ok(TrashData {
        notes: vault_load_notes_filtered(&conn, dek.as_ref(), true).map_err(|e| e.to_string())?,
        todos: vault_load_todos_filtered(&conn, dek.as_ref(), true).map_err(|e| e.to_string())?,
        passwords: vault_load_passwords_filtered(&conn, dek.as_ref(), true)
            .map_err(|e| e.to_string())?,
    })
}

/// 从回收站恢复
#[tauri::command]
fn restore_item(kind: String, id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let table = table_of(&kind)?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(
        &format!("UPDATE {table} SET deleted_at=NULL WHERE id=?1"),
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 彻底删除（物理删除单条）
#[tauri::command]
fn purge_item(kind: String, id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let table = table_of(&kind)?;
    let store = state.vault.store();
    let conn = store.conn_mut();
    conn.execute(&format!("DELETE FROM {table} WHERE id=?1"), params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 清空回收站，返回清除条数
#[tauri::command]
fn empty_trash(state: tauri::State<AppState>) -> Result<usize, String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    let mut total = 0;
    for table in ["notes", "todos", "passwords"] {
        total += conn
            .execute(&format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL"), [])
            .map_err(|e| e.to_string())?;
    }
    Ok(total)
}

/// 自动清理：物理删除 15 天前进入回收站的条目，返回清除条数
#[tauri::command]
fn purge_expired(state: tauri::State<AppState>) -> Result<usize, String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    let mut total = 0;
    for table in ["notes", "todos", "passwords"] {
        total += conn
            .execute(
                &format!(
                    "DELETE FROM {table} WHERE deleted_at IS NOT NULL
                     AND julianday(deleted_at) < julianday('now', '-15 days')"
                ),
                [],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(total)
}

// ============ 内部辅助函数（复用 vault.rs 的加载逻辑） ============

fn vault_load_notes_filtered(
    conn: &rusqlite::Connection,
    dek: &[u8],
    only_deleted: bool,
) -> Result<Vec<Note>, vault::VaultError> {
    let sql = if only_deleted {
        "SELECT id, title, content, notebook, favorite, created_at, updated_at, deleted_at
         FROM notes WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
    } else {
        "SELECT id, title, content, notebook, favorite, created_at, updated_at, deleted_at
         FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, title, content, notebook, favorite, created_at, updated_at, deleted_at) = row?;
        result.push(Note {
            id,
            title: crypto::decrypt_str(&title, dek)?.to_string(),
            content: crypto::decrypt_str(&content, dek)?.to_string(),
            notebook: crypto::decrypt_str(&notebook, dek)?.to_string(),
            favorite: favorite != 0,
            created_at,
            updated_at,
            deleted_at,
        });
    }
    Ok(result)
}

fn vault_get_note(
    conn: &rusqlite::Connection,
    dek: &[u8],
    id: &str,
) -> Result<Note, vault::VaultError> {
    let row = conn.query_row(
        "SELECT id, title, content, notebook, favorite, created_at, updated_at, deleted_at
         FROM notes WHERE id=?1 AND deleted_at IS NULL",
        params![id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        },
    )?;
    let (id, title, content, notebook, favorite, created_at, updated_at, deleted_at) = row;
    Ok(Note {
        id,
        title: crypto::decrypt_str(&title, dek)?.to_string(),
        content: crypto::decrypt_str(&content, dek)?.to_string(),
        notebook: crypto::decrypt_str(&notebook, dek)?.to_string(),
        favorite: favorite != 0,
        created_at,
        updated_at,
        deleted_at,
    })
}

fn vault_load_todos_filtered(
    conn: &rusqlite::Connection,
    dek: &[u8],
    only_deleted: bool,
) -> Result<Vec<Todo>, vault::VaultError> {
    let sql = if only_deleted {
        "SELECT id, title, note, list, priority, due_at, remind_at, repeat, completed,
                completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at
         FROM todos WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
    } else {
        "SELECT id, title, note, list, priority, due_at, remind_at, repeat, completed,
                completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at
         FROM todos WHERE deleted_at IS NULL ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, i64>(10)?,
            r.get::<_, i64>(11)?,
            r.get::<_, i64>(12)?,
            r.get::<_, String>(13)?,
            r.get::<_, String>(14)?,
            r.get::<_, Option<String>>(15)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (
            id, title, note, list, priority, due_at, remind_at, repeat, completed, completed_at,
            sort_order, pinned, favorite, created_at, updated_at, deleted_at,
        ) = row?;
        result.push(Todo {
            id,
            title: crypto::decrypt_str(&title, dek)?.to_string(),
            note: crypto::decrypt_str(&note, dek)?.to_string(),
            list: crypto::decrypt_str(&list, dek)?.to_string(),
            priority,
            due_at,
            remind_at,
            repeat,
            completed: completed != 0,
            completed_at,
            sort_order,
            pinned: pinned != 0,
            favorite: favorite != 0,
            created_at,
            updated_at,
            deleted_at,
        });
    }
    Ok(result)
}

fn vault_load_passwords_filtered(
    conn: &rusqlite::Connection,
    dek: &[u8],
    only_deleted: bool,
) -> Result<Vec<PasswordEntry>, vault::VaultError> {
    let sql = if only_deleted {
        "SELECT id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at
         FROM passwords WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
    } else {
        "SELECT id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at
         FROM passwords WHERE deleted_at IS NULL ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, Option<String>>(11)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at) =
            row?;
        let tags_list: Vec<String> = if tags.is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&crypto::decrypt_str(&tags, dek)?.to_string()).unwrap_or_default()
        };
        result.push(PasswordEntry {
            id,
            name: crypto::decrypt_str(&name, dek)?.to_string(),
            username: crypto::decrypt_str(&username, dek)?.to_string(),
            password: crypto::decrypt_str(&password, dek)?.to_string(),
            url: crypto::decrypt_str(&url, dek)?.to_string(),
            note: crypto::decrypt_str(&note, dek)?.to_string(),
            category: crypto::decrypt_str(&category, dek)?.to_string(),
            favorite: favorite != 0,
            tags: tags_list,
            created_at,
            updated_at,
            deleted_at,
        });
    }
    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&data_dir)?;
            let vault = Arc::new(Vault::new(data_dir)?);
            let scheduler = Arc::new(ReminderScheduler::new());
            // 启动提醒调度器
            scheduler.start(app.handle().clone(), vault.clone());
            app.manage(AppState { vault, scheduler });

            // ============ 系统托盘 ============
            let show = MenuItem::with_id(app, "tray-show", "显示主界面", true, None::<&str>)?;
            let new_note = MenuItem::with_id(app, "tray-new-note", "新建笔记", true, None::<&str>)?;
            let new_todo = MenuItem::with_id(app, "tray-new-todo", "新建待办", true, None::<&str>)?;
            let lock = MenuItem::with_id(app, "tray-lock", "锁定", true, None::<&str>)?;
            let sep = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "tray-quit", "退出个人笔记本", true, None::<&str>)?;
            let tray_menu = Menu::with_items(
                app,
                &[&show, &sep, &new_note, &new_todo, &sep, &lock, &sep, &quit],
            )?;

            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().expect("缺少应用图标").clone())
                .tooltip("个人笔记本")
                .menu(&tray_menu)
                .show_menu_on_left_click(false) // 左键唤起窗口，右键弹菜单
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "tray-show" => show_main_window(app),
                        "tray-new-note" => {
                            show_main_window(app);
                            let _ = app.emit("tray-navigate", "/notes");
                        }
                        "tray-new-todo" => {
                            show_main_window(app);
                            let _ = app.emit("tray-navigate", "/todos");
                        }
                        "tray-lock" => {
                            let state: tauri::State<AppState> = app.state();
                            state.vault.lock();
                            let _ = app.emit("vault-locked", ());
                        }
                        // 退出：备份询问在独立线程执行（对话框不阻塞主线程），完成后退出
                        "tray-quit" => {
                            let handle = app.clone();
                            std::thread::spawn(move || {
                                let _ = auto_backup_on_quit(&handle);
                                handle.exit(0);
                            });
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // 左键单击托盘图标：唤起主窗口
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 点击标题栏关闭按钮 → 隐藏到托盘（提醒调度持续运行），托盘菜单"退出"才真正退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                let _ = window.emit("tray-minimized", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            is_initialized,
            is_unlocked,
            setup_vault,
            unlock_vault,
            lock_vault,
            change_master_password,
            reset_vault,
            list_notes,
            get_note,
            create_note,
            update_note,
            delete_note,
            toggle_note_favorite,
            list_todos,
            create_todo,
            update_todo,
            delete_todo,
            toggle_todo,
            toggle_todo_pinned,
            toggle_todo_favorite,
            reorder_todos,
            import_doc_file,
            import_doc_files,
            import_passwords_file,
            save_template_file,
            save_text_file,
            save_data_file,
            list_passwords,
            create_password,
            update_password,
            delete_password,
            reveal_password,
            generate_password,
            export_vault,
            export_vault_to_file,
            force_backup,
            get_app_version,
            get_backup_dir,
            choose_backup_dir,
            reset_backup_dir,
            set_content_protection,
            import_vault_from_file,
            import_vault,
            open_external_url,
            quit_app,
            schedule_reminder,
            cancel_reminder,
            list_trash,
            restore_item,
            purge_item,
            empty_trash,
            purge_expired,
        ])
        .run(tauri::generate_context!())
        .expect("SecureNote 运行失败");
}

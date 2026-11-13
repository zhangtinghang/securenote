//! SecureNote 主库：Tauri 命令层
pub mod crypto;
mod models;
mod reminder;
pub mod store;
pub mod vault;

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
    graceful_exit(app);
    Ok(())
}

/// 退出中标记：防止 CloseRequested 与 quit_app 双触发叠加执行退出流程
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 统一退出流程：立即隐藏窗口并退出进程（关闭时不再自动备份——备份由用户手动触发或闲时自动备份完成）。
/// 必须用 std::process::exit：Tauri 的 AppHandle::exit() 会走窗口关闭流程，
/// 触发 CloseRequested 被 prevent_close 拦截 → 陷入"要求退出却被拦"的循环，永远退不出去。
fn graceful_exit(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    // 幂等：第二次调用直接忽略（点 × 与托盘退出可能连击）
    if EXITING.swap(true, Ordering::SeqCst) {
        return;
    }
    // 立即隐藏窗口：视觉上瞬间关闭
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    std::process::exit(0);
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
        // 简化符号集：只保留常见的 , . ! @ # $（用户反馈复杂符号影响可用性）
        chars.push_str(",.!@#$");
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

// ============ 后台任务（导出 / 数据迁移 / 备份迁移：不卡 UI，带进度与完成通知） ============

/// 后台任务进度（前端通过 export-progress / export-done 事件接收）
#[derive(Default, Clone, serde::Serialize)]
struct TaskProgress {
    /// 任务类型：export | migrate-data | migrate-backup
    kind: String,
    /// run | done | error
    stage: String,
    /// 0-100
    percent: u32,
    message: String,
    path: String,
    error: String,
}

/// 后台任务状态（managed state；字段用 Arc 包一层便于 clone 进后台线程）
/// 单任务槽：同一时间只允许一个重后台任务（导出/迁移互斥，避免文件级冲突）
#[derive(Default, Clone)]
struct BgTaskState {
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: std::sync::Arc<parking_lot::Mutex<TaskProgress>>,
}

/// 更新进度并广播给前端
fn set_task_progress(
    job: &BgTaskState,
    app: &tauri::AppHandle,
    kind: &str,
    stage: &str,
    percent: u32,
    message: &str,
    path: &str,
) {
    let prog = TaskProgress {
        kind: kind.to_string(),
        stage: stage.to_string(),
        percent,
        message: message.to_string(),
        path: path.to_string(),
        error: String::new(),
    };
    *job.progress.lock() = prog.clone();
    let _ = app.emit("export-progress", prog);
}

/// 发送任务完成/失败事件
fn emit_task_done(job: &BgTaskState, app: &tauri::AppHandle, kind: &str, ok: bool, message: &str, error: &str, path: &str) {
    let prog = TaskProgress {
        kind: kind.to_string(),
        stage: if ok { "done".to_string() } else { "error".to_string() },
        percent: if ok { 100 } else { 0 },
        message: message.to_string(),
        path: path.to_string(),
        error: error.to_string(),
    };
    *job.progress.lock() = prog.clone();
    let _ = app.emit("export-progress", prog.clone());
    let _ = app.emit("export-done", prog);
}

/// 尝试占用后台任务槽；成功返回守卫式标记（简单起见返回 bool，调用方负责释放）
fn try_acquire_task(job: &BgTaskState) -> bool {
    use std::sync::atomic::Ordering;
    job.running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

fn release_task(job: &BgTaskState) {
    use std::sync::atomic::Ordering;
    job.running.store(false, Ordering::SeqCst);
}

/// 分块写入大文件并报告进度（72→100）
fn write_file_chunked(
    path: &std::path::Path,
    bytes: &[u8],
    report: &dyn Fn(u32, &str),
) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    let total = bytes.len().max(1);
    let chunk = 4 * 1024 * 1024;
    let mut written = 0usize;
    while written < bytes.len() {
        let end = (written + chunk).min(bytes.len());
        f.write_all(&bytes[written..end])?;
        written = end;
        let pct = 72 + ((written as f64 / total as f64) * 27.0) as u32;
        report(pct.min(99), "写入文件中…");
    }
    f.flush()?;
    Ok(())
}

/// 后台导出：先弹保存对话框，然后在后台线程执行导出（不阻塞 UI），
/// 进度通过 export-progress 事件推送，完成后发系统通知 + export-done 事件
#[tauri::command]
fn start_export_to_file(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    // 防并发：已有后台任务（导出/迁移）进行中
    if !try_acquire_task(&job) {
        return Err("已有后台任务进行中，请等待完成".to_string());
    }
    // 未解锁提前报错（避免弹框后才发现无法导出）
    if !state.vault.is_unlocked() {
        release_task(&job);
        return Err("尚未解锁，无法导出".to_string());
    }
    // 弹出保存位置对话框（快，不涉及数据读写）
    let path = rfd::FileDialog::new()
        .set_title("导出加密备份")
        .set_file_name(format!(
            "securenote-backup-{}.snvault",
            chrono::Local::now().format("%Y-%m-%d")
        ))
        .add_filter("SecureNote 加密备份", &["snvault"])
        .save_file();
    let path = match path {
        Some(p) => p,
        None => {
            release_task(&job);
            return Err("已取消导出".to_string());
        }
    };
    let path_str = path.display().to_string();

    set_task_progress(&job, &app, "export", "run", 1, "开始导出…", &path_str);

    // 后台线程执行：读取 → 加密 → 写文件，全程通过事件推送进度
    let job2 = job.inner().clone();
    let app2 = app.clone();
    let vault2 = state.vault.clone(); // Arc<Vault>
    let path2 = path.clone();
    let path3 = path_str.clone();
    std::thread::spawn(move || {
        let report = |p: u32, m: &str| {
            set_task_progress(&job2, &app2, "export", "run", p, m, &path3);
        };
        let result = vault2.export_with_progress(&report);
        match result {
            Ok(encoded) => match write_file_chunked(&path2, encoded.as_bytes(), &report) {
                Ok(()) => {
                    emit_task_done(&job2, &app2, "export", true, "导出完成", "", &path3);
                    // 系统通知（不依赖前端页面状态）
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app2
                        .notification()
                        .builder()
                        .title("个人笔记本：导出完成")
                        .body(format!("加密备份已保存到 {}", path3))
                        .show();
                }
                Err(e) => {
                    emit_task_done(&job2, &app2, "export", false, "导出失败", &format!("写入文件失败: {e}"), &path3);
                }
            },
            Err(e) => {
                emit_task_done(&job2, &app2, "export", false, "导出失败", &e.to_string(), &path3);
            }
        }
        release_task(&job2);
    });
    Ok(path_str)
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

/// 读取用户配置的备份目录；未配置时回退到默认 数据目录/backup（跟随数据存储目录）
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
    let vault_dir = vault_data_dir(app).ok()?;
    Some(vault_dir.join("backup"))
}

/// 备份目录/闲时备份配置：读（缺省空对象）
fn read_backup_settings(app: &tauri::AppHandle) -> serde_json::Value {
    if let Ok(data_dir) = app.path().app_data_dir() {
        let cfg_path = data_dir.join(BACKUP_SETTINGS_FILE);
        if let Ok(text) = std::fs::read_to_string(&cfg_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                return v;
            }
        }
    }
    serde_json::json!({})
}

/// 备份目录/闲时备份配置：写
fn write_backup_settings(app: &tauri::AppHandle, value: &serde_json::Value) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let cfg_path = data_dir.join(BACKUP_SETTINGS_FILE);
    std::fs::write(&cfg_path, value.to_string()).map_err(|e| format!("写入配置失败: {e}"))
}

/// 写入自定义备份目录
fn set_backup_dir_inner(app: &tauri::AppHandle, dir: &str) -> Result<(), String> {
    let mut v = read_backup_settings(app);
    v["backup_dir"] = serde_json::json!(dir);
    write_backup_settings(app, &v)
}

/// 最后一次用户活动时间（unix 秒；前端节流上报，0 表示尚无活动）
static LAST_ACTIVITY_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 前端节流上报用户活动（鼠标/键盘/滚动），用于判断"闲时"
#[tauri::command]
fn touch_activity() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    LAST_ACTIVITY_SECS.store(now, std::sync::atomic::Ordering::Relaxed);
}

/// 是否开启闲时自动备份
fn idle_backup_enabled(app: &tauri::AppHandle) -> bool {
    read_backup_settings(app)
        .get("idle_backup")
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
}

/// 写入闲时自动备份开关
fn set_idle_backup_inner(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let mut v = read_backup_settings(app);
    v["idle_backup"] = serde_json::json!(enabled);
    write_backup_settings(app, &v)
}

/// 获取闲时自动备份开关（供设置页展示）
#[tauri::command]
fn get_idle_backup(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(idle_backup_enabled(&app))
}

/// 设置闲时自动备份开关
#[tauri::command]
fn set_idle_backup(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    set_idle_backup_inner(&app, enabled)
}

/// 上次闲时备份对应的数据指纹（None 表示启动后尚未备份过）
static LAST_BACKUP_FINGERPRINT: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);

/// 数据指纹：各表条数 + 最新更新时间（一条 SQL，开销极小）
fn vault_fingerprint(app: &tauri::AppHandle) -> Option<String> {
    let state: tauri::State<AppState> = app.state();
    let store = state.vault.store();
    let conn = store.conn();
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM notes) || '-' || (SELECT COUNT(*) FROM todos) || '-' || \
         (SELECT COUNT(*) FROM passwords) || '-' || (SELECT COUNT(*) FROM attachments) || '-' || \
         (SELECT IFNULL(MAX(updated_at),'') FROM notes) || '-' || \
         (SELECT IFNULL(MAX(updated_at),'') FROM todos) || '-' || \
         (SELECT IFNULL(MAX(updated_at),'') FROM passwords)",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// 闲时自动备份线程：每分钟检查一次；
/// 条件 = 已开启 + 已解锁 + 空闲超过 3 分钟 + 数据自上次备份后有变化
fn start_idle_backup_thread(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        if !idle_backup_enabled(&app) {
            continue;
        }
        let state: tauri::State<AppState> = app.state();
        if !state.vault.is_unlocked() {
            continue;
        }
        // 空闲判定：最近 3 分钟无用户操作（启动后从未活动视为空闲）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last = LAST_ACTIVITY_SECS.load(std::sync::atomic::Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < 180 {
            continue;
        }
        // 数据有变化才备份
        let fp = vault_fingerprint(&app);
        if LAST_BACKUP_FINGERPRINT.lock().as_ref() == fp.as_ref() {
            continue;
        }
        // 增量备份（数据变化时优先增量，避免频繁全量）
        if create_incremental_backup(&app).is_ok() {
            *LAST_BACKUP_FINGERPRINT.lock() = fp;
            // 增量数量达到阈值 → 通知前端建议全量备份
            let manifest = read_manifest(&app);
            let inc_count = manifest
                .points
                .iter()
                .filter(|p| p.kind == BackupKind::Incremental)
                .count();
            if inc_count >= manifest.inc_threshold {
                let _ = app.emit("backup-suggest-full", inc_count);
            }
        }
    });
}

/// 获取当前备份目录（供设置页展示）
#[tauri::command]
fn get_backup_dir(app: tauri::AppHandle) -> Result<String, String> {
    let dir = backup_dir(&app).ok_or("无法获取应用数据目录")?;
    Ok(dir.display().to_string())
}

/// 后台线程：把旧备份目录下所有 .snvault 逐个分块复制到新目录（原目录保留），
/// 全程通过事件推送进度；复制完成后发 done 事件
fn spawn_backup_dir_migration(
    app: tauri::AppHandle,
    job: BgTaskState,
    old_dir: std::path::PathBuf,
    new_dir: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        let new_dir_str = new_dir.display().to_string();
        let report = |p: u32, m: &str| {
            set_task_progress(&job, &app, "migrate-backup", "run", p, m, &new_dir_str);
        };

        // 收集待迁移文件与总大小
        let mut files: Vec<(std::path::PathBuf, u64, String)> = std::fs::read_dir(&old_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |x| x == "snvault"))
            .filter_map(|p| {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Some((p, size, name))
            })
            .collect();
        files.sort_by(|a, b| a.2.cmp(&b.2));
        let total: u64 = files.iter().map(|f| f.1).sum();

        if files.is_empty() {
            emit_task_done(&job, &app, "migrate-backup", true, "旧目录没有历史备份文件", "", &new_dir_str);
            release_task(&job);
            return;
        }

        let _ = std::fs::create_dir_all(&new_dir);
        let mut copied: u64 = 0;
        let mut count = 0usize;
        let mut failed = 0usize;
        for (path, size, name) in &files {
            report(
                ((copied as f64 / total.max(1) as f64) * 100.0) as u32,
                &format!("迁移 {} …", name),
            );
            match std::fs::copy(path, new_dir.join(name)) {
                Ok(_) => {
                    copied += size;
                    count += 1;
                }
                Err(_) => failed += 1,
            }
        }
        if failed > 0 {
            emit_task_done(
                &job,
                &app,
                "migrate-backup",
                false,
                "备份迁移失败",
                &format!("成功 {count} 份，失败 {failed} 份（原目录文件保留，可重试）"),
                &new_dir_str,
            );
        } else {
            emit_task_done(
                &job,
                &app,
                "migrate-backup",
                true,
                &format!("已迁移 {count} 份历史备份"),
                "",
                &new_dir_str,
            );
        }
        release_task(&job);
    });
}

/// 弹出目录选择框，让用户选择备份目录，返回新目录路径
/// 更改后历史备份文件（.snvault）在后台线程复制到新目录（大数据量不卡 UI）
#[tauri::command]
fn choose_backup_dir(
    app: tauri::AppHandle,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    let default = backup_dir(&app).unwrap_or_default();
    let dir = rfd::FileDialog::new()
        .set_title("选择备份存放目录")
        .set_directory(&default)
        .pick_folder();
    let dir = dir.ok_or("已取消选择")?;
    let path = dir.display().to_string();
    set_backup_dir_inner(&app, &path)?;
    // 历史备份文件后台迁移到新目录（复制，原目录保留）
    if default != dir && try_acquire_task(&job) {
        spawn_backup_dir_migration(app, job.inner().clone(), default, dir.clone());
    }
    Ok(path)
}

/// 恢复默认备份目录（数据目录/backup，跟随数据存储目录）
/// 恢复时清除自定义配置文件，使默认目录动态跟随数据目录；历史备份文件后台迁移过去
#[tauri::command]
fn reset_backup_dir(
    app: tauri::AppHandle,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    let old_dir = backup_dir(&app).unwrap_or_default();
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // 清除自定义配置，让默认目录动态跟随数据目录
    let _ = std::fs::remove_file(data_dir.join(BACKUP_SETTINGS_FILE));
    let default = backup_dir(&app).ok_or("无法确定默认备份目录")?;
    if old_dir != default && try_acquire_task(&job) {
        spawn_backup_dir_migration(app, job.inner().clone(), old_dir, default.clone());
    }
    Ok(default.display().to_string())
}

/// 手动触发全量备份：后台线程 + 文件级快照（SQLite 在线备份直接复制加密 vault 文件）。
/// 相比逻辑导出（解密全部附件→重加密→序列化），速度快 1-2 个数量级且不卡 UI。
/// 进度走 export-progress 事件（kind=backup-full），完成后发 export-done。
#[tauri::command]
fn force_backup(app: tauri::AppHandle, job: tauri::State<BgTaskState>) -> Result<String, String> {
    if !try_acquire_task(&job) {
        return Err("已有后台任务进行中，请稍后再试".to_string());
    }
    let app2 = app.clone();
    let job2 = job.inner().clone();
    std::thread::spawn(move || {
        let report = |p: u32, m: &str| {
            set_task_progress(&job2, &app2, "backup-full", "run", p, m, "");
        };
        let result = create_full_backup(&app2, &report);
        match result {
            Ok(point) => {
                // 同步数据指纹，避免闲时线程重复备份
                let fp = vault_fingerprint(&app2);
                *LAST_BACKUP_FINGERPRINT.lock() = fp;
                emit_task_done(&job2, &app2, "backup-full", true, "全量备份完成", "", &point.filename);
            }
            Err(e) => emit_task_done(&job2, &app2, "backup-full", false, "备份失败", &e, ""),
        }
        release_task(&job2);
    });
    Ok("全量备份已开始".to_string())
}

// ==================== 备份版本管理（类 git：全量 + 增量 + 回滚） ====================

const MANIFEST_FILE: &str = "manifest.json";

/// 读取备份清单；不存在或损坏时返回默认（空）清单。
/// 兜底纠正历史坏数据：max_full/inc_threshold 为 0 时（旧版 derive(Default) 写入）恢复默认值。
fn read_manifest(app: &tauri::AppHandle) -> BackupManifest {
    let dir = match backup_dir(app) {
        Some(d) => d,
        None => return BackupManifest::default(),
    };
    let path = dir.join(MANIFEST_FILE);
    let mut manifest = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<BackupManifest>(&t).ok())
        .unwrap_or_default();
    if manifest.max_full == 0 {
        manifest.max_full = 3;
    }
    if manifest.inc_threshold == 0 {
        manifest.inc_threshold = 20;
    }
    manifest
}

/// 写备份清单（备份目录需已存在）
fn write_manifest(app: &tauri::AppHandle, manifest: &BackupManifest) -> Result<(), String> {
    let dir = backup_dir(app).ok_or("无法获取备份目录")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MANIFEST_FILE), json).map_err(|e| format!("写入清单失败: {e}"))
}

/// 生成新备份点 id（时间戳）
fn new_backup_id() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S_%3f").to_string()
}

/// 清理：全量备份保留上限（滚动删除最旧的全量，并级联删除基于它的增量）
fn enforce_full_retention(app: &tauri::AppHandle, manifest: &mut BackupManifest) {
    // 保险丝：至少保留 1 份全量（max_full=0 会导致刚备份就被删光）
    let max_full = manifest.max_full.max(1);
    let fulls: Vec<usize> = manifest
        .points
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == BackupKind::Full)
        .map(|(i, _)| i)
        .collect();
    if fulls.len() <= max_full {
        return;
    }
    // 最旧的一个全量及更早的所有点都要删（保持链完整）
    let cut = fulls[0];
    let dir = match backup_dir(app) {
        Some(d) => d,
        None => return,
    };
    let removed: Vec<BackupPoint> = manifest.points.drain(..=cut).collect();
    for p in &removed {
        let _ = std::fs::remove_file(dir.join(&p.filename));
    }
}

/// 创建全量备份点（逻辑导出：完整加密快照 .snvault），返回新点；自动滚动保留最近 max_full 份。
/// 用独立只读连接导出，不占主连接锁；必须在后台线程调用（配合 report 进度回调）。
fn create_full_backup(
    app: &tauri::AppHandle,
    report: &dyn Fn(u32, &str),
) -> Result<BackupPoint, String> {
    let state: tauri::State<AppState> = app.state();
    let encoded = state
        .vault
        .export_with_independent_conn(&|p, m| report(p, m))
        .map_err(|e| e.to_string())?;
    let dir = backup_dir(app).ok_or("无法获取备份目录")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let id = new_backup_id();
    let filename = format!("SecureNote_full_{}.snvault", id);
    let path = dir.join(&filename);
    std::fs::write(&path, &encoded).map_err(|e| format!("写入备份失败: {e}"))?;

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let point = BackupPoint {
        id,
        kind: BackupKind::Full,
        created_at: chrono::Local::now().to_rfc3339(),
        size,
        filename,
        parent_id: None,
        change_count: 0,
    };

    let mut manifest = read_manifest(app);
    manifest.points.push(point.clone());
    enforce_full_retention(app, &mut manifest);
    write_manifest(app, &manifest)?;
    Ok(point)
}

/// 创建增量备份点（自上一个备份点以来的变更），返回新点
fn create_incremental_backup(app: &tauri::AppHandle) -> Result<BackupPoint, String> {
    let state: tauri::State<AppState> = app.state();
    let manifest = read_manifest(app);
    // 无任何备份点时退化为全量备份（增量必须有基准）
    let last = match manifest.points.last() {
        Some(p) => p.clone(),
        None => return create_full_backup(app, &|_, _| {}),
    };
    let since = last.created_at.clone();
    let (encoded, change_count) = state
        .vault
        .export_incremental(&since, &|_, _| {})
        .map_err(|e| e.to_string())?;

    let dir = backup_dir(app).ok_or("无法获取备份目录")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let id = new_backup_id();
    let filename = format!("SecureNote_inc_{}.sninc", id);
    let path = dir.join(&filename);
    std::fs::write(&path, &encoded).map_err(|e| format!("写入备份失败: {e}"))?;

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let point = BackupPoint {
        id,
        kind: BackupKind::Incremental,
        created_at: chrono::Local::now().to_rfc3339(),
        size,
        filename,
        parent_id: Some(last.id),
        change_count,
    };
    let mut manifest = read_manifest(app);
    manifest.points.push(point.clone());
    write_manifest(app, &manifest)?;
    Ok(point)
}

/// 列出所有备份点（按时间升序）
#[tauri::command]
fn list_backup_points(app: tauri::AppHandle) -> Result<Vec<BackupPoint>, String> {
    let manifest = read_manifest(&app);
    Ok(manifest.points)
}

/// 删除指定备份点；删除全量时级联删除基于它的后续增量（保证链完整）
#[tauri::command]
fn delete_backup_point(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let dir = backup_dir(&app).ok_or("无法获取备份目录")?;
    let mut manifest = read_manifest(&app);
    let idx = manifest
        .points
        .iter()
        .position(|p| p.id == id)
        .ok_or("备份点不存在")?;
    let target = manifest.points[idx].clone();

    let mut to_remove = vec![idx];
    if target.kind == BackupKind::Full {
        // 级联：删除该全量之后、直到下一个全量之前的所有增量
        for (i, p) in manifest.points.iter().enumerate().skip(idx + 1) {
            if p.kind == BackupKind::Full {
                break;
            }
            to_remove.push(i);
        }
    }
    // 从后往前删，避免索引错乱
    to_remove.sort_unstable_by(|a, b| b.cmp(a));
    for i in to_remove {
        let p = manifest.points.remove(i);
        let _ = std::fs::remove_file(dir.join(&p.filename));
    }
    write_manifest(&app, &manifest)
}

/// 回滚到指定备份点：先还原「该点之前最近的完整快照」，再依次应用其后的增量。
/// 回滚前自动创建当前状态的全量备份（防止误操作，可再滚回来）。
#[tauri::command]
fn rollback_to_backup(
    app: tauri::AppHandle,
    target_id: String,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    if !try_acquire_task(&job) {
        return Err("已有后台任务进行中，请稍后再试".to_string());
    }
    let app2 = app.clone();
    let job2 = job.inner().clone();
    std::thread::spawn(move || {
        let report = |p: u32, m: &str| {
            set_task_progress(&job2, &app2, "rollback", "run", p, m, "");
        };
        let result = (|| -> Result<(), String> {
            report(2, "回滚前备份当前状态…");
            // 先全量备份当前状态（安全兜底，可再滚回来）
            let _ = create_full_backup(&app2, &report);

            let state: tauri::State<AppState> = app2.state();
            let manifest = read_manifest(&app2);
            let dir = backup_dir(&app2).ok_or("无法获取备份目录")?;
            let target_idx = manifest
                .points
                .iter()
                .position(|p| p.id == target_id)
                .ok_or("备份点不存在")?;

            // 找该点之前最近的完整快照
            let mut base_idx: Option<usize> = None;
            for (i, p) in manifest.points.iter().enumerate().take(target_idx + 1).rev() {
                if p.kind == BackupKind::Full {
                    base_idx = Some(i);
                    break;
                }
            }
            let base_idx = base_idx.ok_or("该备份点缺少完整快照基准")?;

            // 1. 还原完整快照（replace 模式）
            report(15, "还原完整快照…");
            let base = &manifest.points[base_idx];
            let base_content = std::fs::read_to_string(dir.join(&base.filename))
                .map_err(|e| format!("读取快照失败: {e}"))?;
            state
                .vault
                .import(&base_content, "replace")
                .map_err(|e| e.to_string())?;

            // 2. 依次应用其后的增量（merge 模式）
            let total = target_idx.saturating_sub(base_idx);
            for (k, p) in manifest.points.iter().enumerate().skip(base_idx + 1).take(total) {
                report(20 + ((k as f64 / total.max(1) as f64) * 75.0) as u32, "应用增量备份…");
                let content = std::fs::read_to_string(dir.join(&p.filename))
                    .map_err(|e| format!("读取增量失败: {e}"))?;
                state
                    .vault
                    .import(&content, "merge")
                    .map_err(|e| e.to_string())?;
            }
            report(100, "回滚完成");
            Ok(())
        })();
        match result {
            Ok(()) => emit_task_done(&job2, &app2, "rollback", true, "回滚完成", "", ""),
            Err(e) => emit_task_done(&job2, &app2, "rollback", false, "回滚失败", &e, ""),
        }
        release_task(&job2);
    });
    Ok("回滚任务已开始".to_string())
}

// ==================== 数据存储目录 ====================

/// 数据目录配置文件名（固定存于系统应用数据目录——启动时必须先读它才能定位 vault）
const DATA_SETTINGS_FILE: &str = "data_settings.json";

/// 读取用户配置的数据存储目录（vault 所在目录）；未配置时回退到默认系统应用数据目录
fn vault_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cfg_path = app_data.join(DATA_SETTINGS_FILE);
    if let Ok(text) = std::fs::read_to_string(&cfg_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(dir) = v.get("data_dir").and_then(|d| d.as_str()) {
                if !dir.trim().is_empty() {
                    return Ok(std::path::PathBuf::from(dir));
                }
            }
        }
    }
    Ok(app_data)
}

/// 获取当前数据存储目录（供设置页展示）
#[tauri::command]
fn get_data_dir(app: tauri::AppHandle) -> Result<String, String> {
    Ok(vault_data_dir(&app)?.display().to_string())
}

/// 用「独立只读连接」把源 vault 在线备份（增量分步）到目标文件，全程不碰主连接锁。
/// base→span 为进度区间（如 5..45 表示快照阶段），report 回调透传进度。
fn backup_vault_to_file(
    src_path: &std::path::Path,
    dst_path: &std::path::Path,
    base: u32,
    span: u32,
    msg: &str,
    report: &dyn Fn(u32, &str),
) -> Result<(), String> {
    let mut dst =
        rusqlite::Connection::open(dst_path).map_err(|e| format!("创建目标文件失败: {e}"))?;
    let src = rusqlite::Connection::open(src_path)
        .map_err(|e| format!("打开源数据文件失败: {e}"))?;
    src.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("设置超时失败: {e}"))?;
    // 把 WAL 未落盘数据合并进主文件，使只读连接读到完整快照
    let _ = src.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let bk = rusqlite::backup::Backup::new(&src, &mut dst)
        .map_err(|e| format!("初始化备份失败: {e}"))?;
    loop {
        let stepped = bk.step(256).map_err(|e| format!("复制数据失败: {e}"))?;
        let prog = bk.progress();
        if prog.pagecount > 0 {
            let done = prog.pagecount - prog.remaining;
            report(
                base + ((done as f64 / prog.pagecount as f64) * span as f64) as u32,
                msg,
            );
        }
        match stepped {
            rusqlite::backup::StepResult::Done => break,
            rusqlite::backup::StepResult::More => continue,
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    drop(bk);
    drop(src);
    drop(dst);
    Ok(())
}

/// 后台线程：迁移 vault 数据到目标目录，分两阶段（遮罩内展示明确进度）：
///   阶段1「快照备份」(0-45)：把当前数据快照到备份目录，作为迁移前的时间点兜底；
///   阶段2「迁移数据」(45-95)：把数据复制到新目录。
/// 两阶段都用独立只读连接 + SQLite 在线备份，全程不占主连接锁（UI 不被卡死）。
/// 迁移期间由前端遮罩限制用户操作，避免写入导致快照与迁移不一致。
/// 完成后写配置并发 done 事件，前端自动重启加载新目录。
fn spawn_data_migration(
    app: tauri::AppHandle,
    job: BgTaskState,
    target_dir: std::path::PathBuf,
    clear_config: bool,
) {
    std::thread::spawn(move || {
        let target_str = target_dir.display().to_string();
        let report = |p: u32, m: &str| {
            set_task_progress(&job, &app, "migrate-data", "run", p, m, &target_str);
        };

        let state: tauri::State<AppState> = app.state();
        let store = state.vault.store();
        let src_path = store.path().clone();
        let dst_path = target_dir.join("securenote.vault");

        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建目录失败: {e}"))?;

            // 阶段1：快照备份到备份目录（迁移前的时间点兜底，独立连接不占主锁）
            report(1, "阶段 1/2：快照备份中…");
            if let Some(bdir) = backup_dir(&app) {
                let _ = std::fs::create_dir_all(&bdir);
                let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let snap = bdir.join(format!("SecureNote_premigrate_{ts}.vault"));
                // 快照失败不阻塞迁移（迁移本身即完整复制，原目录也保留）
                let _ = backup_vault_to_file(&src_path, &snap, 2, 42, "快照备份中…", &report);
            }

            // 阶段2：迁移数据到目标目录
            report(45, "阶段 2/2：迁移数据中…");
            if dst_path.exists() {
                let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                std::fs::rename(
                    &dst_path,
                    target_dir.join(format!("securenote.vault.old-{ts}.bak")),
                )
                .map_err(|e| format!("目标目录已存在数据文件且无法挪开: {e}"))?;
            }
            backup_vault_to_file(&src_path, &dst_path, 45, 50, "迁移数据中…", &report)?;
            Ok(())
        })();

        match result {
            Ok(()) => {
                // 写配置（固定存于系统应用数据目录；中途退出则配置未写、旧目录仍生效）
                let cfg_result: Result<(), String> = if clear_config {
                    if let Ok(app_data) = app.path().app_data_dir() {
                        let _ = std::fs::remove_file(app_data.join(DATA_SETTINGS_FILE));
                    }
                    Ok(())
                } else if let Ok(app_data) = app.path().app_data_dir() {
                    let _ = std::fs::create_dir_all(&app_data);
                    let json = serde_json::json!({ "data_dir": target_str }).to_string();
                    std::fs::write(app_data.join(DATA_SETTINGS_FILE), json)
                        .map_err(|e| format!("写入配置失败: {e}"))
                } else {
                    Err("无法获取应用数据目录".to_string())
                };
                match cfg_result {
                    Ok(()) => emit_task_done(
                        &job,
                        &app,
                        "migrate-data",
                        true,
                        "数据迁移完成，即将重启…",
                        "",
                        &target_str,
                    ),
                    Err(e) => emit_task_done(&job, &app, "migrate-data", false, "迁移失败", &e, &target_str),
                }
            }
            Err(e) => emit_task_done(&job, &app, "migrate-data", false, "迁移失败", &e, &target_str),
        }
        release_task(&job);
    });
}

/// 弹出目录选择框，选择新的数据存储目录；迁移在后台线程执行（不卡 UI），
/// 完成后前端自动调用 restart_app 重启加载新目录。
#[tauri::command]
fn choose_data_dir(
    app: tauri::AppHandle,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    let current = vault_data_dir(&app)?;
    let dir = rfd::FileDialog::new()
        .set_title("选择数据存储目录")
        .set_directory(&current)
        .pick_folder();
    let dir = dir.ok_or("已取消选择")?;
    let dir_str = dir.display().to_string();
    if dir == current {
        return Ok(dir_str); // 目录未变化，无需迁移
    }
    if !try_acquire_task(&job) {
        return Err("已有后台任务进行中，请稍后再试".to_string());
    }
    spawn_data_migration(app, job.inner().clone(), dir, false);
    Ok(dir_str)
}

/// 恢复默认数据目录（系统应用数据目录）：后台迁移，完成后前端自动重启应用
#[tauri::command]
fn reset_data_dir(
    app: tauri::AppHandle,
    job: tauri::State<BgTaskState>,
) -> Result<String, String> {
    let current = vault_data_dir(&app)?;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if current == app_data {
        return Ok(app_data.display().to_string());
    }
    if !try_acquire_task(&job) {
        return Err("已有后台任务进行中，请稍后再试".to_string());
    }
    spawn_data_migration(app, job.inner().clone(), app_data, true);
    Ok("后台迁移已开始".to_string())
}

/// 重启应用（数据迁移完成后由前端调用，加载新目录）
#[tauri::command]
fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    // 重启为进程级重启，不触发退出备份
    app.restart();
}

/// 返回当前应用版本号（单一来源：Cargo.toml，随每次迭代 bump）
#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 保存笔记附件（图片等大文件外置存储），返回附件 id；HTML 中以 snatt://<id> 引用
#[tauri::command]
fn save_attachment(
    data_b64: String,
    mime: String,
    note_id: Option<String>,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data_b64)
        .map_err(|e| format!("附件数据解码失败: {e}"))?;
    state
        .vault
        .save_attachment(&bytes, &mime, note_id.as_deref())
        .map_err(|e| e.to_string())
}

/// 读取附件（按需解密），返回 { id, mime, data }（data 为原始字节的 base64）
#[tauri::command]
fn get_attachment(id: String, state: tauri::State<AppState>) -> Result<serde_json::Value, String> {
    let (mime, bytes) = state.vault.get_attachment(&id).map_err(|e| e.to_string())?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(serde_json::json!({ "id": id, "mime": mime, "data": b64 }))
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

/// 从备份文件重建整个库（用于首次启动「从备份初始化」）：
/// 接受备份内容 + 主密码，验证并还原全部数据。返回还原条目数。
#[tauri::command]
fn restore_from_backup(
    encoded: String,
    master_password: String,
    state: tauri::State<AppState>,
) -> Result<usize, String> {
    state
        .vault
        .restore_from_backup(&encoded, &master_password)
        .map_err(|e| e.to_string())
}

/// 选择备份文件并读取其内容（供「从备份初始化」使用）
#[tauri::command]
fn pick_backup_file() -> Result<(String, String), String> {
    let path = rfd::FileDialog::new()
        .set_title("选择备份文件")
        .add_filter("SecureNote 加密备份", &["snvault", "json", "txt"])
        .pick_file();
    let path = path.ok_or("已取消选择")?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok((name, content))
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
    // 永久删除笔记时同步清理其附件
    if table == "notes" {
        conn.execute("DELETE FROM attachments WHERE note_id=?1", params![id])
            .map_err(|e| e.to_string())?;
    }
    conn.execute(&format!("DELETE FROM {table} WHERE id=?1"), params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 清空回收站，返回清除条数
#[tauri::command]
fn empty_trash(state: tauri::State<AppState>) -> Result<usize, String> {
    let store = state.vault.store();
    let conn = store.conn_mut();
    // 清空回收站时同步清理笔记附件
    conn.execute(
        "DELETE FROM attachments WHERE note_id IN
         (SELECT id FROM notes WHERE deleted_at IS NOT NULL)",
        [],
    )
    .map_err(|e| e.to_string())?;
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
    // 先清理过期笔记的附件
    conn.execute(
        "DELETE FROM attachments WHERE note_id IN
         (SELECT id FROM notes WHERE deleted_at IS NOT NULL
          AND julianday(deleted_at) < julianday('now', '-15 days'))",
        [],
    )
    .map_err(|e| e.to_string())?;
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
            let app_data = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&app_data)?;
            // 数据存储目录：读取用户配置（data_settings.json），默认为系统应用数据目录
            // 配置的目录不可用（如移动硬盘未接入）时回退默认目录，保证应用可启动
            let vault_dir = match vault_data_dir(app.handle()) {
                Ok(d) if std::fs::create_dir_all(&d).is_ok() => d,
                _ => app_data.clone(),
            };
            let vault = Arc::new(Vault::new(vault_dir)?);
            let scheduler = Arc::new(ReminderScheduler::new());
            // 启动提醒调度器
            scheduler.start(app.handle().clone(), vault.clone());
            app.manage(AppState { vault, scheduler });
            // 后台任务状态（导出/数据迁移/备份迁移共用单任务槽）
            app.manage(BgTaskState::default());
            // 闲时自动备份线程
            start_idle_backup_thread(app.handle().clone());

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
                        // 退出：统一走 graceful_exit（备份限时 + 强制退出）
                        "tray-quit" => {
                            graceful_exit(app.clone());
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
            // 点击标题栏关闭按钮 → 静默备份后退出（与托盘"退出"一致）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                graceful_exit(window.app_handle().clone());
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
            start_export_to_file,
            force_backup,
            get_idle_backup,
            set_idle_backup,
            touch_activity,
            list_backup_points,
            delete_backup_point,
            rollback_to_backup,
            get_app_version,
            get_backup_dir,
            choose_backup_dir,
            reset_backup_dir,
            get_data_dir,
            choose_data_dir,
            reset_data_dir,
            restart_app,
            set_content_protection,
            import_vault_from_file,
            import_vault,
            restore_from_backup,
            pick_backup_file,
            open_external_url,
            save_attachment,
            get_attachment,
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

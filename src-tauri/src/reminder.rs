//! 定时提醒调度：后台线程轮询待办提醒时间，触发系统通知
use crate::crypto;
use crate::vault::Vault;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

/// 后台调度器，每 30 秒轮询一次待办提醒
pub struct ReminderScheduler {
    running: Arc<AtomicBool>,
}

impl ReminderScheduler {
    pub fn new() -> Self {
        ReminderScheduler {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动调度器
    pub fn start(&self, app: AppHandle, vault: Arc<Vault>) {
        let running = self.running.clone();
        if running.swap(true, Ordering::SeqCst) {
            return; // 已在运行
        }
        std::thread::spawn(move || {
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                // 未解锁时不轮询（数据无法解密）
                if vault.is_unlocked() {
                    Self::process_due_reminders(&app, &vault);
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn process_due_reminders(app: &AppHandle, vault: &Arc<Vault>) {
        let now = Utc::now();
        let dek = match vault.dek() {
            Ok(d) => d,
            Err(_) => return,
        };
        let store = vault.store();
        let rows = {
            let conn = store.conn();
            let query = conn.prepare(
                "SELECT id, title, remind_at FROM todos
                 WHERE remind_at IS NOT NULL AND remind_at != '' AND completed = 0 AND deleted_at IS NULL",
            );
            match query.and_then(|mut stmt| {
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            }) {
                Ok(rows) => rows,
                Err(_) => return,
            }
        }; // conn 在此释放读锁

        for (id, title_enc, remind_at) in rows {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&remind_at) {
                if dt.with_timezone(&Utc) <= now {
                    // 解密标题（密文存储）
                    let title = crypto::decrypt_str(&title_enc, dek.as_ref())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|_| "待办提醒".to_string());
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app
                        .notification()
                        .builder()
                        .title(title)
                        .body("到达提醒时间，请查看待办")
                        .show();
                    // 触发后清空提醒时间，避免重复提醒
                    let store = vault.store();
                    let conn = store.conn_mut();
                    let _ = conn.execute(
                        "UPDATE todos SET remind_at = NULL WHERE id = ?1",
                        rusqlite::params![id],
                    );
                }
            }
        }
    }
}

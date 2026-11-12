//! Vault 管理：主密钥、认证状态、导入导出与增量同步
use crate::crypto::*;
use crate::models::*;
use crate::store::{Store, StoreError};
use chrono::Utc;
use parking_lot::RwLock;
use rusqlite::params;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("存储库已初始化，请使用主密码解锁")]
    AlreadyInitialized,
    #[error("存储库未初始化")]
    NotInitialized,
    #[error("存储库未解锁")]
    Locked,
    #[error("主密码错误")]
    WrongPassword,
    #[error("序列化失败: {0}")]
    Serialize(String),
    #[error("文件操作失败: {0}")]
    Io(String),
}

/// 主密钥（KEK），仅存于内存
struct MasterKey(Zeroizing<[u8; 32]>);

impl MasterKey {
    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub struct Vault {
    store: Arc<Store>,
    master_key: RwLock<Option<MasterKey>>,
    /// 数据加密密钥（DEK），用 KEK 加密后存于 meta
    dek: RwLock<Option<Zeroizing<[u8; 32]>>>,
}

pub struct VaultState {
    pub initialized: bool,
    pub unlocked: bool,
}

impl Vault {
    pub fn new(data_dir: PathBuf) -> Result<Self, VaultError> {
        let vault_path = data_dir.join("securenote.vault");
        let store = if vault_path.exists() {
            Store::open(vault_path.clone())?
        } else {
            Store::create(vault_path.clone())?
        };
        Ok(Vault {
            store: Arc::new(store),
            master_key: RwLock::new(None),
            dek: RwLock::new(None),
        })
    }

    pub fn store(&self) -> Arc<Store> {
        self.store.clone()
    }

    pub fn is_initialized(&self) -> bool {
        self.store
            .get_meta("initialized")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    pub fn is_unlocked(&self) -> bool {
        self.master_key.read().is_some()
    }

    /// 首次设置主密码
    /// 若存储库已初始化（如重复提交），验证密码匹配则视为设置成功
    pub fn setup(&self, master_password: &str) -> Result<(), VaultError> {
        if self.is_initialized() {
            // 已初始化：校验传入密码与现有密钥是否匹配
            // 匹配 → 视为成功（处理双击重复提交等场景），并保持解锁状态
            // 不匹配 → 明确提示用主密码解锁
            if self.unlock(master_password)? {
                return Ok(());
            }
            return Err(VaultError::AlreadyInitialized);
        }
        // 1. 生成盐，派生 KEK
        let salt = generate_salt(16);
        let kek = derive_key(master_password, &salt)?;
        // 2. 生成 DEK
        let dek = generate_dek();
        // 3. 用 KEK 加密 DEK 并存储
        let enc_dek = encrypt(&dek, &kek)?;
        let enc_salt = base64_encode(&salt);
        // 4. 生成校验值：用 KEK 加密固定字符串，用于后续验证密码
        let check = encrypt_str("securenote-key-check", &kek)?;

        let store = self.store();
        store.set_meta("initialized", "true")?;
        store.set_meta("salt", &enc_salt)?;
        store.set_meta("enc_dek", &enc_dek)?;
        store.set_meta("key_check", &check)?;
        store.set_meta("created_at", &Utc::now().to_rfc3339())?;

        // 5. 设置内存中的密钥
        let mut kek_arr = [0u8; 32];
        kek_arr.copy_from_slice(&kek);
        *self.master_key.write() = Some(MasterKey(Zeroizing::new(kek_arr)));
        let mut dek_arr = [0u8; 32];
        dek_arr.copy_from_slice(&dek);
        *self.dek.write() = Some(Zeroizing::new(dek_arr));
        Ok(())
    }

    /// 解锁：验证主密码，解密 DEK
    pub fn unlock(&self, master_password: &str) -> Result<bool, VaultError> {
        if !self.is_initialized() {
            return Err(VaultError::NotInitialized);
        }
        let store = self.store();
        let salt = base64_decode(
            &store
                .get_meta("salt")?
                .ok_or(VaultError::NotInitialized)?,
        )
        .map_err(|e| VaultError::Serialize(e.to_string()))?;
        let kek = derive_key(master_password, &salt)?;
        // 验证密码
        let check = store
            .get_meta("key_check")?
            .ok_or(VaultError::NotInitialized)?;
        let decrypted = decrypt_str(&check, &kek);
        if decrypted.is_err() || decrypted.unwrap().as_str() != "securenote-key-check" {
            return Ok(false);
        }
        // 解密 DEK
        let enc_dek = store
            .get_meta("enc_dek")?
            .ok_or(VaultError::NotInitialized)?;
        let dek_bytes = decrypt(&enc_dek, &kek)?;
        let mut dek = [0u8; 32];
        dek.copy_from_slice(&dek_bytes);

        let mut kek_arr = [0u8; 32];
        kek_arr.copy_from_slice(&kek);
        *self.master_key.write() = Some(MasterKey(Zeroizing::new(kek_arr)));
        *self.dek.write() = Some(Zeroizing::new(dek));
        // 解锁成功后自动清理：物理删除 15 天前进入回收站的条目
        let _ = self.purge_expired();
        Ok(true)
    }

    /// 物理删除 15 天前进入回收站的条目（julianday 可解析 RFC3339 时间串）
    pub fn purge_expired(&self) -> Result<usize, VaultError> {
        let store = self.store();
        let conn = store.conn_mut();
        let mut total = 0;
        for table in ["notes", "todos", "passwords"] {
            total += conn.execute(
                &format!(
                    "DELETE FROM {table} WHERE deleted_at IS NOT NULL
                     AND julianday(deleted_at) < julianday('now', '-15 days')"
                ),
                [],
            )?;
        }
        Ok(total)
    }

    /// 锁定：清除内存中的密钥
    pub fn lock(&self) {
        *self.master_key.write() = None;
        *self.dek.write() = None;
    }

    /// 获取 DEK（内部使用，需已解锁）
    pub fn dek(&self) -> Result<Zeroizing<[u8; 32]>, VaultError> {
        let guard = self.dek.read();
        guard.clone().ok_or(VaultError::Locked)
    }

    /// 修改主密码
    pub fn change_master_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), VaultError> {
        if !self.unlock(old_password)? {
            return Err(VaultError::WrongPassword);
        }
        // 重新派生、重新加密 DEK
        let salt = generate_salt(16);
        let kek = derive_key(new_password, &salt)?;
        let dek = self.dek()?;
        let enc_dek = encrypt(dek.as_ref(), &kek)?;
        let enc_salt = base64_encode(&salt);
        let check = encrypt_str("securenote-key-check", &kek)?;

        let store = self.store();
        store.set_meta("salt", &enc_salt)?;
        store.set_meta("enc_dek", &enc_dek)?;
        store.set_meta("key_check", &check)?;
        Ok(())
    }

    /// 重置（清空所有数据）
    pub fn reset(&self) -> Result<(), VaultError> {
        let store = self.store();
        let conn = store.conn_mut();
        conn.execute_batch(
            "DELETE FROM notes; DELETE FROM todos; DELETE FROM passwords; DELETE FROM meta;",
        )?;
        drop(conn);
        *self.master_key.write() = None;
        *self.dek.write() = None;
        Ok(())
    }

    /// 全量导出：返回加密后的 base64 字符串
    pub fn export(&self) -> Result<String, VaultError> {
        let dek = self.dek()?;
        let payload = self.collect_payload()?;
        let json_str = serde_json::to_string(&payload)
            .map_err(|e| VaultError::Serialize(e.to_string()))?;
        let enc = encrypt(json_str.as_bytes(), dek.as_ref())?;
        Ok(enc)
    }

    /// 收集所有数据为 payload
    fn collect_payload(&self) -> Result<SyncPayload, VaultError> {
        let dek = self.dek()?;
        let store = self.store();
        let conn = store.conn();

        let notes = Self::load_notes(&conn, dek.as_ref())?;
        let todos = Self::load_todos(&conn, dek.as_ref())?;
        let passwords = Self::load_passwords(&conn, dek.as_ref())?;

        Ok(SyncPayload {
            format: "securenote-vault".to_string(),
            version: 1,
            exported_at: Utc::now().to_rfc3339(),
            payload: VaultData {
                notes,
                todos,
                passwords,
                meta: json!({}),
            },
        })
    }

    /// 导入：从加密包还原
    /// mode: "merge" 合并（增量），"replace" 覆盖
    pub fn import(&self, encoded: &str, mode: &str) -> Result<usize, VaultError> {
        let dek = self.dek()?;
        let plaintext = decrypt(encoded, dek.as_ref())?;
        let payload: SyncPayload = serde_json::from_slice(&plaintext)
            .map_err(|e| VaultError::Serialize(e.to_string()))?;

        let store = self.store();
        let conn = store.conn_mut();

        if mode == "replace" {
            conn.execute("DELETE FROM notes", [])?;
            conn.execute("DELETE FROM todos", [])?;
            conn.execute("DELETE FROM passwords", [])?;
        }

        let mut count = 0;
        for note in &payload.payload.notes {
            count += Self::upsert_note(&conn, dek.as_ref(), note, mode)?;
        }
        for todo in &payload.payload.todos {
            count += Self::upsert_todo(&conn, dek.as_ref(), todo, mode)?;
        }
        for pwd in &payload.payload.passwords {
            count += Self::upsert_password(&conn, dek.as_ref(), pwd, mode)?;
        }
        Ok(count)
    }

    // ---- 辅助加载与 upsert ----

    fn load_notes(conn: &rusqlite::Connection, dek: &[u8]) -> Result<Vec<Note>, VaultError> {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, notebook, favorite, created_at, updated_at, deleted_at
             FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let content: String = r.get(2)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                content,
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
                title: decrypt_str(&title, dek)?.to_string(),
                content: decrypt_str(&content, dek)?.to_string(),
                notebook: decrypt_str(&notebook, dek)?.to_string(),
                favorite: favorite != 0,
                created_at,
                updated_at,
                deleted_at,
            });
        }
        Ok(result)
    }

    fn load_todos(conn: &rusqlite::Connection, dek: &[u8]) -> Result<Vec<Todo>, VaultError> {
        let mut stmt = conn.prepare(
            "SELECT id, title, note, list, priority, due_at, remind_at, repeat, completed,
                    completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at
             FROM todos WHERE deleted_at IS NULL ORDER BY created_at DESC",
        )?;
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
                id, title, note, list, priority, due_at, remind_at, repeat, completed,
                completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at,
            ) = row?;
            result.push(Todo {
                id,
                title: decrypt_str(&title, dek)?.to_string(),
                note: decrypt_str(&note, dek)?.to_string(),
                list: decrypt_str(&list, dek)?.to_string(),
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

    fn load_passwords(
        conn: &rusqlite::Connection,
        dek: &[u8],
    ) -> Result<Vec<PasswordEntry>, VaultError> {
        let mut stmt = conn.prepare(
            "SELECT id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at
             FROM passwords WHERE deleted_at IS NULL ORDER BY created_at DESC",
        )?;
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
            // tags：加密存储的 JSON 字符串数组，空串视为空列表
            let tags_list: Vec<String> = if tags.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&decrypt_str(&tags, dek)?.to_string()).unwrap_or_default()
            };
            result.push(PasswordEntry {
                id,
                name: decrypt_str(&name, dek)?.to_string(),
                username: decrypt_str(&username, dek)?.to_string(),
                password: decrypt_str(&password, dek)?.to_string(),
                url: decrypt_str(&url, dek)?.to_string(),
                note: decrypt_str(&note, dek)?.to_string(),
                category: decrypt_str(&category, dek)?.to_string(),
                favorite: favorite != 0,
                tags: tags_list,
                created_at,
                updated_at,
                deleted_at,
            });
        }
        Ok(result)
    }

    fn upsert_note(
        conn: &rusqlite::Connection,
        dek: &[u8],
        note: &Note,
        mode: &str,
    ) -> Result<usize, VaultError> {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE id = ?1",
            params![note.id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            // 冲突策略：最后修改时间优先
            let local_updated: String = conn.query_row(
                "SELECT updated_at FROM notes WHERE id = ?1",
                params![note.id],
                |r| r.get(0),
            )?;
            if note.updated_at <= local_updated {
                return Ok(0);
            }
            conn.execute(
                "UPDATE notes SET title=?2, content=?3, notebook=?4, favorite=?5, updated_at=?6, deleted_at=NULL WHERE id=?1",
                params![
                    note.id,
                    encrypt_str(&note.title, dek)?,
                    encrypt_str(&note.content, dek)?,
                    encrypt_str(&note.notebook, dek)?,
                    note.favorite as i64,
                    note.updated_at,
                ],
            )?;
        } else {
            conn.execute(
                "INSERT INTO notes(id, title, content, notebook, favorite, created_at, updated_at, deleted_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
                params![
                    note.id,
                    encrypt_str(&note.title, dek)?,
                    encrypt_str(&note.content, dek)?,
                    encrypt_str(&note.notebook, dek)?,
                    note.favorite as i64,
                    note.created_at,
                    note.updated_at,
                ],
            )?;
        }
        Ok(1)
    }

    fn upsert_todo(
        conn: &rusqlite::Connection,
        dek: &[u8],
        todo: &Todo,
        mode: &str,
    ) -> Result<usize, VaultError> {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE id = ?1",
            params![todo.id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            let local_updated: String = conn.query_row(
                "SELECT updated_at FROM todos WHERE id = ?1",
                params![todo.id],
                |r| r.get(0),
            )?;
            if todo.updated_at <= local_updated {
                return Ok(0);
            }
            conn.execute(
                "UPDATE todos SET title=?2, note=?3, list=?4, priority=?5, due_at=?6, remind_at=?7,
                        repeat=?8, completed=?9, completed_at=?10, sort_order=?11, pinned=?12, favorite=?13,
                        updated_at=?14, deleted_at=NULL WHERE id=?1",
                params![
                    todo.id,
                    encrypt_str(&todo.title, dek)?,
                    encrypt_str(&todo.note, dek)?,
                    encrypt_str(&todo.list, dek)?,
                    todo.priority,
                    todo.due_at,
                    todo.remind_at,
                    todo.repeat,
                    todo.completed as i64,
                    todo.completed_at,
                    todo.sort_order,
                    todo.pinned as i64,
                    todo.favorite as i64,
                    todo.updated_at,
                ],
            )?;
        } else {
            conn.execute(
                "INSERT INTO todos(id, title, note, list, priority, due_at, remind_at, repeat, completed, completed_at, sort_order, pinned, favorite, created_at, updated_at, deleted_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL)",
                params![
                    todo.id,
                    encrypt_str(&todo.title, dek)?,
                    encrypt_str(&todo.note, dek)?,
                    encrypt_str(&todo.list, dek)?,
                    todo.priority,
                    todo.due_at,
                    todo.remind_at,
                    todo.repeat,
                    todo.completed as i64,
                    todo.completed_at,
                    todo.sort_order,
                    todo.pinned as i64,
                    todo.favorite as i64,
                    todo.created_at,
                    todo.updated_at,
                ],
            )?;
        }
        Ok(1)
    }

    fn upsert_password(
        conn: &rusqlite::Connection,
        dek: &[u8],
        pwd: &PasswordEntry,
        mode: &str,
    ) -> Result<usize, VaultError> {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM passwords WHERE id = ?1",
            params![pwd.id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            let local_updated: String = conn.query_row(
                "SELECT updated_at FROM passwords WHERE id = ?1",
                params![pwd.id],
                |r| r.get(0),
            )?;
            if pwd.updated_at <= local_updated {
                return Ok(0);
            }
            conn.execute(
                "UPDATE passwords SET name=?2, username=?3, password=?4, url=?5, note=?6, category=?7, favorite=?8, tags=?9, updated_at=?10, deleted_at=NULL WHERE id=?1",
                params![
                    pwd.id,
                    encrypt_str(&pwd.name, dek)?,
                    encrypt_str(&pwd.username, dek)?,
                    encrypt_str(&pwd.password, dek)?,
                    encrypt_str(&pwd.url, dek)?,
                    encrypt_str(&pwd.note, dek)?,
                    encrypt_str(&pwd.category, dek)?,
                    pwd.favorite as i64,
                    encrypt_str(&serde_json::to_string(&pwd.tags).unwrap_or_default(), dek)?,
                    pwd.updated_at,
                ],
            )?;
        } else {
            conn.execute(
                "INSERT INTO passwords(id, name, username, password, url, note, category, favorite, tags, created_at, updated_at, deleted_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL)",
                params![
                    pwd.id,
                    encrypt_str(&pwd.name, dek)?,
                    encrypt_str(&pwd.username, dek)?,
                    encrypt_str(&pwd.password, dek)?,
                    encrypt_str(&pwd.url, dek)?,
                    encrypt_str(&pwd.note, dek)?,
                    encrypt_str(&pwd.category, dek)?,
                    pwd.favorite as i64,
                    encrypt_str(&serde_json::to_string(&pwd.tags).unwrap_or_default(), dek)?,
                    pwd.created_at,
                    pwd.updated_at,
                ],
            )?;
        }
        Ok(1)
    }
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(data)
}

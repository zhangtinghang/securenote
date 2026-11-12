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

    /// 全量导出：返回自包含的加密备份字符串。
    /// 格式 = 明文密钥头(JSON) + "\n" + DEK 加密的 payload(base64)
    /// 明文头仅含 salt/enc_dek/key_check（都是加密后的密钥材料，不含明文数据），
    /// 安全性仍由主密码保证；这样备份文件可跨设备「从备份初始化」还原。
    pub fn export(&self) -> Result<String, VaultError> {
        let dek = self.dek()?;
        let store = self.store();
        let salt = store.get_meta("salt")?.ok_or(VaultError::NotInitialized)?;
        let enc_dek = store.get_meta("enc_dek")?.ok_or(VaultError::NotInitialized)?;
        let key_check = store.get_meta("key_check")?.ok_or(VaultError::NotInitialized)?;

        let payload = self.collect_payload()?;
        let json_str = serde_json::to_string(&payload)
            .map_err(|e| VaultError::Serialize(e.to_string()))?;
        let enc = encrypt(json_str.as_bytes(), dek.as_ref())?;

        // 明文头：密钥材料（供跨设备还原）
        let head = serde_json::json!({
            "format": "securenote-vault",
            "version": 2,
            "salt": salt,
            "enc_dek": enc_dek,
            "key_check": key_check,
        });
        Ok(format!("{}\n{}", head.to_string(), enc))
    }

    /// 收集所有数据为 payload
    fn collect_payload(&self) -> Result<SyncPayload, VaultError> {
        let dek = self.dek()?;
        let store = self.store();
        let conn = store.conn();

        let notes = Self::load_notes(&conn, dek.as_ref())?;
        let todos = Self::load_todos(&conn, dek.as_ref())?;
        let passwords = Self::load_passwords(&conn, dek.as_ref())?;

        // 读取密钥材料，让备份文件自包含、可跨设备还原
        let salt = store.get_meta("salt")?;
        let enc_dek = store.get_meta("enc_dek")?;
        let key_check = store.get_meta("key_check")?;

        Ok(SyncPayload {
            format: "securenote-vault".to_string(),
            version: 2,
            exported_at: Utc::now().to_rfc3339(),
            payload: VaultData {
                notes,
                todos,
                passwords,
                meta: json!({}),
                salt,
                enc_dek,
                key_check,
            },
        })
    }

    /// 导入：从加密包还原
    /// mode: "merge" 合并（增量），"replace" 覆盖
    pub fn import(&self, encoded: &str, mode: &str) -> Result<usize, VaultError> {
        let dek = self.dek()?;
        // 兼容新版自包含格式（"明文头\n密文"）与旧版纯密文
        let body = Self::strip_backup_header(encoded);
        let plaintext = decrypt(body, dek.as_ref())?;
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

    /// 剥离备份文件的明文头（若有），返回纯密文部分
    fn strip_backup_header(encoded: &str) -> &str {
        // 新版格式："{...}\n<base64密文>"，密文是 base64 不换行
        if let Some((head, body)) = encoded.split_once('\n') {
            if head.trim_start().starts_with('{') {
                return body;
            }
        }
        encoded
    }

    /// 从备份文件重建整个库（用于换电脑/重装后「从备份初始化」）：
    /// 用主密码 + 备份内的 salt 派生 KEK，解开 enc_dek 得到 DEK，
    /// 再用 DEK 解出数据写入当前（空）库，并设置 initialized 与密钥 meta。
    /// 返回还原的条目总数。
    pub fn restore_from_backup(
        &self,
        encoded: &str,
        master_password: &str,
    ) -> Result<usize, VaultError> {
        // 解析自包含备份头（salt/enc_dek/key_check）
        let (salt_b64, enc_dek_b64, key_check_b64) = Self::parse_backup_header(encoded)?;
        let salt = base64_decode(&salt_b64).map_err(|e| VaultError::Serialize(e.to_string()))?;

        // 主密码 → KEK
        let kek = derive_key(master_password, &salt)?;

        // 验证主密码（解 key_check）
        let check = decrypt_str(&key_check_b64, &kek)?;
        if check.as_str() != "securenote-key-check" {
            return Err(VaultError::WrongPassword);
        }

        // 解 DEK
        let dek_bytes = decrypt(&enc_dek_b64, &kek)?;
        let mut dek = [0u8; 32];
        dek.copy_from_slice(&dek_bytes);

        // 解出数据 payload
        let body = Self::strip_backup_header(encoded);
        let plaintext = decrypt(body, &dek)?;
        let payload: SyncPayload = serde_json::from_slice(&plaintext)
            .map_err(|e| VaultError::Serialize(e.to_string()))?;

        let store = self.store();
        // 清空现有数据（全新重建）
        {
            let conn = store.conn_mut();
            conn.execute("DELETE FROM notes", [])?;
            conn.execute("DELETE FROM todos", [])?;
            conn.execute("DELETE FROM passwords", [])?;
        }

        // 写入数据（用 DEK 加密存库）
        let mut count = 0;
        {
            let conn = store.conn_mut();
            for note in &payload.payload.notes {
                count += Self::upsert_note(&conn, &dek, note, "replace")?;
            }
            for todo in &payload.payload.todos {
                count += Self::upsert_todo(&conn, &dek, todo, "replace")?;
            }
            for pwd in &payload.payload.passwords {
                count += Self::upsert_password(&conn, &dek, pwd, "replace")?;
            }
        }

        // 写入密钥 meta，使新库可用相同主密码解锁
        store.set_meta("initialized", "true")?;
        store.set_meta("salt", &salt_b64)?;
        store.set_meta("enc_dek", &enc_dek_b64)?;
        store.set_meta("key_check", &key_check_b64)?;
        store.set_meta("created_at", &Utc::now().to_rfc3339())?;

        // 设置内存密钥（还原后即处于解锁状态）
        let mut kek_arr = [0u8; 32];
        kek_arr.copy_from_slice(&kek);
        *self.master_key.write() = Some(MasterKey(Zeroizing::new(kek_arr)));
        let mut dek_arr = [0u8; 32];
        dek_arr.copy_from_slice(&dek);
        *self.dek.write() = Some(Zeroizing::new(dek_arr));

        Ok(count)
    }

    /// 解析自包含备份的明文头，返回 (salt, enc_dek, key_check)
    fn parse_backup_header(encoded: &str) -> Result<(String, String, String), VaultError> {
        let head = encoded.split_once('\n').map(|(h, _)| h).unwrap_or(encoded);
        let meta: serde_json::Value = serde_json::from_str(head.trim())
            .map_err(|e| VaultError::Serialize(format!("备份格式无法识别: {e}")))?;
        let salt = meta
            .get("salt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| VaultError::Serialize("备份缺少 salt（可能是旧版格式）".to_string()))?
            .to_string();
        let enc_dek = meta
            .get("enc_dek")
            .and_then(|v| v.as_str())
            .ok_or_else(|| VaultError::Serialize("备份缺少 enc_dek".to_string()))?
            .to_string();
        let key_check = meta
            .get("key_check")
            .and_then(|v| v.as_str())
            .ok_or_else(|| VaultError::Serialize("备份缺少 key_check".to_string()))?
            .to_string();
        Ok((salt, enc_dek, key_check))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// 创建临时目录下的独立 Vault 实例（隔离测试数据）
    fn temp_vault() -> Vault {
        let dir = std::env::temp_dir().join(format!("securenote-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Vault::new(dir).unwrap()
    }

    #[test]
    fn test_export_restore_roundtrip_cross_device() {
        // 源库：设置主密码，写入一条笔记
        let src = temp_vault();
        src.setup("my-master-password-123").unwrap();

        // 写入一条笔记
        {
            let store = src.store();
            let dek = src.dek().unwrap();
            let conn = store.conn_mut();
            let note = Note {
                id: "note-1".to_string(),
                title: "测试笔记".to_string(),
                content: "这是正文内容".to_string(),
                notebook: "默认".to_string(),
                favorite: false,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                deleted_at: None,
            };
            conn.execute(
                "INSERT INTO notes(id,title,content,notebook,favorite,created_at,updated_at,deleted_at) VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
                params![
                    note.id,
                    encrypt_str(&note.title, dek.as_ref()).unwrap(),
                    encrypt_str(&note.content, dek.as_ref()).unwrap(),
                    encrypt_str(&note.notebook, dek.as_ref()).unwrap(),
                    0i64,
                    note.created_at,
                    note.updated_at,
                ],
            ).unwrap();
        }

        // 导出备份（自包含格式）
        let backup = src.export().unwrap();
        assert!(backup.contains('\n'), "备份应为「明文头\\n密文」格式");
        assert!(backup.starts_with('{'), "备份应以明文 JSON 头开头");

        // 模拟换电脑：新建一个空库（不同 salt），用「从备份初始化」还原
        let dst = temp_vault();
        // 目标库此时未初始化
        assert!(!dst.is_initialized());

        let count = dst
            .restore_from_backup(&backup, "my-master-password-123")
            .unwrap();
        assert_eq!(count, 1, "应还原 1 条笔记");
        assert!(dst.is_initialized(), "还原后应已初始化");
        assert!(dst.is_unlocked(), "还原后应处于解锁状态");

        // 验证数据还原正确
        let store = dst.store();
        let dek = dst.dek().unwrap();
        let notes = Vault::load_notes(&store.conn(), dek.as_ref()).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "测试笔记");
        assert_eq!(notes[0].content, "这是正文内容");
    }

    #[test]
    fn test_restore_wrong_password_fails() {
        let src = temp_vault();
        src.setup("correct-password-123").unwrap();
        let backup = src.export().unwrap();

        let dst = temp_vault();
        let result = dst.restore_from_backup(&backup, "wrong-password-456");
        assert!(result.is_err(), "错误主密码应还原失败");
        assert!(!dst.is_initialized(), "失败后不应初始化");
    }

    #[test]
    fn test_export_import_same_vault_merge() {
        let v = temp_vault();
        v.setup("pwd-12345678").unwrap();
        // 写入一条
        {
            let store = v.store();
            let dek = v.dek().unwrap();
            let conn = store.conn_mut();
            conn.execute(
                "INSERT INTO notes(id,title,content,notebook,favorite,created_at,updated_at,deleted_at) VALUES('n1','标题','内容','默认',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',NULL)",
                params![
                    encrypt_str("标题", dek.as_ref()).unwrap(),
                    encrypt_str("内容", dek.as_ref()).unwrap(),
                    encrypt_str("默认", dek.as_ref()).unwrap(),
                ],
            ).unwrap();
        }
        let backup = v.export().unwrap();
        // 同库导入（merge）应成功且不重复
        let count = v.import(&backup, "merge").unwrap();
        assert_eq!(count, 1);
    }
}

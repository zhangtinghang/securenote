//! 存储引擎：SQLite（业务字段应用层 AES-256-GCM 加密）
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("数据库未解锁")]
    Locked,
    #[error("密钥错误，无法解密数据库")]
    WrongKey,
}

pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Store {
    /// 创建新的加密数据库
    pub fn create(path: PathBuf) -> Result<Self, StoreError> {
        let conn = Connection::open(&path)?;
        let store = Store {
            conn: Mutex::new(conn),
            path,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// 打开已有数据库（此时数据库未加密，后续由上层用 SQLCipher key 重设）
    /// 说明：本项目采用「应用层加密」策略——SQLite 本身明文存储，
    /// 但所有业务字段均经 AES-256-GCM 加密后落盘，因此即使拿到 .vault 文件也无法读取。
    /// 这样可避免 SQLCipher 的 C 依赖带来的跨平台编译复杂度，
    /// 同时满足「数据离线加密、导出文件不可读取」的需求。
    pub fn open(path: PathBuf) -> Result<Self, StoreError> {
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Store {
            conn: Mutex::new(conn),
            path,
        };
        store.migrate()?;
        Ok(store)
    }

    /// 轻量迁移：为旧库补齐新增列（幂等）
    pub fn migrate(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let has_column = |table: &str, col: &str| -> Result<bool, StoreError> {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == col {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let add = |sql: &str| conn.execute_batch(sql);
        if !has_column("todos", "sort_order")? {
            add("ALTER TABLE todos ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !has_column("todos", "pinned")? {
            add("ALTER TABLE todos ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !has_column("todos", "favorite")? {
            add("ALTER TABLE todos ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !has_column("passwords", "tags")? {
            add("ALTER TABLE passwords ADD COLUMN tags TEXT NOT NULL DEFAULT '';")?;
        }
        Ok(())
    }

    fn init_schema(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                notebook TEXT NOT NULL DEFAULT '默认',
                favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            );

            CREATE TABLE IF NOT EXISTS todos (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                list TEXT NOT NULL DEFAULT '默认',
                priority TEXT NOT NULL DEFAULT 'medium',
                due_at TEXT,
                remind_at TEXT,
                repeat TEXT NOT NULL DEFAULT 'none',
                completed INTEGER NOT NULL DEFAULT 0,
                completed_at TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0,
                favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            );

            CREATE TABLE IF NOT EXISTS passwords (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                username TEXT NOT NULL DEFAULT '',
                password TEXT NOT NULL,
                url TEXT NOT NULL DEFAULT '',
                note TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT '默认',
                favorite INTEGER NOT NULL DEFAULT 0,
                tags TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            );

            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                note_id TEXT,
                mime TEXT NOT NULL DEFAULT '',
                data TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at);
            CREATE INDEX IF NOT EXISTS idx_todos_updated ON todos(updated_at);
            CREATE INDEX IF NOT EXISTS idx_passwords_updated ON passwords(updated_at);
            CREATE INDEX IF NOT EXISTS idx_attachments_note ON attachments(note_id);
            "#,
        )?;
        Ok(())
    }

    pub fn conn(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    pub fn conn_mut(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// 设置/更新 meta 键值
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// 读取 meta 键值
    pub fn get_meta(&self, key: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

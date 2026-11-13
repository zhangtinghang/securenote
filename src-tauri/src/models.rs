//! 数据模型定义
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub notebook: String,
    pub favorite: bool,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub note: String,
    pub list: String,
    pub priority: String,
    pub due_at: Option<String>,
    pub remind_at: Option<String>,
    #[serde(rename = "repeat")]
    pub repeat: String,
    pub completed: bool,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub id: String,
    pub name: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub note: String,
    pub category: String,
    pub favorite: bool,
    /// 标签列表（JSON 数组，整串加密存储）
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// 新建待办的输入（前端传入，缺省字段由后端补全）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoInput {
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_list")]
    pub list: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    pub due_at: Option<String>,
    pub remind_at: Option<String>,
    #[serde(default = "default_repeat")]
    pub repeat: String,
}

/// 新建密码条目的输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordInput {
    pub name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_list() -> String {
    "默认".to_string()
}
fn default_priority() -> String {
    "medium".to_string()
}
fn default_repeat() -> String {
    "none".to_string()
}
fn default_category() -> String {
    "默认".to_string()
}

/// 全量导出 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub payload: VaultData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultData {
    pub notes: Vec<Note>,
    pub todos: Vec<Todo>,
    pub passwords: Vec<PasswordEntry>,
    pub meta: serde_json::Value,
    /// 笔记附件（图片等，base64 原始字节；payload 整体仍由 DEK 加密）。
    /// HTML 内容中以 `snatt://<id>` 引用，实现大图外置存储（v1.3.1 起）。
    #[serde(default)]
    pub attachments: Vec<AttachmentItem>,
    /// 密钥材料（用于「从备份初始化」跨设备还原）：
    /// - salt：主密码派生 KEK 用的盐（base64）
    /// - enc_dek：用 KEK 加密后的数据加密密钥 DEK（base64）
    /// - key_check：用 KEK 加密的校验串，用于验证主密码正确性（base64）
    /// 这三个字段为新版导出才包含；旧版备份缺省，无法跨设备还原（仅能同库还原）
    #[serde(default)]
    pub salt: Option<String>,
    #[serde(default)]
    pub enc_dek: Option<String>,
    #[serde(default)]
    pub key_check: Option<String>,
}

/// 单个附件（备份 payload 用）：data 为原始字节的 base64（明文，整体 payload 加密）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentItem {
    pub id: String,
    #[serde(default)]
    pub note_id: Option<String>,
    #[serde(default)]
    pub mime: String,
    pub data: String,
}

/// 备份点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupKind {
    /// 全量备份（完整快照，可独立还原）
    Full,
    /// 增量备份（基于前一个备份点的差异）
    Incremental,
}

/// 备份点元信息（manifest.json 中的一条记录）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPoint {
    /// 备份点唯一 id（文件名去扩展名）
    pub id: String,
    /// 全量 or 增量
    pub kind: BackupKind,
    /// 创建时间（RFC3339）
    pub created_at: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 文件名（含扩展名）
    pub filename: String,
    /// 该增量基于的备份点 id（全量为 None）
    #[serde(default)]
    pub parent_id: Option<String>,
    /// 本次备份包含的变更条目数（增量有意义，全量为总条目数）
    #[serde(default)]
    pub change_count: usize,
}

/// 备份清单（backup 目录下 manifest.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    /// 按时间升序排列的备份点
    pub points: Vec<BackupPoint>,
    /// 全量备份保留上限（默认 3）
    #[serde(default = "default_max_full")]
    pub max_full: usize,
    /// 增量备份数量达到此阈值后建议全量备份（默认 20）
    #[serde(default = "default_inc_threshold")]
    pub inc_threshold: usize,
}

// 注意：不能用 derive(Default)——usize 默认是 0，会导致首次备份时
// enforce_full_retention 把刚创建的全量备份当作"超额"立刻删掉。
// serde(default=...) 只影响反序列化缺字段的情况，不影响 Default::default()。
impl Default for BackupManifest {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            max_full: default_max_full(),
            inc_threshold: default_inc_threshold(),
        }
    }
}

fn default_max_full() -> usize {
    3
}
fn default_inc_threshold() -> usize {
    20
}

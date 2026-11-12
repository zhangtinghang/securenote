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
}

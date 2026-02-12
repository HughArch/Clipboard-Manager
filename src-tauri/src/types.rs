use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use sqlx::SqlitePool;

// 常量定义
pub const SETTINGS_FILE: &str = "clipboard_settings.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub max_history_items: usize,
    pub max_history_time: u64,
    pub hotkey: String,
    pub auto_start: bool,
    #[serde(default = "default_lan_queue_role")]
    pub lan_queue_role: String,
    #[serde(default)]
    pub lan_queue_host: String,
    #[serde(default = "default_lan_queue_port")]
    pub lan_queue_port: u16,
    #[serde(default)]
    pub lan_queue_password: String,
    #[serde(default = "default_lan_queue_name")]
    pub lan_queue_name: String,
    #[serde(default)]
    pub lan_queue_member_name: String,
}

fn default_lan_queue_role() -> String {
    "off".to_string()
}

fn default_lan_queue_port() -> u16 {
    21991
}

fn default_lan_queue_name() -> String {
    "LAN Queue".to_string()
}

#[derive(Debug, Serialize, Clone)]
pub struct SourceAppInfo {
    pub name: String,
    pub icon: Option<String>, // base64 encoded icon
    pub bundle_id: Option<String>, // macOS bundle identifier
}

// 数据库连接池状态管理
pub struct DatabaseState {
    pub pool: SqlitePool,
}

// 剪贴板监听器控制
pub struct ClipboardWatcherState {
    pub should_stop: Arc<AtomicBool>,
} 

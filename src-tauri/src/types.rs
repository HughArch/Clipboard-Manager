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
}

#[derive(Debug, Serialize, Clone)]
pub struct SourceAppInfo {
    pub name: String,
    pub icon: Option<String>, // base64 encoded icon
}

// 数据库连接池状态管理
pub struct DatabaseState {
    pub pool: SqlitePool,
}

// 剪贴板监听器控制
pub struct ClipboardWatcherState {
    pub should_stop: Arc<AtomicBool>,
} 
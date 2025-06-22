use tauri::{AppHandle, Manager};
use crate::types::{AppSettings, DatabaseState};
use serde::{Serialize};
use std::fs;
use std::path::PathBuf;
use dirs_next::config_dir;
use std::time::Duration;
use base64::{engine::general_purpose, Engine as _};
use tauri_plugin_global_shortcut::{self, GlobalShortcutExt, Shortcut};
// 使用第三方剪贴板插件，解决arboard内存泄漏问题
use std::env;
use chrono;
use tokio;
use tokio::sync::Mutex;

use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use sqlx::{self, Row};
use enigo::{Enigo, Key, Keyboard, Settings};


const SETTINGS_FILE: &str = "clipboard_settings.json";

fn settings_file_path() -> Result<PathBuf, String> {
    let dir = config_dir().ok_or("无法获取设置文件路径")?;
    Ok(dir.join(SETTINGS_FILE))
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// 清理过期的剪贴板历史数据
async fn cleanup_expired_data(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    println!("开始清理过期数据，设置：max_items={}, max_time={}", settings.max_history_items, settings.max_history_time);
    
    // 获取数据库连接池
    let db_state = match app.try_state::<Mutex<DatabaseState>>() {
        Some(state) => state,
        None => {
            println!("数据库状态还未初始化，跳过清理");
            return Ok(());
        }
    };
    
    let db_guard = db_state.lock().await;
    let db = &db_guard.pool;
    
    println!("数据库连接可用，开始清理操作");
    
    // 首先查看数据库中的所有记录
    match sqlx::query("SELECT id, timestamp, is_favorite FROM clipboard_history ORDER BY timestamp DESC LIMIT 5")
        .fetch_all(db)
        .await {
        Ok(rows) => {
            println!("数据库中的前5条记录:");
            for row in rows {
                let id: i64 = row.get("id");
                let timestamp: String = row.get("timestamp");
                let is_favorite: i64 = row.get("is_favorite");
                println!("  ID: {}, 时间戳: {}, 收藏: {}", id, timestamp, is_favorite);
            }
        }
        Err(e) => {
            println!("查询记录失败: {}", e);
        }
    }
    
    // 1. 按时间清理：删除超过指定天数的记录（但保留收藏的）
    // 使用 ISO 格式的时间戳，与前端保持一致
    let days_ago = chrono::Utc::now() - chrono::Duration::days(settings.max_history_time as i64);
    let timestamp_cutoff = days_ago.to_rfc3339(); // 使用 ISO 8601 格式
    
    println!("时间清理：删除 {} 之前的记录", timestamp_cutoff);
    
    // 首先获取需要删除的图片文件路径
    let time_images_query = "
        SELECT image_path FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0 AND image_path IS NOT NULL
    ";
    
    let time_expired_images = match sqlx::query(time_images_query)
        .bind(&timestamp_cutoff)
        .fetch_all(db)
        .await {
        Ok(rows) => {
            let mut paths = Vec::new();
            for row in rows {
                if let Ok(path) = row.try_get::<String, &str>("image_path") {
                    paths.push(path);
                }
            }
            paths
        }
        Err(e) => {
            println!("查询过期图片路径失败: {}", e);
            Vec::new()
        }
    };
    
    // 删除过期的图片文件
    for image_path in &time_expired_images {
        if let Err(e) = std::fs::remove_file(image_path) {
            println!("删除图片文件失败 {}: {}", image_path, e);
        } else {
            println!("已删除图片文件: {}", image_path);
        }
    }
    
    let time_cleanup_query = "
        DELETE FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0
    ";
    
    match sqlx::query(time_cleanup_query)
        .bind(&timestamp_cutoff)
        .execute(db)
        .await {
        Ok(result) => {
            println!("按时间清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), time_expired_images.len());
        }
        Err(e) => {
            println!("按时间清理失败: {}", e);
            return Err(format!("按时间清理数据失败: {}", e));
        }
    }
    
    // 2. 按数量清理：保留最新的指定数量记录（收藏的不计入数量限制）
    // 首先获取当前非收藏记录的总数
    let count_query = "SELECT COUNT(*) as count FROM clipboard_history WHERE is_favorite = 0";
    let count_result = match sqlx::query(count_query)
        .fetch_one(db)
        .await {
        Ok(result) => result,
        Err(e) => {
            println!("查询记录数量失败: {}", e);
            return Err(format!("查询记录数量失败: {}", e));
        }
    };
    
    let current_count: i64 = count_result.get("count");
    println!("当前非收藏记录数量: {}, 最大允许: {}", current_count, settings.max_history_items);
    
    if current_count > settings.max_history_items as i64 {
        let excess_count = current_count - settings.max_history_items as i64;
        println!("需要删除 {} 条多余记录", excess_count);
        
        // 首先获取需要删除的记录的图片路径
        let count_images_query = "
            SELECT image_path FROM clipboard_history 
            WHERE is_favorite = 0 
            AND image_path IS NOT NULL
            AND id IN (
                SELECT id FROM clipboard_history 
                WHERE is_favorite = 0 
                ORDER BY timestamp ASC 
                LIMIT ?
            )
        ";
        
        let count_expired_images = match sqlx::query(count_images_query)
            .bind(excess_count)
            .fetch_all(db)
            .await {
            Ok(rows) => {
                let mut paths = Vec::new();
                for row in rows {
                    if let Ok(path) = row.try_get::<String, &str>("image_path") {
                        paths.push(path);
                    }
                }
                paths
            }
            Err(e) => {
                println!("查询需删除图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除图片文件
        for image_path in &count_expired_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                println!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                println!("已删除图片文件: {}", image_path);
            }
        }
        
        // 删除最旧的非收藏记录
        let count_cleanup_query = "
            DELETE FROM clipboard_history 
            WHERE is_favorite = 0 
            AND id IN (
                SELECT id FROM clipboard_history 
                WHERE is_favorite = 0 
                ORDER BY timestamp ASC 
                LIMIT ?
            )
        ";
        
        match sqlx::query(count_cleanup_query)
            .bind(excess_count)
            .execute(db)
            .await {
            Ok(result) => {
                println!("按数量清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), count_expired_images.len());
            }
            Err(e) => {
                println!("按数量清理失败: {}", e);
                return Err(format!("按数量清理数据失败: {}", e));
            }
        }
    } else {
        println!("记录数量未超出限制，无需按数量清理");
    }
    
    // 清理后再次查看记录数量
    match sqlx::query("SELECT COUNT(*) as total, COUNT(CASE WHEN is_favorite = 1 THEN 1 END) as favorites FROM clipboard_history")
        .fetch_one(db)
        .await {
        Ok(row) => {
            let total: i64 = row.get("total");
            let favorites: i64 = row.get("favorites");
            println!("清理后统计：总记录数: {}, 收藏数: {}", total, favorites);
        }
        Err(e) => {
            println!("查询清理后统计失败: {}", e);
        }
    }
    
    // 3. 清理孤立的图片文件（数据库中没有对应记录的文件）
    if let Ok(images_dir) = get_app_images_dir() {
        if images_dir.exists() {
            match std::fs::read_dir(&images_dir) {
                Ok(entries) => {
                    let mut orphaned_files = Vec::new();
                    
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let file_path = entry.path();
                            if file_path.is_file() {
                                let file_path_str = file_path.to_string_lossy().to_string();
                                
                                // 检查数据库中是否存在此文件路径的记录
                                let check_query = "SELECT COUNT(*) as count FROM clipboard_history WHERE image_path = ?";
                                match sqlx::query(check_query)
                                    .bind(&file_path_str)
                                    .fetch_one(db)
                                    .await {
                                    Ok(row) => {
                                        let count: i64 = row.get("count");
                                        if count == 0 {
                                            orphaned_files.push(file_path_str);
                                        }
                                    }
                                    Err(e) => {
                                        println!("检查孤立文件失败: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    
                    // 删除孤立的图片文件
                    for orphaned_file in &orphaned_files {
                        if let Err(e) = std::fs::remove_file(orphaned_file) {
                            println!("删除孤立图片文件失败 {}: {}", orphaned_file, e);
                        } else {
                            println!("已删除孤立图片文件: {}", orphaned_file);
                        }
                    }
                    
                    if !orphaned_files.is_empty() {
                        println!("清理了 {} 个孤立的图片文件", orphaned_files.len());
                    }
                }
                Err(e) => {
                    println!("无法读取图片目录: {}", e);
                }
            }
        }
    }
    
    println!("数据清理完成");
    Ok(())
}

#[tauri::command]
pub async fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    println!("保存设置: {:?}", settings);
    let path = settings_file_path()?;
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    
    println!("设置已保存，开始执行清理");
    // 保存设置后自动清理过期数据
    match cleanup_expired_data(&app, &settings).await {
        Ok(_) => println!("清理操作完成"),
        Err(e) => println!("清理操作失败: {}", e),
    }
    
    Ok(())
}

#[tauri::command]
pub async fn load_settings(_app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_file_path()?;
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let settings: AppSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command]
pub async fn register_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
    // 先尝试注销已有的快捷键
    let _ = app.global_shortcut().unregister_all();
    
    // 将字符串转换为 Shortcut 类型
    let shortcut = shortcut.parse::<Shortcut>().map_err(|e| e.to_string())?;
    
    // 注册快捷键
    app.global_shortcut().register(shortcut).map_err(|e| e.to_string())?;
    
    Ok(())
}

// 检查 Windows 自启动状态
#[cfg(target_os = "windows")]
fn get_windows_auto_start_status(app_name: &str) -> Result<bool, String> {
    use std::process::Command;
    
    let key_path = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run";
    
    let output = Command::new("reg")
        .args(&[
            "query",
            key_path,
            "/v",
            app_name
        ])
        .output()
        .map_err(|e| format!("查询注册表失败: {}", e))?;
        
    // 如果查询成功且输出包含应用名称，说明自启动已启用
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).contains(app_name))
}

// Windows 注册表操作
#[cfg(target_os = "windows")]
fn set_windows_auto_start(enable: bool, app_name: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::process::Command;
    
    let key_path = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run";
    
    if enable {
        // 添加到启动项
        let output = Command::new("reg")
            .args(&[
                "add",
                key_path,
                "/v",
                app_name,
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\"", exe_path.display()),
                "/f"
            ])
            .output()
            .map_err(|e| format!("执行注册表命令失败: {}", e))?;
            
        if !output.status.success() {
            return Err(format!("添加启动项失败: {}", String::from_utf8_lossy(&output.stderr)));
        }
    } else {
        // 从启动项移除
        let output = Command::new("reg")
            .args(&[
                "delete",
                key_path,
                "/v",
                app_name,
                "/f"
            ])
            .output()
            .map_err(|e| format!("执行注册表命令失败: {}", e))?;
            
        // 注意：如果键不存在，reg delete 会返回错误，但这是正常的
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("无法找到指定的注册表项或值") && !stderr.contains("The system was unable to find the specified registry key or value") {
                return Err(format!("移除启动项失败: {}", stderr));
            }
        }
    }
    
    Ok(())
}

// 非 Windows 系统的占位实现
#[cfg(not(target_os = "windows"))]
fn set_windows_auto_start(_enable: bool, _app_name: &str, _exe_path: &PathBuf) -> Result<(), String> {
    Err("当前系统不支持自启动功能".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_windows_auto_start_status(_app_name: &str) -> Result<bool, String> {
    Ok(false)
}

#[tauri::command]
pub async fn set_auto_start(_app: AppHandle, enable: bool) -> Result<(), String> {
    let app_name = "ClipboardManager"; // 应用程序在注册表中的名称
    let exe_path = get_app_exe_path()?;
    
    set_windows_auto_start(enable, app_name, &exe_path)?;
    
    Ok(())
}

#[tauri::command]
pub async fn get_auto_start_status(_app: AppHandle) -> Result<bool, String> {
    let app_name = "ClipboardManager";
    get_windows_auto_start_status(app_name)
}

#[tauri::command]
pub async fn cleanup_history(app: AppHandle) -> Result<(), String> {
    // 加载当前设置
    let settings = load_settings(app.clone()).await.unwrap_or_else(|_| AppSettings {
        max_history_items: 100,
        max_history_time: 30,
        hotkey: "Ctrl+Shift+V".to_string(),
        auto_start: false,
    });
    
    cleanup_expired_data(&app, &settings).await
}

// 跨平台自动粘贴功能
#[tauri::command]
pub async fn auto_paste() -> Result<(), String> {
    println!("开始执行跨平台自动粘贴...");
    
    // 移除额外等待时间，前端已经处理了必要的同步等待，直接执行粘贴以获得最快响应
    // tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    
    // 在新线程中执行按键模拟，避免阻塞异步运行时
    let result = tokio::task::spawn_blocking(|| {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("无法初始化键盘模拟器: {}", e))?;
        
        // 跨平台粘贴快捷键
        #[cfg(target_os = "macos")]
        {
            // macOS 使用 Cmd+V
            enigo.key(Key::Meta, enigo::Direction::Press)
                .map_err(|e| format!("按下Cmd键失败: {}", e))?;
            enigo.key(Key::Unicode('v'), enigo::Direction::Press)
                .map_err(|e| format!("按下V键失败: {}", e))?;
            enigo.key(Key::Unicode('v'), enigo::Direction::Release)
                .map_err(|e| format!("释放V键失败: {}", e))?;
            enigo.key(Key::Meta, enigo::Direction::Release)
                .map_err(|e| format!("释放Cmd键失败: {}", e))?;
        }
        
        #[cfg(not(target_os = "macos"))]
        {
            // Windows 和 Linux 使用 Ctrl+V
            enigo.key(Key::Control, enigo::Direction::Press)
                .map_err(|e| format!("按下Ctrl键失败: {}", e))?;
            enigo.key(Key::Unicode('v'), enigo::Direction::Press)
                .map_err(|e| format!("按下V键失败: {}", e))?;
            enigo.key(Key::Unicode('v'), enigo::Direction::Release)
                .map_err(|e| format!("释放V键失败: {}", e))?;
            enigo.key(Key::Control, enigo::Direction::Release)
                .map_err(|e| format!("释放Ctrl键失败: {}", e))?;
        }
        
        Ok::<(), String>(())
    }).await;
    
    match result {
        Ok(Ok(())) => {
            println!("跨平台自动粘贴操作完成");
    Ok(())
}
        Ok(Err(e)) => {
            println!("自动粘贴失败: {}", e);
            Err(format!("粘贴操作失败: {}", e))
        }
        Err(e) => {
            println!("粘贴任务执行失败: {}", e);
            Err(format!("粘贴任务失败: {}", e))
        }
    }
}

// 获取应用程序的可执行文件路径
fn get_app_exe_path() -> Result<PathBuf, String> {
    env::current_exe().map_err(|e| format!("无法获取应用程序路径: {}", e))
}

// 获取应用程序安装目录下的图片目录
fn get_app_images_dir() -> Result<PathBuf, String> {
    let exe_path = get_app_exe_path()?;
    
    // 获取可执行文件所在的目录
    let exe_dir = exe_path.parent()
        .ok_or("无法获取程序目录")?;
    
    // 在程序目录下创建 images 文件夹
    let images_dir = exe_dir.join("images");
    
    // 确保目录存在
    if !images_dir.exists() {
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| format!("无法创建图片目录: {}", e))?;
    }
    
    Ok(images_dir)
}

#[tauri::command]
pub async fn reset_database(app: AppHandle) -> Result<(), String> {
    println!("开始重置数据库...");
    
    // 尝试获取数据库状态
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 首先获取所有图片文件路径
        let all_images = match sqlx::query("SELECT image_path FROM clipboard_history WHERE image_path IS NOT NULL")
            .fetch_all(pool)
            .await {
            Ok(rows) => {
                let mut paths = Vec::new();
                for row in rows {
                    if let Ok(path) = row.try_get::<String, &str>("image_path") {
                        paths.push(path);
                    }
                }
                paths
            }
            Err(e) => {
                println!("查询图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除所有图片文件
        for image_path in &all_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                println!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                println!("已删除图片文件: {}", image_path);
            }
        }
        println!("已删除 {} 个图片文件", all_images.len());
        
        // 删除整个图片目录（如果存在且为空）
        if let Ok(images_dir) = get_app_images_dir() {
            if images_dir.exists() {
                if let Err(e) = std::fs::remove_dir(&images_dir) {
                    println!("删除图片目录失败（可能不为空）: {}", e);
                } else {
                    println!("已删除图片目录: {:?}", images_dir);
                }
            }
        }
        
        // 清空表数据而不是删除表结构，这样可以保持迁移状态
        sqlx::query("DELETE FROM clipboard_history").execute(pool).await
            .map_err(|e| format!("清空表数据失败: {}", e))?;
        
        println!("数据库数据已清空");
        
        // 不需要手动添加列，因为迁移系统已经处理了这些
        // 只确保索引存在
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content)")
            .execute(pool).await
            .map_err(|e| format!("创建索引失败: {}", e))?;
        
        println!("数据库重置完成");
        Ok(())
    } else {
        Err("无法访问数据库状态".to_string())
    }
}

#[tauri::command]
pub async fn load_image_file(image_path: String) -> Result<String, String> {
    let path = PathBuf::from(&image_path);
    
    // 检查文件是否存在
    if !path.exists() {
        return Err("图片文件不存在".to_string());
    }
    
    // 读取图片文件
    let image_data = std::fs::read(&path)
        .map_err(|e| format!("无法读取图片文件: {}", e))?;
    
    // 转换为 base64
    let b64 = general_purpose::STANDARD.encode(&image_data);
    let data_url = format!("data:image/png;base64,{}", b64);
    
    Ok(data_url)
}

// 改进的图标缓存，使用LRU和更严格的内存管理

struct IconCacheEntry {
    icon: Option<String>,
    access_time: std::time::Instant,
}

struct IconCache {
    cache: HashMap<String, IconCacheEntry>,
    access_order: BTreeMap<std::time::Instant, String>,
    max_size: usize,
}

// 剪贴板监听器控制
struct ClipboardWatcherState {
    should_stop: Arc<AtomicBool>,
}

impl IconCache {
    fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: BTreeMap::new(),
            max_size,
        }
    }

    fn get(&mut self, key: &str) -> Option<Option<String>> {
        if let Some(entry) = self.cache.get_mut(key) {
            // 更新访问时间
            self.access_order.remove(&entry.access_time);
            entry.access_time = std::time::Instant::now();
            self.access_order.insert(entry.access_time, key.to_string());
            Some(entry.icon.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: String, icon: Option<String>) {
        let now = std::time::Instant::now();
        
        // 如果缓存已满，移除最旧的条目
        while self.cache.len() >= self.max_size {
            if let Some((oldest_time, oldest_key)) = self.access_order.pop_first() {
                self.cache.remove(&oldest_key);
            } else {
                break;
            }
        }

        let entry = IconCacheEntry {
            icon,
            access_time: now,
        };

        self.cache.insert(key.clone(), entry);
        self.access_order.insert(now, key);
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }
}

#[derive(Debug, Serialize, Clone)]
struct SourceAppInfo {
    name: String,
    icon: Option<String>, // base64 encoded icon
}

// 添加限流机制，避免频繁获取窗口信息
static LAST_WINDOW_INFO_CALL: std::sync::OnceLock<Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>>> = std::sync::OnceLock::new();

fn get_last_window_info() -> &'static Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>> {
    LAST_WINDOW_INFO_CALL.get_or_init(|| {
        Arc::new(RwLock::new((std::time::Instant::now() - Duration::from_secs(10), None)))
    })
}

// 使用改进的图标缓存
static ICON_CACHE: std::sync::OnceLock<Arc<RwLock<IconCache>>> = std::sync::OnceLock::new();

fn get_icon_cache() -> &'static Arc<RwLock<IconCache>> {
    ICON_CACHE.get_or_init(|| Arc::new(RwLock::new(IconCache::new(10)))) // 减少到10个条目
}

// 手动清理内存缓存
#[tauri::command]
pub async fn clear_memory_cache() -> Result<(), String> {
    // 清理图标缓存
    cleanup_icon_cache();
    
    // 强制清理所有缓存
    let cache = get_icon_cache();
    if let Ok(mut cache_guard) = cache.write() {
        cache_guard.clear();
        println!("图标缓存已完全清空");
    }
    
    // 清理窗口信息缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.1 = None;
        println!("窗口信息缓存已清理");
    }
    
    println!("内存缓存已清理");
    Ok(())
}

// 更严格的图标缓存清理
fn cleanup_icon_cache() {
    let cache = get_icon_cache();
    if let Ok(mut cache_guard) = cache.write() {
        if cache_guard.len() > 5 {  // 只保留5个最新的
            // 清空一半缓存
            let to_clear = cache_guard.len() / 2;
            for _ in 0..to_clear {
                if let Some((oldest_time, oldest_key)) = cache_guard.access_order.pop_first() {
                    cache_guard.cache.remove(&oldest_key);
                } else {
                    break;
                }
            }
            println!("清理图标缓存，保留 {} 项", cache_guard.len());
        }
    }
}

#[tauri::command]
pub async fn force_memory_cleanup() -> Result<String, String> {
    println!("开始强制内存清理...");
    
    // 强制清理所有内存缓存
    cleanup_icon_cache();
    
    // 清空图标缓存
    let cache = get_icon_cache();
    let cache_size = if let Ok(mut cache_guard) = cache.write() {
        let size = cache_guard.len();
        cache_guard.clear();
        size
    } else {
        0
    };
    
    // 清理窗口信息缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.1 = None;
    }
    
    // 尝试强制内存回收 - 多次调用以确保效果
    #[cfg(target_os = "windows")]
    unsafe {
        use winapi::um::winbase::{SetProcessWorkingSetSize};
        use winapi::um::processthreadsapi::GetCurrentProcess;
        
        // 多次调用SetProcessWorkingSetSize以强制内存回收
        for _ in 0..3 {
            let result = SetProcessWorkingSetSize(
                GetCurrentProcess(),
                usize::MAX,
                usize::MAX,
            );
            if result == 0 {
                println!("警告: SetProcessWorkingSetSize 调用失败");
            }
            // 在调用之间稍作等待
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        
        // 额外调用以确保内存清理效果
        let additional_result = SetProcessWorkingSetSize(
            GetCurrentProcess(),
            0,
            0,
        );
        if additional_result == 0 {
            println!("警告: 额外的内存清理调用失败");
        }
    }
    
    let message = format!(
        "强制内存清理完成 - 清理了 {} 个图标缓存项，执行了多轮内存回收", 
        cache_size
    );
    println!("{}", message);
    Ok(message)
}

#[tauri::command]
pub async fn get_memory_stats() -> Result<String, String> {
    let cache = get_icon_cache();
    let cache_size = if let Ok(cache_guard) = cache.read() {
        cache_guard.len()
    } else {
        0
    };
    
    let window_cache_status = if let Ok(guard) = get_last_window_info().read() {
        if guard.1.is_some() { "已缓存" } else { "未缓存" }
    } else {
        "无法访问"
    };
    
    #[cfg(target_os = "windows")]
    let memory_info = unsafe {
        use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use winapi::um::processthreadsapi::GetCurrentProcess;
        
        let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            format!("工作集: {} MB, 峰值工作集: {} MB", 
                    pmc.WorkingSetSize / 1024 / 1024,
                    pmc.PeakWorkingSetSize / 1024 / 1024)
        } else {
            "无法获取内存信息".to_string()
        }
    };
    
    #[cfg(not(target_os = "windows"))]
    let memory_info = "非Windows系统".to_string();
    
    let stats = format!(
        "内存统计:\n图标缓存项: {}\n窗口信息缓存: {}\n{}",
        cache_size, window_cache_status, memory_info
    );
    
    Ok(stats)
}

#[tauri::command]
pub async fn stop_clipboard_watcher(app: AppHandle) -> Result<(), String> {
    if let Some(watcher_state) = app.try_state::<ClipboardWatcherState>() {
        watcher_state.should_stop.store(true, Ordering::Relaxed);
        println!("剪贴板监听器停止信号已发送");
        Ok(())
    } else {
        Err("无法找到剪贴板监听器状态".to_string())
    }
}

// 新的剪贴板监听器 - 使用事件驱动而不是轮询
fn start_clipboard_watcher(app: AppHandle) -> Arc<AtomicBool> {
    let should_stop = Arc::new(AtomicBool::new(false));
    
    // 使用新的插件，剪贴板监听由插件自动处理
    // 不再需要手动轮询，避免了arboard的内存泄漏问题
    
    // TODO: 这里将由前端通过事件监听器设置剪贴板监听
    // tauri-plugin-clipboard 插件会在前端处理剪贴板事件
    
    println!("剪贴板监听器已初始化（事件驱动模式，无内存泄漏）");
    
    should_stop
}

#[tauri::command]
pub async fn start_new_clipboard_watcher(app: AppHandle) -> Result<(), String> {
    println!("开始重启剪贴板监听器...");
    
    // 停止现有的监听器
    if let Some(watcher_state) = app.try_state::<ClipboardWatcherState>() {
        watcher_state.should_stop.store(true, Ordering::Relaxed);
        println!("已发送停止信号给旧监听器");
    }
    
    // 等待更长时间确保旧监听器完全停止
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
    
    // 执行强制内存清理
    let _ = force_memory_cleanup().await;
    
    // 再等待一段时间
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // 启动新的监听器
    let should_stop = start_clipboard_watcher(app.clone());
    
    // 更新状态
    app.manage(ClipboardWatcherState { should_stop });
    
    println!("新的剪贴板监听器已启动");
    Ok(())
}

#[tauri::command]
pub async fn ensure_database_compatibility(_app: AppHandle) -> Result<(), String> {
    // Database compatibility is now handled by tauri_plugin_sql migration system
    // This function is kept for backward compatibility but does nothing
    println!("数据库兼容性由迁移系统自动处理");
    Ok(())
} 

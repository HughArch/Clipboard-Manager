use tauri::{AppHandle, Manager};
use crate::types::{AppSettings, DatabaseState};
use crate::logging;
use std::fs;
use std::path::PathBuf;
use dirs_next::config_dir;
use base64::{engine::general_purpose, Engine as _};
use tauri_plugin_global_shortcut::{self, GlobalShortcutExt, Shortcut};
use std::env;
use chrono;
use tokio;
use tokio::sync::Mutex;
use sqlx::{self, Row};
use image::{ImageFormat, imageops::FilterType};
// enigo 导入将在具体使用处声明


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
    tracing::info!("开始清理过期数据，设置：max_items={}, max_time={}", settings.max_history_items, settings.max_history_time);
    
    // 获取数据库连接池
    let db_state = match app.try_state::<Mutex<DatabaseState>>() {
        Some(state) => state,
        None => {
            tracing::warn!("数据库状态还未初始化，跳过清理");
            return Ok(());
        }
    };
    
    let db_guard = db_state.lock().await;
    let db = &db_guard.pool;
    
    tracing::debug!("数据库连接可用，开始清理操作");
    
    // 首先查看数据库中的所有记录
    match sqlx::query("SELECT id, timestamp, is_favorite FROM clipboard_history ORDER BY timestamp DESC LIMIT 5")
        .fetch_all(db)
        .await {
        Ok(rows) => {
            tracing::info!("数据库中的前5条记录:");
            for row in rows {
                let id: i64 = row.get("id");
                let timestamp: String = row.get("timestamp");
                let is_favorite: i64 = row.get("is_favorite");
                tracing::info!("  ID: {}, 时间戳: {}, 收藏: {}", id, timestamp, is_favorite);
            }
        }
        Err(e) => {
            tracing::error!("查询记录失败: {}", e);
        }
    }
    
    // 1. 按时间清理：删除超过指定天数的记录（但保留收藏的）
    // 使用 ISO 格式的时间戳，与前端保持一致
    let days_ago = chrono::Utc::now() - chrono::Duration::days(settings.max_history_time as i64);
    let timestamp_cutoff = days_ago.to_rfc3339(); // 使用 ISO 8601 格式
    
    tracing::info!("时间清理：删除 {} 之前的记录", timestamp_cutoff);
    
    // 首先获取需要删除的图片文件路径
    let time_images_query = "
        SELECT image_path FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0 AND group_id IS NULL AND image_path IS NOT NULL
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
            tracing::info!("查询过期图片路径失败: {}", e);
            Vec::new()
        }
    };
    
    // 删除过期的图片文件
    for image_path in &time_expired_images {
        if let Err(e) = std::fs::remove_file(image_path) {
            tracing::info!("删除图片文件失败 {}: {}", image_path, e);
        } else {
            tracing::info!("已删除图片文件: {}", image_path);
        }
    }
    
    let time_cleanup_query = "
        DELETE FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0 AND group_id IS NULL
    ";
    
    match sqlx::query(time_cleanup_query)
        .bind(&timestamp_cutoff)
        .execute(db)
        .await {
        Ok(result) => {
            tracing::info!("按时间清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), time_expired_images.len());
        }
        Err(e) => {
            tracing::error!("按时间清理失败: {}", e);
            return Err(format!("按时间清理数据失败: {}", e));
        }
    }
    
    // 2. 按数量清理：保留最新的指定数量记录（收藏的和分组的不计入数量限制）
    // 首先获取当前非收藏且非分组记录的总数
    let count_query = "SELECT COUNT(*) as count FROM clipboard_history WHERE is_favorite = 0 AND group_id IS NULL";
    let count_result = match sqlx::query(count_query)
        .fetch_one(db)
        .await {
        Ok(result) => result,
        Err(e) => {
            tracing::info!("查询记录数量失败: {}", e);
            return Err(format!("查询记录数量失败: {}", e));
        }
    };
    
    let current_count: i64 = count_result.get("count");
    tracing::info!("当前非收藏且非分组记录数量: {}, 最大允许: {}", current_count, settings.max_history_items);
    
    if current_count > settings.max_history_items as i64 {
        let excess_count = current_count - settings.max_history_items as i64;
        tracing::info!("需要删除 {} 条多余记录", excess_count);
        
        // 首先获取需要删除的记录的图片路径
        let count_images_query = "
            SELECT image_path FROM clipboard_history 
            WHERE is_favorite = 0 
            AND group_id IS NULL
            AND image_path IS NOT NULL
            AND id IN (
                SELECT id FROM clipboard_history 
                WHERE is_favorite = 0 
                AND group_id IS NULL
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
                tracing::info!("查询需删除图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除图片文件
        for image_path in &count_expired_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                tracing::info!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                tracing::info!("已删除图片文件: {}", image_path);
            }
        }
        
        // 删除最旧的非收藏且非分组记录
        let count_cleanup_query = "
            DELETE FROM clipboard_history 
            WHERE is_favorite = 0 
            AND group_id IS NULL
            AND id IN (
                SELECT id FROM clipboard_history 
                WHERE is_favorite = 0 
                AND group_id IS NULL
                ORDER BY timestamp ASC 
                LIMIT ?
            )
        ";
        
        match sqlx::query(count_cleanup_query)
            .bind(excess_count)
            .execute(db)
            .await {
            Ok(result) => {
                tracing::info!("按数量清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), count_expired_images.len());
            }
            Err(e) => {
                tracing::info!("按数量清理失败: {}", e);
                return Err(format!("按数量清理数据失败: {}", e));
            }
        }
    } else {
        tracing::info!("记录数量未超出限制，无需按数量清理");
    }
    
    // 清理后再次查看记录数量
    match sqlx::query("SELECT COUNT(*) as total, COUNT(CASE WHEN is_favorite = 1 THEN 1 END) as favorites FROM clipboard_history")
        .fetch_one(db)
        .await {
        Ok(row) => {
            let total: i64 = row.get("total");
            let favorites: i64 = row.get("favorites");
            tracing::info!("清理后统计：总记录数: {}, 收藏数: {}", total, favorites);
        }
        Err(e) => {
            tracing::info!("查询清理后统计失败: {}", e);
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
                                        tracing::info!("检查孤立文件失败: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    
                    // 删除孤立的图片文件
                    for orphaned_file in &orphaned_files {
                        if let Err(e) = std::fs::remove_file(orphaned_file) {
                            tracing::info!("删除孤立图片文件失败 {}: {}", orphaned_file, e);
                        } else {
                            tracing::info!("已删除孤立图片文件: {}", orphaned_file);
                        }
                    }
                    
                    if !orphaned_files.is_empty() {
                        tracing::info!("清理了 {} 个孤立的图片文件", orphaned_files.len());
                    }
                }
                Err(e) => {
                    tracing::info!("无法读取图片目录: {}", e);
                }
            }
        }
    }
    
    tracing::info!("数据清理完成");
    Ok(())
}

#[tauri::command]
pub async fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    tracing::info!("保存设置: {:?}", settings);
    let path = settings_file_path()?;
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    
    tracing::info!("设置已保存，开始执行清理");
    // 保存设置后自动清理过期数据
    match cleanup_expired_data(&app, &settings).await {
        Ok(_) => tracing::info!("清理操作完成"),
        Err(e) => tracing::info!("清理操作失败: {}", e),
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
    tracing::info!("尝试注册快捷键: {}", shortcut);
    
    // 先尝试注销已有的快捷键
    let _ = app.global_shortcut().unregister_all();
    
    // macOS 特殊处理：标准化快捷键格式
    let normalized_shortcut = normalize_shortcut_for_macos(&shortcut)?;
    tracing::info!("标准化后的快捷键: {}", normalized_shortcut);
    
    // 将字符串转换为 Shortcut 类型
    let shortcut_parsed = normalized_shortcut.parse::<Shortcut>().map_err(|e| {
        let error_msg = format!("Invalid hotkey format: {}. Please use format like 'Cmd+Shift+V' on macOS or 'Ctrl+Shift+V' on other platforms", e);
        tracing::info!("快捷键解析失败: {}", error_msg);
        error_msg
    })?;
    
    // 注册快捷键
    app.global_shortcut().register(shortcut_parsed).map_err(|e| {
        let error_str = e.to_string();
        
        // 处理常见的错误类型
        if error_str.contains("HotKey already registered") || error_str.contains("already registered") {
            let friendly_msg = format!("HotKey already registered: The hotkey '{}' is already in use by another application", normalized_shortcut);
            tracing::info!("快捷键冲突: {}", friendly_msg);
            friendly_msg
        } else if error_str.contains("Invalid") || error_str.contains("invalid") {
            let friendly_msg = format!("Invalid hotkey format: '{}' is not a valid hotkey format", normalized_shortcut);
            tracing::info!("快捷键格式错误: {}", friendly_msg);
            friendly_msg
        } else {
            tracing::info!("快捷键注册失败: {}", error_str);
            format!("Failed to register hotkey '{}': {}", normalized_shortcut, error_str)
        }
    })?;
    
    tracing::info!("快捷键注册成功: {}", normalized_shortcut);
    Ok(())
}

// macOS 快捷键格式标准化函数
fn normalize_shortcut_for_macos(shortcut: &str) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        // 先检查是否包含特殊字符（macOS Alt+字母产生的）
        let has_special_chars = shortcut.chars().any(|c| {
            !c.is_ascii() && c != '+'
        });
        
        if has_special_chars {
            // 如果包含特殊字符，这可能是用户按了 Alt+字母
            // macOS 会将 Alt+V 转换为特殊字符如 Å
            return Err(
                "macOS does not support Alt+Letter combinations for global shortcuts. Please use Cmd+Letter, Cmd+Shift+Letter, or Ctrl+Letter instead. Recommended: Cmd+Shift+V".to_string()
            );
        }
        
        // 检查是否包含 Alt 或 Option
        if shortcut.to_lowercase().contains("alt") || shortcut.to_lowercase().contains("option") {
            return Err(
                "macOS global shortcuts do not support Option/Alt key combinations. Please use Cmd+Shift+V or Ctrl+Shift+V instead.".to_string()
            );
        }
        
        let parts: Vec<&str> = shortcut.split('+').collect();
        let mut normalized_parts = Vec::new();
        
        // 处理修饰键
        for part in parts {
            let trimmed = part.trim();
            match trimmed.to_lowercase().as_str() {
                "ctrl" => {
                    normalized_parts.push("Ctrl".to_string());
                },
                "cmd" | "command" => {
                    normalized_parts.push("Cmd".to_string());
                },
                "shift" => {
                    normalized_parts.push("Shift".to_string());
                },
                _ => {
                    // 主键保持不变，但转换为大写
                    if trimmed.len() == 1 {
                        normalized_parts.push(trimmed.to_uppercase());
                    } else {
                        normalized_parts.push(trimmed.to_string());
                    }
                }
            }
        }
        
        let result = normalized_parts.join("+");
        tracing::info!("macOS 快捷键转换: {} -> {}", shortcut, result);
        Ok(result)
    }
    
    #[cfg(not(target_os = "macos"))]
    {
        // 非 macOS 平台直接返回原始快捷键
        Ok(shortcut.to_string())
    }
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

#[tauri::command]
pub async fn set_auto_start(app: AppHandle, enable: bool) -> Result<(), String> {
    let app_name = "Clipboard Manager"; // 显示名称
    let bundle_id = "com.clipboardmanager.app"; // Bundle ID
    
    #[cfg(target_os = "windows")]
    {
        let exe_path = get_app_exe_path()?;
        set_windows_auto_start(enable, "ClipboardManager", &exe_path).map_err(|e| {
            format!("Failed to update auto-start settings: {}", e)
        })?;
    }
    
    #[cfg(target_os = "macos")]
    {
        let exe_path = get_app_exe_path()?;
        set_macos_auto_start(enable, app_name, bundle_id, &exe_path).map_err(|e| {
            format!("设置 macOS 自启动失败: {}", e)
        })?;
    }
    
    #[cfg(target_os = "linux")]
    {
        let exe_path = get_app_exe_path()?;
        set_linux_auto_start(enable, app_name, &exe_path).map_err(|e| {
            format!("设置 Linux 自启动失败: {}", e)
        })?;
    }
    
    Ok(())
}

#[tauri::command]
pub async fn get_auto_start_status(_app: AppHandle) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        get_windows_auto_start_status("ClipboardManager")
    }
    
    #[cfg(target_os = "macos")]
    {
        get_macos_auto_start_status("Clipboard Manager", "com.clipboardmanager.app")
    }
    
    #[cfg(target_os = "linux")]
    {
        get_linux_auto_start_status("Clipboard Manager")
    }
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
        .map_err(|e| format!("Failed to query registry: {}", e))?;
    
    // 如果查询成功且找到了值，说明自启动已启用
    Ok(output.status.success())
}

// 非 Windows 系统的占位实现
#[cfg(not(target_os = "windows"))]
fn get_windows_auto_start_status(_app_name: &str) -> Result<bool, String> {
    Ok(false) // 非Windows系统默认返回false
}

// ==================== macOS 自启动实现 ====================

#[cfg(target_os = "macos")]
fn set_macos_auto_start(enable: bool, app_name: &str, bundle_id: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::process::Command;
    
    tracing::debug!("🍎 macOS: 设置自启动状态: {} (应用: {})", enable, app_name);
    
    if enable {
        // 方法1: 尝试使用 Login Items (推荐方法)
        if let Err(e1) = add_to_login_items_applescript(app_name, exe_path) {
            tracing::warn!("⚠️ AppleScript 方法失败: {}", e1);
            
            // 方法2: 回退到 LaunchAgent 方法
            tracing::debug!("🔄 尝试 LaunchAgent 方法...");
            add_to_launch_agent(app_name, bundle_id, exe_path)?;
        }
    } else {
        // 移除自启动：尝试两种方法
        let _ = remove_from_login_items_applescript(app_name);
        let _ = remove_from_launch_agent(bundle_id);
    }
    
    Ok(())
}

#[cfg(target_os = "macos")]
fn get_macos_auto_start_status(app_name: &str, bundle_id: &str) -> Result<bool, String> {
    tracing::debug!("🔍 macOS: 检查自启动状态: {}", app_name);
    
    // 方法1: 检查 Login Items
    if check_login_items_status(app_name).unwrap_or(false) {
        return Ok(true);
    }
    
    // 方法2: 检查 LaunchAgent
    if check_launch_agent_status(bundle_id).unwrap_or(false) {
        return Ok(true);
    }
    
    Ok(false)
}

// 使用 AppleScript 添加到登录项
#[cfg(target_os = "macos")]
fn add_to_login_items_applescript(app_name: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::process::Command;
    
    // 获取应用程序的父目录路径（.app bundle）
    let app_bundle_path = if exe_path.to_string_lossy().contains(".app/Contents/MacOS/") {
        // 如果是 .app bundle 内的可执行文件，获取 .app 路径
        let path_str = exe_path.to_string_lossy();
        if let Some(app_end) = path_str.find(".app/Contents/MacOS/") {
            format!("{}.app", &path_str[..app_end])
        } else {
            exe_path.to_string_lossy().to_string()
        }
    } else {
        exe_path.to_string_lossy().to_string()
    };
    
    tracing::debug!("📁 应用 Bundle 路径: {}", app_bundle_path);
    
    let script = format!(r#"
tell application "System Events"
    -- 检查应用是否已经在登录项中
    set loginItems to login items
    set appExists to false
    repeat with loginItem in loginItems
        if name of loginItem is "{}" then
            set appExists to true
            exit repeat
        end if
    end repeat
    
    -- 如果不存在，则添加
    if not appExists then
        make login item at end with properties {{path:"{}", name:"{}", hidden:false}}
        return "SUCCESS_ADDED"
    else
        return "ALREADY_EXISTS"
    end if
end tell
    "#, app_name, app_bundle_path, app_name);
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!("✅ AppleScript 结果: {}", result);
        Ok(())
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("AppleScript 执行失败: {}", error_msg))
    }
}

// 使用 AppleScript 从登录项移除
#[cfg(target_os = "macos")]
fn remove_from_login_items_applescript(app_name: &str) -> Result<(), String> {
    use std::process::Command;
    
    let script = format!(r#"
tell application "System Events"
    set loginItems to login items
    repeat with loginItem in loginItems
        if name of loginItem is "{}" then
            delete loginItem
            return "SUCCESS_REMOVED"
        end if
    end repeat
    return "NOT_FOUND"
end tell
    "#, app_name);
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!("✅ 移除结果: {}", result);
        Ok(())
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("移除失败: {}", error_msg))
    }
}

// 检查登录项状态
#[cfg(target_os = "macos")]
fn check_login_items_status(app_name: &str) -> Result<bool, String> {
    use std::process::Command;
    
    let script = format!(r#"
tell application "System Events"
    set loginItems to login items
    repeat with loginItem in loginItems
        if name of loginItem is "{}" then
            return "FOUND"
        end if
    end repeat
    return "NOT_FOUND"
end tell
    "#, app_name);
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("检查登录项失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(result == "FOUND")
    } else {
        Ok(false)
    }
}

// 添加到 LaunchAgent（备用方法）
#[cfg(target_os = "macos")]
fn add_to_launch_agent(app_name: &str, bundle_id: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::fs;
    use std::path::Path;
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let launch_agents_dir = Path::new(&home_dir).join("Library/LaunchAgents");
    
    // 确保目录存在
    fs::create_dir_all(&launch_agents_dir)
        .map_err(|e| format!("创建 LaunchAgents 目录失败: {}", e))?;
    
    let plist_filename = format!("{}.plist", bundle_id);
    let plist_path = launch_agents_dir.join(&plist_filename);
    
    let plist_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>LaunchOnlyOnce</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/{}.out</string>
    <key>StandardErrorPath</key>
    <string>/tmp/{}.err</string>
</dict>
</plist>"#, bundle_id, exe_path.to_string_lossy(), bundle_id, bundle_id);
    
    fs::write(&plist_path, plist_content)
        .map_err(|e| format!("写入 plist 文件失败: {}", e))?;
    
    tracing::info!("✅ LaunchAgent plist 已创建: {}", plist_path.display());
    Ok(())
}

// 从 LaunchAgent 移除
#[cfg(target_os = "macos")]
fn remove_from_launch_agent(bundle_id: &str) -> Result<(), String> {
    use std::fs;
    use std::path::Path;
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let plist_filename = format!("{}.plist", bundle_id);
    let plist_path = Path::new(&home_dir)
        .join("Library/LaunchAgents")
        .join(&plist_filename);
    
    if plist_path.exists() {
        fs::remove_file(&plist_path)
            .map_err(|e| format!("删除 plist 文件失败: {}", e))?;
        tracing::info!("✅ LaunchAgent plist 已删除: {}", plist_path.display());
    }
    
    Ok(())
}

// 检查 LaunchAgent 状态
#[cfg(target_os = "macos")]
fn check_launch_agent_status(bundle_id: &str) -> Result<bool, String> {
    use std::path::Path;
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let plist_filename = format!("{}.plist", bundle_id);
    let plist_path = Path::new(&home_dir)
        .join("Library/LaunchAgents")
        .join(&plist_filename);
    
    Ok(plist_path.exists())
}

// ==================== Linux 自启动实现 ====================

#[cfg(target_os = "linux")]
fn set_linux_auto_start(enable: bool, app_name: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::fs;
    use std::path::Path;
    
    tracing::debug!("🐧 Linux: 设置自启动状态: {} (应用: {})", enable, app_name);
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let autostart_dir = Path::new(&home_dir).join(".config/autostart");
    let desktop_filename = format!("{}.desktop", app_name.replace(" ", "-").to_lowercase());
    let desktop_path = autostart_dir.join(&desktop_filename);
    
    if enable {
        // 创建自启动目录
        fs::create_dir_all(&autostart_dir)
            .map_err(|e| format!("创建 autostart 目录失败: {}", e))?;
        
        // 创建 .desktop 文件
        let desktop_content = format!(r#"[Desktop Entry]
Type=Application
Version=1.0
Name={}
Comment=Clipboard Manager for productivity
Exec={}
Icon=clipboard
Terminal=false
StartupNotify=false
Hidden=false
X-GNOME-Autostart-enabled=true
"#, app_name, exe_path.to_string_lossy());
        
        fs::write(&desktop_path, desktop_content)
            .map_err(|e| format!("写入 .desktop 文件失败: {}", e))?;
        
        tracing::info!("✅ Linux: 自启动 .desktop 文件已创建: {}", desktop_path.display());
    } else {
        // 删除 .desktop 文件
        if desktop_path.exists() {
            fs::remove_file(&desktop_path)
                .map_err(|e| format!("删除 .desktop 文件失败: {}", e))?;
            
            tracing::info!("✅ Linux: 自启动 .desktop 文件已删除: {}", desktop_path.display());
        }
    }
    
    Ok(())
}

#[cfg(target_os = "linux")]
fn get_linux_auto_start_status(app_name: &str) -> Result<bool, String> {
    use std::path::Path;
    
    tracing::debug!("🔍 Linux: 检查自启动状态: {}", app_name);
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let desktop_filename = format!("{}.desktop", app_name.replace(" ", "-").to_lowercase());
    let desktop_path = Path::new(&home_dir)
        .join(".config/autostart")
        .join(&desktop_filename);
    
    let exists = desktop_path.exists();
    tracing::debug!("📋 Linux: .desktop 文件状态: {}", if exists { "存在" } else { "不存在" });
    
    Ok(exists)
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

// 改进的自动粘贴功能 - 先激活目标应用，再执行粘贴
#[tauri::command]
pub async fn auto_paste(app: AppHandle) -> Result<(), String> {
    tracing::info!("开始执行智能自动粘贴...");
    
    #[cfg(target_os = "macos")]
    {
        macos_simple_paste(app)
    }
    
    #[cfg(target_os = "windows")]
    {
        // 在新线程中执行粘贴操作
        let result = tokio::task::spawn_blocking(|| {
            windows_auto_paste()
        }).await;
        
        match result {
            Ok(Ok(())) => {
                tracing::info!("智能自动粘贴操作完成");
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::info!("自动粘贴失败: {}", e);
                Err(format!("粘贴操作失败: {}", e))
            }
            Err(e) => {
                tracing::info!("粘贴任务执行失败: {}", e);
                Err(format!("粘贴任务失败: {}", e))
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        // 在新线程中执行粘贴操作
        let result = tokio::task::spawn_blocking(|| {
            linux_auto_paste()
        }).await;
        
        match result {
            Ok(Ok(())) => {
                tracing::info!("智能自动粘贴操作完成");
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::info!("自动粘贴失败: {}", e);
                Err(format!("粘贴操作失败: {}", e))
            }
            Err(e) => {
                tracing::info!("粘贴任务执行失败: {}", e);
                Err(format!("粘贴任务失败: {}", e))
            }
        }
    }
}

// 新增：智能粘贴功能 - 先激活指定应用，再粘贴
#[tauri::command]
pub async fn smart_paste_to_app(app: AppHandle, app_name: String, bundle_id: Option<String>) -> Result<(), String> {
    tracing::info!("开始执行智能粘贴到应用: {} (bundle: {:?})", app_name, bundle_id);
    
    #[cfg(target_os = "macos")]
    {
        macos_smart_paste_to_app(app, app_name, bundle_id)
    }
    
    #[cfg(target_os = "windows")]
    {
        // 克隆参数用于后续日志输出
        let app_name_for_log = app_name.clone();
        
        // 在新线程中执行粘贴操作
        let result = tokio::task::spawn_blocking(move || {
            windows_auto_paste()
        }).await;
        
        match result {
            Ok(Ok(())) => {
                tracing::info!("智能粘贴到应用 {} 完成", app_name_for_log);
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::info!("智能粘贴失败: {}", e);
                Err(format!("粘贴操作失败: {}", e))
            }
            Err(e) => {
                tracing::info!("粘贴任务执行失败: {}", e);
                Err(format!("粘贴任务失败: {}", e))
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        // 克隆参数用于后续日志输出
        let app_name_for_log = app_name.clone();
        
        // 在新线程中执行粘贴操作
        let result = tokio::task::spawn_blocking(move || {
            linux_auto_paste()
        }).await;
        
        match result {
            Ok(Ok(())) => {
                tracing::info!("智能粘贴到应用 {} 完成", app_name_for_log);
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::info!("智能粘贴失败: {}", e);
                Err(format!("粘贴操作失败: {}", e))
            }
            Err(e) => {
                tracing::info!("粘贴任务执行失败: {}", e);
                Err(format!("粘贴任务失败: {}", e))
            }
        }
    }
}


// macOS 使用新的智能粘贴逻辑（基于 EcoPaste 实现）
#[cfg(target_os = "macos")]
fn macos_simple_paste(app: AppHandle) -> Result<(), String> {
    tracing::info!("🍎 使用新的 macOS 智能粘贴逻辑...");
    
    // 使用新的 macos_paste 模块
    crate::macos_paste::smart_paste(Some(app))
}

// macOS 使用新的智能粘贴到指定应用
#[cfg(target_os = "macos")]
fn macos_smart_paste_to_app(app: AppHandle, app_name: String, bundle_id: Option<String>) -> Result<(), String> {
    tracing::info!("🍎 执行 macOS 智能粘贴到应用: {}", app_name);
    
    crate::macos_paste::smart_paste_to_app(&app_name, bundle_id.as_deref(), Some(app))
}



// Windows 使用 rdev 库进行键盘模拟
#[cfg(target_os = "windows")]
fn windows_auto_paste() -> Result<(), String> {
    use rdev::{simulate, EventType, Key, SimulateError};
    use std::thread;
    use std::time::Duration;
    
    tracing::info!("使用 rdev 库执行 Windows 自动粘贴...");
    
    fn send(event_type: &EventType) -> Result<(), SimulateError> {
        let delay = Duration::from_millis(5);
        simulate(event_type)?;
        thread::sleep(delay);
        Ok(())
    }
    
    // 模拟 Ctrl+V 按键序列
    send(&EventType::KeyPress(Key::ControlLeft))
        .map_err(|e| format!("按下 Ctrl 键失败: {:?}", e))?;
    
    send(&EventType::KeyPress(Key::KeyV))
        .map_err(|e| format!("按下 V 键失败: {:?}", e))?;
    
    send(&EventType::KeyRelease(Key::KeyV))
        .map_err(|e| format!("释放 V 键失败: {:?}", e))?;
    
    send(&EventType::KeyRelease(Key::ControlLeft))
        .map_err(|e| format!("释放 Ctrl 键失败: {:?}", e))?;
    
    tracing::info!("rdev Windows 粘贴操作执行完成");
    Ok(())
}

// Linux 使用 rdev 库进行键盘模拟
#[cfg(target_os = "linux")]
fn linux_auto_paste() -> Result<(), String> {
    use rdev::{simulate, EventType, Key, SimulateError};
    use std::thread;
    use std::time::Duration;
    
    tracing::info!("使用 rdev 库执行 Linux 自动粘贴...");
    
    fn send(event_type: &EventType) -> Result<(), SimulateError> {
        let delay = Duration::from_millis(5);
        simulate(event_type)?;
        thread::sleep(delay);
        Ok(())
    }
    
    // 模拟 Ctrl+V 按键序列
    send(&EventType::KeyPress(Key::ControlLeft))
        .map_err(|e| format!("按下 Ctrl 键失败: {:?}", e))?;
    
    send(&EventType::KeyPress(Key::KeyV))
        .map_err(|e| format!("按下 V 键失败: {:?}", e))?;
    
    send(&EventType::KeyRelease(Key::KeyV))
        .map_err(|e| format!("释放 V 键失败: {:?}", e))?;
    
    send(&EventType::KeyRelease(Key::ControlLeft))
        .map_err(|e| format!("释放 Ctrl 键失败: {:?}", e))?;
    
    tracing::info!("rdev Linux 粘贴操作执行完成");
    Ok(())
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
    tracing::info!("开始重置数据库...");
    
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
                tracing::info!("查询图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除所有图片文件
        for image_path in &all_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                tracing::info!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                tracing::info!("已删除图片文件: {}", image_path);
            }
        }
        tracing::info!("已删除 {} 个图片文件", all_images.len());
        
        // 删除整个图片目录（如果存在且为空）
        if let Ok(images_dir) = get_app_images_dir() {
            if images_dir.exists() {
                if let Err(e) = std::fs::remove_dir(&images_dir) {
                    tracing::info!("删除图片目录失败（可能不为空）: {}", e);
                } else {
                    tracing::info!("已删除图片目录: {:?}", images_dir);
                }
            }
        }
        
        // 清空表数据而不是删除表结构，这样可以保持迁移状态
        sqlx::query("DELETE FROM clipboard_history").execute(pool).await
            .map_err(|e| format!("清空表数据失败: {}", e))?;
        
        tracing::info!("数据库数据已清空");
        
        // 不需要手动添加列，因为迁移系统已经处理了这些
        // 只确保索引存在
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content)")
            .execute(pool).await
            .map_err(|e| format!("创建索引失败: {}", e))?;
        
        tracing::info!("数据库重置完成");
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

#[tauri::command]
pub async fn generate_thumbnail(base64_data: String, width: Option<u32>, height: Option<u32>) -> Result<String, String> {
    let width = width.unwrap_or(200);
    let height = height.unwrap_or(150);
    
    // 解析base64数据
    let base64_start = base64_data.find("base64,").ok_or("无效的base64格式")?;
    let base64_str = &base64_data[base64_start + 7..]; // "base64,".len() = 7
    
    // 解码base64
    let image_bytes = general_purpose::STANDARD
        .decode(base64_str)
        .map_err(|e| format!("base64解码失败: {}", e))?;
    
    // 加载图片
    let img = image::load_from_memory(&image_bytes)
        .map_err(|e| format!("图片加载失败: {}", e))?;
    
    // 计算等比例缩放尺寸
    let (img_width, img_height) = (img.width(), img.height());
    let aspect_ratio = img_width as f64 / img_height as f64;
    let target_aspect_ratio = width as f64 / height as f64;
    
    let (target_width, target_height) = if aspect_ratio > target_aspect_ratio {
        // 图片更宽，以宽度为准
        (width, (width as f64 / aspect_ratio) as u32)
    } else {
        // 图片更高，以高度为准
        ((height as f64 * aspect_ratio) as u32, height)
    };
    
    // 生成缩略图
    let thumbnail = img.resize(target_width, target_height, FilterType::Lanczos3);
    
    // 转换为JPEG格式以减小文件大小
    let mut jpeg_buffer = Vec::new();
    thumbnail.write_to(&mut std::io::Cursor::new(&mut jpeg_buffer), ImageFormat::Jpeg)
        .map_err(|e| format!("JPEG编码失败: {}", e))?;
    
    // 转换为base64
    let b64 = general_purpose::STANDARD.encode(&jpeg_buffer);
    let thumbnail_data_url = format!("data:image/jpeg;base64,{}", b64);
    
    Ok(thumbnail_data_url)
}

// ===== 日志相关命令 =====

/// 前端写入日志到文件
#[tauri::command]
pub async fn write_frontend_log(
    level: String,
    message: String,
    context: Option<String>,
) -> Result<(), String> {
    match level.as_str() {
        "error" => {
            if let Some(ctx) = context {
                tracing::error!(target: "frontend", context = %ctx, "{}", message);
            } else {
                tracing::error!(target: "frontend", "{}", message);
            }
        }
        "warn" => {
            if let Some(ctx) = context {
                tracing::warn!(target: "frontend", context = %ctx, "{}", message);
            } else {
                tracing::warn!(target: "frontend", "{}", message);
            }
        }
        "info" => {
            if let Some(ctx) = context {
                tracing::info!(target: "frontend", context = %ctx, "{}", message);
            } else {
                tracing::info!(target: "frontend", "{}", message);
            }
        }
        "debug" => {
            if let Some(ctx) = context {
                tracing::debug!(target: "frontend", context = %ctx, "{}", message);
            } else {
                tracing::debug!(target: "frontend", "{}", message);
            }
        }
        _ => {
            if let Some(ctx) = context {
                tracing::trace!(target: "frontend", context = %ctx, "{}", message);
            } else {
                tracing::trace!(target: "frontend", "{}", message);
            }
        }
    }
    Ok(())
}

/// 获取日志目录路径
#[tauri::command]
pub fn get_log_directory() -> Result<String, String> {
    let log_dir = logging::get_log_dir();
    Ok(log_dir.to_string_lossy().to_string())
}

/// 获取当前日志文件路径
#[tauri::command]
pub fn get_current_log_file() -> Result<String, String> {
    let log_file = logging::get_current_log_file();
    Ok(log_file.to_string_lossy().to_string())
}

/// 获取所有日志文件列表
#[tauri::command]
pub fn get_log_files() -> Result<Vec<String>, String> {
    let files = logging::get_log_files()
        .map_err(|e| format!("获取日志文件列表失败: {}", e))?;
    
    Ok(files.into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// 读取日志文件内容
#[tauri::command]
pub async fn read_log_file(file_path: String) -> Result<String, String> {
    let path = PathBuf::from(file_path);
    
    // 安全检查：确保路径在日志目录内
    let log_dir = logging::get_log_dir();
    if !path.starts_with(&log_dir) {
        return Err("无效的日志文件路径".to_string());
    }
    
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("读取日志文件失败: {}", e))
}

/// 清理旧日志文件
#[tauri::command]
pub async fn cleanup_old_logs(max_files: Option<usize>) -> Result<(), String> {
    let log_dir = logging::get_log_dir();
    let max = max_files.unwrap_or(30);
    
    // 这里重用logging模块的清理逻辑
    if !log_dir.exists() {
        return Ok(());
    }
    
    let mut log_files = Vec::new();
    
    let mut entries = tokio::fs::read_dir(&log_dir)
        .await
        .map_err(|e| format!("读取日志目录失败: {}", e))?;
    
    while let Some(entry) = entries.next_entry()
        .await
        .map_err(|e| format!("读取目录条目失败: {}", e))? {
        
        let path = entry.path();
        
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("app.log") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(created) = metadata.created() {
                            log_files.push((path, created));
                        }
                    }
                }
            }
        }
    }
    
    // 按创建时间排序，最新的在前
    log_files.sort_by(|a, b| b.1.cmp(&a.1));
    
    // 删除超过限制的文件
    for (path, _) in log_files.iter().skip(max) {
        if let Err(e) = tokio::fs::remove_file(path).await {
            tracing::warn!("删除日志文件失败 {}: {}", path.display(), e);
        } else {
            tracing::info!("已删除旧日志文件: {}", path.display());
        }
    }
    
    Ok(())
}

/// 打开日志文件夹
#[tauri::command]
pub async fn open_log_folder() -> Result<(), String> {
    let log_dir = logging::get_log_dir();
    
    // 确保日志目录存在
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)
            .map_err(|e| format!("创建日志目录失败: {}", e))?;
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&log_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }
    
    tracing::info!("已打开日志文件夹: {}", log_dir.display());
    Ok(())
}

/// 删除所有日志文件
#[tauri::command]
pub async fn delete_all_logs() -> Result<(), String> {
    let log_dir = logging::get_log_dir();
    
    if !log_dir.exists() {
        return Ok(()); // 目录不存在，认为已删除
    }
    
    let mut deleted_count = 0;
    let mut entries = tokio::fs::read_dir(&log_dir)
        .await
        .map_err(|e| format!("读取日志目录失败: {}", e))?;
    
    while let Some(entry) = entries.next_entry()
        .await
        .map_err(|e| format!("读取目录条目失败: {}", e))? {
        
        let path = entry.path();
        
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("app.log") {
                    if let Err(e) = tokio::fs::remove_file(&path).await {
                        tracing::warn!("删除日志文件失败 {}: {}", path.display(), e);
                    } else {
                        deleted_count += 1;
                        tracing::info!("已删除日志文件: {}", path.display());
                    }
                }
            }
        }
    }
    
    tracing::info!("删除操作完成，共删除 {} 个日志文件", deleted_count);
    
    // 重新激活日志系统：确保日志目录存在
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        tracing::warn!("重新创建日志目录失败: {}", e);
    }
    
    // 强制创建新的日志文件，绕过tracing_appender的缓存问题
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file_path = log_dir.join(format!("app.log.{}", today));
    
    // 手动创建日志文件来激活文件系统
    match std::fs::File::create(&log_file_path) {
        Ok(mut file) => {
            use std::io::Write;
            let init_log = format!(
                "{} INFO [日志系统重新激活] 删除所有日志文件后重新创建\n",
                chrono::Local::now().to_rfc3339()
            );
            if let Err(e) = file.write_all(init_log.as_bytes()) {
                tracing::warn!("写入初始化日志失败: {}", e);
            } else {
                tracing::info!("🔄 已手动创建新日志文件: {}", log_file_path.display());
            }
        }
        Err(e) => {
            tracing::warn!("手动创建日志文件失败: {}", e);
        }
    }
    
    // 重新激活日志文件写入器的多重策略：
    // 1. 写入多条不同级别的日志来激活所有写入器
    tracing::info!("🔄 日志系统重新激活开始...");
    tracing::warn!("⚠️  日志文件已清理，正在重新初始化写入器");
    tracing::error!("🔴 测试错误级别日志写入");
    tracing::debug!("🔧 测试调试级别日志写入");
    
    // 2. 强制刷新日志缓冲区（通过创建大量日志）
    for i in 1..=5 {
        tracing::info!("📝 重新激活日志系统 - 步骤 {}/5", i);
        // 短暂延迟让日志系统处理
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    
    tracing::info!("✅ 日志系统重新激活完成，新日志文件已创建");
    
    Ok(())
}

// 备注管理 API

#[tauri::command]
pub async fn update_item_note(app: AppHandle, item_id: i64, note: String) -> Result<(), String> {
    tracing::info!("更新条目备注: ID={}, note='{}'", item_id, note);
    
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 更新数据库中的备注
        let result = sqlx::query("UPDATE clipboard_history SET note = ? WHERE id = ?")
            .bind(&note)
            .bind(item_id)
            .execute(pool)
            .await;
            
            match result {
                Ok(query_result) => {
                    if query_result.rows_affected() > 0 {
                        tracing::info!("✅ 备注更新成功: ID={}", item_id);
                        Ok(())
                    } else {
                        let error_msg = format!("未找到ID为{}的条目", item_id);
                        tracing::warn!("❌ 备注更新失败: {}", error_msg);
                        Err(error_msg)
                    }
                }
                Err(e) => {
                    let error_msg = format!("数据库更新失败: {}", e);
                    tracing::error!("❌ 备注更新失败: {}", error_msg);
                    Err(error_msg)
                }
            }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 备注更新失败: {}", error_msg);
        Err(error_msg)
    }
}

#[tauri::command]
pub async fn get_item_note(app: AppHandle, item_id: i64) -> Result<Option<String>, String> {
    tracing::debug!("获取条目备注: ID={}", item_id);
    
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 从数据库获取备注
        let result = sqlx::query_as::<_, (Option<String>,)>("SELECT note FROM clipboard_history WHERE id = ?")
            .bind(item_id)
            .fetch_optional(pool)
            .await;
            
            match result {
                Ok(Some((note,))) => {
                    tracing::debug!("✅ 获取备注成功: ID={}, note={:?}", item_id, note);
                    Ok(note)
                }
                Ok(None) => {
                    let error_msg = format!("未找到ID为{}的条目", item_id);
                    tracing::warn!("❌ 获取备注失败: {}", error_msg);
                    Err(error_msg)
                }
                Err(e) => {
                    let error_msg = format!("数据库查询失败: {}", e);
                    tracing::error!("❌ 获取备注失败: {}", error_msg);
                    Err(error_msg)
                }
            }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 获取备注失败: {}", error_msg);
        Err(error_msg)
    }
}

// 分组管理相关命令

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub created_at: String,
    pub item_count: i64,
}

#[tauri::command]
pub async fn create_group(app: AppHandle, name: String, color: String) -> Result<Group, String> {
    tracing::info!("创建分组: name='{}', color='{}'", name, color);
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        let created_at = chrono::Utc::now().to_rfc3339();
        
        // 插入新分组
        let result = sqlx::query("INSERT INTO groups (name, color, created_at) VALUES (?, ?, ?)")
            .bind(&name)
            .bind(&color)
            .bind(&created_at)
            .execute(pool)
            .await;
            
        match result {
            Ok(_) => {
                // 获取新创建的分组ID
                let id_result = sqlx::query_as::<_, (i64,)>("SELECT last_insert_rowid()")
                    .fetch_one(pool)
                    .await;
                    
                match id_result {
                    Ok((id,)) => {
                        tracing::info!("✅ 分组创建成功: ID={}", id);
                        Ok(Group {
                            id,
                            name,
                            color,
                            created_at,
                            item_count: 0,
                        })
                    }
                    Err(e) => {
                        let error_msg = format!("获取新分组ID失败: {}", e);
                        tracing::error!("❌ 创建分组失败: {}", error_msg);
                        Err(error_msg)
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("插入分组失败: {}", e);
                tracing::error!("❌ 创建分组失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 创建分组失败: {}", error_msg);
        Err(error_msg)
    }
}

#[tauri::command]
pub async fn get_groups(app: AppHandle) -> Result<Vec<Group>, String> {
    tracing::debug!("获取所有分组");
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        let result = sqlx::query_as::<_, (i64, String, String, String, i64)>(
            "SELECT id, name, color, created_at, 
                    (SELECT COUNT(*) FROM clipboard_history WHERE group_id = groups.id) as item_count 
             FROM groups ORDER BY created_at DESC"
        )
        .fetch_all(pool)
        .await;
        
        match result {
            Ok(rows) => {
                let groups: Vec<Group> = rows.into_iter().map(|(id, name, color, created_at, item_count)| {
                    Group { id, name, color, created_at, item_count }
                }).collect();
                tracing::debug!("✅ 获取分组成功: {} 个分组", groups.len());
                Ok(groups)
            }
            Err(e) => {
                let error_msg = format!("查询分组失败: {}", e);
                tracing::error!("❌ 获取分组失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 获取分组失败: {}", error_msg);
        Err(error_msg)
    }
}

#[tauri::command]
pub async fn update_group(app: AppHandle, id: i64, name: String, color: String) -> Result<(), String> {
    tracing::info!("更新分组: ID={}, name='{}', color='{}'", id, name, color);
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        let result = sqlx::query("UPDATE groups SET name = ?, color = ? WHERE id = ?")
            .bind(&name)
            .bind(&color)
            .bind(id)
            .execute(pool)
            .await;
            
        match result {
            Ok(_) => {
                tracing::info!("✅ 分组更新成功: ID={}", id);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("更新分组失败: {}", e);
                tracing::error!("❌ 更新分组失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 更新分组失败: {}", error_msg);
        Err(error_msg)
    }
}

#[tauri::command]
pub async fn delete_group(app: AppHandle, id: i64) -> Result<(), String> {
    tracing::info!("删除分组: ID={}", id);
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 先将该分组下的所有条目的group_id设为NULL
        let update_result = sqlx::query("UPDATE clipboard_history SET group_id = NULL WHERE group_id = ?")
            .bind(id)
            .execute(pool)
            .await;
            
        if let Err(e) = update_result {
            let error_msg = format!("清除分组关联失败: {}", e);
            tracing::error!("❌ 删除分组失败: {}", error_msg);
            return Err(error_msg);
        }
        
        // 删除分组
        let result = sqlx::query("DELETE FROM groups WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await;
            
        match result {
            Ok(_) => {
                tracing::info!("✅ 分组删除成功: ID={}", id);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("删除分组失败: {}", e);
                tracing::error!("❌ 删除分组失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 删除分组失败: {}", error_msg);
        Err(error_msg)
    }
}

#[tauri::command]
pub async fn add_item_to_group(app: AppHandle, item_id: i64, group_id: Option<i64>) -> Result<(), String> {
    tracing::info!("设置条目分组: item_id={}, group_id={:?}", item_id, group_id);
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        let result = sqlx::query("UPDATE clipboard_history SET group_id = ? WHERE id = ?")
            .bind(group_id)
            .bind(item_id)
            .execute(pool)
            .await;
            
        match result {
            Ok(_) => {
                tracing::info!("✅ 条目分组设置成功: item_id={}", item_id);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("设置条目分组失败: {}", e);
                tracing::error!("❌ 设置条目分组失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 设置条目分组失败: {}", error_msg);
        Err(error_msg)
    }
}


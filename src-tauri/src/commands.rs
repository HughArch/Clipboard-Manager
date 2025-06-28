use tauri::{AppHandle, Manager};
use crate::types::{AppSettings, DatabaseState};
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
    println!("尝试注册快捷键: {}", shortcut);
    
    // 先尝试注销已有的快捷键
    let _ = app.global_shortcut().unregister_all();
    
    // 将字符串转换为 Shortcut 类型
    let shortcut_parsed = shortcut.parse::<Shortcut>().map_err(|e| {
        let error_msg = format!("Invalid hotkey format: {}. Please use format like 'Ctrl+Shift+V'", e);
        println!("快捷键解析失败: {}", error_msg);
        error_msg
    })?;
    
    // 注册快捷键
    app.global_shortcut().register(shortcut_parsed).map_err(|e| {
        let error_str = e.to_string();
        
        // 处理常见的错误类型
        if error_str.contains("HotKey already registered") || error_str.contains("already registered") {
            let friendly_msg = format!("HotKey already registered: The hotkey '{}' is already in use by another application", shortcut);
            println!("快捷键冲突: {}", friendly_msg);
            friendly_msg
        } else if error_str.contains("Invalid") || error_str.contains("invalid") {
            let friendly_msg = format!("Invalid hotkey format: '{}' is not a valid hotkey format", shortcut);
            println!("快捷键格式错误: {}", friendly_msg);
            friendly_msg
        } else {
            println!("快捷键注册失败: {}", error_str);
            format!("Failed to register hotkey '{}': {}", shortcut, error_str)
        }
    })?;
    
    println!("快捷键注册成功: {}", shortcut);
    Ok(())
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
    
    println!("🍎 macOS: 设置自启动状态: {} (应用: {})", enable, app_name);
    
    if enable {
        // 方法1: 尝试使用 Login Items (推荐方法)
        if let Err(e1) = add_to_login_items_applescript(app_name, exe_path) {
            println!("⚠️ AppleScript 方法失败: {}", e1);
            
            // 方法2: 回退到 LaunchAgent 方法
            println!("🔄 尝试 LaunchAgent 方法...");
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
    println!("🔍 macOS: 检查自启动状态: {}", app_name);
    
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
    
    println!("📁 应用 Bundle 路径: {}", app_bundle_path);
    
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
        println!("✅ AppleScript 结果: {}", result);
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
        println!("✅ 移除结果: {}", result);
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
    
    println!("✅ LaunchAgent plist 已创建: {}", plist_path.display());
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
        println!("✅ LaunchAgent plist 已删除: {}", plist_path.display());
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
    
    println!("🐧 Linux: 设置自启动状态: {} (应用: {})", enable, app_name);
    
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
        
        println!("✅ Linux: 自启动 .desktop 文件已创建: {}", desktop_path.display());
    } else {
        // 删除 .desktop 文件
        if desktop_path.exists() {
            fs::remove_file(&desktop_path)
                .map_err(|e| format!("删除 .desktop 文件失败: {}", e))?;
            
            println!("✅ Linux: 自启动 .desktop 文件已删除: {}", desktop_path.display());
        }
    }
    
    Ok(())
}

#[cfg(target_os = "linux")]
fn get_linux_auto_start_status(app_name: &str) -> Result<bool, String> {
    use std::path::Path;
    
    println!("🔍 Linux: 检查自启动状态: {}", app_name);
    
    let home_dir = std::env::var("HOME")
        .map_err(|_| "无法获取 HOME 环境变量".to_string())?;
    
    let desktop_filename = format!("{}.desktop", app_name.replace(" ", "-").to_lowercase());
    let desktop_path = Path::new(&home_dir)
        .join(".config/autostart")
        .join(&desktop_filename);
    
    let exists = desktop_path.exists();
    println!("📋 Linux: .desktop 文件状态: {}", if exists { "存在" } else { "不存在" });
    
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
pub async fn auto_paste() -> Result<(), String> {
    println!("开始执行智能自动粘贴...");
    
    // 短暂等待确保窗口已隐藏
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // 在新线程中执行粘贴操作
    let result = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            macos_simple_paste()
        }
        
        #[cfg(target_os = "windows")]
        {
            windows_auto_paste()
        }
        
        #[cfg(target_os = "linux")]
        {
            linux_auto_paste()
        }
    }).await;
    
    match result {
        Ok(Ok(())) => {
            println!("智能自动粘贴操作完成");
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

// 新增：智能粘贴功能 - 先激活指定应用，再粘贴
#[tauri::command]
pub async fn smart_paste_to_app(app_name: String, bundle_id: Option<String>) -> Result<(), String> {
    println!("开始执行智能粘贴到应用: {} (bundle: {:?})", app_name, bundle_id);
    
    // 短暂等待确保窗口已隐藏
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    
    // 克隆参数用于后续日志输出
    let app_name_for_log = app_name.clone();
    let bundle_id_clone = bundle_id.clone();
    
    // 在新线程中执行激活和粘贴操作
    let result = tokio::task::spawn_blocking(move || {
        // 先激活目标应用程序
        activate_application(&app_name, bundle_id.as_deref())?;
        
        // 短暂等待应用程序激活
        std::thread::sleep(std::time::Duration::from_millis(300));
        
        // 然后执行粘贴
        #[cfg(target_os = "macos")]
        {
            macos_simple_paste()
        }
        
        #[cfg(target_os = "windows")]
        {
            windows_auto_paste()
        }
        
        #[cfg(target_os = "linux")]
        {
            linux_auto_paste()
        }
    }).await;
    
    match result {
        Ok(Ok(())) => {
            println!("智能粘贴到应用 {} 完成", app_name_for_log);
            Ok(())
        }
        Ok(Err(e)) => {
            println!("智能粘贴失败: {}", e);
            Err(format!("粘贴操作失败: {}", e))
        }
        Err(e) => {
            println!("粘贴任务执行失败: {}", e);
            Err(format!("粘贴任务失败: {}", e))
        }
    }
}

// 激活指定的应用程序
fn activate_application(app_name: &str, bundle_id: Option<&str>) -> Result<(), String> {
    use std::process::Command;
    
    #[cfg(target_os = "macos")]
    {
        println!("🎯 macOS: 激活应用程序 {} (bundle: {:?})", app_name, bundle_id);
        
        // 方法1: 如果有 bundle_id，优先使用 bundle identifier 激活（最可靠）
        if let Some(bundle) = bundle_id {
            if !bundle.is_empty() && bundle != "missing value" {
                println!("尝试使用 bundle identifier 激活: {}", bundle);
                let script = format!(r#"tell application id "{}" to activate"#, bundle);
                let output = Command::new("osascript")
                    .arg("-e")
                    .arg(&script)
                    .output()
                    .map_err(|e| format!("使用 bundle ID 激活失败: {}", e))?;
                
                if output.status.success() {
                    println!("✅ 成功通过 bundle ID 激活应用程序: {}", app_name);
                    return Ok(());
                } else {
                    let error_msg = String::from_utf8_lossy(&output.stderr);
                    println!("⚠️ bundle ID 激活失败，尝试其他方法: {}", error_msg);
                }
            }
        }
        
        // 方法2: 使用 open 命令激活应用程序（通过应用名称）
        println!("尝试使用 open 命令激活应用程序");
        let open_output = Command::new("open")
            .arg("-a")
            .arg(app_name)
            .output()
            .map_err(|e| format!("open 命令执行失败: {}", e))?;
        
        if open_output.status.success() {
            println!("✅ 成功通过 open 命令激活应用程序: {}", app_name);
            return Ok(());
        } else {
            let open_error = String::from_utf8_lossy(&open_output.stderr);
            println!("⚠️ open 命令激活失败，尝试其他方法: {}", open_error);
        }
        
        // 方法3: 使用 System Events 通过进程名称激活
        println!("尝试使用 System Events 激活应用程序");
        let script = format!(r#"
tell application "System Events"
    set targetApp to first application process whose name is "{}"
    set frontmost of targetApp to true
end tell
        "#, app_name);
        
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("System Events 激活失败: {}", e))?;
        
        if output.status.success() {
            println!("✅ 成功通过 System Events 激活应用程序: {}", app_name);
            Ok(())
        } else {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            println!("❌ System Events 激活失败: {}", error_msg);
            
            // 方法4: 最后尝试直接通过应用名称激活
            println!("尝试直接通过应用名称激活");
            let backup_script = format!(r#"tell application "{}" to activate"#, app_name);
            let backup_output = Command::new("osascript")
                .arg("-e")
                .arg(&backup_script)
                .output()
                .map_err(|e| format!("直接激活失败: {}", e))?;
            
            if backup_output.status.success() {
                println!("✅ 成功通过直接方法激活应用程序: {}", app_name);
                Ok(())
            } else {
                let backup_error = String::from_utf8_lossy(&backup_output.stderr);
                Err(format!("所有激活方法都失败了: System Events错误: {}, 直接激活错误: {}", error_msg, backup_error))
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        println!("🎯 Windows: 激活应用程序 {}", app_name);
        // TODO: 实现 Windows 的应用程序激活
        Ok(())
    }
    
    #[cfg(target_os = "linux")]
    {
        println!("🎯 Linux: 激活应用程序 {}", app_name);
        // TODO: 实现 Linux 的应用程序激活
        Ok(())
    }
}



// macOS 使用 rdev 库进行键盘模拟（更稳定）
#[cfg(target_os = "macos")]
fn macos_simple_paste() -> Result<(), String> {
    use rdev::{simulate, EventType, Key, SimulateError};
    use std::thread;
    use std::time::Duration;
    
    println!("使用 rdev 库执行 macOS 自动粘贴...");
    
    fn send_with_delay(event_type: &EventType, delay_ms: u64) -> Result<(), SimulateError> {
        let delay = Duration::from_millis(delay_ms);
        simulate(event_type)?;
        thread::sleep(delay);
        Ok(())
    }
    
    // 鉴于时序不稳定问题，优先使用最可靠的 AppleScript 方案
    println!("🎯 开始模拟 Cmd+V 按键组合...");
    
    // 方法1: 优先使用 AppleScript（最可靠的方案）
    println!("方法1: 使用 AppleScript (最可靠)");
    let applescript_result = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to keystroke \"v\" using command down")
        .output();
    
    match applescript_result {
        Ok(output) if output.status.success() => {
            println!("✅ AppleScript 粘贴成功");
            return Ok(());
        }
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            println!("❌ AppleScript 失败: {}", error);
        }
        Err(e) => {
            println!("❌ 执行 AppleScript 失败: {}", e);
        }
    }
    
    // 方法2: rdev 备用方案（改进的时序控制）
    println!("方法2: 使用 rdev (改进时序控制)");
    let rdev_result = (|| -> Result<(), SimulateError> {
        println!("🔧 使用改进的时序控制...");
        
        // 1. 按下 Cmd 键并等待系统注册
        send_with_delay(&EventType::KeyPress(Key::MetaLeft), 150)?;
        println!("✅ Cmd键按下，等待150ms确保系统注册");
        
        // 2. 按下 V 键
        send_with_delay(&EventType::KeyPress(Key::KeyV), 50)?;
        println!("✅ V键按下");
        
        // 3. 保持一段时间让组合键生效
        thread::sleep(Duration::from_millis(100));
        println!("⏳ 保持按键状态100ms");
        
        // 4. 释放 V 键
        send_with_delay(&EventType::KeyRelease(Key::KeyV), 50)?;
        println!("✅ V键释放");
        
        // 5. 释放 Cmd 键
        send_with_delay(&EventType::KeyRelease(Key::MetaLeft), 50)?;
        println!("✅ Cmd键释放");
        
        Ok(())
    })();
    
    match rdev_result {
        Ok(()) => {
            println!("✅ rdev 方法2执行成功");
            return Ok(());
        }
        Err(e) => {
            println!("❌ rdev 方法2失败: {:?}", e);
        }
    }
    
    // 方法3: 更激进的 rdev 方案（更长延迟）
    println!("方法3: 使用 rdev (极长延迟)");
    let aggressive_result = (|| -> Result<(), SimulateError> {
        println!("🔧 使用极长延迟策略...");
        
        // 使用更长的延迟
        send_with_delay(&EventType::KeyPress(Key::MetaLeft), 300)?;
        println!("✅ Cmd键按下，等待300ms");
        
        send_with_delay(&EventType::KeyPress(Key::KeyV), 100)?;
        println!("✅ V键按下，等待100ms");
        
        // 保持更长时间
        thread::sleep(Duration::from_millis(200));
        println!("⏳ 保持按键状态200ms");
        
        send_with_delay(&EventType::KeyRelease(Key::KeyV), 100)?;
        println!("✅ V键释放，等待100ms");
        
        send_with_delay(&EventType::KeyRelease(Key::MetaLeft), 100)?;
        println!("✅ Cmd键释放");
        
        Ok(())
    })();
    
    match aggressive_result {
        Ok(()) => {
            println!("✅ rdev 方法3 (极长延迟) 执行成功");
            Ok(())
        }
        Err(e) => {
            println!("❌ 所有方法都失败了");
            Err(format!("所有键盘模拟方法都失败: 最后一个错误: {:?}", e))
        }
    }
}



// Windows 使用 rdev 库进行键盘模拟
#[cfg(target_os = "windows")]
fn windows_auto_paste() -> Result<(), String> {
    use rdev::{simulate, EventType, Key, SimulateError};
    use std::thread;
    use std::time::Duration;
    
    println!("使用 rdev 库执行 Windows 自动粘贴...");
    
    fn send(event_type: &EventType) -> Result<(), SimulateError> {
        let delay = Duration::from_millis(10);
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
    
    println!("rdev Windows 粘贴操作执行完成");
    Ok(())
}

// Linux 使用 rdev 库进行键盘模拟
#[cfg(target_os = "linux")]
fn linux_auto_paste() -> Result<(), String> {
    use rdev::{simulate, EventType, Key, SimulateError};
    use std::thread;
    use std::time::Duration;
    
    println!("使用 rdev 库执行 Linux 自动粘贴...");
    
    fn send(event_type: &EventType) -> Result<(), SimulateError> {
        let delay = Duration::from_millis(10);
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
    
    println!("rdev Linux 粘贴操作执行完成");
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


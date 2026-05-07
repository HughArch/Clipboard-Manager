use tauri::{AppHandle, Manager, Emitter};
use crate::types::{AppSettings, DatabaseState};
use crate::logging;
use std::fs;
use std::path::PathBuf;
use std::io::{Write, Read};
use dirs_next::config_dir;
use base64::{engine::general_purpose, Engine as _};
use tauri_plugin_global_shortcut::{self, GlobalShortcutExt, Shortcut};
use std::env;
use chrono;
use tokio;
use tokio::sync::Mutex;
use sqlx::{self, Row};
use image::{ImageFormat, imageops::FilterType};
use zip::{ZipWriter, ZipArchive, write::SimpleFileOptions};
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

// Windows 注册表操作（使用 Windows API，避免 CMD 弹窗）
#[cfg(target_os = "windows")]
fn set_windows_auto_start(enable: bool, app_name: &str, exe_path: &PathBuf) -> Result<(), String> {
    use winapi::um::winreg::{
        RegCreateKeyExW, RegSetValueExW, RegDeleteValueW, RegCloseKey,
        HKEY_CURRENT_USER
    };
    use winapi::um::winnt::{KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ};
    use winapi::shared::winerror::{ERROR_SUCCESS, ERROR_FILE_NOT_FOUND};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    
    tracing::debug!("🪟 Windows: 设置自启动状态: {} (应用: {})", enable, app_name);
    
    // 转换路径为 UTF-16
    let subkey_path = OsStr::new(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    
    let value_name = OsStr::new(app_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    
    unsafe {
        let mut hkey = ptr::null_mut();
        
        // 打开或创建注册表键
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_path.as_ptr(),
            0,
            ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            ptr::null_mut(),
            &mut hkey,
            ptr::null_mut(),
        );
        
        if result != ERROR_SUCCESS as i32 {
            return Err(format!("无法打开注册表键: 错误代码 {}", result));
        }
        
        let final_result = if enable {
            // 添加启动项
            let exe_path_str = format!("\"{}\"", exe_path.display());
            let exe_path_wide = OsStr::new(&exe_path_str)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>();
            
            tracing::debug!("📝 添加注册表值: {} = {}", app_name, exe_path_str);
            
            let set_result = RegSetValueExW(
                hkey,
                value_name.as_ptr(),
                0,
                REG_SZ,
                exe_path_wide.as_ptr() as *const u8,
                (exe_path_wide.len() * 2) as u32,
            );
            
            if set_result == ERROR_SUCCESS as i32 {
                tracing::info!("✅ Windows: 成功添加自启动项");
                Ok(())
            } else {
                Err(format!("设置注册表值失败: 错误代码 {}", set_result))
            }
        } else {
            // 移除启动项
            tracing::debug!("🗑️ 删除注册表值: {}", app_name);
            
            let delete_result = RegDeleteValueW(hkey, value_name.as_ptr());
            
            if delete_result == ERROR_SUCCESS as i32 {
                tracing::info!("✅ Windows: 成功移除自启动项");
                Ok(())
            } else if delete_result == ERROR_FILE_NOT_FOUND as i32 {
                tracing::info!("ℹ️ Windows: 自启动项不存在，无需移除");
                Ok(()) // 不存在也算成功
            } else {
                Err(format!("删除注册表值失败: 错误代码 {}", delete_result))
            }
        };
        
        // 关闭注册表键
        RegCloseKey(hkey);
        
        final_result
    }
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

// 检查 Windows 自启动状态（使用 Windows API）
#[cfg(target_os = "windows")]
fn get_windows_auto_start_status(app_name: &str) -> Result<bool, String> {
    use winapi::um::winreg::{
        RegOpenKeyExW, RegQueryValueExW, RegCloseKey,
        HKEY_CURRENT_USER
    };
    use winapi::um::winnt::KEY_READ;
    use winapi::shared::winerror::{ERROR_SUCCESS, ERROR_FILE_NOT_FOUND};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    
    tracing::debug!("🔍 Windows: 检查自启动状态: {}", app_name);
    
    // 转换路径为 UTF-16
    let subkey_path = OsStr::new(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    
    let value_name = OsStr::new(app_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    
    unsafe {
        let mut hkey = ptr::null_mut();
        
        // 打开注册表键
        let open_result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_path.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        );
        
        if open_result != ERROR_SUCCESS as i32 {
            tracing::debug!("📋 Windows: 无法打开注册表键，自启动未启用");
            return Ok(false);
        }
        
        // 查询值是否存在
        let mut value_type = 0u32;
        let mut data_size = 0u32;
        
        let query_result = RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            ptr::null_mut(),
            &mut value_type,
            ptr::null_mut(),
            &mut data_size,
        );
        
        // 关闭注册表键
        RegCloseKey(hkey);
        
        let exists = query_result == ERROR_SUCCESS as i32;
        
        if exists {
            tracing::info!("✅ Windows: 自启动已启用");
        } else {
            tracing::debug!("📋 Windows: 自启动未启用");
        }
        
        Ok(exists)
    }
}

// 非 Windows 系统的占位实现
#[cfg(not(target_os = "windows"))]
fn get_windows_auto_start_status(_app_name: &str) -> Result<bool, String> {
    Ok(false) // 非Windows系统默认返回false
}

// ==================== macOS 自启动实现 ====================

#[cfg(target_os = "macos")]
fn set_macos_auto_start(enable: bool, app_name: &str, bundle_id: &str, exe_path: &PathBuf) -> Result<(), String> {
    tracing::debug!("🍎 macOS: 设置自启动状态: {} (应用: {})", enable, app_name);
    
    if enable {
        // 清理可能存在的旧配置
        let _ = remove_from_login_items_applescript(app_name);
        let _ = remove_from_launch_agent(bundle_id);
        
        // 优先使用 Login Items (系统偏好设置中可见，用户体验更好)
        match add_to_login_items_applescript(app_name, exe_path) {
            Ok(_) => {
                tracing::info!("✅ 成功使用 Login Items 设置自启动");
                Ok(())
            }
            Err(e1) => {
                tracing::warn!("⚠️ Login Items 方法失败: {}", e1);
                
                // 回退到 LaunchAgent 方法
                tracing::debug!("🔄 尝试 LaunchAgent 方法...");
                match add_to_launch_agent(app_name, bundle_id, exe_path) {
                    Ok(_) => {
                        tracing::info!("✅ 成功使用 LaunchAgent 设置自启动");
                        Ok(())
                    }
                    Err(e2) => {
                        let error_msg = format!("所有自启动方法都失败了 - Login Items: {}, LaunchAgent: {}", e1, e2);
                        tracing::error!("❌ {}", error_msg);
                        Err(error_msg)
                    }
                }
            }
        }
    } else {
        // 移除自启动：尝试两种方法，确保彻底清理
        let login_result = remove_from_login_items_applescript(app_name);
        let agent_result = remove_from_launch_agent(bundle_id);
        
        // 只要有一个成功就认为移除成功
        match (login_result, agent_result) {
            (Ok(_), _) | (_, Ok(_)) => {
                tracing::info!("✅ 成功移除自启动配置");
                Ok(())
            }
            (Err(e1), Err(e2)) => {
                tracing::warn!("⚠️ 移除自启动时出现错误 - Login Items: {}, LaunchAgent: {}", e1, e2);
                // 移除操作即使失败也不报错，因为可能本来就没有配置
                Ok(())
            }
        }
    }
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

// 使用 AppleScript 添加到登录项（优化版本）
#[cfg(target_os = "macos")]
fn add_to_login_items_applescript(app_name: &str, exe_path: &PathBuf) -> Result<(), String> {
    use std::process::Command;
    
    // 获取应用程序的 .app bundle 路径
    let app_bundle_path = get_app_bundle_path(exe_path)?;
    
    tracing::debug!("📁 应用 Bundle 路径: {}", app_bundle_path);
    
    // 使用更简单和可靠的 AppleScript
    let script = format!(r#"
tell application "System Events"
    try
        -- 检查应用是否已经在登录项中
        set loginItems to login items
        repeat with loginItem in loginItems
            if path of loginItem is "{}" then
                return "ALREADY_EXISTS"
            end if
        end repeat
        
        -- 添加到登录项，设置为隐藏启动
        make login item at end with properties {{path:"{}", hidden:true}}
        return "SUCCESS_ADDED"
    on error errMsg
        return "ERROR: " & errMsg
    end try
end tell
    "#, app_bundle_path, app_bundle_path);
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!("✅ AppleScript 结果: {}", result);
        
        if result.starts_with("ERROR:") {
            return Err(format!("AppleScript 错误: {}", result));
        }
        
        Ok(())
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("AppleScript 执行失败: {}", error_msg))
    }
}

// 获取 .app bundle 路径的辅助函数
#[cfg(target_os = "macos")]
fn get_app_bundle_path(exe_path: &PathBuf) -> Result<String, String> {
    let path_str = exe_path.to_string_lossy();
    
    // 如果是开发环境或者直接的可执行文件
    if path_str.contains("/target/debug/") || path_str.contains("/target/release/") {
        // 开发环境，尝试找到 .app bundle
        if let Some(app_end) = path_str.find(".app/Contents/MacOS/") {
            return Ok(format!("{}.app", &path_str[..app_end]));
        }
        // 如果找不到 .app，可能是开发环境，返回错误让其使用 LaunchAgent
        return Err("开发环境，无法找到 .app bundle".to_string());
    }
    
    // 生产环境，应该在 .app bundle 内
    if let Some(app_end) = path_str.find(".app/Contents/MacOS/") {
        Ok(format!("{}.app", &path_str[..app_end]))
    } else {
        // 如果不在 .app bundle 内，可能是直接的可执行文件
        Err("不在 .app bundle 内，使用 LaunchAgent 方法".to_string())
    }
}

// 使用 AppleScript 从登录项移除（优化版本）
#[cfg(target_os = "macos")]
fn remove_from_login_items_applescript(app_name: &str) -> Result<(), String> {
    use std::process::Command;
    
    // 更灵活的移除脚本，支持按名称和路径匹配
    let script = format!(r#"
tell application "System Events"
    try
        set loginItems to login items
        set itemsToDelete to {{}}
        
        -- 收集需要删除的项目
        repeat with loginItem in loginItems
            set itemName to name of loginItem
            set itemPath to path of loginItem
            
            -- 按名称匹配或路径包含应用名称
            if itemName is "{}" or itemPath contains "{}" or itemPath contains "Clipboard" then
                set end of itemsToDelete to loginItem
            end if
        end repeat
        
        -- 删除匹配的项目
        repeat with itemToDelete in itemsToDelete
            delete itemToDelete
        end repeat
        
        if (count of itemsToDelete) > 0 then
            return "SUCCESS_REMOVED"
        else
            return "NOT_FOUND"
        end if
        
    on error errMsg
        return "ERROR: " & errMsg
    end try
end tell
    "#, app_name, app_name);
    
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
    
    // 尝试使用 .app bundle 路径，如果失败则使用可执行文件路径
    let launch_path = match get_app_bundle_path(exe_path) {
        Ok(app_bundle) => {
            tracing::info!("✅ 使用 .app bundle 路径: {}", app_bundle);
            app_bundle
        }
        Err(_) => {
            tracing::warn!("⚠️ 无法获取 .app bundle，使用可执行文件路径");
            exe_path.to_string_lossy().to_string()
        }
    };

    let plist_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>open</string>
        <string>-a</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>LaunchOnlyOnce</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardOutPath</key>
    <string>/dev/null</string>
    <key>StandardErrorPath</key>
    <string>/dev/null</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
</dict>
</plist>"#, bundle_id, launch_path);
    
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
        lan_queue_role: "off".to_string(),
        lan_queue_host: String::new(),
        lan_queue_port: 21991,
        lan_queue_password: String::new(),
        lan_queue_name: "LAN Queue".to_string(),
        lan_queue_member_name: String::new(),
        theme: "light".to_string(),
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

    // 先释放可能被用户按住的修饰键（Shift），防止干扰 Ctrl+V 模拟
    send(&EventType::KeyRelease(Key::ShiftLeft))
        .map_err(|e| format!("释放 Shift 键失败: {:?}", e))?;
    send(&EventType::KeyRelease(Key::ShiftRight))
        .map_err(|e| format!("释放 Shift 键失败: {:?}", e))?;

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

    // 先释放可能被用户按住的修饰键（Shift），防止干扰 Ctrl+V 模拟
    send(&EventType::KeyRelease(Key::ShiftLeft))
        .map_err(|e| format!("释放 Shift 键失败: {:?}", e))?;
    send(&EventType::KeyRelease(Key::ShiftRight))
        .map_err(|e| format!("释放 Shift 键失败: {:?}", e))?;

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
pub async fn save_clipboard_image(base64_data: String) -> Result<String, String> {
    // 1. 解析base64数据
    // 处理可能的前缀 "data:image/png;base64,"
    let base64_start = base64_data.find("base64,").map(|i| i + 7).unwrap_or(0);
    let base64_str = &base64_data[base64_start..];

    // 2. 解码base64
    let image_bytes = general_purpose::STANDARD
        .decode(base64_str)
        .map_err(|e| format!("base64解码失败: {}", e))?;

    // 3. 获取图片信息（宽度、高度、大小）
    let (width, height, format) = match image::load_from_memory(&image_bytes) {
        Ok(img) => {
            let width = img.width();
            let height = img.height();
            let format = match img {
                image::DynamicImage::ImageLuma8(_) => "Luma8",
                image::DynamicImage::ImageLumaA8(_) => "LumaA8",
                image::DynamicImage::ImageRgb8(_) => "RGB8",
                image::DynamicImage::ImageRgba8(_) => "RGBA8",
                image::DynamicImage::ImageLuma16(_) => "Luma16",
                image::DynamicImage::ImageLumaA16(_) => "LumaA16",
                image::DynamicImage::ImageRgb16(_) => "RGB16",
                image::DynamicImage::ImageRgba16(_) => "RGBA16",
                image::DynamicImage::ImageRgb32F(_) => "RGB32F",
                image::DynamicImage::ImageRgba32F(_) => "RGBA32F",
                _ => "Unknown"
            };
            (width, height, format)
        }
        Err(_) => (0, 0, "Unknown"),
    };

    // 4. 获取图片目录
    let images_dir = get_app_images_dir()?;

    // 5. 生成文件名 (使用时间戳)
    let timestamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let filename = format!("img_{}.png", timestamp);
    let file_path = images_dir.join(&filename);
    println!("保存图片到: {:?}", file_path);

    // 6. 保存文件
    std::fs::write(&file_path, &image_bytes)
        .map_err(|e| format!("写入图片文件失败: {}", e))?;

    // 7. 构建元数据 JSON
    let metadata = serde_json::json!({
        "width": width,
        "height": height,
        "size": image_bytes.len(),
        "format": format
    });

    // 8. 构建返回结果
    let result = serde_json::json!({
        "path": file_path.to_string_lossy().to_string(),
        "metadata": metadata
    });

    // 9. 返回包含路径和元数据的JSON对象
    Ok(result.to_string())
}

#[tauri::command]
pub async fn get_image_metadata(image_path: String) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(&image_path);

    // 检查文件是否存在
    if !path.exists() {
        return Err("图片文件不存在".to_string());
    }

    // 读取图片文件
    let image_data = std::fs::read(&path)
        .map_err(|e| format!("无法读取图片文件: {}", e))?;

    // 获取图片信息（宽度、高度、大小）
    let (width, height, format) = match image::load_from_memory(&image_data) {
        Ok(img) => {
            let width = img.width();
            let height = img.height();
            let format = match img {
                image::DynamicImage::ImageLuma8(_) => "Luma8",
                image::DynamicImage::ImageLumaA8(_) => "LumaA8",
                image::DynamicImage::ImageRgb8(_) => "RGB8",
                image::DynamicImage::ImageRgba8(_) => "RGBA8",
                image::DynamicImage::ImageLuma16(_) => "Luma16",
                image::DynamicImage::ImageLumaA16(_) => "LumaA16",
                image::DynamicImage::ImageRgb16(_) => "RGB16",
                image::DynamicImage::ImageRgba16(_) => "RGBA16",
                image::DynamicImage::ImageRgb32F(_) => "RGB32F",
                image::DynamicImage::ImageRgba32F(_) => "RGBA32F",
                _ => "Unknown"
            };
            (width, height, format)
        }
        Err(_) => (0, 0, "Unknown"),
    };

    // 构建元数据 JSON
    let metadata = serde_json::json!({
        "width": width,
        "height": height,
        "size": image_data.len(),
        "format": format
    });

    Ok(metadata)
}

#[tauri::command]
pub async fn copy_image_to_clipboard(image_path: String) -> Result<(), String> {
    let start = std::time::Instant::now();
    tracing::info!("复制图片到剪贴板: {}", image_path);
    
    let path = PathBuf::from(&image_path);
    if !path.exists() {
        return Err(format!("图片文件不存在: {}", image_path));
    }

    #[cfg(target_os = "windows")]
    {
        // Windows 优化：使用文件复制 (CF_HDROP) 代替图片数据写入
        // 这避免了昂贵的解码和位图转换操作，耗时通常 < 10ms
        use clipboard_win::{formats, Clipboard, Setter};

        let _clip = Clipboard::new_attempts(10)
            .map_err(|e| format!("无法打开剪贴板: {}", e))?;

        // 清空剪贴板，确保之前的文本等内容被清除
        // clipboard_win 的 set_file_list 默认使用 NoClear，不会自动清空
        clipboard_win::raw::empty()
            .map_err(|e| format!("清空剪贴板失败: {}", e))?;

        // 设置文件列表 (CF_HDROP)
        let paths = vec![image_path.clone()];

        // 使用 formats::FileList
        formats::FileList.write_clipboard(&paths)
            .map_err(|e| format!("设置剪贴板文件失败: {}", e))?;
            
        tracing::info!("✅ 图片以文件形式写入剪贴板 (Windows CF_HDROP), 耗时: {:?}", start.elapsed());
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        // 其他平台继续使用 arboard 处理图像数据
        
        // 读取图片文件
        let read_start = std::time::Instant::now();
        let image_bytes = std::fs::read(&path)
            .map_err(|e| format!("读取图片文件失败: {}", e))?;
        tracing::debug!("读取文件耗时: {:?}", read_start.elapsed());
            
        // 解码图片
        let decode_start = std::time::Instant::now();
        let img = image::load_from_memory(&image_bytes)
            .map_err(|e| format!("解码图片失败: {}", e))?;
        tracing::debug!("解码图片耗时: {:?}", decode_start.elapsed());
        
        let rgba8 = img.to_rgba8();
        let (width, height) = rgba8.dimensions();
        let image_data = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Borrowed(&rgba8),
        };
        
        // 使用 arboard 写入剪贴板
        let write_start = std::time::Instant::now();
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| format!("初始化剪贴板失败: {}", e))?;
            
        clipboard.set_image(image_data)
            .map_err(|e| format!("写入剪贴板失败: {}", e))?;
        tracing::debug!("写入剪贴板耗时: {:?}", write_start.elapsed());
            
        tracing::info!("✅ 图片成功写入剪贴板 (Rust Arboard), 总耗时: {:?}", start.elapsed());
        Ok(())
    }
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
pub async fn delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    tracing::info!("删除条目: ID={}", id);
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 1. 获取条目信息，检查是否有图片文件
        let result = sqlx::query_as::<_, (Option<String>,)>("SELECT image_path FROM clipboard_history WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await;
            
        if let Ok(Some((Some(image_path),))) = result {
            // 如果有图片文件，尝试删除
            let path = PathBuf::from(&image_path);
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("删除图片文件失败: {} ({})", image_path, e);
                } else {
                    tracing::info!("已删除图片文件: {}", image_path);
                }
            }
        }
        
        // 2. 从数据库删除记录
        let delete_result = sqlx::query("DELETE FROM clipboard_history WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await;
            
        match delete_result {
            Ok(_) => {
                tracing::info!("✅ 条目删除成功: ID={}", id);
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("数据库删除失败: {}", e);
                tracing::error!("❌ 删除条目失败: {}", error_msg);
                Err(error_msg)
            }
        }
    } else {
        let error_msg = "无法获取数据库状态".to_string();
        tracing::error!("❌ 删除条目失败: {}", error_msg);
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

// ===== 文件剪贴板相关命令 =====

/// 文件元信息结构
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct FileMetadata {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub exists: bool,
    pub is_directory: bool,
    pub modified_time: String, // ISO 8601 string
}

/// 复制文件到剪贴板 (Windows CF_HDROP)
#[tauri::command]
pub async fn copy_files_to_clipboard(file_paths: Vec<String>) -> Result<(), String> {
    let start = std::time::Instant::now();
    tracing::info!("复制文件到剪贴板: {:?}", file_paths);
    
    if file_paths.is_empty() {
        return Err("文件路径列表为空".to_string());
    }
    
    // 验证所有文件是否存在
    for path_str in &file_paths {
        let path = PathBuf::from(path_str);
        if !path.exists() {
            return Err(format!("文件不存在: {}", path_str));
        }
    }

    #[cfg(target_os = "windows")]
    {
        use clipboard_win::{formats, Clipboard, Setter};

        let _clip = Clipboard::new_attempts(10)
            .map_err(|e| format!("无法打开剪贴板: {}", e))?;

        // 清空剪贴板，确保之前的内容被清除
        clipboard_win::raw::empty()
            .map_err(|e| format!("清空剪贴板失败: {}", e))?;

        // 设置文件列表 (CF_HDROP)
        formats::FileList.write_clipboard(&file_paths)
            .map_err(|e| format!("设置剪贴板文件失败: {}", e))?;

        tracing::info!("✅ 文件已写入剪贴板 (Windows CF_HDROP), 文件数: {}, 耗时: {:?}",
            file_paths.len(), start.elapsed());
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        // 其他平台暂不支持文件剪贴板
        Err("文件剪贴板功能目前仅支持 Windows 平台".to_string())
    }
}

/// 获取文件元信息
#[tauri::command]
pub async fn get_file_metadata(file_path: String) -> Result<FileMetadata, String> {
    let path = PathBuf::from(&file_path);
    
    let name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
        
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    
    let exists = path.exists();
    let is_directory = path.is_dir();
    
    let size = if exists && !is_directory {
        std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };
    
    let modified_time = if exists {
        std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "".to_string())
    } else {
        "".to_string()
    };
    
    Ok(FileMetadata {
        path: file_path,
        name,
        extension,
        size,
        exists,
        is_directory,
        modified_time,
    })
}

/// 批量获取文件元信息
#[tauri::command]
pub async fn get_files_metadata(file_paths: Vec<String>) -> Result<Vec<FileMetadata>, String> {
    let mut results = Vec::with_capacity(file_paths.len());
    
    for file_path in file_paths {
        let metadata = get_file_metadata(file_path).await?;
        results.push(metadata);
    }
    
    Ok(results)
}

/// 检查文件是否存在
#[tauri::command]
pub async fn check_files_exist(file_paths: Vec<String>) -> Result<Vec<bool>, String> {
    let results: Vec<bool> = file_paths.iter()
        .map(|p| PathBuf::from(p).exists())
        .collect();
    
    Ok(results)
}

/// 获取文件图标 (Windows Shell API)
#[tauri::command]
pub async fn get_file_icon(file_path: String) -> Result<String, String> {
    let path = PathBuf::from(&file_path);
    
    // 如果文件不存在，尝试根据扩展名获取图标
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    #[cfg(target_os = "windows")]
    {
        use std::ptr::null_mut;
        use winapi::um::shellapi::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES};
        use winapi::um::winuser::{DestroyIcon, GetIconInfo, ICONINFO};
        use winapi::um::wingdi::{GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DeleteObject, GetObjectW, BITMAP};
        use winapi::shared::windef::HBITMAP;
        use winapi::um::winuser::GetDC;
        use winapi::um::wingdi::CreateCompatibleDC;
        use winapi::um::wingdi::SelectObject;
        use winapi::um::wingdi::DeleteDC;
        use winapi::um::winuser::ReleaseDC;
        use winapi::shared::minwindef::DWORD;
        
        // 将路径转换为宽字符
        let wide_path: Vec<u16> = file_path.encode_utf16().chain(std::iter::once(0)).collect();
        
        let mut shfi: SHFILEINFOW = unsafe { std::mem::zeroed() };
        
        // 如果文件存在，直接获取图标；否则使用扩展名
        let flags = if path.exists() {
            SHGFI_ICON | SHGFI_SMALLICON
        } else {
            SHGFI_ICON | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES
        };
        
        let result = unsafe {
            SHGetFileInfoW(
                wide_path.as_ptr(),
                0x80, // FILE_ATTRIBUTE_NORMAL
                &mut shfi,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        
        if result == 0 || shfi.hIcon.is_null() {
            // 返回默认图标占位符
            return Ok(get_default_file_icon(extension));
        }
        
        // 获取图标信息
        let mut icon_info: ICONINFO = unsafe { std::mem::zeroed() };
        let got_info = unsafe { GetIconInfo(shfi.hIcon, &mut icon_info) };
        
        if got_info == 0 {
            unsafe { DestroyIcon(shfi.hIcon) };
            return Ok(get_default_file_icon(extension));
        }
        
        // 获取位图信息
        let hbm_color = icon_info.hbmColor;
        if hbm_color.is_null() {
            unsafe {
                if !icon_info.hbmMask.is_null() { DeleteObject(icon_info.hbmMask as _); }
                DestroyIcon(shfi.hIcon);
            }
            return Ok(get_default_file_icon(extension));
        }
        
        // 获取位图尺寸
        let mut bm: BITMAP = unsafe { std::mem::zeroed() };
        unsafe {
            GetObjectW(
                hbm_color as _,
                std::mem::size_of::<BITMAP>() as i32,
                &mut bm as *mut _ as _,
            );
        }
        
        let width = bm.bmWidth as usize;
        let height = bm.bmHeight as usize;
        
        if width == 0 || height == 0 {
            unsafe {
                if !icon_info.hbmMask.is_null() { DeleteObject(icon_info.hbmMask as _); }
                DeleteObject(hbm_color as _);
                DestroyIcon(shfi.hIcon);
            }
            return Ok(get_default_file_icon(extension));
        }
        
        // 准备位图信息头
        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // 负数表示自上而下
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        
        // 分配像素缓冲区
        let mut pixels: Vec<u8> = vec![0u8; width * height * 4];
        
        // 获取设备上下文
        let hdc_screen = unsafe { GetDC(null_mut()) };
        let hdc_mem = unsafe { CreateCompatibleDC(hdc_screen) };
        let old_bmp = unsafe { SelectObject(hdc_mem, hbm_color as _) };
        
        // 读取位图数据
        unsafe {
            GetDIBits(
                hdc_mem,
                hbm_color,
                0,
                height as u32,
                pixels.as_mut_ptr() as _,
                &mut bmi,
                0, // DIB_RGB_COLORS
            );
        }
        
        // 清理资源
        unsafe {
            SelectObject(hdc_mem, old_bmp);
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
            if !icon_info.hbmMask.is_null() { DeleteObject(icon_info.hbmMask as _); }
            DeleteObject(hbm_color as _);
            DestroyIcon(shfi.hIcon);
        }
        
        // BGRA -> RGBA 转换
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2); // B <-> R
        }
        
        // 编码为 PNG
        let png_data = encode_rgba_to_png(&pixels, width as u32, height as u32)?;
        
        // 转换为 base64 data URL
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
        let data_url = format!("data:image/png;base64,{}", b64);
        
        return Ok(data_url);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // 非 Windows 平台返回默认图标
        Ok(get_default_file_icon(extension))
    }
}

/// 根据扩展名返回默认图标（SVG data URL）
fn get_default_file_icon(extension: &str) -> String {
    // 使用简单的文件图标 SVG - 所有类型使用相同的基础图标，颜色不同
    let (color, label) = match extension.to_lowercase().as_str() {
        "pdf" => ("E53935", "PDF"),
        "doc" | "docx" => ("1976D2", "DOC"),
        "xls" | "xlsx" => ("388E3C", "XLS"),
        "ppt" | "pptx" => ("D84315", "PPT"),
        "zip" | "rar" | "7z" | "tar" | "gz" => ("FFA000", "ZIP"),
        "mp3" | "wav" | "flac" | "aac" | "ogg" => ("7B1FA2", "♪"),
        "mp4" | "avi" | "mkv" | "mov" | "wmv" => ("C62828", "▶"),
        "exe" | "msi" => ("455A64", "EXE"),
        "txt" | "md" | "log" => ("616161", "TXT"),
        "js" | "ts" | "jsx" | "tsx" => ("F7DF1E", "JS"),
        "py" => ("3776AB", "PY"),
        "rs" => ("DEA584", "RS"),
        "html" | "htm" => ("E34F26", "HTML"),
        "css" | "scss" | "sass" => ("1572B6", "CSS"),
        "json" => ("000000", "{ }"),
        "xml" => ("F16529", "XML"),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => ("4CAF50", "IMG"),
        _ => ("757575", "FILE"),
    };
    
    // 构建简单的 SVG 文件图标
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#{}" d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zm4 18H6V4h7v5h5v11z"/><text x="12" y="16" text-anchor="middle" font-size="4" fill="#{}">{}</text></svg>"##,
        color, color, label
    );
    
    let encoded = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
    format!("data:image/svg+xml;base64,{}", encoded)
}

/// 将 RGBA 像素数据编码为 PNG
fn encode_rgba_to_png(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::{ImageBuffer, RgbaImage, ImageEncoder, ColorType};
    
    let img: RgbaImage = ImageBuffer::from_raw(width, height, pixels.to_vec())
        .ok_or_else(|| "无法创建图片缓冲区".to_string())?;
    
    let mut png_data = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
    
    encoder.write_image(
        img.as_raw(),
        width,
        height,
        ColorType::Rgba8,
    ).map_err(|e| format!("PNG 编码失败: {}", e))?;
    
    Ok(png_data)
}

/// 打开文件所在文件夹并选中文件 (Windows Explorer)
#[tauri::command]
pub async fn open_file_location(file_path: String) -> Result<(), String> {
    let path = PathBuf::from(&file_path);
    
    if !path.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: 使用 explorer /select, 命令
        std::process::Command::new("explorer")
            .args(["/select,", &file_path])
            .spawn()
            .map_err(|e| format!("打开文件位置失败: {}", e))?;
        
        tracing::info!("✅ 已打开文件位置: {}", file_path);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: 使用 open -R 命令
        std::process::Command::new("open")
            .args(["-R", &file_path])
            .spawn()
            .map_err(|e| format!("打开文件位置失败: {}", e))?;
        
        tracing::info!("✅ 已打开文件位置: {}", file_path);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 尝试使用 xdg-open 打开父目录
        if let Some(parent) = path.parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("打开文件位置失败: {}", e))?;
            
            tracing::info!("✅ 已打开文件位置: {}", file_path);
            return Ok(());
        }
        return Err("无法获取文件父目录".to_string());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err("不支持的操作系统".to_string())
    }
}

/// 读取文本文件内容
#[tauri::command]
pub async fn read_text_file(file_path: String) -> Result<String, String> {
    let start = std::time::Instant::now();
    tracing::info!("读取文本文件: {}", file_path);

    // 验证文件存在
    let path = std::path::PathBuf::from(&file_path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }

    // 验证是文件而不是目录
    if path.is_dir() {
        return Err("无法读取目录内容".to_string());
    }

    // 检查文件大小（限制为 10MB）
    let metadata = std::fs::metadata(&path)
        .map_err(|e| format!("无法获取文件元数据: {}", e))?;
    
    if metadata.len() > 10 * 1024 * 1024 {
        return Err("文件过大（超过10MB），无法预览".to_string());
    }

    // 读取文件内容
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取文件失败: {}", e))?;

    tracing::info!("✅ 文本文件读取成功，大小: {} 字节，耗时: {:?}",
        content.len(), start.elapsed());

    Ok(content)
}

// ==================== 数据导入导出 ====================

#[tauri::command]
pub async fn export_data(app: AppHandle, export_path: String) -> Result<(), String> {
    tracing::info!("开始导出数据到: {}", export_path);

    let export_path = PathBuf::from(&export_path);
    let file = fs::File::create(&export_path)
        .map_err(|e| format!("无法创建导出文件: {}", e))?;

    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // 1. 写入数据库文件
    let app_data_dir = app.path().app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {}", e))?;
    let db_path = app_data_dir.join("clipboard.db");

    if db_path.exists() {
        let db_bytes = fs::read(&db_path)
            .map_err(|e| format!("无法读取数据库文件: {}", e))?;
        zip.start_file("clipboard.db", options)
            .map_err(|e| format!("写入数据库到zip失败: {}", e))?;
        zip.write_all(&db_bytes)
            .map_err(|e| format!("写入数据库数据失败: {}", e))?;
        tracing::info!("已写入数据库文件 ({} 字节)", db_bytes.len());
    } else {
        return Err("数据库文件不存在".to_string());
    }

    // 2. 写入图片文件 - 从数据库查询所有图片路径
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;

        let image_rows = sqlx::query("SELECT DISTINCT image_path FROM clipboard_history WHERE image_path IS NOT NULL")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

        let mut image_count = 0u64;
        let mut image_size = 0u64;

        for row in &image_rows {
            if let Ok(Some(image_path)) = row.try_get::<Option<String>, &str>("image_path") {
                let path = PathBuf::from(&image_path);
                if path.exists() && path.is_file() {
                    if let Some(filename) = path.file_name() {
                        let zip_path = format!("images/{}", filename.to_string_lossy());
                        match fs::read(&path) {
                            Ok(file_bytes) => {
                                if let Err(e) = zip.start_file(&zip_path, options) {
                                    tracing::warn!("写入图片到zip失败: {}", e);
                                    continue;
                                }
                                if let Err(e) = zip.write_all(&file_bytes) {
                                    tracing::warn!("写入图片数据失败: {}", e);
                                    continue;
                                }
                                image_count += 1;
                                image_size += file_bytes.len() as u64;
                            }
                            Err(e) => {
                                tracing::warn!("无法读取图片 {}: {}", image_path, e);
                            }
                        }
                    }
                } else {
                    tracing::warn!("图片文件不存在，跳过: {}", image_path);
                }
            }
        }
        drop(db_guard);
        tracing::info!("已写入 {} 个图片文件 ({} 字节)", image_count, image_size);
    } else {
        tracing::warn!("无法获取数据库状态，跳过图片导出");
    }

    zip.finish().map_err(|e| format!("完成zip写入失败: {}", e))?;

    tracing::info!("✅ 数据导出完成");
    Ok(())
}

/// 从 zip 中同步提取所有需要的数据（避免 ZipFile 跨 await 的 Send 问题）
struct ExtractedZipData {
    db_bytes: Vec<u8>,
    images: Vec<(String, Vec<u8>)>, // (filename, bytes)
}

fn extract_zip_data(import_path: &PathBuf) -> Result<ExtractedZipData, String> {
    let file = fs::File::open(import_path)
        .map_err(|e| format!("无法打开导入文件: {}", e))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| format!("无法读取zip文件: {}", e))?;

    // 提取数据库文件
    let mut db_file = archive.by_name("clipboard.db")
        .map_err(|e| format!("zip中未找到数据库文件: {}", e))?;
    let mut db_bytes = Vec::new();
    std::io::Read::read_to_end(&mut db_file, &mut db_bytes)
        .map_err(|e| format!("无法读取数据库数据: {}", e))?;
    drop(db_file);

    // 提取所有图片文件
    let mut images = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("读取zip条目失败: {}", e))?;
        let entry_path = entry.name().to_string();

        if entry_path.starts_with("images/") && !entry.is_dir() {
            if let Some(filename) = std::path::Path::new(&entry_path).file_name() {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf)
                    .map_err(|e| format!("无法读取图片 {}: {}", entry_path, e))?;
                images.push((filename.to_string_lossy().to_string(), buf));
            }
        }
    }

    Ok(ExtractedZipData { db_bytes, images })
}

#[tauri::command]
pub async fn import_data(app: AppHandle, import_path: String, mode: String) -> Result<(), String> {
    tracing::info!("开始导入数据: {} (模式: {})", import_path, mode);

    let import_path = PathBuf::from(&import_path);

    // 第一步：同步提取所有 zip 数据（不跨 await）
    let zip_data = tokio::task::spawn_blocking(move || extract_zip_data(&import_path))
        .await
        .map_err(|e| format!("提取zip数据失败: {}", e))??;

    // 第二步：写入临时数据库文件
    let temp_dir = tempfile::tempdir()
        .map_err(|e| format!("无法创建临时目录: {}", e))?;
    let temp_db_path = temp_dir.path().join("import.db");
    fs::write(&temp_db_path, &zip_data.db_bytes)
        .map_err(|e| format!("无法写入临时数据库文件: {}", e))?;

    // 第三步：连接临时数据库并读取数据
    let temp_options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&temp_db_path)
        .read_only(true);
    let temp_pool = sqlx::SqlitePool::connect_with(temp_options)
        .await
        .map_err(|e| format!("无法连接临时数据库: {}", e))?;

    // 获取当前数据库连接
    let db_state = app.try_state::<Mutex<DatabaseState>>()
        .ok_or("无法访问数据库状态")?;
    let db_guard = db_state.lock().await;
    let pool = &db_guard.pool;

    let new_images_dir = get_app_images_dir()?;
    let new_images_dir_str = new_images_dir.to_string_lossy().to_string();

    if mode == "replace" {
        // 替换模式：清空现有数据
        if new_images_dir.exists() {
            if let Ok(entries) = fs::read_dir(&new_images_dir) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        sqlx::query("DELETE FROM clipboard_history").execute(pool).await
            .map_err(|e| format!("清空历史记录失败: {}", e))?;
        sqlx::query("DELETE FROM groups").execute(pool).await
            .map_err(|e| format!("清空分组失败: {}", e))?;
        sqlx::query("DELETE FROM sqlite_sequence WHERE name IN ('clipboard_history', 'groups')").execute(pool).await
            .ok();

        tracing::info!("替换模式：已清空现有数据");
    }

    // === 导入分组 ===
    let old_groups: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, name, color, created_at FROM groups ORDER BY id"
    )
    .fetch_all(&temp_pool)
    .await
    .map_err(|e| format!("读取导入分组失败: {}", e))?;

    let mut group_id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();

    for (old_id, name, color, created_at) in &old_groups {
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM groups WHERE name = ?"
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询分组失败: {}", e))?;

        if let Some((existing_id,)) = existing {
            group_id_map.insert(*old_id, existing_id);
        } else {
            let result: (i64,) = sqlx::query_as(
                "INSERT INTO groups (name, color, created_at, item_count) VALUES (?, ?, ?, 0) RETURNING id"
            )
            .bind(name)
            .bind(color)
            .bind(created_at)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("创建分组失败: {}", e))?;

            group_id_map.insert(*old_id, result.0);
        }
    }
    tracing::info!("分组映射完成: {} 个分组", group_id_map.len());

    // === 导入剪贴板记录 ===
    let old_records = sqlx::query(
        "SELECT content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id, data_hash, metadata FROM clipboard_history ORDER BY id"
    )
    .fetch_all(&temp_pool)
    .await
    .map_err(|e| format!("读取导入记录失败: {}", e))?;

    let mut imported_count = 0u64;
    let mut skipped_count = 0u64;

    for record in &old_records {
        let content: String = record.try_get("content").unwrap_or_default();
        let record_type: String = record.try_get("type").unwrap_or_default();
        let timestamp: String = record.try_get("timestamp").unwrap_or_default();
        let is_favorite: i64 = record.try_get("is_favorite").unwrap_or(0);
        let old_image_path: Option<String> = record.try_get("image_path").ok().flatten();
        let source_app_name: Option<String> = record.try_get("source_app_name").ok().flatten();
        let source_app_icon: Option<String> = record.try_get("source_app_icon").ok().flatten();
        let thumbnail_data: Option<String> = record.try_get("thumbnail_data").ok().flatten();
        let note: Option<String> = record.try_get("note").ok().flatten();
        let old_group_id: Option<i64> = record.try_get("group_id").ok().flatten();
        let data_hash: Option<String> = record.try_get("data_hash").ok().flatten();
        let metadata: Option<String> = record.try_get("metadata").ok().flatten();

        let new_image_path = old_image_path.as_ref().and_then(|p| {
            std::path::Path::new(p).file_name().map(|f| {
                format!("{}/{}", new_images_dir_str, f.to_string_lossy())
            })
        });

        let new_group_id = old_group_id.and_then(|gid| group_id_map.get(&gid).copied());

        // 去重检查
        let is_duplicate = if let Some(ref hash) = data_hash {
            if !hash.is_empty() {
                let existing: Option<(i64,)> = sqlx::query_as(
                    "SELECT id FROM clipboard_history WHERE data_hash = ?"
                )
                .bind(hash)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("去重查询失败: {}", e))?;
                existing.is_some()
            } else {
                false
            }
        } else {
            let existing: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM clipboard_history WHERE content = ? AND type = ?"
            )
            .bind(&content)
            .bind(&record_type)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("去重查询失败: {}", e))?;
            existing.is_some()
        };

        if is_duplicate {
            skipped_count += 1;
            continue;
        }

        sqlx::query(
            "INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id, data_hash, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&content)
        .bind(&record_type)
        .bind(&timestamp)
        .bind(is_favorite)
        .bind(&new_image_path)
        .bind(&source_app_name)
        .bind(&source_app_icon)
        .bind(&thumbnail_data)
        .bind(&note)
        .bind(new_group_id)
        .bind(&data_hash)
        .bind(&metadata)
        .execute(pool)
        .await
        .map_err(|e| format!("插入记录失败: {}", e))?;

        imported_count += 1;
    }

    tracing::info!("记录导入完成: 导入 {} 条, 跳过 {} 条(重复)", imported_count, skipped_count);

    // 更新分组的 item_count
    for (_, new_gid) in &group_id_map {
        let _ = sqlx::query("UPDATE groups SET item_count = (SELECT COUNT(*) FROM clipboard_history WHERE group_id = ?) WHERE id = ?")
            .bind(new_gid)
            .bind(new_gid)
            .execute(pool)
            .await;
    }

    // 关闭临时数据库连接
    temp_pool.close().await;
    drop(db_guard); // 释放数据库锁

    // 第四步：写入图片文件（纯同步操作）
    fs::create_dir_all(&new_images_dir)
        .map_err(|e| format!("无法创建图片目录: {}", e))?;

    let mut image_count = 0u64;
    for (filename, data) in &zip_data.images {
        let dest_path = new_images_dir.join(filename);
        if !dest_path.exists() {
            fs::write(&dest_path, data)
                .map_err(|e| format!("无法写入图片文件 {}: {}", filename, e))?;
            image_count += 1;
        }
    }

    tracing::info!("图片文件提取完成: {} 个新文件", image_count);
    tracing::info!("✅ 数据导入完成");

    let _ = app.emit("data-imported", ());

    Ok(())
}

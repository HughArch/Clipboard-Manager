use tauri::Emitter;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use dirs_next::config_dir;
use arboard::Clipboard;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use base64::{engine::general_purpose, Engine as _};
use tauri_plugin_sql::{Migration, MigrationKind};
use tauri_plugin_global_shortcut::{self, GlobalShortcutExt, Shortcut, ShortcutState};
use std::env;
use chrono;
use sqlx::{self, Row, SqlitePool, sqlite::SqliteConnectOptions};
use tokio;
use tokio::sync::Mutex;
use enigo::{Enigo, Key, Keyboard, Settings};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri::menu::{Menu, MenuItem};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub max_history_items: usize,
    pub max_history_time: u64,
    pub hotkey: String,
    pub auto_start: bool,
}

// 数据库连接池状态管理
struct DatabaseState {
    pool: SqlitePool,
}

const SETTINGS_FILE: &str = "clipboard_settings.json";

fn settings_file_path() -> Result<PathBuf, String> {
    let dir = config_dir().ok_or("无法获取设置文件路径")?;
    Ok(dir.join(SETTINGS_FILE))
}

// 初始化数据库连接
async fn init_database(app: &AppHandle) -> Result<SqlitePool, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("无法获取应用数据目录: {}", e))?;
    
    // 确保目录存在
    if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
        return Err(format!("无法创建应用数据目录: {}", e));
    }
    
    let db_path = app_data_dir.join("clipboard.db");
    
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    
    let pool = SqlitePool::connect_with(options)
        .await
        .map_err(|e| format!("无法连接到数据库: {}", e))?;
    
    // 运行迁移
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS clipboard_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL,
            type TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            image_path TEXT
        )"
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("无法创建数据库表: {}", e))?;
    
    // 创建索引
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建索引: {}", e))?;
    
    println!("数据库初始化完成");
    Ok(pool)
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
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
async fn load_settings(_app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_file_path()?;
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let settings: AppSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn start_clipboard_watcher(app: AppHandle) {
    thread::spawn(move || {
        let mut clipboard = Clipboard::new().unwrap();
        let mut last_text = String::new();
        let mut last_image_hash = 0u64;

        loop {
            // 检查文本
            if let Ok(text) = clipboard.get_text() {
                if text != last_text {
                    last_text = text.clone();
                    app.emit("clipboard-text", text).ok();
                }
            }
            // 检查图片
            if let Ok(image) = clipboard.get_image() {
                let hash = image.bytes.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64));
                if hash != last_image_hash {
                    last_image_hash = hash;
                    // 转为 base64
                    let img = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.to_vec()).unwrap();
                    let mut buf = vec![];
                    image::codecs::png::PngEncoder::new(&mut buf)
                        .encode(&img, img.width(), img.height(), image::ColorType::Rgba8)
                        .unwrap();
                    let b64 = general_purpose::STANDARD.encode(&buf);
                    let data_url = format!("data:image/png;base64,{}", b64);
                    app.emit("clipboard-image", data_url).ok();
                }
            }
            thread::sleep(Duration::from_millis(800));
        }
    });
}

#[tauri::command]
async fn register_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
    // 先尝试注销已有的快捷键
    let _ = app.global_shortcut().unregister_all();
    
    // 将字符串转换为 Shortcut 类型
    let shortcut = shortcut.parse::<Shortcut>().map_err(|e| e.to_string())?;
    
    // 注册快捷键
    app.global_shortcut().register(shortcut).map_err(|e| e.to_string())?;
    
    Ok(())
}

// 获取应用程序的可执行文件路径
fn get_app_exe_path() -> Result<PathBuf, String> {
    env::current_exe().map_err(|e| format!("无法获取应用程序路径: {}", e))
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
async fn set_auto_start(_app: AppHandle, enable: bool) -> Result<(), String> {
    let app_name = "ClipboardManager"; // 应用程序在注册表中的名称
    let exe_path = get_app_exe_path()?;
    
    set_windows_auto_start(enable, app_name, &exe_path)?;
    
    Ok(())
}

#[tauri::command]
async fn get_auto_start_status(_app: AppHandle) -> Result<bool, String> {
    let app_name = "ClipboardManager";
    get_windows_auto_start_status(app_name)
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
    
    let time_cleanup_query = "
        DELETE FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0
    ";
    
    match sqlx::query(time_cleanup_query)
        .bind(&timestamp_cutoff)
        .execute(db)
        .await {
        Ok(result) => {
            println!("按时间清理完成，删除了 {} 条记录", result.rows_affected());
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
                println!("按数量清理完成，删除了 {} 条记录", result.rows_affected());
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
    
    println!("数据清理完成");
    Ok(())
}

#[tauri::command]
async fn cleanup_history(app: AppHandle) -> Result<(), String> {
    // 加载当前设置
    let settings = load_settings(app.clone()).await.unwrap_or_else(|_| AppSettings {
        max_history_items: 100,
        max_history_time: 30,
        hotkey: "Ctrl+Shift+V".to_string(),
        auto_start: false,
    });
    
    cleanup_expired_data(&app, &settings).await
}

// 粘贴内容到系统剪贴板并自动粘贴
#[tauri::command]
async fn paste_to_clipboard(content: String, content_type: String) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {}", e))?;
    
    match content_type.as_str() {
        "text" => {
            clipboard.set_text(content).map_err(|e| format!("无法设置文本到剪贴板: {}", e))?;
            println!("文本已复制到剪贴板");
        }
        "image" => {
            // 处理 base64 图片数据
            if content.starts_with("data:image/") {
                // 提取 base64 部分
                if let Some(base64_start) = content.find("base64,") {
                    let base64_data = &content[base64_start + 7..];
                    
                    // 解码 base64
                    let image_data = general_purpose::STANDARD
                        .decode(base64_data)
                        .map_err(|e| format!("无法解码图片数据: {}", e))?;
                    
                    // 解析图片
                    let img = image::load_from_memory(&image_data)
                        .map_err(|e| format!("无法加载图片: {}", e))?;
                    
                    // 转换为 RGBA 格式
                    let rgba_img = img.to_rgba8();
                    let (width, height) = rgba_img.dimensions();
                    
                    // 创建 arboard 图片数据
                    let img_data = arboard::ImageData {
                        width: width as usize,
                        height: height as usize,
                        bytes: rgba_img.into_raw().into(),
                    };
                    
                    clipboard.set_image(img_data).map_err(|e| format!("无法设置图片到剪贴板: {}", e))?;
                    println!("图片已复制到剪贴板");
                } else {
                    return Err("无效的图片数据格式".to_string());
                }
            } else {
                return Err("不支持的图片格式".to_string());
            }
        }
        _ => {
            return Err(format!("不支持的内容类型: {}", content_type));
        }
    }
    
    // 等待一段时间确保剪贴板内容已设置且焦点已切换
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // 自动模拟 Ctrl+V 粘贴操作
    match simulate_paste().await {
        Ok(_) => {
            println!("自动粘贴操作完成");
        }
        Err(e) => {
            println!("自动粘贴失败: {}", e);
            // 即使自动粘贴失败，也不返回错误，因为内容已经复制到剪贴板
        }
    }
    
    Ok(())
}

// 模拟 Ctrl+V 按键操作
async fn simulate_paste() -> Result<(), String> {
    // 在新线程中执行按键模拟，避免阻塞异步运行时
    let result = tokio::task::spawn_blocking(|| {
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("无法初始化键盘模拟器: {}", e))?;
        
        // 模拟 Ctrl+V
        enigo.key(Key::Control, enigo::Direction::Press).map_err(|e| format!("按下Ctrl键失败: {}", e))?;
        enigo.key(Key::Unicode('v'), enigo::Direction::Press).map_err(|e| format!("按下V键失败: {}", e))?;
        enigo.key(Key::Unicode('v'), enigo::Direction::Release).map_err(|e| format!("释放V键失败: {}", e))?;
        enigo.key(Key::Control, enigo::Direction::Release).map_err(|e| format!("释放Ctrl键失败: {}", e))?;
        
        Ok::<(), String>(())
    }).await;
    
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("按键模拟任务失败: {}", e)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default()
            .add_migrations(
                "sqlite:clipboard.db",
                vec![Migration {
                    version: 1,
                    description: "create clipboard_history table",
                    sql: "
                        CREATE TABLE IF NOT EXISTS clipboard_history (
                            id INTEGER PRIMARY KEY AUTOINCREMENT,
                            content TEXT NOT NULL,
                            type TEXT NOT NULL,
                            timestamp TEXT NOT NULL,
                            is_favorite INTEGER NOT NULL DEFAULT 0,
                            image_path TEXT
                        );
                        CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content);
                    ".into(),
                    kind: MigrationKind::Up,
                }],
            )
            .build()
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
            })
            .build()
        )
        .setup(|app| {
            let app_handle = app.handle().clone();
            start_clipboard_watcher(app_handle.clone());

            // 创建系统托盘菜单
            let show_hide_item = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_hide_item, &quit_item])?;

            // 创建系统托盘
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .menu_on_left_click(false)
                .tooltip("Clipboard Manager")
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click { 
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                match window.is_visible() {
                                    Ok(true) => {
                                        let _ = window.hide();
                                    }
                                    Ok(false) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                    Err(_) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                }
                            }
                        }
                        TrayIconEvent::DoubleClick { 
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "toggle" => {
                            if let Some(window) = app.get_webview_window("main") {
                                match window.is_visible() {
                                    Ok(true) => {
                                        let _ = window.hide();
                                    }
                                    Ok(false) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                    Err(_) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                }
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // 异步初始化数据库和其他操作
            let app_handle_for_delayed = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                // 等待一小段时间确保应用完全启动
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                
                // 初始化数据库
                match init_database(&app_handle_for_delayed).await {
                    Ok(pool) => {
                        // 将数据库连接池注册为应用状态
                        app_handle_for_delayed.manage(Mutex::new(DatabaseState { pool }));
                        println!("数据库状态已注册");
                        
                        // 加载设置并注册默认快捷键
                        match load_settings(app_handle_for_delayed.clone()).await {
                            Ok(settings) => {
                                let _ = register_shortcut(app_handle_for_delayed.clone(), settings.hotkey.clone()).await;
                                // 应用自启动设置
                                let _ = set_auto_start(app_handle_for_delayed.clone(), settings.auto_start).await;
                                // 启动时清理过期数据
                                let _ = cleanup_expired_data(&app_handle_for_delayed, &settings).await;
                            }
                            Err(_) => {
                                // 如果没有保存的设置，使用默认快捷键
                                let _ = register_shortcut(app_handle_for_delayed.clone(), "Ctrl+Shift+V".to_string()).await;
                                // 默认不启用自启动
                                let _ = set_auto_start(app_handle_for_delayed.clone(), false).await;
                                // 使用默认设置清理数据
                                let default_settings = AppSettings {
                                    max_history_items: 100,
                                    max_history_time: 30,
                                    hotkey: "Ctrl+Shift+V".to_string(),
                                    auto_start: false,
                                };
                                let _ = cleanup_expired_data(&app_handle_for_delayed, &default_settings).await;
                            }
                        }
                    }
                    Err(e) => {
                        println!("数据库初始化失败: {}", e);
                    }
                }
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, save_settings, load_settings, register_shortcut, set_auto_start, get_auto_start_status, cleanup_history, paste_to_clipboard])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
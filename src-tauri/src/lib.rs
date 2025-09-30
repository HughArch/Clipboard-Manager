// 模块声明
mod types;
mod resource_manager;
mod icon_cache;
mod window_info;
mod commands;
mod logging;

// 重新导出公共类型
pub use types::*;

// 基础导入
use tauri::{Manager, Emitter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri_plugin_global_shortcut::{ShortcutState};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::menu::{Menu, MenuItem};
use tokio::sync::Mutex;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

// 初始化数据库连接
async fn init_database(app: &tauri::AppHandle) -> Result<SqlitePool, String> {
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
    
    // 直接创建包含所有字段的完整表结构
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS clipboard_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL,
            type TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            image_path TEXT,
            source_app_name TEXT,
            source_app_icon TEXT,
            thumbnail_data TEXT
        )"
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("无法创建数据库表: {}", e))?;
    
    // 进行数据库迁移 - 添加缩略图字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE clipboard_history ADD COLUMN thumbnail_data TEXT")
        .execute(&pool)
        .await; // 忽略错误，因为字段可能已存在
    
    // 进行数据库迁移 - 添加备注字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE clipboard_history ADD COLUMN note TEXT")
        .execute(&pool)
        .await; // 忽略错误，因为字段可能已存在
    
    // 创建索引
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建 content 索引: {}", e))?;
    
    // 为 type 字段创建索引以提高查询性能
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_type ON clipboard_history(type)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建 type 索引: {}", e))?;
    
    // 为 timestamp 字段创建索引以提高排序性能
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_timestamp ON clipboard_history(timestamp DESC)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建 timestamp 索引: {}", e))?;
    
    // 为 is_favorite 字段创建索引以提高收藏查询性能
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_favorite ON clipboard_history(is_favorite)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建 is_favorite 索引: {}", e))?;
    
    // 创建复合索引以优化常用查询组合
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_type_timestamp ON clipboard_history(type, timestamp DESC)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建复合索引: {}", e))?;
    
    // 为收藏查询创建复合索引
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_favorite_timestamp ON clipboard_history(is_favorite, timestamp DESC)")
        .execute(&pool)
        .await
        .map_err(|e| format!("无法创建收藏复合索引: {}", e))?;
    
    tracing::info!("数据库初始化完成");
    tracing::info!("已创建数据库索引: type, timestamp, is_favorite, 以及复合索引");
    Ok(pool)
}

// 简化的剪贴板监听器 - 使用事件驱动而不是轮询
fn start_clipboard_watcher(_app: tauri::AppHandle) -> Arc<AtomicBool> {
    let should_stop = Arc::new(AtomicBool::new(false));
    
    // 使用新的插件，剪贴板监听由插件自动处理
    // 不再需要手动轮询，避免了arboard的内存泄漏问题
    tracing::info!("剪贴板监听器已初始化（事件驱动模式，无内存泄漏）");
    
    should_stop
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化日志系统
    if let Err(e) = logging::init_logging(logging::LogConfig::default()) {
        eprintln!("日志系统初始化失败: {}", e);
        // 注意：此时日志系统尚未初始化，必须使用eprintln!
    }
    
    // 在生产环境中重定向stdio到日志
    if !cfg!(debug_assertions) {
        if let Err(e) = logging::redirect_stdio_to_log() {
            tracing::warn!("重定向stdio失败: {}", e);
        }
    }
    
    tracing::info!("🚀 应用程序启动中...");
    tracing::info!("📋 准备初始化 Tauri Builder...");
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard::init())
                .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            show_window_with_context(app);
                        }
                    }
                }
            })
            .build()
        )
        .setup(|app| {
            let app_handle = app.handle().clone();
            let should_stop = start_clipboard_watcher(app_handle.clone());
            
            // 将剪贴板监听器的停止控制保存到应用状态
            app.manage(ClipboardWatcherState { should_stop: should_stop.clone() });

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
                        tracing::info!("数据库状态已注册");
                        
                        // 加载设置并注册默认快捷键
                        match commands::load_settings(app_handle_for_delayed.clone()).await {
                            Ok(settings) => {
                                let _ = commands::register_shortcut(app_handle_for_delayed.clone(), settings.hotkey.clone()).await;
                                // 应用自启动设置
                                let _ = commands::set_auto_start(app_handle_for_delayed.clone(), settings.auto_start).await;
                                // 启动时清理过期数据
                                let _ = commands::cleanup_history(app_handle_for_delayed.clone()).await;
                            }
                            Err(_) => {
                                // 如果没有保存的设置，使用默认快捷键
                                let _ = commands::register_shortcut(app_handle_for_delayed.clone(), "Ctrl+Shift+V".to_string()).await;
                                // 默认不启用自启动
                                let _ = commands::set_auto_start(app_handle_for_delayed.clone(), false).await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("数据库初始化失败: {}", e);
                    }
                }
            });

            // 创建系统托盘菜单
            let show_hide_item = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_hide_item, &quit_item])?;

            // 创建系统托盘
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Clipboard Manager")
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click { 
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } => {
                            toggle_window_visibility(tray.app_handle());
                        }
                        TrayIconEvent::DoubleClick { 
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } => {
                            show_window(tray.app_handle());
                        }
                        _ => {}
                    }
                })
                .on_menu_event({
                    let should_stop_clone = should_stop.clone();
                    move |app, event| {
                        match event.id().as_ref() {
                            "toggle" => {
                                toggle_window_visibility(app);
                            }
                            "quit" => {
                                // 停止剪贴板监听器
                                should_stop_clone.store(true, Ordering::Relaxed);
                                tracing::info!("正在停止剪贴板监听器...");
                                
                                // 等待一小段时间让监听器线程停止
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                
                                tracing::info!("应用程序正常退出");
                                app.exit(0);
                            }
                            _ => {}
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::save_settings,
            commands::auto_paste,
            commands::smart_paste_to_app,
            commands::reset_database,
            commands::load_image_file,
            commands::generate_thumbnail,
            commands::cleanup_history,
            commands::load_settings,
            commands::set_auto_start,
            commands::get_auto_start_status,
            commands::register_shortcut,
            window_info::get_active_window_info,
            window_info::get_active_window_info_for_clipboard,
            // 日志相关命令  
            commands::open_log_folder,
            commands::delete_all_logs,
            commands::write_frontend_log,
            // 备注管理命令
            commands::update_item_note,
            commands::get_item_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// 辅助函数
fn toggle_window_visibility(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            Ok(false) => {
                show_window(app);
            }
            Err(_) => {
                show_window(app);
            }
        }
    }
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        // 添加小延迟确保窗口完全显示
        std::thread::sleep(std::time::Duration::from_millis(50));
        // 再次设置焦点，确保焦点在 webview 上
        let _ = window.set_focus();
    }
}

// 改进的显示窗口函数 - 在显示前获取活动窗口上下文
fn show_window_with_context(app: &tauri::AppHandle) {
    // 先获取当前活动窗口信息（在显示剪贴板管理器之前）
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 获取活动窗口信息
        let active_app_info = window_info::get_active_window_info().await;
        
        // 显示窗口
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
            
            // 将活动窗口信息发送给前端
            if let Ok(app_info) = active_app_info {
                tracing::debug!("📤 发送前一个活动应用信息到前端: {}", app_info.name);
                let _ = window.emit("previous-app-info", app_info);
            } else {
                tracing::warn!("⚠️ 无法获取前一个活动应用信息");
            }
            
            // 添加小延迟确保窗口完全显示
            std::thread::sleep(std::time::Duration::from_millis(50));
            // 再次设置焦点，确保焦点在 webview 上
            let _ = window.set_focus();
        }
    });
}
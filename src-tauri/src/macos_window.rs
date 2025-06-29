#[cfg(target_os = "macos")]
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use cocoa::appkit::NSWindow;
#[cfg(target_os = "macos")]
use cocoa::base::{id, YES};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};

// macOS 窗口级别常量（基于 NSWindowLevel）
#[cfg(target_os = "macos")]
const NS_NORMAL_WINDOW_LEVEL: i32 = 0;
#[cfg(target_os = "macos")]
const NS_FLOATING_WINDOW_LEVEL: i32 = 3;
#[cfg(target_os = "macos")]
const NS_MODAL_PANEL_WINDOW_LEVEL: i32 = 8;
#[cfg(target_os = "macos")]
const NS_SCREEN_SAVER_WINDOW_LEVEL: i32 = 1000;

// macOS 窗口集合行为常量
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_BEHAVIOR_DEFAULT: u64 = 0;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE: u64 = 1 << 1;
#[cfg(target_os = "macos")]
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

// 新增一个极高的窗口级别
#[cfg(target_os = "macos")]
const SUPER_HIGH_WINDOW_LEVEL: i32 = 20000;

/// 检测是否有应用处于全屏模式
#[cfg(target_os = "macos")]
pub fn detect_fullscreen_app() -> Result<String, String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(r#"
            tell application "System Events"
                try
                    -- 获取所有可见的应用进程
                    set visibleProcesses to (every application process whose visible is true)
                    
                    repeat with proc in visibleProcesses
                        try
                            set procWindows to windows of proc
                            repeat with win in procWindows
                                -- 检查窗口是否为全屏
                                set winProps to properties of win
                                if (get value of attribute "AXFullScreen" of win) is true then
                                    return "fullscreen:" & (name of proc)
                                end if
                            end repeat
                        end try
                    end repeat
                    
                    return "windowed"
                on error
                    return "unknown"
                end try
            end tell
        "#)
        .output()
        .map_err(|e| format!("Failed to detect fullscreen: {}", e))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    tracing::info!("全屏检测结果: {}", result);
    
    Ok(result)
}

/// 显示窗口并设置为最高层级（可覆盖全屏应用）
#[cfg(target_os = "macos")]
pub fn show_window_on_top(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        tracing::info!("🚀 [最终方案] 开始显示窗口");
        let _ = window.show();
        let _ = window.set_focus();
        
        if let Ok(ns_window) = window.ns_window() {
            let ns_window = ns_window as id;
            tracing::info!("✅ 成功获取原生窗口句柄: {:p}", ns_window);
            
            unsafe {
                // 1. 设置极高的窗口级别
                let level = SUPER_HIGH_WINDOW_LEVEL;
                tracing::info!("🔧 [调试] 设置窗口级别为超高等级: {}", level);
                let _: () = msg_send![ns_window, setLevel: level];
                tracing::info!("✅ setLevel 完成");
                
                // 2. 设置正确的集合行为
                let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES 
                             | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;
                tracing::info!("🔧 [调试] 设置窗口集合行为，值为: {}", behavior);
                let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
                tracing::info!("✅ setCollectionBehavior 完成");

                // 移除有问题的调用：setBecomesKeyOnlyIfNeeded
                // 这个方法在标准 NSWindow 上调用会引发 Objective-C 异常
                tracing::info!("⚠️ [调试] 跳过 setBecomesKeyOnlyIfNeeded 调用");

                // 4. 将窗口提到最前面
                tracing::info!("🔧 [调试] 调用 makeKeyAndOrderFront");
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: ns_window];
                tracing::info!("✅ makeKeyAndOrderFront 完成");

                let new_level: i32 = msg_send![ns_window, level];
                tracing::info!("✅ [调试] 窗口设置完成，新级别: {}", new_level);
            }
        } else {
            return Err("无法获取原生窗口句柄".to_string());
        }
        
        Ok(())
    } else {
        Err("无法找到主窗口".to_string())
    }
}

/// 重置窗口为普通级别
#[cfg(target_os = "macos")]
pub fn reset_window_level(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(ns_window) = window.ns_window() {
            let ns_window = ns_window as id;
            
            unsafe {
                // 重置为普通窗口级别
                let normal_level = NS_NORMAL_WINDOW_LEVEL;
                let _: () = msg_send![ns_window, setLevel: normal_level];
                
                // --- 新增：重置集合行为 ---
                let _: () = msg_send![ns_window, setCollectionBehavior: NS_WINDOW_COLLECTION_BEHAVIOR_DEFAULT];
                tracing::info!("✅ 窗口集合行为已重置");
                // --- 结束新增 ---
                
                tracing::info!("✅ 窗口级别已重置为普通级别: {}", normal_level);
            }
        }
    }
    
    Ok(())
}

/// 智能显示窗口：检测全屏状态并选择合适的显示方式
#[cfg(target_os = "macos")]
pub fn show_window_smart(app: &AppHandle) -> Result<(), String> {
    match detect_fullscreen_app() {
        Ok(result) if result.starts_with("fullscreen:") => {
            let app_name = result.strip_prefix("fullscreen:").unwrap_or("Unknown");
            tracing::info!("🔍 检测到全屏应用: {}，将使用覆盖模式", app_name);
            show_window_on_top(app)
        }
        Ok(_) => {
            tracing::info!("📱 无全屏应用，使用普通显示模式");
            show_window_normal(app)
        }
        Err(e) => {
            tracing::warn!("⚠️ 无法检测全屏状态: {}，使用普通显示", e);
            show_window_normal(app)
        }
    }
}

/// 普通方式显示窗口
#[cfg(target_os = "macos")]
pub fn show_window_normal(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        // 先重置窗口级别
        let _ = reset_window_level(app);
        
        let _ = window.show();
        let _ = window.set_focus();
        
        // 添加短暂延迟确保窗口完全显示
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = window.set_focus();
        
        tracing::info!("✅ 窗口以普通模式显示");
    }
    
    Ok(())
}

/// 隐藏窗口并重置级别
#[cfg(target_os = "macos")]
pub fn hide_window_and_reset(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        
        // 重置窗口级别，避免影响下次显示
        let _ = reset_window_level(app);
        
        tracing::info!("✅ 窗口已隐藏并重置级别");
    }
    
    Ok(())
}

// ==================== 非 macOS 平台的占位实现 ====================

#[cfg(not(target_os = "macos"))]
pub fn show_window_smart(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn detect_fullscreen_app() -> Result<String, String> {
    Ok("windowed".to_string()) // 非 macOS 平台默认返回无全屏
}

#[cfg(not(target_os = "macos"))]
pub fn hide_window_and_reset(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn show_window_on_top(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        // 在非 macOS 平台使用标准的 always on top
        let _ = window.set_always_on_top(true);
    }
    Ok(())
} 
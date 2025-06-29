#[cfg(target_os = "macos")]
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use cocoa::appkit::NSWindow;
#[cfg(target_os = "macos")]
use cocoa::base::id;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

// macOS 窗口级别常量（基于 NSWindowLevel）
#[cfg(target_os = "macos")]
const NS_NORMAL_WINDOW_LEVEL: i32 = 0;
#[cfg(target_os = "macos")]
const NS_FLOATING_WINDOW_LEVEL: i32 = 3;
#[cfg(target_os = "macos")]
const NS_MODAL_PANEL_WINDOW_LEVEL: i32 = 8;
#[cfg(target_os = "macos")]
const NS_SCREEN_SAVER_WINDOW_LEVEL: i32 = 1000;

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
        // 首先，确保窗口可见并获得焦点
        tracing::info!("🚀 开始显示窗口覆盖全屏应用");
        
        let _ = window.show();
        let _ = window.set_focus();
        
        // 获取原生的 NSWindow 指针
        if let Ok(ns_window) = window.ns_window() {
            let ns_window = ns_window as id;
            tracing::info!("✅ 成功获取原生窗口句柄: {:p}", ns_window);
            
            unsafe {
                // 获取当前窗口级别
                let current_level: i32 = msg_send![ns_window, level];
                tracing::info!("🔍 当前窗口级别: {}", current_level);
                
                // 使用 NSScreenSaverWindowLevel，这个级别足够高，可以覆盖全屏应用
                let level = NS_SCREEN_SAVER_WINDOW_LEVEL;
                tracing::info!("🔧 设置窗口级别为 NSScreenSaverWindowLevel: {}", level);
                
                // 调用 NSWindow 的 setLevel: 方法
                let _: () = msg_send![ns_window, setLevel: level];
                
                // 验证级别是否设置成功
                let new_level: i32 = msg_send![ns_window, level];
                tracing::info!("✅ 窗口级别设置完成，新级别: {}", new_level);
                
                // 确保窗口在最前面
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: ns_window];
                let _: () = msg_send![ns_window, orderFrontRegardless];
                
                // 设置窗口属性以确保能够覆盖全屏应用
                let _: () = msg_send![ns_window, setCanHide: false];
                let _: () = msg_send![ns_window, setIgnoresMouseEvents: false];
                
                // 检查最终状态
                let is_visible: bool = msg_send![ns_window, isVisible];
                let is_key: bool = msg_send![ns_window, isKeyWindow];
                let is_main: bool = msg_send![ns_window, isMainWindow];
                
                tracing::info!("🔍 最终窗口状态 - 级别: {}, 可见: {}, 关键窗口: {}, 主窗口: {}", 
                              new_level, is_visible, is_key, is_main);
                
                if new_level == level && is_visible {
                    tracing::info!("🎉 窗口成功设置为屏保级别，可以覆盖全屏应用！");
                } else {
                    tracing::warn!("⚠️ 窗口设置可能不完整");
                }
            }
        } else {
            return Err("无法获取原生窗口句柄".to_string());
        }
        
        // 再次确保焦点
        let _ = window.set_focus();
        
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
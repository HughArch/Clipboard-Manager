#[cfg(target_os = "macos")]
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use cocoa::base::id;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl, runtime};

// macOS 窗口级别常量
#[cfg(target_os = "macos")]
const OVERLAY_WINDOW_LEVEL: i32 = 25; // kCGOverlayWindowLevelKey - 覆盖层级别
#[cfg(target_os = "macos")]
const SCREEN_SAVER_WINDOW_LEVEL: i32 = 1000; // kCGScreenSaverWindowLevel - 更高级别
#[cfg(target_os = "macos")]
const MAXIMUM_WINDOW_LEVEL: i32 = 2147483647; // CGWindowLevelForKey(kCGMaximumWindowLevelKey)

#[cfg(target_os = "macos")]
pub fn detect_fullscreen_app() -> Result<bool, String> {
    // 检测是否有应用处于全屏模式
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
    
    Ok(result.starts_with("fullscreen:"))
}

#[cfg(target_os = "macos")]
pub fn set_window_overlay_level(app: &AppHandle) -> Result<(), String> {
    // 设置窗口为覆盖级别，能够在全屏应用上层显示
    if let Some(window) = app.get_webview_window("main") {
        unsafe {
            // 获取原生窗口句柄
            if let Ok(native_window) = window.ns_window() {
                let ns_window = native_window as id;
                
                // 首先尝试最高级别，确保能覆盖全屏应用
                let _: () = msg_send![ns_window, setLevel: MAXIMUM_WINDOW_LEVEL];
                tracing::info!("🔧 设置窗口级别为最高级别: {}", MAXIMUM_WINDOW_LEVEL);
                
                // 设置窗口集合行为，允许在全屏空间中显示
                let ns_window_collection_behavior_can_join_all_spaces: i32 = 1 << 0;
                let ns_window_collection_behavior_full_screen_auxiliary: i32 = 1 << 8;
                let ns_window_collection_behavior_stationary: i32 = 1 << 4;
                let behavior = ns_window_collection_behavior_can_join_all_spaces | 
                              ns_window_collection_behavior_full_screen_auxiliary |
                              ns_window_collection_behavior_stationary;
                
                let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
                tracing::info!("🔧 设置窗口集合行为: {}", behavior);
                
                // 确保窗口不会被全屏应用遮挡
                let _: () = msg_send![ns_window, setIgnoresMouseEvents: false];
                let _: () = msg_send![ns_window, setCanHide: false];
                let _: () = msg_send![ns_window, setIsExcludedFromWindowsMenu: false];
                
                // 获取当前窗口状态用于调试
                let current_level: i32 = msg_send![ns_window, level];
                let is_visible: bool = msg_send![ns_window, isVisible];
                let is_key: bool = msg_send![ns_window, isKeyWindow];
                tracing::info!("🔍 窗口状态 - 级别: {}, 可见: {}, 关键窗口: {}", current_level, is_visible, is_key);
                
                tracing::info!("✅ 窗口已设置为最高级别，可在全屏模式下显示");
                
                return Ok(());
            }
        }
        
        return Err("无法获取原生窗口句柄".to_string());
    }
    
    Err("无法找到主窗口".to_string())
}

#[cfg(target_os = "macos")]
pub fn reset_window_level(app: &AppHandle) -> Result<(), String> {
    // 重置窗口级别为普通级别
    if let Some(window) = app.get_webview_window("main") {
        unsafe {
            if let Ok(native_window) = window.ns_window() {
                let ns_window = native_window as id;
                
                // 重置为普通窗口级别
                let normal_level: i32 = 0; // NSNormalWindowLevel
                let _: () = msg_send![ns_window, setLevel: normal_level];
                
                // 重置集合行为
                let ns_window_collection_behavior_default: i32 = 0;
                let _: () = msg_send![ns_window, setCollectionBehavior: ns_window_collection_behavior_default];
                
                tracing::info!("✅ 窗口级别已重置为普通级别");
                
                return Ok(());
            }
        }
    }
    
    Err("无法重置窗口级别".to_string())
}

#[cfg(target_os = "macos")]
pub fn show_window_smart(app: &AppHandle) -> Result<(), String> {
    // 智能显示窗口：根据是否有全屏应用来决定窗口级别
    match detect_fullscreen_app() {
        Ok(true) => {
            tracing::info!("🔍 检测到全屏应用，将窗口设置为覆盖模式");
            
            // 首先设置窗口为覆盖级别
            if let Err(e) = set_window_overlay_level(app) {
                tracing::warn!("❌ 设置覆盖级别失败: {}, 尝试普通显示", e);
                return show_window_normal(app);
            }
            
            // 显示窗口
            if let Some(window) = app.get_webview_window("main") {
                // 首先确保窗口是可见的
                let _ = window.show();
                let _ = window.unminimize();
                
                // 使用原生方法强制显示窗口
                unsafe {
                    if let Ok(native_window) = window.ns_window() {
                        let ns_window = native_window as id;
                        
                        // 获取显示前的状态
                        let level_before: i32 = msg_send![ns_window, level];
                        let visible_before: bool = msg_send![ns_window, isVisible];
                        tracing::info!("🔍 显示前状态 - 级别: {}, 可见: {}", level_before, visible_before);
                        
                        // 设置窗口为非透明，确保可见
                        let _: () = msg_send![ns_window, setOpaque: true];
                        let _: () = msg_send![ns_window, setAlphaValue: 1.0f64];
                        tracing::info!("🔧 设置窗口透明度为完全不透明");
                        
                        // 强制显示在最前方
                        let _: () = msg_send![ns_window, orderFrontRegardless];
                        tracing::info!("🔧 执行 orderFrontRegardless");
                        
                        let _: () = msg_send![ns_window, makeKeyAndOrderFront: ns_window];
                        tracing::info!("🔧 执行 makeKeyAndOrderFront");
                        
                        // 设置窗口为关键窗口
                        let _: () = msg_send![ns_window, makeKeyWindow];
                        let _: () = msg_send![ns_window, makeMainWindow];
                        tracing::info!("🔧 设置为关键和主窗口");
                        
                        // 激活应用程序
                        let app_class = runtime::Class::get("NSApplication").unwrap();
                        let shared_app: id = msg_send![app_class, sharedApplication];
                        let _: () = msg_send![shared_app, activateIgnoringOtherApps: true];
                        tracing::info!("🔧 激活应用程序忽略其他应用");
                        
                        // 获取显示后的状态
                        let level_after: i32 = msg_send![ns_window, level];
                        let visible_after: bool = msg_send![ns_window, isVisible];
                        let is_key_after: bool = msg_send![ns_window, isKeyWindow];
                        let is_main_after: bool = msg_send![ns_window, isMainWindow];
                        tracing::info!("🔍 显示后状态 - 级别: {}, 可见: {}, 关键窗口: {}, 主窗口: {}", 
                                      level_after, visible_after, is_key_after, is_main_after);
                    }
                }
                
                // 使用 Tauri 的方法再次确保焦点
                let _ = window.set_focus();
                
                tracing::info!("✅ 窗口已在全屏模式下显示");
            }
        }
        Ok(false) => {
            tracing::info!("📱 无全屏应用，使用普通显示模式");
            
            // 确保窗口级别为普通级别
            let _ = reset_window_level(app);
            
            // 普通显示
            show_window_normal(app)?;
        }
        Err(e) => {
            tracing::warn!("⚠️ 无法检测全屏状态: {}, 使用普通显示", e);
            show_window_normal(app)?;
        }
    }
    
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn show_window_normal(app: &AppHandle) -> Result<(), String> {
    // 普通窗口显示
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        
        // 添加短暂延迟确保窗口完全显示
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = window.set_focus();
        
        tracing::info!("✅ 窗口以普通模式显示");
    }
    
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn hide_window_and_reset(app: &AppHandle) -> Result<(), String> {
    // 隐藏窗口并重置级别
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        
        // 重置窗口级别，避免影响下次显示
        let _ = reset_window_level(app);
        
        tracing::info!("✅ 窗口已隐藏并重置级别");
    }
    
    Ok(())
}

// 非 macOS 平台的占位实现
#[cfg(not(target_os = "macos"))]
pub fn show_window_smart(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn detect_fullscreen_app() -> Result<bool, String> {
    Ok(false) // 非 macOS 平台默认返回 false
}

#[cfg(not(target_os = "macos"))]
pub fn hide_window_and_reset(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(())
} 
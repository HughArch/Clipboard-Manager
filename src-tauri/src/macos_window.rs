#[cfg(target_os = "macos")]
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use cocoa::base::id;
#[cfg(target_os = "macos")]
use cocoa::foundation::{NSRect, NSPoint, NSSize};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl, runtime};

// macOS 窗口级别常量 (基于Apple官方文档和网络搜索结果)
#[cfg(target_os = "macos")]
const FLOATING_WINDOW_LEVEL: i32 = 3; // NSFloatingWindowLevel - 浮动窗口级别
#[cfg(target_os = "macos")]
const MODAL_PANEL_WINDOW_LEVEL: i32 = 8; // NSModalPanelWindowLevel - 模态面板级别
#[cfg(target_os = "macos")]
const OVERLAY_WINDOW_LEVEL: i32 = 25; // kCGOverlayWindowLevelKey - 覆盖层级别
#[cfg(target_os = "macos")]
const SCREEN_SAVER_WINDOW_LEVEL: i32 = 1000; // kCGScreenSaverWindowLevel - 屏保级别
#[cfg(target_os = "macos")]
const ASSISTIVE_TECH_HIGH_LEVEL: i32 = 1500; // kCGAssistiveTechHighWindowLevel - 辅助技术高级别

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
                
                // 逐步尝试不同的窗口级别，从保守到激进
                let levels_to_try = [
                    (FLOATING_WINDOW_LEVEL, "浮动窗口级别"),
                    (MODAL_PANEL_WINDOW_LEVEL, "模态面板级别"), 
                    (OVERLAY_WINDOW_LEVEL, "覆盖层级别"),
                    (SCREEN_SAVER_WINDOW_LEVEL, "屏保级别")
                ];
                
                let mut level_set = false;
                for (level, description) in levels_to_try.iter().rev() {
                    // 从最高级别开始尝试
                    let _: () = msg_send![ns_window, setLevel: *level];
                    let actual_level: i32 = msg_send![ns_window, level];
                    
                    if actual_level == *level {
                        tracing::info!("🔧 成功设置窗口级别为{}: {}", description, level);
                        level_set = true;
                        break;
                    } else {
                        tracing::warn!("⚠️ 设置{}失败，尝试次级别", description);
                    }
                }
                
                if !level_set {
                    tracing::warn!("⚠️ 所有级别设置都失败，使用默认级别");
                }
                
                // 设置窗口集合行为，允许在全屏空间中显示
                // 使用正确的类型：macOS 期望 NSUInteger (u64)  
                let ns_window_collection_behavior_can_join_all_spaces: u64 = 1 << 0;  // 1
                let ns_window_collection_behavior_move_to_active_space: u64 = 1 << 1;  // 2  
                let ns_window_collection_behavior_full_screen_auxiliary: u64 = 1 << 8; // 256
                
                // 使用保守的集合行为设置
                let behavior = ns_window_collection_behavior_can_join_all_spaces | 
                              ns_window_collection_behavior_move_to_active_space |
                              ns_window_collection_behavior_full_screen_auxiliary;
                
                let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
                tracing::info!("🔧 设置窗口集合行为: {} (保守模式)", behavior);
                
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
pub fn set_window_level_only(app: &AppHandle) -> Result<(), String> {
    // 只设置窗口级别，不设置其他属性
    if let Some(window) = app.get_webview_window("main") {
        unsafe {
            if let Ok(native_window) = window.ns_window() {
                let ns_window = native_window as id;
                
                // 使用Apple推荐的窗口级别，从低到高
                let levels_to_try = [
                    (FLOATING_WINDOW_LEVEL, "浮动窗口级别"),        // 3
                    (MODAL_PANEL_WINDOW_LEVEL, "模态面板级别"),    // 8  
                    (OVERLAY_WINDOW_LEVEL, "覆盖层级别"),          // 25
                    (SCREEN_SAVER_WINDOW_LEVEL, "屏保级别"),       // 1000
                    (ASSISTIVE_TECH_HIGH_LEVEL, "辅助技术高级别"), // 1500 - 专用于覆盖全屏应用
                ];
                
                let mut level_set = false;
                for (level, description) in levels_to_try.iter().rev() {
                    let _: () = msg_send![ns_window, setLevel: *level];
                    let actual_level: i32 = msg_send![ns_window, level];
                    
                    if actual_level == *level {
                        tracing::info!("🔧 成功设置窗口级别为{}: {}", description, level);
                        level_set = true;
                        break;
                    } else {
                        tracing::warn!("⚠️ 设置{}失败，尝试次级别", description);
                    }
                }
                
                if !level_set {
                    tracing::warn!("⚠️ 所有级别设置都失败，保持当前级别");
                }
                
                // 设置集合行为 - 根据Apple文档，一次只能使用一个Spaces行为
                tracing::info!("🔧 准备设置窗口集合行为以支持全屏显示");
                
                // 根据Apple官方建议，只使用CanJoinAllSpaces让窗口出现在所有空间
                let ns_window_collection_behavior_can_join_all_spaces: u64 = 1 << 0;  // 1
                
                tracing::info!("🔧 设置集合行为为CanJoinAllSpaces (1)");
                let _: () = msg_send![ns_window, setCollectionBehavior: ns_window_collection_behavior_can_join_all_spaces];
                tracing::info!("✅ 成功设置集合行为: CanJoinAllSpaces (1)");
                
                // 设置其他重要属性
                tracing::info!("🔧 设置窗口其他属性");
                
                tracing::info!("🔧 准备设置 setCanHide: false");
                let _: () = msg_send![ns_window, setCanHide: false];
                tracing::info!("✅ 成功设置 setCanHide: false");
                
                tracing::info!("🔧 准备设置 setIgnoresMouseEvents: false");
                let _: () = msg_send![ns_window, setIgnoresMouseEvents: false];
                tracing::info!("✅ 成功设置 setIgnoresMouseEvents: false");
                
                // 移除有问题的 setIsExcludedFromWindowsMenu 方法调用
                // 这个方法在某些 macOS 版本中可能不可用或有问题
                tracing::info!("⚠️ 跳过 setIsExcludedFromWindowsMenu 设置（已知问题）");
                
                // 使用Apple推荐的方式强制显示窗口
                tracing::info!("🔧 强制窗口显示在最前面");
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: ns_window];
                let _: () = msg_send![ns_window, orderFrontRegardless];
                
                // 激活应用程序
                if let Some(app_class) = runtime::Class::get("NSApplication") {
                    let shared_app: id = msg_send![app_class, sharedApplication];
                    let _: () = msg_send![shared_app, activateIgnoringOtherApps: true];
                    tracing::info!("🔧 激活应用程序");
                }
                
                // 获取最终状态和详细位置信息
                let final_level: i32 = msg_send![ns_window, level];
                let final_visible: bool = msg_send![ns_window, isVisible];
                let final_key: bool = msg_send![ns_window, isKeyWindow];
                let final_main: bool = msg_send![ns_window, isMainWindow];
                let is_on_active_space: bool = msg_send![ns_window, isOnActiveSpace];
                
                // 获取窗口位置和大小
                let frame: NSRect = msg_send![ns_window, frame];
                let screen_frame: NSRect = {
                    let screen: id = msg_send![ns_window, screen];
                    if !screen.is_null() {
                        msg_send![screen, frame]
                    } else {
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
                    }
                };
                
                tracing::info!("🔍 最终窗口状态 - 级别: {}, 可见: {}, 关键窗口: {}, 主窗口: {}", 
                              final_level, final_visible, final_key, final_main);
                tracing::info!("🔍 窗口空间状态 - 在活动空间: {}", is_on_active_space);
                tracing::info!("🔍 窗口位置 - x: {:.0}, y: {:.0}, 宽: {:.0}, 高: {:.0}", 
                              frame.origin.x, frame.origin.y, frame.size.width, frame.size.height);
                tracing::info!("🔍 屏幕尺寸 - 宽: {:.0}, 高: {:.0}", screen_frame.size.width, screen_frame.size.height);
                
                tracing::info!("✅ 窗口级别和集合行为设置完成");
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
                let ns_window_collection_behavior_default: u64 = 0;
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
            
            // 先用普通方式显示窗口，然后再设置覆盖级别
            if let Some(window) = app.get_webview_window("main") {
                tracing::info!("🔧 准备调用 Tauri window.show()");
                // 首先确保窗口是可见的
                let show_result = window.show();
                tracing::info!("✅ Tauri window.show() 完成，结果: {:?}", show_result);
                
                tracing::info!("🔧 准备调用 Tauri window.unminimize()");
                let unminimize_result = window.unminimize();
                tracing::info!("✅ Tauri window.unminimize() 完成，结果: {:?}", unminimize_result);
                
                // 安全地显示窗口，逐步调试每个方法调用
                tracing::info!("🔧 准备获取原生窗口句柄用于显示");
                unsafe {
                    tracing::info!("🔧 准备调用 window.ns_window()");
                    if let Ok(native_window) = window.ns_window() {
                        let ns_window = native_window as id;
                        tracing::info!("🔧 成功获取原生窗口句柄，地址: {:p}", ns_window);
                        
                        // 验证窗口对象是否有效
                        if ns_window.is_null() {
                            tracing::error!("❌ 窗口句柄为空指针");
                        } else {
                            tracing::info!("✅ 窗口对象有效");
                            
                            // 获取显示前的状态
                            tracing::info!("🔧 准备获取窗口级别");
                            let level_before: i32 = msg_send![ns_window, level];
                            tracing::info!("🔧 准备获取窗口可见性");
                            let visible_before: bool = msg_send![ns_window, isVisible];
                            tracing::info!("🔍 显示前状态 - 级别: {}, 可见: {}", level_before, visible_before);
                            
                            // 逐步设置窗口属性，每步都有日志
                            tracing::info!("🔧 准备设置窗口为不透明");
                            let _: () = msg_send![ns_window, setOpaque: true];
                            tracing::info!("✅ 成功设置窗口为不透明");
                            
                            tracing::info!("🔧 准备设置窗口透明度");
                            let _: () = msg_send![ns_window, setAlphaValue: 1.0f64];
                            tracing::info!("✅ 成功设置窗口透明度为完全不透明");
                            
                            // 使用最基本的显示方法
                            tracing::info!("🔧 准备执行 orderFrontRegardless");
                            let _: () = msg_send![ns_window, orderFrontRegardless];
                            tracing::info!("✅ 成功执行 orderFrontRegardless");
                            
                            // 等待一小段时间让窗口系统处理
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            
                            // 安全地激活应用程序
                            tracing::info!("🔧 准备获取 NSApplication 类");
                            if let Some(app_class) = runtime::Class::get("NSApplication") {
                                tracing::info!("✅ 成功获取 NSApplication 类");
                                tracing::info!("🔧 准备获取共享应用实例");
                                let shared_app: id = msg_send![app_class, sharedApplication];
                                tracing::info!("✅ 成功获取共享应用实例");
                                tracing::info!("🔧 准备激活应用程序忽略其他应用");
                                let _: () = msg_send![shared_app, activateIgnoringOtherApps: true];
                                tracing::info!("✅ 成功激活应用程序忽略其他应用");
                            } else {
                                tracing::warn!("⚠️ 无法获取 NSApplication 类");
                            }
                            
                            // 最后设置为关键窗口（这一步比较安全）
                            tracing::info!("🔧 准备设置为关键窗口");
                            let _: () = msg_send![ns_window, makeKeyWindow];
                            tracing::info!("✅ 成功设置为关键窗口");
                            
                            // 获取显示后的状态
                            tracing::info!("🔧 准备获取显示后的窗口状态");
                            let level_after: i32 = msg_send![ns_window, level];
                            let visible_after: bool = msg_send![ns_window, isVisible];
                            let is_key_after: bool = msg_send![ns_window, isKeyWindow];
                            tracing::info!("🔍 显示后状态 - 级别: {}, 可见: {}, 关键窗口: {}", 
                                          level_after, visible_after, is_key_after);
                            tracing::info!("✅ 窗口显示流程全部完成");
                        }
                    } else {
                        tracing::error!("❌ 无法获取原生窗口句柄");
                    }
                }
                
                // 使用 Tauri 的方法再次确保焦点
                let _ = window.set_focus();
                
                tracing::info!("✅ 窗口显示完成，现在设置覆盖级别");
                
                // 现在只设置窗口级别，不重复其他属性
                if let Err(e) = set_window_level_only(app) {
                    tracing::warn!("❌ 设置窗口级别失败: {}, 但窗口已显示", e);
                }
                
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
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use objc::runtime::Class;

#[cfg(target_os = "macos")]
use cocoa::{
    appkit::{NSRunningApplication, NSApplicationActivationOptions},
    base::{id, nil},
    foundation::NSString,
};

#[cfg(target_os = "macos")]
use tauri_nspanel::ManagerExt;

// 全局变量存储前一个活动窗口的进程 ID
static PREVIOUS_WINDOW: Mutex<Option<i32>> = Mutex::new(None);

// 简化的应用观察器启动函数
pub fn start_app_observer() {
    tracing::info!("🍎 macOS 粘贴模块已初始化");
    // 暂时不实现复杂的应用切换监听，专注于粘贴功能
}

// 获取前一个活动窗口的进程 ID
pub fn get_previous_window() -> Option<i32> {
    PREVIOUS_WINDOW.lock().ok().and_then(|guard| *guard)
}

// 设置前一个活动窗口的进程 ID（供测试使用）
pub fn set_previous_window(pid: i32) {
    if let Ok(mut previous) = PREVIOUS_WINDOW.lock() {
        *previous = Some(pid);
        tracing::info!("🎯 设置前一个活动应用 PID: {}", pid);
    }
}

// 通过 PID 激活应用程序 - 使用 Cocoa API（超快！）
#[cfg(target_os = "macos")]
pub fn activate_application_by_pid(pid: i32) -> Result<(), String> {
    let start = std::time::Instant::now();
    
    unsafe {
        let workspace_class = Class::get("NSWorkspace").ok_or("无法获取 NSWorkspace 类")?;
        let workspace: id = msg_send![workspace_class, sharedWorkspace];
        let apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];
        
        for i in 0..count {
            let app: id = msg_send![apps, objectAtIndex:i];
            let app_pid: i32 = msg_send![app, processIdentifier];
            
            if app_pid == pid {
                let options = NSApplicationActivationOptions::NSApplicationActivateIgnoringOtherApps;
                let _: bool = msg_send![app, activateWithOptions:options];
                
                tracing::debug!("✅ 激活应用 PID: {} 耗时: {:?}", pid, start.elapsed());
                return Ok(());
            }
        }
        
        Err(format!("未找到 PID 为 {} 的应用", pid))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_application_by_pid(pid: i32) -> Result<(), String> {
    Err("仅支持 macOS 平台".to_string())
}

// 执行粘贴操作 - 参考 EcoPaste 的实现
pub fn paste(app_handle: Option<tauri::AppHandle>) -> Result<(), String> {
    let start = std::time::Instant::now();
    tracing::debug!("🍎 执行 macOS 粘贴操作...");
    
    // 关键：在粘贴前让 NSPanel resign（放弃激活状态）
    // 注意：必须在主线程调用
    #[cfg(target_os = "macos")]
    if let Some(app) = app_handle {
        // 克隆 AppHandle 以便在闭包中使用
        let _ = app.clone().run_on_main_thread(move || {
            if let Ok(panel) = app.get_webview_panel("main") {
                panel.resign_key_window();
                tracing::debug!("✅ NSPanel 已放弃焦点");
            }
        });
        
        // 等待一小段时间确保 resign 完成
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    
    // 使用 AppleScript 执行粘贴（因为 Panel 已 resign，会粘贴到目标应用）
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to keystroke \"v\" using command down")
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        tracing::info!("✅ 粘贴操作成功，耗时: {:?}", start.elapsed());
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ 粘贴操作失败: {}", error);
        Err(format!("粘贴操作失败: {}", error))
    }
}

// 获取当前前台应用的 PID - 使用 Cocoa API（超快！）
#[cfg(target_os = "macos")]
pub fn get_frontmost_app_pid() -> Result<i32, String> {
    let start = std::time::Instant::now();
    
    unsafe {
        let workspace_class = Class::get("NSWorkspace").ok_or("无法获取 NSWorkspace 类")?;
        let workspace: id = msg_send![workspace_class, sharedWorkspace];
        let frontmost_app: id = msg_send![workspace, frontmostApplication];
        
        if frontmost_app == nil {
            return Err("无法获取前台应用".to_string());
        }
        
        let pid: i32 = msg_send![frontmost_app, processIdentifier];
        
        tracing::debug!("⚡ 获取前台应用 PID 耗时: {:?}", start.elapsed());
        Ok(pid)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_frontmost_app_pid() -> Result<i32, String> {
    Err("仅支持 macOS 平台".to_string())
}

// 智能粘贴：激活目标应用程序，然后粘贴 - 超极速版本
pub fn smart_paste(app_handle: Option<tauri::AppHandle>) -> Result<(), String> {
    let total_start = std::time::Instant::now();
    tracing::info!("🧠 开始智能粘贴...");
    
    // 获取当前前台应用的 PID，保存为"前一个"应用
    let current_pid = match get_frontmost_app_pid() {
        Ok(pid) => {
            tracing::debug!("📱 当前前台应用 PID: {}", pid);
            set_previous_window(pid);
            pid
        }
        Err(e) => {
            tracing::warn!("⚠️ 无法获取当前前台应用: {}, 直接粘贴", e);
            return paste(app_handle);
        }
    };
    
    // 检查是否有之前保存的目标应用
    if let Some(previous_pid) = get_previous_window() {
        if previous_pid != current_pid {
            tracing::debug!("🎯 切换到目标应用 PID: {}", previous_pid);
            
            // 激活目标应用
            if let Err(e) = activate_application_by_pid(previous_pid) {
                tracing::warn!("⚠️ 激活目标应用失败: {}, 直接粘贴", e);
                return paste(app_handle);
            }
            
            // 优化：减少到 15ms（大多数应用已足够）
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    
    // 执行粘贴操作
    let result = paste(app_handle);
    tracing::info!("🚀 智能粘贴总耗时: {:?}", total_start.elapsed());
    result
}

// 根据应用类型获取合适的延时时间 - 超极速优化版本
fn get_optimal_delay_for_app(app_name: &str) -> u64 {
    // 常见的快速响应应用 - 几乎无延时
    let fast_apps = [
        "TextEdit", "Notes", "Terminal", "iTerm", "Code", "Visual Studio Code",
        "Sublime Text", "Atom", "Vim", "Emacs", "Finder", "Safari", "Chrome",
        "Firefox", "Messages", "Slack", "Discord", "Telegram", "Calculator",
        "Preview", "System Preferences", "Activity Monitor", "WeChat", "QQ"
    ];
    
    // 可能需要更多时间的应用
    let slow_apps = [
        "Photoshop", "Illustrator", "Final Cut Pro", "Logic Pro", "Xcode",
        "Android Studio", "IntelliJ IDEA", "Eclipse", "Unity", "Blender"
    ];
    
    let app_lower = app_name.to_lowercase();
    
    if fast_apps.iter().any(|&fast_app| app_lower.contains(&fast_app.to_lowercase())) {
        5  // 快速应用只需要 5ms - 超极速模式
    } else if slow_apps.iter().any(|&slow_app| app_lower.contains(&slow_app.to_lowercase())) {
        30  // 重型应用优化到 30ms
    } else {
        15  // 默认 15ms - 进一步优化
    }
}

// 智能粘贴到指定应用：先激活应用，再粘贴 - 超极速版本
pub fn smart_paste_to_app(app_name: &str, bundle_id: Option<&str>, app_handle: Option<tauri::AppHandle>) -> Result<(), String> {
    let total_start = std::time::Instant::now();
    tracing::info!("🎯 智能粘贴到应用: {} (bundle: {:?})", app_name, bundle_id);
    
    // 获取当前前台应用的 PID
    let current_pid = match get_frontmost_app_pid() {
        Ok(pid) => {
            tracing::debug!("📱 当前前台应用 PID: {}", pid);
            pid
        }
        Err(e) => {
            tracing::warn!("⚠️ 无法获取当前前台应用: {}", e);
            return Err(format!("无法获取当前应用信息: {}", e));
        }
    };
    
    // 使用 AppleScript 激活目标应用
    let activate_result = if let Some(bundle) = bundle_id {
        activate_application_by_bundle_id(bundle)
    } else {
        activate_application_by_name(app_name)
    };
    
    match activate_result {
        Ok(()) => {
            // 根据应用类型智能调整延时
            let delay = get_optimal_delay_for_app(app_name);
            tracing::debug!("⏱️ 为应用 {} 设置延时: {}ms", app_name, delay);
            std::thread::sleep(std::time::Duration::from_millis(delay));
            
            // 保存当前应用为"前一个"应用，以便后续切换回来
            set_previous_window(current_pid);
            
            // 执行粘贴操作
            let result = paste(app_handle);
            tracing::info!("🚀 智能粘贴总耗时: {:?}", total_start.elapsed());
            result
        }
        Err(e) => {
            tracing::error!("❌ 激活应用失败: {}", e);
            Err(format!("激活应用失败: {}", e))
        }
    }
}

// 通过应用名称激活应用 - 使用 Cocoa API（超快！）
#[cfg(target_os = "macos")]
pub fn activate_application_by_name(app_name: &str) -> Result<(), String> {
    let start = std::time::Instant::now();
    
    unsafe {
        let workspace_class = Class::get("NSWorkspace").ok_or("无法获取 NSWorkspace 类")?;
        let workspace: id = msg_send![workspace_class, sharedWorkspace];
        let apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];
        
        let search_name = NSString::alloc(nil);
        let search_name = NSString::init_str(search_name, app_name);
        
        for i in 0..count {
            let app: id = msg_send![apps, objectAtIndex:i];
            let localized_name: id = msg_send![app, localizedName];
            
            if localized_name != nil {
                let is_equal: bool = msg_send![localized_name, isEqualToString:search_name];
                if is_equal {
                    let options = NSApplicationActivationOptions::NSApplicationActivateIgnoringOtherApps;
                    let _: bool = msg_send![app, activateWithOptions:options];
                    
                    tracing::debug!("✅ 激活应用 {} 耗时: {:?}", app_name, start.elapsed());
                    return Ok(());
                }
            }
        }
        
        Err(format!("未找到应用: {}", app_name))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_application_by_name(app_name: &str) -> Result<(), String> {
    Err("仅支持 macOS 平台".to_string())
}

// 通过 Bundle ID 激活应用 - 使用 Cocoa API（超快！）  
#[cfg(target_os = "macos")]
pub fn activate_application_by_bundle_id(bundle_id: &str) -> Result<(), String> {
    let start = std::time::Instant::now();
    
    unsafe {
        let workspace_class = Class::get("NSWorkspace").ok_or("无法获取 NSWorkspace 类")?;
        let workspace: id = msg_send![workspace_class, sharedWorkspace];
        let bundle_string = NSString::alloc(nil);
        let bundle_string = NSString::init_str(bundle_string, bundle_id);
        
        // 使用 launchApplicationAtURL 或 runningApplications 查找
        let apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![apps, count];
        
        for i in 0..count {
            let app: id = msg_send![apps, objectAtIndex:i];
            let app_bundle: id = msg_send![app, bundleIdentifier];
            
            if app_bundle != nil {
                let is_equal: bool = msg_send![app_bundle, isEqualToString:bundle_string];
                if is_equal {
                    let options = NSApplicationActivationOptions::NSApplicationActivateIgnoringOtherApps;
                    let _: bool = msg_send![app, activateWithOptions:options];
                    
                    tracing::debug!("✅ 激活应用 (Bundle: {}) 耗时: {:?}", bundle_id, start.elapsed());
                    return Ok(());
                }
            }
        }
        
        Err(format!("未找到 Bundle ID: {}", bundle_id))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_application_by_bundle_id(bundle_id: &str) -> Result<(), String> {
    Err("仅支持 macOS 平台".to_string())
}
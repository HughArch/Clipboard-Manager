use std::sync::Mutex;

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

// 通过 PID 激活应用程序
pub fn activate_application_by_pid(pid: i32) -> Result<(), String> {
    let script = format!(
        r#"
        tell application "System Events"
            set targetApp to first application process whose unix id is {}
            set frontmost of targetApp to true
        end tell
        "#,
        pid
    );
    
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        tracing::info!("✅ 成功激活应用程序 PID: {}", pid);
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ 激活应用程序失败: {}", error);
        Err(format!("激活应用程序失败: {}", error))
    }
}

// 执行粘贴操作
pub fn paste() -> Result<(), String> {
    tracing::info!("🍎 执行 macOS 粘贴操作...");
    
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to keystroke \"v\" using command down")
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        tracing::info!("✅ 粘贴操作成功");
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ 粘贴操作失败: {}", error);
        Err(format!("粘贴操作失败: {}", error))
    }
}

// 获取当前前台应用的 PID - 极速优化版本
pub fn get_frontmost_app_pid() -> Result<i32, String> {
    // 使用更简洁的 AppleScript，减少执行时间
    let script = "tell app \"System Events\" to get unix id of first process whose frontmost is true";
    
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("获取前台应用失败: {}", e))?;
    
    if output.status.success() {
        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        pid_str.parse::<i32>()
            .map_err(|e| format!("解析 PID 失败: {}", e))
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(format!("获取前台应用失败: {}", error))
    }
}

// 智能粘贴：激活目标应用程序，然后粘贴 - 极速版本
pub fn smart_paste() -> Result<(), String> {
    tracing::info!("🧠 开始智能粘贴...");
    
    // 获取当前前台应用的 PID，保存为"前一个"应用
    let current_pid = match get_frontmost_app_pid() {
        Ok(pid) => {
            tracing::info!("📱 当前前台应用 PID: {}", pid);
            set_previous_window(pid);
            pid
        }
        Err(e) => {
            tracing::warn!("⚠️ 无法获取当前前台应用: {}, 直接粘贴", e);
            return paste();
        }
    };
    
    // 检查是否有之前保存的目标应用
    if let Some(previous_pid) = get_previous_window() {
        if previous_pid != current_pid {
            tracing::info!("🎯 切换到目标应用 PID: {}", previous_pid);
            
            // 激活目标应用
            if let Err(e) = activate_application_by_pid(previous_pid) {
                tracing::warn!("⚠️ 激活目标应用失败: {}, 直接粘贴", e);
                return paste();
            }
            
            // 极速模式：只等待 20ms
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    
    // 执行粘贴操作
    paste()
}

// 根据应用类型获取合适的延时时间 - 极速优化版本
fn get_optimal_delay_for_app(app_name: &str) -> u64 {
    // 常见的快速响应应用 - 几乎无延时
    let fast_apps = [
        "TextEdit", "Notes", "Terminal", "iTerm", "Code", "Visual Studio Code",
        "Sublime Text", "Atom", "Vim", "Emacs", "Finder", "Safari", "Chrome",
        "Firefox", "Messages", "Slack", "Discord", "Telegram", "Calculator",
        "Preview", "System Preferences", "Activity Monitor"
    ];
    
    // 可能需要更多时间的应用
    let slow_apps = [
        "Photoshop", "Illustrator", "Final Cut Pro", "Logic Pro", "Xcode",
        "Android Studio", "IntelliJ IDEA", "Eclipse", "Unity", "Blender"
    ];
    
    let app_lower = app_name.to_lowercase();
    
    if fast_apps.iter().any(|&fast_app| app_lower.contains(&fast_app.to_lowercase())) {
        10  // 快速应用只需要 10ms - 极速模式
    } else if slow_apps.iter().any(|&slow_app| app_lower.contains(&slow_app.to_lowercase())) {
        50  // 重型应用也只需要 50ms
    } else {
        25  // 默认 25ms - 大幅减少
    }
}

// 智能粘贴到指定应用：先激活应用，再粘贴
pub fn smart_paste_to_app(app_name: &str, bundle_id: Option<&str>) -> Result<(), String> {
    tracing::info!("🎯 智能粘贴到应用: {} (bundle: {:?})", app_name, bundle_id);
    
    // 获取当前前台应用的 PID
    let current_pid = match get_frontmost_app_pid() {
        Ok(pid) => {
            tracing::info!("📱 当前前台应用 PID: {}", pid);
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
            tracing::info!("✅ 成功激活目标应用: {}", app_name);
            
            // 根据应用类型智能调整延时
            let delay = get_optimal_delay_for_app(app_name);
            tracing::info!("⏱️ 为应用 {} 设置延时: {}ms", app_name, delay);
            std::thread::sleep(std::time::Duration::from_millis(delay));
            
            // 保存当前应用为"前一个"应用，以便后续切换回来
            set_previous_window(current_pid);
            
            // 执行粘贴操作
            paste()
        }
        Err(e) => {
            tracing::error!("❌ 激活应用失败: {}", e);
            Err(format!("激活应用失败: {}", e))
        }
    }
}

// 通过应用名称激活应用 - 极速优化版本
pub fn activate_application_by_name(app_name: &str) -> Result<(), String> {
    // 使用更简洁的 AppleScript，减少执行时间
    let script = format!("tell app \"{}\" to activate", app_name.replace("\"", "\\\""));
    
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        tracing::info!("✅ 成功激活应用程序: {}", app_name);
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ 激活应用程序失败: {}", error);
        Err(format!("激活应用程序失败: {}", error))
    }
}

// 通过 Bundle ID 激活应用 - 极速优化版本
pub fn activate_application_by_bundle_id(bundle_id: &str) -> Result<(), String> {
    // 使用更简洁的 AppleScript，减少执行时间
    let script = format!("tell app id \"{}\" to activate", bundle_id.replace("\"", "\\\""));
    
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        tracing::info!("✅ 成功激活应用程序 (Bundle ID): {}", bundle_id);
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ 激活应用程序失败: {}", error);
        Err(format!("激活应用程序失败: {}", error))
    }
}
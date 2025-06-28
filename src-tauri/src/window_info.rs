use std::time::Duration;
use std::sync::{Arc, RwLock};
use crate::types::SourceAppInfo;
#[cfg(target_os = "windows")]
use crate::icon_cache::get_icon_cache;
#[cfg(target_os = "windows")]
use crate::resource_manager::WindowsResourceManager;
#[cfg(target_os = "windows")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(target_os = "windows")]
use image::ImageEncoder;

#[cfg(target_os = "windows")]
use winapi::um::{
    winuser::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, GetDC},
    processthreadsapi::OpenProcess,
    handleapi::CloseHandle,
    psapi::GetModuleFileNameExW,
    shellapi::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, ExtractIconExW},
    winnt::PROCESS_QUERY_INFORMATION,
    wingdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, GetDIBits, BITMAPINFOHEADER, BITMAPINFO, DIB_RGB_COLORS, BI_RGB, SetStretchBltMode},
    winuser::FillRect,
};
#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStringExt;

// 添加限流机制，避免频繁获取窗口信息
static LAST_WINDOW_INFO_CALL: std::sync::OnceLock<Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>>> = std::sync::OnceLock::new();

pub fn get_last_window_info() -> &'static Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>> {
    LAST_WINDOW_INFO_CALL.get_or_init(|| {
        Arc::new(RwLock::new((std::time::Instant::now() - Duration::from_secs(10), None)))
    })
}

// 获取当前活动窗口的应用程序信息（增加限流）
#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn get_active_window_info() -> Result<SourceAppInfo, String> {
    println!("🔍 get_active_window_info() 被调用");
    
    // 减少缓存时间以适应剪贴板监听需求（每2秒最多调用一次）
    let cache_duration = Duration::from_secs(2);
    
    if let Ok(guard) = get_last_window_info().read() {
        if guard.0.elapsed() < cache_duration {
            if let Some(ref cached_info) = guard.1 {
                println!("📋 使用缓存的窗口信息: {}", cached_info.name);
                return Ok(cached_info.clone());
            }
        }
    }

    println!("🔄 开始获取新的窗口信息...");
    let new_info = get_active_window_info_impl();
    println!("✅ 获取到窗口信息: 名称='{}', 图标='{}'", new_info.name, if new_info.icon.is_some() { "有" } else { "无" });
    
    // 更新缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.0 = std::time::Instant::now();
        guard.1 = Some(new_info.clone());
        println!("💾 窗口信息已缓存");
    }

    Ok(new_info)
}

// 专门用于剪贴板监听的窗口信息获取函数（不使用缓存）
#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn get_active_window_info_for_clipboard() -> Result<SourceAppInfo, String> {
    println!("🔍 get_active_window_info_for_clipboard() 被调用（无缓存）");
    
    let new_info = get_active_window_info_impl();
    println!("✅ 剪贴板专用：获取到窗口信息: 名称='{}', 图标='{}'", new_info.name, if new_info.icon.is_some() { "有" } else { "无" });
    
    Ok(new_info)
}

#[cfg(target_os = "windows")]
fn get_active_window_info_impl() -> SourceAppInfo {
    println!("🪟 开始实现获取活动窗口信息...");
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            println!("❌ 无法获取前台窗口句柄");
            return SourceAppInfo {
                name: "Unknown".to_string(),
                icon: None,
                bundle_id: None,
            };
        }
        println!("✅ 获取到前台窗口句柄: {:?}", hwnd);

        // 获取窗口标题
        let mut window_title = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, window_title.as_mut_ptr(), window_title.len() as i32);
        let window_title_str = if title_len > 0 {
            OsString::from_wide(&window_title[..title_len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            "Empty".to_string()
        };
        println!("📝 窗口标题: '{}'", window_title_str);
        
        // 获取进程ID
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        println!("🆔 进程ID: {}", process_id);
        
        // 打开进程句柄
        let process_handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, process_id);
        if process_handle.is_null() {
            println!("❌ 无法打开进程句柄，使用窗口标题作为应用名");
            let title = if title_len > 0 {
                OsString::from_wide(&window_title[..title_len as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                "Unknown".to_string()
            };
            return SourceAppInfo {
                name: title,
                icon: None,
                bundle_id: None,
            };
        }
        println!("✅ 成功打开进程句柄: {:?}", process_handle);

        // 获取进程可执行文件路径
        let mut exe_path = [0u16; 256];
        let path_len = GetModuleFileNameExW(process_handle, ptr::null_mut(), exe_path.as_mut_ptr(), exe_path.len() as u32);
        
        CloseHandle(process_handle);

        let (app_name, exe_path_str) = if path_len > 0 {
            let path_os = OsString::from_wide(&exe_path[..path_len as usize]);
            let path_str = path_os.to_string_lossy().to_string();
            println!("📂 可执行文件路径: '{}'", path_str);
            
            // 提取文件名（不包含扩展名）
            let name = if let Some(file_name) = std::path::Path::new(&path_str).file_stem() {
                file_name.to_string_lossy().to_string()
            } else {
                "Unknown".to_string()
            };
            println!("📛 提取的应用名: '{}'", name);
            (name, Some(path_str))
        } else if title_len > 0 {
            // 如果无法获取进程路径，使用窗口标题
            println!("⚠️  无法获取可执行文件路径，使用窗口标题");
            let title = OsString::from_wide(&window_title[..title_len as usize])
                .to_string_lossy()
                .to_string();
            (title, None)
        } else {
            println!("❌ 无法获取进程信息和窗口标题");
            ("Unknown".to_string(), None)
        };

        // 获取应用程序图标（使用改进的缓存）
        let icon_base64 = if let Some(exe_path_str) = exe_path_str {
            println!("🎨 开始获取应用图标...");
            
            // 先检查缓存
            let icon_cache = get_icon_cache();
            if let Ok(mut cache) = icon_cache.write() {
                if let Some(cached_icon) = cache.get(&exe_path_str) {
                    println!("📋 使用缓存的图标");
                    cached_icon
                } else {
                    println!("🔄 获取新图标...");
                    // 获取图标
                    let icon = get_app_icon_base64(&exe_path[..path_len as usize]);
                    if icon.is_some() {
                        println!("✅ 成功获取图标，长度: {}", icon.as_ref().unwrap().len());
                    } else {
                        println!("❌ 获取图标失败");
                    }
                    cache.insert(exe_path_str, icon.clone());
                    icon
                }
            } else {
                println!("❌ 无法访问图标缓存，直接获取");
                get_app_icon_base64(&exe_path[..path_len as usize])
            }
        } else {
            println!("⚠️  没有可执行文件路径，跳过图标获取");
            None
        };

        let result = SourceAppInfo {
            name: app_name,
            icon: icon_base64,
            bundle_id: None, // Windows 下没有 bundle_id
        };
        
        println!("🎯 最终结果: 名称='{}', 图标={}", result.name, if result.icon.is_some() { "有" } else { "无" });
        result
    }
}

#[cfg(target_os = "windows")]
pub fn get_app_icon_base64(exe_path: &[u16]) -> Option<String> {
    println!("🎨 开始获取应用图标 (get_app_icon_base64)");
    // 使用资源管理器确保所有图标都被正确释放
    let mut resource_manager = WindowsResourceManager::new();
    
    unsafe {
        // 方法1: 尝试获取高质量大图标 (通过指定更大的尺寸)
        let mut large_icons: [winapi::shared::windef::HICON; 1] = [ptr::null_mut()];
        let mut small_icons: [winapi::shared::windef::HICON; 1] = [ptr::null_mut()];
        
        // 首先尝试获取高质量大图标
        let icon_count = ExtractIconExW(
            exe_path.as_ptr(),
            0, // 提取第一个图标
            large_icons.as_mut_ptr(),
            small_icons.as_mut_ptr(),
            1
        );

        if icon_count > 0 && !large_icons[0].is_null() {
            println!("✅ 通过ExtractIconExW获取到大图标");
            // 注册图标资源到管理器
            resource_manager.track_icon(large_icons[0]);
            if !small_icons[0].is_null() {
                resource_manager.track_icon(small_icons[0]);
            }
            
            let icon_base64 = hicon_to_base64(large_icons[0]);
            
            if icon_base64.is_some() {
                println!("✅ 大图标转换成功");
                return icon_base64;
            }
        } else {
            // 如果获取失败但有图标句柄，也要注册以确保清理
            if !large_icons[0].is_null() {
                resource_manager.track_icon(large_icons[0]);
            }
            if !small_icons[0].is_null() {
                resource_manager.track_icon(small_icons[0]);
            }
        }

        // 方法2: 尝试通过SHGetFileInfoW获取超大图标
        let mut shfi: SHFILEINFOW = std::mem::zeroed();
        
        // 首先尝试获取超大图标 (SHGFI_LARGEICON | SHGFI_SHELLICONSIZE)
        let result = SHGetFileInfoW(
            exe_path.as_ptr(),
            0,
            &mut shfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | 0x0004, // SHGFI_SHELLICONSIZE
        );

        if result != 0 && !shfi.hIcon.is_null() {
            println!("✅ 通过SHGetFileInfoW获取到超大图标");
            // 注册图标到资源管理器
            resource_manager.track_icon(shfi.hIcon);
            let icon_base64 = hicon_to_base64(shfi.hIcon);
            if icon_base64.is_some() {
                println!("✅ 超大图标转换成功");
                return icon_base64;
            }
        }

        // 方法3: 回退到标准大图标
        shfi = std::mem::zeroed();
        let result = SHGetFileInfoW(
            exe_path.as_ptr(),
            0,
            &mut shfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result != 0 && !shfi.hIcon.is_null() {
            println!("✅ 通过SHGetFileInfoW获取到标准大图标");
            // 注册图标到资源管理器
            resource_manager.track_icon(shfi.hIcon);
            let icon_base64 = hicon_to_base64(shfi.hIcon);
            if icon_base64.is_some() {
                println!("✅ 标准大图标转换成功");
                return icon_base64;
            }
        }

        println!("❌ 所有图标获取方法都失败了");
        None
        
        // 所有图标资源将由resource_manager的Drop trait自动清理
    }
}

#[cfg(target_os = "windows")]
pub fn hicon_to_base64(hicon: winapi::shared::windef::HICON) -> Option<String> {
    use std::mem;
    
    println!("🖼️  开始转换图标为base64 (hicon_to_base64)");
    // 使用资源管理器确保所有资源都被正确释放
    let mut resource_manager = WindowsResourceManager::new();
    
    unsafe {
        // 使用更大的图标尺寸以提高清晰度
        let icon_size = 48; // 增加到48像素以获得更清晰的图标
        
        // 获取屏幕 DC
        let screen_dc = GetDC(ptr::null_mut());
        if screen_dc.is_null() {
            println!("警告: 无法获取屏幕DC");
            return None;
        }
        resource_manager.track_dc(screen_dc);

        // 创建兼容的内存 DC
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            println!("警告: 无法创建内存DC");
            return None;
        }
        resource_manager.track_dc(mem_dc);

        // 创建位图
        let bitmap = CreateCompatibleBitmap(screen_dc, icon_size, icon_size);
        if bitmap.is_null() {
            println!("警告: 无法创建位图");
            return None;
        }
        resource_manager.track_handle(bitmap as winapi::shared::windef::HGDIOBJ);

        // 选择位图到内存 DC
        let old_bitmap = SelectObject(mem_dc, bitmap as *mut winapi::ctypes::c_void);
        
        // 填充白色背景
        let white_brush = winapi::um::wingdi::CreateSolidBrush(0xFFFFFF);
        if !white_brush.is_null() {
            let rect = winapi::shared::windef::RECT {
                left: 0,
                top: 0,
                right: icon_size,
                bottom: icon_size,
            };
            FillRect(mem_dc, &rect, white_brush);
            winapi::um::wingdi::DeleteObject(white_brush as *mut winapi::ctypes::c_void);
        }

        // 设置高质量绘制模式
        SetStretchBltMode(mem_dc, 4); // HALFTONE mode for better quality
        
        // 绘制图标到位图，使用高质量设置
        let draw_result = winapi::um::winuser::DrawIconEx(
            mem_dc, 
            0, 
            0, 
            hicon, 
            icon_size, 
            icon_size, 
            0, 
            ptr::null_mut(), 
            0x0003 // DI_NORMAL
        );
        
        println!("🎨 DrawIconEx结果: {}", if draw_result != 0 { "成功" } else { "失败" });

        let result = if draw_result != 0 {
            // 准备位图信息结构
            let mut bitmap_info: BITMAPINFO = mem::zeroed();
            bitmap_info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
            bitmap_info.bmiHeader.biWidth = icon_size;
            bitmap_info.bmiHeader.biHeight = -icon_size; // 负值表示自上而下
            bitmap_info.bmiHeader.biPlanes = 1;
            bitmap_info.bmiHeader.biBitCount = 32; // RGBA
            bitmap_info.bmiHeader.biCompression = BI_RGB;

            // 分配缓冲区
            let buffer_size = (icon_size * icon_size * 4) as usize;
            let mut buffer: Vec<u8> = vec![0; buffer_size];

            // 获取位图数据
            let lines = GetDIBits(
                mem_dc,
                bitmap,
                0,
                icon_size as u32,
                buffer.as_mut_ptr() as *mut winapi::ctypes::c_void,
                &mut bitmap_info,
                DIB_RGB_COLORS,
            );

            if lines > 0 {
                // 转换 BGRA 到 RGBA 并编码为 PNG
                convert_bgra_to_png_base64(&buffer, icon_size as u32, icon_size as u32)
            } else {
                None
            }
        } else {
            None
        };

        // 恢复原始位图对象
        if !old_bitmap.is_null() {
            SelectObject(mem_dc, old_bitmap);
        }

        // 资源自动清理由WindowsResourceManager的Drop trait处理
        // 这确保了即使函数提前返回，所有资源都会被正确释放
        result
    }
}

#[cfg(target_os = "windows")]
pub fn convert_bgra_to_png_base64(bgra_data: &[u8], width: u32, height: u32) -> Option<String> {
    println!("🔄 开始转换BGRA到PNG, 尺寸: {}x{}", width, height);
    
    // 转换 BGRA 到 RGBA，并处理预乘alpha问题
    let mut rgba_data = Vec::with_capacity(bgra_data.len());
    for chunk in bgra_data.chunks_exact(4) {
        let b = chunk[0] as f32;
        let g = chunk[1] as f32;
        let r = chunk[2] as f32;
        let a = chunk[3] as f32;
        
        // 如果alpha不为0，进行反预乘处理以恢复真实颜色
        if a > 0.0 {
            let alpha_factor = 255.0 / a;
            rgba_data.push((r * alpha_factor).min(255.0) as u8); // R
            rgba_data.push((g * alpha_factor).min(255.0) as u8); // G
            rgba_data.push((b * alpha_factor).min(255.0) as u8); // B
            rgba_data.push(a as u8); // A
        } else {
            // 透明像素保持原样
            rgba_data.push(r as u8); // R
            rgba_data.push(g as u8); // G
            rgba_data.push(b as u8); // B
            rgba_data.push(a as u8); // A
        }
    }

    // 使用 image crate 编码为PNG，采用高质量设置
    let img = image::RgbaImage::from_raw(width, height, rgba_data)?;
    let mut png_buffer = Vec::new();
    
    // 使用高质量PNG编码设置
    let encoder = image::codecs::png::PngEncoder::new(&mut png_buffer);
    
    if encoder.write_image(&img, width, height, image::ColorType::Rgba8).is_ok() {
        let base64_string = general_purpose::STANDARD.encode(&png_buffer);
        println!("✅ PNG转换成功，大小: {} bytes", png_buffer.len());
        Some(format!("data:image/png;base64,{}", base64_string))
    } else {
        println!("❌ PNG编码失败");
        None
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_active_window_info() -> Result<SourceAppInfo, String> {
    use std::process::Command;
    
    println!("🔍 macOS: 获取当前活动窗口信息");
    
    // 使用 AppleScript 获取当前活动应用程序的信息
    let script = r#"
tell application "System Events"
    set frontApp to first application process whose frontmost is true
    set appName to name of frontApp
    set appBundleID to bundle identifier of frontApp
    return appName & "|" & appBundleID
end tell
    "#;
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let parts: Vec<&str> = result.split('|').collect();
        
        if parts.len() >= 2 {
            let app_name = parts[0].to_string();
            let bundle_id = parts[1].to_string();
            
            println!("✅ 获取到活动应用: {} ({})", app_name, bundle_id);
            
            // 获取应用图标
            let app_icon = get_app_icon_base64_macos(&bundle_id);
            if app_icon.is_some() {
                println!("✅ 成功获取应用图标");
            } else {
                println!("⚠️ 无法获取应用图标");
            }
            
            Ok(SourceAppInfo {
                name: app_name,
                icon: app_icon,
                bundle_id: Some(bundle_id),
            })
        } else {
            println!("⚠️ 解析应用信息失败: {}", result);
            Ok(SourceAppInfo {
                name: result,
                icon: None,
                bundle_id: None,
            })
        }
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        println!("❌ 获取活动窗口失败: {}", error_msg);
            Ok(SourceAppInfo {
        name: "Unknown".to_string(),
        icon: None,
        bundle_id: None,
    })
}

// 专门用于剪贴板监听的窗口信息获取函数（不使用缓存）
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn get_active_window_info_for_clipboard() -> Result<SourceAppInfo, String> {
    use std::process::Command;
    
    println!("🔍 Linux: 获取当前活动窗口信息（剪贴板专用，无缓存）");
    
    // 尝试使用 xdotool 获取活动窗口信息
    let window_id_output = Command::new("xdotool")
        .args(&["getactivewindow"])
        .output();
    
    match window_id_output {
        Ok(output) if output.status.success() => {
            let window_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            
            // 获取窗口名称
            let window_name_output = Command::new("xdotool")
                .args(&["getwindowname", &window_id])
                .output();
            
            if let Ok(name_output) = window_name_output {
                if name_output.status.success() {
                    let window_name = String::from_utf8_lossy(&name_output.stdout).trim().to_string();
                    println!("✅ 剪贴板专用：获取到活动窗口: {}", window_name);
                    
                    return Ok(SourceAppInfo {
                        name: window_name,
                        icon: None,
                        bundle_id: None,
                    });
                }
            }
        }
        _ => {
            println!("⚠️ xdotool 不可用，回退到默认值");
        }
    }
    
    Ok(SourceAppInfo {
        name: "Unknown".to_string(),
        icon: None,
        bundle_id: None,
    })
    }
}

// macOS 专用：根据 bundle ID 获取应用图标
#[cfg(target_os = "macos")]
fn get_app_icon_base64_macos(bundle_id: &str) -> Option<String> {
    use std::process::Command;
    
    println!("🎨 macOS: 开始获取应用图标，bundle_id: {}", bundle_id);
    
    // 方法1：使用 mdfind 查找应用路径
    let find_output = Command::new("mdfind")
        .arg(format!("kMDItemCFBundleIdentifier=={}", bundle_id))
        .output();
    
    if let Ok(result) = find_output {
        if result.status.success() {
            let app_paths = String::from_utf8_lossy(&result.stdout);
            let app_path = app_paths.lines().next();
            
            if let Some(path) = app_path {
                println!("📁 macOS: 找到应用路径: {}", path);
                return get_icon_from_app_path(path);
            }
        }
    }
    
    // 方法2：回退到通过 System Events 获取路径
    println!("🔄 macOS: 尝试备用方法...");
    get_app_icon_simple_macos(bundle_id)
}

// 从应用路径提取图标
#[cfg(target_os = "macos")]
fn get_icon_from_app_path(app_path: &str) -> Option<String> {
    use std::process::Command;
    use std::time::Duration;
    
    println!("🔍 macOS: 从应用路径提取图标: {}", app_path);
    
    // 方法1: 直接提取 .app bundle 中的 icon 文件
    let icon_paths = vec![
        format!("{}/Contents/Resources/AppIcon.icns", app_path),
        format!("{}/Contents/Resources/icon.icns", app_path),
        format!("{}/Contents/Resources/application.icns", app_path),
        format!("{}/Contents/Resources/App.icns", app_path),
        format!("{}/Contents/Resources/{}.icns", 
            app_path,
            std::path::Path::new(app_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app")
                .replace(".app", "")
        ),
    ];
    
    for icon_path in &icon_paths {
        if std::path::Path::new(icon_path).exists() {
            println!("📁 macOS: 找到图标文件: {}", icon_path);
            
            if let Some(icon_data) = extract_icon_with_sips(icon_path) {
                return Some(icon_data);
            }
        }
    }
    
    // 方法2: 使用更简单的 iconutil 方法
    println!("🔄 macOS: 尝试 iconutil 方法...");
    if let Some(icon_data) = extract_icon_with_iconutil(app_path) {
        return Some(icon_data);
    }
    
    // 方法3: 最后尝试 osascript 获取图标（避免使用 qlmanage）
    println!("🔄 macOS: 尝试 AppleScript 方法...");
    if let Some(icon_data) = extract_icon_with_applescript(app_path) {
        return Some(icon_data);
    }
    
    println!("❌ macOS: 所有图标提取方法都失败了");
    None
}

// 使用 sips 提取图标（macOS 原生方法）
#[cfg(target_os = "macos")]
fn extract_icon_with_sips(icon_path: &str) -> Option<String> {
    use std::process::Command;
    
    let tmp_png = format!("/tmp/clipboard_icon_{}.png", std::process::id());
    
    println!("🔧 macOS: 使用 sips 转换图标: {} -> {}", icon_path, tmp_png);
    
    // 直接使用 sips 命令，不依赖 timeout
    let sips_output = Command::new("sips")
        .args(&["-s", "format", "png", "-Z", "64", icon_path, "--out", &tmp_png])
        .output();
    
    match sips_output {
        Ok(result) if result.status.success() => {
            println!("✅ macOS: sips 转换成功");
            
            // 检查输出文件是否存在
            if std::path::Path::new(&tmp_png).exists() {
                // 转换为 base64
                let b64_output = Command::new("base64")
                    .args(&["-i", &tmp_png])
                    .output();
                
                let base64_result = match b64_output {
                    Ok(b64_result) if b64_result.status.success() => {
                        let base64_data = String::from_utf8_lossy(&b64_result.stdout)
                            .trim()
                            .replace("\n", "");
                        
                        if !base64_data.is_empty() {
                            Some(format!("data:image/png;base64,{}", base64_data))
                        } else {
                            None
                        }
                    }
                    Ok(b64_result) => {
                        println!("⚠️ macOS: base64 转换失败: {}", String::from_utf8_lossy(&b64_result.stderr));
                        None
                    }
                    Err(e) => {
                        println!("❌ macOS: base64 命令执行失败: {}", e);
                        None
                    }
                };
                
                // 清理临时文件
                let _ = Command::new("rm").arg(&tmp_png).output();
                
                if base64_result.is_some() {
                    println!("✅ macOS: 成功从 icns 提取图标");
                }
                
                base64_result
            } else {
                println!("⚠️ macOS: sips 没有生成输出文件");
                None
            }
        }
        Ok(result) => {
            println!("⚠️ macOS: sips 转换失败，返回码: {}", result.status);
            println!("⚠️ macOS: sips stderr: {}", String::from_utf8_lossy(&result.stderr));
            println!("⚠️ macOS: sips stdout: {}", String::from_utf8_lossy(&result.stdout));
            // 清理可能的临时文件
            let _ = Command::new("rm").arg(&tmp_png).output();
            None
        }
        Err(e) => {
            println!("❌ macOS: sips 命令执行失败: {}", e);
            None
        }
    }
}

// 使用 iconutil 提取图标
#[cfg(target_os = "macos")]
fn extract_icon_with_iconutil(app_path: &str) -> Option<String> {
    use std::process::Command;
    
    // 查找 .iconset 目录
    let iconset_path = format!("{}/Contents/Resources/AppIcon.iconset", app_path);
    
    if std::path::Path::new(&iconset_path).exists() {
        println!("📁 macOS: 找到 iconset: {}", iconset_path);
        
        let tmp_png = format!("/tmp/clipboard_iconset_{}.png", std::process::id());
        
        // 直接复制一个合适大小的图标文件
        let icon_files = vec![
            format!("{}/icon_64x64.png", iconset_path),
            format!("{}/icon_32x32@2x.png", iconset_path),
            format!("{}/icon_32x32.png", iconset_path),
            format!("{}/icon_16x16@2x.png", iconset_path),
        ];
        
        for icon_file in &icon_files {
            if std::path::Path::new(icon_file).exists() {
                println!("📁 macOS: 找到图标文件: {}", icon_file);
                
                // 直接复制文件
                let cp_output = Command::new("cp")
                    .args(&[icon_file, &tmp_png])
                    .output();
                
                if let Ok(result) = cp_output {
                    if result.status.success() {
                        // 转换为 base64
                        let b64_output = Command::new("base64")
                            .args(&["-i", &tmp_png])
                            .output();
                        
                        if let Ok(b64_result) = b64_output {
                            if b64_result.status.success() {
                                let base64_data = String::from_utf8_lossy(&b64_result.stdout)
                                    .trim()
                                    .replace("\n", "");
                                
                                // 清理临时文件
                                let _ = Command::new("rm").arg(&tmp_png).output();
                                
                                if !base64_data.is_empty() {
                                    println!("✅ macOS: 成功从 iconset 提取图标");
                                    return Some(format!("data:image/png;base64,{}", base64_data));
                                }
                            }
                        }
                    }
                }
                
                // 清理临时文件
                let _ = Command::new("rm").arg(&tmp_png).output();
                break;
            }
        }
    }
    
    None
}

// 使用 osascript 提取图标（简化方法）
#[cfg(target_os = "macos")]
fn extract_icon_with_applescript(app_path: &str) -> Option<String> {
    use std::process::Command;
    
    println!("🍎 macOS: 尝试使用 osascript 获取图标");
    
    // 方法1: 最简单的方法，直接用应用路径
    if let Some(icon_data) = extract_icon_with_mdls(app_path) {
        return Some(icon_data);
    }
    
    // 方法2: 使用更简单的 shell 命令组合
    if let Some(icon_data) = extract_icon_with_shell(app_path) {
        return Some(icon_data);
    }
    
    println!("❌ macOS: AppleScript 方法全部失败");
    None
}

// 使用 mdls 和系统工具的组合方法
#[cfg(target_os = "macos")]
fn extract_icon_with_mdls(app_path: &str) -> Option<String> {
    use std::process::Command;
    
    println!("🔍 macOS: 使用 mdls 方法获取图标");
    
    // 获取应用的 CFBundleIconFile
    let mdls_output = Command::new("mdls")
        .args(&["-name", "kMDItemCFBundleIdentifier", "-name", "kMDItemDisplayName", app_path])
        .output();
    
    if let Ok(result) = mdls_output {
        if result.status.success() {
            let output_str = String::from_utf8_lossy(&result.stdout);
            println!("📋 macOS: mdls 输出: {}", output_str);
        }
    }
    
    // 尝试最直接的方法：直接使用 Finder 复制图标
    let tmp_png = format!("/tmp/clipboard_mdls_icon_{}.png", std::process::id());
    
    let script = format!(r#"
set appPath to "{}"
set outputPath to "{}"

try
    -- 使用 QuickLook 生成缩略图
    do shell script "qlmanage -t -s 64 -o /tmp " & quoted form of appPath
    
    -- 查找生成的文件
    set appName to do shell script "basename " & quoted form of appPath & " .app"
    set qlPath to "/tmp/" & appName & ".png"
    
    -- 如果文件存在，复制到目标位置
    do shell script "if [ -f " & quoted form of qlPath & " ]; then cp " & quoted form of qlPath & " " & quoted form of outputPath & " && echo SUCCESS; else echo NOTFOUND; fi"
    
on error errMsg
    return "ERROR: " & errMsg
end try
    "#, app_path, tmp_png);
    
    let output = Command::new("osascript")
        .args(&["-e", &script])
        .output();
    
    match output {
        Ok(result) if result.status.success() => {
            let response = String::from_utf8_lossy(&result.stdout).trim().to_string();
            println!("📋 macOS: osascript 返回: {}", response);
            
            if response.contains("SUCCESS") && std::path::Path::new(&tmp_png).exists() {
                // 转换为 base64
                if let Some(base64_data) = convert_png_to_base64(&tmp_png) {
                    // 清理临时文件
                    let _ = Command::new("rm").arg(&tmp_png).output();
                    println!("✅ macOS: mdls 方法成功");
                    return Some(base64_data);
                }
            }
            
            // 清理临时文件
            let _ = Command::new("rm").arg(&tmp_png).output();
        }
        Ok(result) => {
            println!("⚠️ macOS: osascript 失败: {}", String::from_utf8_lossy(&result.stderr));
        }
        Err(e) => {
            println!("❌ macOS: osascript 命令失败: {}", e);
        }
    }
    
    None
}

// 使用纯 shell 命令的方法
#[cfg(target_os = "macos")]
fn extract_icon_with_shell(app_path: &str) -> Option<String> {
    use std::process::Command;
    
    println!("🐚 macOS: 使用纯 shell 方法获取图标");
    
    let tmp_png = format!("/tmp/clipboard_shell_icon_{}.png", std::process::id());
    
    // 使用 shell 脚本组合多种方法
    let script = format!(r#"
APP_PATH="{}"
OUTPUT_PATH="{}"

# 方法1: 直接从 .icns 转换
ICNS_FILES=("$APP_PATH/Contents/Resources/"*.icns)
for icns in "${{ICNS_FILES[@]}}"; do
    if [ -f "$icns" ]; then
        echo "找到图标文件: $icns"
        if sips -s format png -Z 64 "$icns" --out "$OUTPUT_PATH" 2>/dev/null; then
            echo "SUCCESS_SIPS"
            exit 0
        fi
    fi
done

# 方法2: 使用 iconutil（如果有 iconset）
ICONSET_PATH="$APP_PATH/Contents/Resources/AppIcon.iconset"
if [ -d "$ICONSET_PATH" ]; then
    # 查找合适大小的图标
    for size in "icon_64x64.png" "icon_32x32@2x.png" "icon_32x32.png"; do
        if [ -f "$ICONSET_PATH/$size" ]; then
            cp "$ICONSET_PATH/$size" "$OUTPUT_PATH"
            echo "SUCCESS_ICONSET"
            exit 0
        fi
    done
fi

echo "FAILED"
    "#, app_path, tmp_png);
    
    let output = Command::new("sh")
        .args(&["-c", &script])
        .output();
    
    match output {
        Ok(result) if result.status.success() => {
            let response = String::from_utf8_lossy(&result.stdout);
            println!("📋 macOS: shell 脚本输出: {}", response);
            
            if (response.contains("SUCCESS_SIPS") || response.contains("SUCCESS_ICONSET")) 
                && std::path::Path::new(&tmp_png).exists() {
                
                if let Some(base64_data) = convert_png_to_base64(&tmp_png) {
                    // 清理临时文件
                    let _ = Command::new("rm").arg(&tmp_png).output();
                    println!("✅ macOS: shell 方法成功");
                    return Some(base64_data);
                }
            }
            
            // 清理临时文件
            let _ = Command::new("rm").arg(&tmp_png).output();
        }
        Ok(result) => {
            println!("⚠️ macOS: shell 脚本失败: {}", String::from_utf8_lossy(&result.stderr));
            let _ = Command::new("rm").arg(&tmp_png).output();
        }
        Err(e) => {
            println!("❌ macOS: shell 命令失败: {}", e);
        }
    }
    
    None
}

// 统一的 PNG 转 base64 函数
#[cfg(target_os = "macos")]
fn convert_png_to_base64(png_path: &str) -> Option<String> {
    use std::process::Command;
    
    let b64_output = Command::new("base64")
        .args(&["-i", png_path])
        .output();
    
    match b64_output {
        Ok(result) if result.status.success() => {
            let base64_data = String::from_utf8_lossy(&result.stdout)
                .trim()
                .replace("\n", "");
            
            if !base64_data.is_empty() {
                Some(format!("data:image/png;base64,{}", base64_data))
            } else {
                None
            }
        }
        _ => None
    }
}

// macOS 备用方法：使用 qlmanage 获取图标
#[cfg(target_os = "macos")]
fn get_app_icon_simple_macos(bundle_id: &str) -> Option<String> {
    use std::process::Command;
    
    // 首先获取应用路径
    let script = format!(r#"
try
    tell application "System Events"
        set appPath to path of (first application process whose bundle identifier is "{}")
        return appPath
    end tell
on error
    return ""
end try
    "#, bundle_id);
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();
    
    if let Ok(result) = output {
        if result.status.success() {
            let app_path = String::from_utf8_lossy(&result.stdout).trim().to_string();
            
            if !app_path.is_empty() {
                println!("📁 macOS: 获取到应用路径: {}", app_path);
                
                // 使用 sips 命令提取图标
                let icon_output = Command::new("sips")
                    .args(&["-s", "format", "png", "--resampleHeight", "64", &app_path, "--out", "/tmp/clipboard_app_icon_simple.png"])
                    .output();
                
                if let Ok(sips_result) = icon_output {
                    if sips_result.status.success() {
                        // 读取生成的图标文件并转换为 base64
                        let base64_output = Command::new("base64")
                            .args(&["-i", "/tmp/clipboard_app_icon_simple.png"])
                            .output();
                        
                        if let Ok(b64_result) = base64_output {
                            if b64_result.status.success() {
                                let base64_data = String::from_utf8_lossy(&b64_result.stdout)
                                    .trim()
                                    .replace("\n", "");
                                
                                // 清理临时文件
                                let _ = Command::new("rm")
                                    .arg("/tmp/clipboard_app_icon_simple.png")
                                    .output();
                                
                                if !base64_data.is_empty() {
                                    println!("✅ macOS: 备用方法成功获取图标");
                                    return Some(format!("data:image/png;base64,{}", base64_data));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    println!("❌ macOS: 所有图标获取方法都失败了");
    None
}

// 专门用于剪贴板监听的窗口信息获取函数（不使用缓存）
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_active_window_info_for_clipboard() -> Result<SourceAppInfo, String> {
    use std::process::Command;
    
    println!("🔍 macOS: 获取当前活动窗口信息（剪贴板专用，无缓存）");
    
    // 使用 AppleScript 获取当前活动应用程序的信息
    let script = r#"
tell application "System Events"
    set frontApp to first application process whose frontmost is true
    set appName to name of frontApp
    set appBundleID to bundle identifier of frontApp
    return appName & "|" & appBundleID
end tell
    "#;
    
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("执行 AppleScript 失败: {}", e))?;
    
    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let parts: Vec<&str> = result.split('|').collect();
        
        if parts.len() >= 2 {
            let app_name = parts[0].to_string();
            let bundle_id = parts[1].to_string();
            
            println!("✅ 剪贴板专用：获取到活动应用: {} ({})", app_name, bundle_id);
            
            // 获取应用图标
            let app_icon = get_app_icon_base64_macos(&bundle_id);
            if app_icon.is_some() {
                println!("✅ 成功获取应用图标");
            } else {
                println!("⚠️ 无法获取应用图标");
            }
            
            Ok(SourceAppInfo {
                name: app_name,
                icon: app_icon,
                bundle_id: Some(bundle_id),
            })
        } else {
            println!("⚠️ 解析应用信息失败: {}", result);
            Ok(SourceAppInfo {
                name: result,
                icon: None,
                bundle_id: None,
            })
        }
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        println!("❌ 获取活动窗口失败: {}", error_msg);
        Ok(SourceAppInfo {
            name: "Unknown".to_string(),
            icon: None,
            bundle_id: None,
        })
    }
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn get_active_window_info() -> Result<SourceAppInfo, String> {
    use std::process::Command;
    
    println!("🔍 Linux: 获取当前活动窗口信息");
    
    // 尝试使用 xdotool 获取活动窗口信息
    let window_id_output = Command::new("xdotool")
        .args(&["getactivewindow"])
        .output();
    
    match window_id_output {
        Ok(output) if output.status.success() => {
            let window_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            
            // 获取窗口名称
            let window_name_output = Command::new("xdotool")
                .args(&["getwindowname", &window_id])
                .output();
            
            if let Ok(name_output) = window_name_output {
                if name_output.status.success() {
                    let window_name = String::from_utf8_lossy(&name_output.stdout).trim().to_string();
                    println!("✅ 获取到活动窗口: {}", window_name);
                    
                    return Ok(SourceAppInfo {
                        name: window_name,
                        icon: None,
                        bundle_id: None,
                    });
                }
            }
        }
        _ => {
            println!("⚠️ xdotool 不可用，回退到默认值");
        }
    }
    
    Ok(SourceAppInfo {
        name: "Unknown".to_string(),
        icon: None,
        bundle_id: None,
    })
} 
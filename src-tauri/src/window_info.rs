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
    
    // 合理的限流时间（每8秒最多调用一次），资源管理已改善
    let cache_duration = Duration::from_secs(8);
    
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

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn get_active_window_info() -> Result<SourceAppInfo, String> {
    Ok(SourceAppInfo {
        name: "Unknown".to_string(),
        icon: None,
    })
} 
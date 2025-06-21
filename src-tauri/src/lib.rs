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
use image::ImageEncoder;
use tauri_plugin_sql::{Migration, MigrationKind};
use tauri_plugin_global_shortcut::{self, GlobalShortcutExt, Shortcut, ShortcutState};
use std::env;
use chrono;
use sqlx::{self, Row, SqlitePool, sqlite::SqliteConnectOptions};
use tokio;
use tokio::sync::Mutex;
use enigo::{Enigo, Key, Keyboard, Settings};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::menu::{Menu, MenuItem};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "windows")]
use winapi::um::{
    winuser::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, GetDC, ReleaseDC, DestroyIcon},
    processthreadsapi::OpenProcess,
    handleapi::CloseHandle,
    psapi::GetModuleFileNameExW,
    shellapi::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, ExtractIconExW},
    winnt::PROCESS_QUERY_INFORMATION,
    wingdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteDC, DeleteObject, GetDIBits, BITMAPINFOHEADER, BITMAPINFO, DIB_RGB_COLORS, BI_RGB},
};
#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStringExt;

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

// 剪贴板监听器控制
struct ClipboardWatcherState {
    should_stop: Arc<AtomicBool>,
}

const SETTINGS_FILE: &str = "clipboard_settings.json";

fn settings_file_path() -> Result<PathBuf, String> {
    let dir = config_dir().ok_or("无法获取设置文件路径")?;
    Ok(dir.join(SETTINGS_FILE))
}

#[derive(Debug, Serialize, Clone)]
struct SourceAppInfo {
    name: String,
    icon: Option<String>, // base64 encoded icon
}

// 改进的图标缓存，使用LRU和更严格的内存管理
use std::collections::BTreeMap;

struct IconCacheEntry {
    icon: Option<String>,
    access_time: std::time::Instant,
}

struct IconCache {
    cache: HashMap<String, IconCacheEntry>,
    access_order: BTreeMap<std::time::Instant, String>,
    max_size: usize,
}

impl IconCache {
    fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: BTreeMap::new(),
            max_size,
        }
    }

    fn get(&mut self, key: &str) -> Option<Option<String>> {
        if let Some(entry) = self.cache.get_mut(key) {
            // 更新访问时间
            self.access_order.remove(&entry.access_time);
            entry.access_time = std::time::Instant::now();
            self.access_order.insert(entry.access_time, key.to_string());
            Some(entry.icon.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: String, icon: Option<String>) {
        let now = std::time::Instant::now();
        
        // 如果缓存已满，移除最旧的条目
        while self.cache.len() >= self.max_size {
            if let Some((oldest_time, oldest_key)) = self.access_order.pop_first() {
                self.cache.remove(&oldest_key);
            } else {
                break;
            }
        }

        let entry = IconCacheEntry {
            icon,
            access_time: now,
        };

        self.cache.insert(key.clone(), entry);
        self.access_order.insert(now, key);
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }
}

// 使用改进的图标缓存
static ICON_CACHE: std::sync::OnceLock<Arc<RwLock<IconCache>>> = std::sync::OnceLock::new();

fn get_icon_cache() -> &'static Arc<RwLock<IconCache>> {
    ICON_CACHE.get_or_init(|| Arc::new(RwLock::new(IconCache::new(10)))) // 减少到10个条目
}

// 更严格的图标缓存清理
fn cleanup_icon_cache() {
    let cache = get_icon_cache();
    if let Ok(mut cache_guard) = cache.write() {
        if cache_guard.len() > 5 {  // 只保留5个最新的
            // 清空一半缓存
            let to_clear = cache_guard.len() / 2;
            for _ in 0..to_clear {
                if let Some((oldest_time, oldest_key)) = cache_guard.access_order.pop_first() {
                    cache_guard.cache.remove(&oldest_key);
                } else {
                    break;
                }
            }
            println!("清理图标缓存，保留 {} 项", cache_guard.len());
        }
    }
}

// 添加限流机制，避免频繁获取窗口信息
static LAST_WINDOW_INFO_CALL: std::sync::OnceLock<Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>>> = std::sync::OnceLock::new();

fn get_last_window_info() -> &'static Arc<RwLock<(std::time::Instant, Option<SourceAppInfo>)>> {
    LAST_WINDOW_INFO_CALL.get_or_init(|| {
        Arc::new(RwLock::new((std::time::Instant::now() - Duration::from_secs(10), None)))
    })
}

// 获取当前活动窗口的应用程序信息（增加限流）
#[cfg(target_os = "windows")]
fn get_active_window_info() -> SourceAppInfo {
    // 检查是否需要限流（每5秒最多调用一次）
    let cache_duration = Duration::from_secs(5);
    
    if let Ok(guard) = get_last_window_info().read() {
        if guard.0.elapsed() < cache_duration {
            if let Some(ref cached_info) = guard.1 {
                return cached_info.clone();
            }
        }
    }

    let new_info = get_active_window_info_impl();
    
    // 更新缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.0 = std::time::Instant::now();
        guard.1 = Some(new_info.clone());
    }

    new_info
}

#[cfg(target_os = "windows")]
fn get_active_window_info_impl() -> SourceAppInfo {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return SourceAppInfo {
                name: "Unknown".to_string(),
                icon: None,
            };
        }

        // 获取窗口标题
        let mut window_title = [0u16; 256]; // 减少缓冲区大小
        let title_len = GetWindowTextW(hwnd, window_title.as_mut_ptr(), window_title.len() as i32);
        
        // 获取进程ID
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        
        // 打开进程句柄
        let process_handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, process_id);
        if process_handle.is_null() {
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

        // 获取进程可执行文件路径
        let mut exe_path = [0u16; 256]; // 减少缓冲区大小
        let path_len = GetModuleFileNameExW(process_handle, ptr::null_mut(), exe_path.as_mut_ptr(), exe_path.len() as u32);
        
        CloseHandle(process_handle);

        let app_name = if path_len > 0 {
            let path_os = OsString::from_wide(&exe_path[..path_len as usize]);
            let path_str = path_os.to_string_lossy();
            // 提取文件名（不包含扩展名）
            if let Some(file_name) = std::path::Path::new(&*path_str).file_stem() {
                file_name.to_string_lossy().to_string()
            } else {
                "Unknown".to_string()
            }
        } else if title_len > 0 {
            // 如果无法获取进程路径，使用窗口标题
            OsString::from_wide(&window_title[..title_len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            "Unknown".to_string()
        };

        // 获取应用程序图标（使用改进的缓存）
        let icon_base64 = if path_len > 0 {
            let exe_path_str = OsString::from_wide(&exe_path[..path_len as usize])
                .to_string_lossy()
                .to_string();
            
            // 先检查缓存
            let icon_cache = get_icon_cache();
            if let Ok(mut cache) = icon_cache.write() {
                if let Some(cached_icon) = cache.get(&exe_path_str) {
                    cached_icon
                } else {
                    // 获取图标
                    let icon = get_app_icon_base64(&exe_path[..path_len as usize]);
                    cache.insert(exe_path_str, icon.clone());
                    icon
                }
            } else {
                get_app_icon_base64(&exe_path[..path_len as usize])
            }
        } else {
            None
        };

        SourceAppInfo {
            name: app_name,
            icon: icon_base64,
        }
    }
}

#[cfg(target_os = "windows")]
fn get_app_icon_base64(exe_path: &[u16]) -> Option<String> {
    unsafe {
        // 方法1: 尝试使用 ExtractIconEx 获取最高质量图标
        let mut large_icons: [winapi::shared::windef::HICON; 1] = [ptr::null_mut()];
        let mut small_icons: [winapi::shared::windef::HICON; 1] = [ptr::null_mut()];
        
        let icon_count = ExtractIconExW(
            exe_path.as_ptr(),
            0, // 提取第一个图标
            large_icons.as_mut_ptr(),
            small_icons.as_mut_ptr(),
            1
        );

        if icon_count > 0 && !large_icons[0].is_null() {
            let icon_base64 = hicon_to_base64(large_icons[0]);
            
            // 确保清理图标资源
            if !large_icons[0].is_null() {
                DestroyIcon(large_icons[0]);
            }
            if !small_icons[0].is_null() {
                DestroyIcon(small_icons[0]);
            }
            
            if icon_base64.is_some() {
                return icon_base64;
            }
        }

        // 清理可能分配的图标句柄
        if !large_icons[0].is_null() {
            DestroyIcon(large_icons[0]);
        }
        if !small_icons[0].is_null() {
            DestroyIcon(small_icons[0]);
        }

        // 方法2: 如果 ExtractIconEx 失败，回退到 SHGetFileInfoW
        let mut shfi: SHFILEINFOW = std::mem::zeroed();
        let result = SHGetFileInfoW(
            exe_path.as_ptr(),
            0,
            &mut shfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result != 0 && !shfi.hIcon.is_null() {
            let icon_base64 = hicon_to_base64(shfi.hIcon);
            DestroyIcon(shfi.hIcon); // 确保释放图标句柄
            icon_base64
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn hicon_to_base64(hicon: winapi::shared::windef::HICON) -> Option<String> {
    use std::mem;
    
    unsafe {
        // 获取屏幕 DC
        let screen_dc = GetDC(ptr::null_mut());
        if screen_dc.is_null() {
            return None;
        }

        // 创建兼容的内存 DC
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(ptr::null_mut(), screen_dc);
            return None;
        }

        // 减小图标尺寸以减少内存使用（从48改为32）
        let icon_size = 32;
        let bitmap = CreateCompatibleBitmap(screen_dc, icon_size, icon_size);
        if bitmap.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
            return None;
        }

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
            winapi::um::winuser::FillRect(mem_dc, &rect, white_brush);
            winapi::um::wingdi::DeleteObject(white_brush as *mut winapi::ctypes::c_void);
        }

        // 绘制图标到位图
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

        // 确保清理所有GDI资源
        SelectObject(mem_dc, old_bitmap);
        DeleteObject(bitmap as *mut winapi::ctypes::c_void);
        DeleteDC(mem_dc);
        ReleaseDC(ptr::null_mut(), screen_dc);

        result
    }
}

#[cfg(target_os = "windows")]
fn convert_bgra_to_png_base64(bgra_data: &[u8], width: u32, height: u32) -> Option<String> {
    // 转换 BGRA 到 RGBA
    let mut rgba_data = Vec::with_capacity(bgra_data.len());
    for chunk in bgra_data.chunks_exact(4) {
        // BGRA -> RGBA
        rgba_data.push(chunk[2]); // R
        rgba_data.push(chunk[1]); // G
        rgba_data.push(chunk[0]); // B
        rgba_data.push(chunk[3]); // A
    }

    // 使用 image crate 编码为PNG
    let img = image::RgbaImage::from_raw(width, height, rgba_data)?;
    let mut png_buffer = Vec::new();
    
    // 使用PNG编码设置
    let encoder = image::codecs::png::PngEncoder::new(&mut png_buffer);
    
    if encoder.write_image(&img, width, height, image::ColorType::Rgba8).is_ok() {
        let base64_string = general_purpose::STANDARD.encode(&png_buffer);
        Some(format!("data:image/png;base64,{}", base64_string))
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn get_active_window_info() -> SourceAppInfo {
    SourceAppInfo {
        name: "Unknown".to_string(),
        icon: None,
    }
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
            image_path TEXT,
            source_app_name TEXT,
            source_app_icon TEXT
        )"
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("无法创建数据库表: {}", e))?;

    // 迁移：添加新的列（如果不存在）
    // 检查并添加 source_app_name 列
    let add_source_app_name = sqlx::query(
        "ALTER TABLE clipboard_history ADD COLUMN source_app_name TEXT"
    )
    .execute(&pool)
    .await;
    
    if let Err(e) = add_source_app_name {
        // 如果列已存在，SQLite会返回错误，这是正常的
        if !e.to_string().contains("duplicate column name") {
            println!("添加 source_app_name 列时的警告: {}", e);
        }
    } else {
        println!("已添加 source_app_name 列");
    }

    // 检查并添加 source_app_icon 列
    let add_source_app_icon = sqlx::query(
        "ALTER TABLE clipboard_history ADD COLUMN source_app_icon TEXT"
    )
    .execute(&pool)
    .await;
    
    if let Err(e) = add_source_app_icon {
        // 如果列已存在，SQLite会返回错误，这是正常的
        if !e.to_string().contains("duplicate column name") {
            println!("添加 source_app_icon 列时的警告: {}", e);
        }
    } else {
        println!("已添加 source_app_icon 列");
    }
    
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

fn start_clipboard_watcher(app: AppHandle) -> Arc<AtomicBool> {
    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = should_stop.clone();
    
    thread::spawn(move || {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                println!("无法初始化剪贴板监听器: {}", e);
                return;
            }
        };
        
        let mut last_text = String::new();
        let mut last_image_hash = 0u64;
        let mut cleanup_counter = 0u32;
        let mut last_window_check = std::time::Instant::now();
        let mut cached_source_info: Option<SourceAppInfo> = None;
        let mut recent_image_filenames: VecDeque<String> = VecDeque::with_capacity(10);
        
        // 更严格的内存限制
        const MAX_TEXT_LENGTH: usize = 100_000; // 降低到100KB
        const MAX_IMAGE_SIZE: usize = 10_000_000; // 降低到10MB
        const WINDOW_CHECK_INTERVAL: Duration = Duration::from_secs(3); // 每3秒检查一次活动窗口

        while !should_stop_clone.load(Ordering::Relaxed) {
            // 更频繁的清理
            cleanup_counter = cleanup_counter.wrapping_add(1);
            
            // 每50次循环（约50秒）执行一次内存清理
            if cleanup_counter % 50 == 0 {
                cleanup_icon_cache();
                
                // 更积极的字符串容量管理
                if last_text.capacity() > 500 && last_text.len() < last_text.capacity() / 3 {
                    last_text.shrink_to_fit();
                }
                
                // 清理缓存的窗口信息
                if cleanup_counter % 200 == 0 {
                    cached_source_info = None;
                }
            }
            
            // 检查文本 - 优化内存使用
            match clipboard.get_text() {
                Ok(text) => {
                    // 更严格的文本长度限制
                    if text.len() > MAX_TEXT_LENGTH {
                        println!("警告：剪贴板文本过大，跳过: {} bytes", text.len());
                        thread::sleep(Duration::from_millis(1200)); // 增加等待时间
                        continue;
                    }
                    
                    if text != last_text {
                        // 优化窗口信息获取 - 减少频率
                        let source_info = if last_window_check.elapsed() > WINDOW_CHECK_INTERVAL {
                            let info = get_active_window_info();
                            last_window_check = std::time::Instant::now();
                            cached_source_info = Some(info.clone());
                            info
                        } else {
                            cached_source_info.clone().unwrap_or_else(|| SourceAppInfo {
                                name: "Unknown".to_string(),
                                icon: None,
                            })
                        };
                        
                        // 使用预分配的字符串构建器减少内存分配
                        let mut event_data = String::with_capacity(text.len() + 200);
                        event_data.push_str(r#"{"content":""#);
                        
                        // 更高效的字符串转义
                        for ch in text.chars() {
                            match ch {
                                '"' => event_data.push_str(r#"\""#),
                                '\n' => event_data.push_str(r#"\n"#),
                                '\r' => event_data.push_str(r#"\r"#),
                                '\\' => event_data.push_str(r#"\\"#),
                                _ => event_data.push(ch),
                            }
                        }
                        
                        event_data.push_str(r#"","source_app_name":""#);
                        // 转义应用名称
                        for ch in source_info.name.chars() {
                            match ch {
                                '"' => event_data.push_str(r#"\""#),
                                '\\' => event_data.push_str(r#"\\"#),
                                _ => event_data.push(ch),
                            }
                        }
                        
                        event_data.push_str(r#"","source_app_icon":"#);
                        match source_info.icon {
                            Some(icon) => {
                                event_data.push('"');
                                event_data.push_str(&icon);
                                event_data.push('"');
                            }
                            None => event_data.push_str("null"),
                        }
                        event_data.push('}');
                        
                        let _ = app.emit("clipboard-text", event_data);
                        
                        // 使用swap避免克隆
                        std::mem::swap(&mut last_text, &mut String::from(text));
                        
                        // 立即收缩大文本的容量
                        if last_text.capacity() > last_text.len() * 2 {
                            last_text.shrink_to_fit();
                        }
                    }
                }
                Err(_) => {
                    // 忽略剪贴板访问错误
                }
            }
            
            // 检查图片 - 更严格的内存管理
            match clipboard.get_image() {
                Ok(image) => {
                    // 更严格的图片大小检查
                    let image_size = image.bytes.len();
                    if image_size > MAX_IMAGE_SIZE {
                        println!("警告：剪贴板图片过大，跳过: {} bytes", image_size);
                        thread::sleep(Duration::from_millis(1500)); // 增加等待时间
                        continue;
                    }
                    
                    // 改进的图片哈希算法 - 更稳定的重复检测
                    let hash = {
                        // 使用更稳定的方法：图片尺寸 + 数据长度 + 固定采样点
                        let bytes = &image.bytes;
                        let len = bytes.len();
                        
                        // 基础哈希：尺寸和长度
                        let mut hasher = 0u64;
                        hasher = hasher.wrapping_mul(31).wrapping_add(image.width as u64);
                        hasher = hasher.wrapping_mul(31).wrapping_add(image.height as u64);
                        hasher = hasher.wrapping_mul(31).wrapping_add(len as u64);
                        
                        // 固定位置采样，避免因内存布局变化导致的哈希不同
                        let sample_positions = [
                            0,                    // 起始
                            len / 4,             // 1/4位置
                            len / 2,             // 中间
                            len * 3 / 4,         // 3/4位置
                            len.saturating_sub(4), // 接近末尾（避免越界）
                        ];
                        
                        for &pos in &sample_positions {
                            if pos < len {
                                // 采样4个字节（如果可用）
                                for offset in 0..4 {
                                    if pos + offset < len {
                                        hasher = hasher.wrapping_mul(31).wrapping_add(bytes[pos + offset] as u64);
                                    }
                                }
                            }
                        }
                        
                        hasher
                    };
                    
                    if hash != last_image_hash {
                        last_image_hash = hash;
                        
                        // 额外的重复检测：检查最近保存的图片文件
                        let mut is_duplicate = false;
                        if let Ok(images_dir) = get_app_images_dir() {
                            if images_dir.exists() {
                                // 检查最近5分钟内创建的文件
                                let five_minutes_ago = std::time::SystemTime::now() - Duration::from_secs(300);
                                
                                if let Ok(entries) = std::fs::read_dir(&images_dir) {
                                    for entry in entries.flatten() {
                                        if let Ok(metadata) = entry.metadata() {
                                            if let Ok(created) = metadata.created() {
                                                if created > five_minutes_ago {
                                                    // 对于最近的文件，比较文件大小作为快速检查
                                                    if metadata.len() as usize == image.bytes.len() {
                                                        println!("检测到可能的重复图片，大小匹配: {} bytes", metadata.len());
                                                        is_duplicate = true;
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        if is_duplicate {
                            println!("跳过重复的图片");
                            continue;
                        }
                        
                        // 获取程序安装目录下的图片目录
                        match get_app_images_dir() {
                            Ok(images_dir) => {
                                // 生成唯一的文件名，避免与最近的文件重复
                                let timestamp = chrono::Utc::now().timestamp_millis();
                                let mut filename = format!("clipboard_{}.png", timestamp);
                                
                                // 检查文件名是否与最近的重复
                                let mut counter = 0;
                                while recent_image_filenames.contains(&filename) && counter < 10 {
                                    counter += 1;
                                    filename = format!("clipboard_{}_{}.png", timestamp, counter);
                                }
                                
                                // 添加到最近文件名列表
                                if recent_image_filenames.len() >= 10 {
                                    recent_image_filenames.pop_front();
                                }
                                recent_image_filenames.push_back(filename.clone());
                                
                                let image_path = images_dir.join(&filename);
                                
                                // 在限制作用域中处理图片，确保内存及时释放
                                let processing_result = {
                                    // 修复 Alpha 通道问题：Windows 剪贴板有时会将 Alpha 设为 0
                                    let mut fixed_bytes = image.bytes.to_vec();
                                    
                                    // 检查并修复 Alpha 通道（优化：减少内存分配）
                                    let total_pixels = (image.width * image.height) as usize;
                                    let mut zero_alpha_count = 0;
                                    
                                    // 快速扫描Alpha通道
                                    for chunk in fixed_bytes.chunks_exact(4) {
                                        if chunk[3] == 0 {
                                            zero_alpha_count += 1;
                                        }
                                    }
                                    
                                    // 如果大部分像素的 Alpha 都是 0，就将它们设为 255（不透明）
                                    if zero_alpha_count > total_pixels / 2 {
                                        for chunk in fixed_bytes.chunks_exact_mut(4) {
                                            if chunk[3] == 0 {
                                                chunk[3] = 255; // 设为完全不透明
                                            }
                                        }
                                    }
                                    
                                    // 创建图片并保存
                                    match image::RgbaImage::from_raw(image.width as u32, image.height as u32, fixed_bytes) {
                                        Some(rgba_img) => {
                                            let img = image::DynamicImage::ImageRgba8(rgba_img);
                                            img.save(&image_path)
                                        },
                                        None => {
                                            // 尝试 BGRA 到 RGBA 转换
                                            let mut rgba_bytes = image.bytes.to_vec();
                                            
                                            // 将 BGRA 转换为 RGBA
                                            for chunk in rgba_bytes.chunks_exact_mut(4) {
                                                chunk.swap(0, 2); // 交换 B 和 R 通道
                                            }
                                            
                                            match image::RgbaImage::from_raw(image.width as u32, image.height as u32, rgba_bytes) {
                                                Some(rgba_img) => {
                                                    let img = image::DynamicImage::ImageRgba8(rgba_img);
                                                    img.save(&image_path)
                                                },
                                                None => {
                                                    println!("所有格式转换都失败，跳过此图片");
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                };
                                
                                if processing_result.is_ok() {
                                    // 创建缩略图（在新的作用域中，限制内存使用）
                                    let thumbnail_result = {
                                        match image::open(&image_path) {
                                            Ok(img) => {
                                                // 创建一个小的缩略图，限制最大尺寸
                                                let thumbnail = img.resize(150, 150, image::imageops::FilterType::Lanczos3).to_rgba8();
                                                let mut thumb_buf = Vec::with_capacity(30_000); // 预分配更小的缓冲区
                                                
                                                if image::codecs::png::PngEncoder::new(&mut thumb_buf)
                                                    .write_image(&thumbnail, thumbnail.width(), thumbnail.height(), image::ColorType::Rgba8)
                                                    .is_ok() {
                                                    let thumb_b64 = general_purpose::STANDARD.encode(&thumb_buf);
                                                    Some(format!("data:image/png;base64,{}", thumb_b64))
                                                } else {
                                                    None
                                                }
                                            },
                                            Err(_) => None
                                        }
                                    };
                                    
                                    if let Some(thumb_data_url) = thumbnail_result {
                                        // 使用缓存的窗口信息，减少API调用
                                        let source_info = if last_window_check.elapsed() > WINDOW_CHECK_INTERVAL {
                                            let info = get_active_window_info();
                                            last_window_check = std::time::Instant::now();
                                            cached_source_info = Some(info.clone());
                                            info
                                        } else {
                                            cached_source_info.clone().unwrap_or_else(|| SourceAppInfo {
                                                name: "Unknown".to_string(),
                                                icon: None,
                                            })
                                        };
                                        
                                        // 使用预分配构建事件数据
                                        let mut event_data = String::with_capacity(thumb_data_url.len() + 200);
                                        event_data.push_str(r#"{"path":""#);
                                        
                                        // 转义路径
                                        let path_str = image_path.to_string_lossy();
                                        for ch in path_str.chars() {
                                            match ch {
                                                '"' => event_data.push_str(r#"\""#),
                                                '\\' => event_data.push_str(r#"\\"#),
                                                _ => event_data.push(ch),
                                            }
                                        }
                                        
                                        event_data.push_str(r#"","thumbnail":""#);
                                        event_data.push_str(&thumb_data_url);
                                        event_data.push_str(r#"","source_app_name":""#);
                                        
                                        // 转义应用名称
                                        for ch in source_info.name.chars() {
                                            match ch {
                                                '"' => event_data.push_str(r#"\""#),
                                                '\\' => event_data.push_str(r#"\\"#),
                                                _ => event_data.push(ch),
                                            }
                                        }
                                        
                                        event_data.push_str(r#"","source_app_icon":"#);
                                        match source_info.icon {
                                            Some(icon) => {
                                                event_data.push('"');
                                                event_data.push_str(&icon);
                                                event_data.push('"');
                                            }
                                            None => event_data.push_str("null"),
                                        }
                                        event_data.push('}');
                                        
                                        let _ = app.emit("clipboard-image", event_data);
                                    }
                                }
                                
                                // 立即释放图片数据内存
                                drop(image);
                            },
                            Err(e) => {
                                println!("无法获取图片保存目录: {}", e);
                            }
                        }
                    }
                }
                Err(_) => {
                    // 忽略图片获取错误
                }
            }
            
            // 显著增加睡眠时间，大幅减少CPU使用和内存压力
            thread::sleep(Duration::from_millis(2000)); // 从1秒增加到2秒
        }
        
        // 线程退出前执行最终清理
        cleanup_icon_cache();
        last_text.clear();
        last_text.shrink_to_fit();
        cached_source_info = None;
        
        println!("剪贴板监听器已停止并完成清理");
    });
    
    should_stop
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

// 获取应用程序安装目录下的图片目录
fn get_app_images_dir() -> Result<PathBuf, String> {
    let exe_path = get_app_exe_path()?;
    
    // 获取可执行文件所在的目录
    let exe_dir = exe_path.parent()
        .ok_or("无法获取程序目录")?;
    
    // 在程序目录下创建 images 文件夹
    let images_dir = exe_dir.join("images");
    
    // 确保目录存在
    if !images_dir.exists() {
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| format!("无法创建图片目录: {}", e))?;
    }
    
    Ok(images_dir)
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
    
    // 首先获取需要删除的图片文件路径
    let time_images_query = "
        SELECT image_path FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0 AND image_path IS NOT NULL
    ";
    
    let time_expired_images = match sqlx::query(time_images_query)
        .bind(&timestamp_cutoff)
        .fetch_all(db)
        .await {
        Ok(rows) => {
            let mut paths = Vec::new();
            for row in rows {
                if let Ok(path) = row.try_get::<String, &str>("image_path") {
                    paths.push(path);
                }
            }
            paths
        }
        Err(e) => {
            println!("查询过期图片路径失败: {}", e);
            Vec::new()
        }
    };
    
    // 删除过期的图片文件
    for image_path in &time_expired_images {
        if let Err(e) = std::fs::remove_file(image_path) {
            println!("删除图片文件失败 {}: {}", image_path, e);
        } else {
            println!("已删除图片文件: {}", image_path);
        }
    }
    
    let time_cleanup_query = "
        DELETE FROM clipboard_history 
        WHERE timestamp < ? AND is_favorite = 0
    ";
    
    match sqlx::query(time_cleanup_query)
        .bind(&timestamp_cutoff)
        .execute(db)
        .await {
        Ok(result) => {
            println!("按时间清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), time_expired_images.len());
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
        
        // 首先获取需要删除的记录的图片路径
        let count_images_query = "
            SELECT image_path FROM clipboard_history 
            WHERE is_favorite = 0 
            AND image_path IS NOT NULL
            AND id IN (
                SELECT id FROM clipboard_history 
                WHERE is_favorite = 0 
                ORDER BY timestamp ASC 
                LIMIT ?
            )
        ";
        
        let count_expired_images = match sqlx::query(count_images_query)
            .bind(excess_count)
            .fetch_all(db)
            .await {
            Ok(rows) => {
                let mut paths = Vec::new();
                for row in rows {
                    if let Ok(path) = row.try_get::<String, &str>("image_path") {
                        paths.push(path);
                    }
                }
                paths
            }
            Err(e) => {
                println!("查询需删除图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除图片文件
        for image_path in &count_expired_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                println!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                println!("已删除图片文件: {}", image_path);
            }
        }
        
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
                println!("按数量清理完成，删除了 {} 条记录，删除了 {} 个图片文件", result.rows_affected(), count_expired_images.len());
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
    
    // 3. 清理孤立的图片文件（数据库中没有对应记录的文件）
    if let Ok(images_dir) = get_app_images_dir() {
        if images_dir.exists() {
            match std::fs::read_dir(&images_dir) {
                Ok(entries) => {
                    let mut orphaned_files = Vec::new();
                    
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let file_path = entry.path();
                            if file_path.is_file() {
                                let file_path_str = file_path.to_string_lossy().to_string();
                                
                                // 检查数据库中是否存在此文件路径的记录
                                let check_query = "SELECT COUNT(*) as count FROM clipboard_history WHERE image_path = ?";
                                match sqlx::query(check_query)
                                    .bind(&file_path_str)
                                    .fetch_one(db)
                                    .await {
                                    Ok(row) => {
                                        let count: i64 = row.get("count");
                                        if count == 0 {
                                            orphaned_files.push(file_path_str);
                                        }
                                    }
                                    Err(e) => {
                                        println!("检查孤立文件失败: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    
                    // 删除孤立的图片文件
                    for orphaned_file in &orphaned_files {
                        if let Err(e) = std::fs::remove_file(orphaned_file) {
                            println!("删除孤立图片文件失败 {}: {}", orphaned_file, e);
                        } else {
                            println!("已删除孤立图片文件: {}", orphaned_file);
                        }
                    }
                    
                    if !orphaned_files.is_empty() {
                        println!("清理了 {} 个孤立的图片文件", orphaned_files.len());
                    }
                }
                Err(e) => {
                    println!("无法读取图片目录: {}", e);
                }
            }
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

// 读取图片文件并返回 base64 数据
#[tauri::command]
async fn load_image_file(image_path: String) -> Result<String, String> {
    let path = PathBuf::from(&image_path);
    
    // 检查文件是否存在
    if !path.exists() {
        return Err("图片文件不存在".to_string());
    }
    
    // 读取图片文件
    let image_data = std::fs::read(&path)
        .map_err(|e| format!("无法读取图片文件: {}", e))?;
    
    // 转换为 base64
    let b64 = general_purpose::STANDARD.encode(&image_data);
    let data_url = format!("data:image/png;base64,{}", b64);
    
    Ok(data_url)
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

// 手动清理内存缓存
#[tauri::command]
async fn clear_memory_cache() -> Result<(), String> {
    // 清理图标缓存
    cleanup_icon_cache();
    
    // 强制清理所有缓存
    let cache = get_icon_cache();
    if let Ok(mut cache_guard) = cache.write() {
        cache_guard.clear();
        println!("图标缓存已完全清空");
    }
    
    // 清理窗口信息缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.1 = None;
        println!("窗口信息缓存已清理");
    }
    
    println!("内存缓存已清理");
    Ok(())
}

#[tauri::command]
async fn force_memory_cleanup() -> Result<String, String> {
    // 强制清理所有内存缓存
    cleanup_icon_cache();
    
    // 清空图标缓存
    let cache = get_icon_cache();
    let cache_size = if let Ok(mut cache_guard) = cache.write() {
        let size = cache_guard.len();
        cache_guard.clear();
        size
    } else {
        0
    };
    
    // 清理窗口信息缓存
    if let Ok(mut guard) = get_last_window_info().write() {
        guard.1 = None;
    }
    
    // 尝试强制内存回收
    #[cfg(target_os = "windows")]
    unsafe {
        // 调用Windows API强制内存清理
        use winapi::um::winbase::{SetProcessWorkingSetSize};
        use winapi::um::processthreadsapi::GetCurrentProcess;
        
        // 设置进程工作集大小来强制内存回收
        let _result = SetProcessWorkingSetSize(
            GetCurrentProcess(),
            usize::MAX,
            usize::MAX,
        );
    }
    
    let message = format!(
        "强制内存清理完成 - 清理了 {} 个图标缓存项", 
        cache_size
    );
    println!("{}", message);
    Ok(message)
}

#[tauri::command]
async fn stop_clipboard_watcher(app: AppHandle) -> Result<(), String> {
    if let Some(watcher_state) = app.try_state::<ClipboardWatcherState>() {
        watcher_state.should_stop.store(true, Ordering::Relaxed);
        println!("剪贴板监听器停止信号已发送");
        Ok(())
    } else {
        Err("无法找到剪贴板监听器状态".to_string())
    }
}

#[tauri::command]
async fn start_new_clipboard_watcher(app: AppHandle) -> Result<(), String> {
    // 停止现有的监听器
    if let Some(watcher_state) = app.try_state::<ClipboardWatcherState>() {
        watcher_state.should_stop.store(true, Ordering::Relaxed);
    }
    
    // 等待一段时间让旧监听器停止
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    
    // 启动新的监听器
    let should_stop = start_clipboard_watcher(app.clone());
    
    // 更新状态
    app.manage(ClipboardWatcherState { should_stop });
    
    println!("新的剪贴板监听器已启动");
    Ok(())
}

#[tauri::command]
async fn reset_database(app: AppHandle) -> Result<(), String> {
    println!("开始重置数据库...");
    
    // 尝试获取数据库状态
    if let Some(db_state) = app.try_state::<Mutex<DatabaseState>>() {
        let db_guard = db_state.lock().await;
        let pool = &db_guard.pool;
        
        // 首先获取所有图片文件路径
        let all_images = match sqlx::query("SELECT image_path FROM clipboard_history WHERE image_path IS NOT NULL")
            .fetch_all(pool)
            .await {
            Ok(rows) => {
                let mut paths = Vec::new();
                for row in rows {
                    if let Ok(path) = row.try_get::<String, &str>("image_path") {
                        paths.push(path);
                    }
                }
                paths
            }
            Err(e) => {
                println!("查询图片路径失败: {}", e);
                Vec::new()
            }
        };
        
        // 删除所有图片文件
        for image_path in &all_images {
            if let Err(e) = std::fs::remove_file(image_path) {
                println!("删除图片文件失败 {}: {}", image_path, e);
            } else {
                println!("已删除图片文件: {}", image_path);
            }
        }
        println!("已删除 {} 个图片文件", all_images.len());
        
        // 删除整个图片目录（如果存在且为空）
        if let Ok(images_dir) = get_app_images_dir() {
            if images_dir.exists() {
                if let Err(e) = std::fs::remove_dir(&images_dir) {
                    println!("删除图片目录失败（可能不为空）: {}", e);
                } else {
                    println!("已删除图片目录: {:?}", images_dir);
                }
            }
        }
        
        // 删除所有表
        sqlx::query("DROP TABLE IF EXISTS clipboard_history").execute(pool).await
            .map_err(|e| format!("删除表失败: {}", e))?;
        
        // 删除迁移信息表（Tauri SQL插件使用的内部表）
        sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations").execute(pool).await
            .map_err(|e| format!("删除迁移表失败: {}", e))?;
        
        println!("数据库表已删除");
        
        // 重新创建表
        sqlx::query("
            CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                type TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                image_path TEXT
            )
        ").execute(pool).await
            .map_err(|e| format!("重新创建表失败: {}", e))?;
        
        // 重新创建索引
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_clipboard_content ON clipboard_history(content)")
            .execute(pool).await
            .map_err(|e| format!("创建索引失败: {}", e))?;
        
        println!("数据库重置完成");
        Ok(())
    } else {
        Err("无法访问数据库状态".to_string())
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
                            // 添加小延迟确保窗口完全显示
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            // 再次设置焦点，确保焦点在 webview 上
                            let _ = window.set_focus();
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
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                match window.is_visible() {
                                    Ok(true) => {
                                        let _ = window.hide();
                                    }
                                    Ok(false) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        // 添加小延迟确保窗口完全显示
                                        std::thread::sleep(std::time::Duration::from_millis(50));
                                        // 再次设置焦点，确保焦点在 webview 上
                                        let _ = window.set_focus();
                                    }
                                    Err(_) => {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        // 添加小延迟确保窗口完全显示
                                        std::thread::sleep(std::time::Duration::from_millis(50));
                                        // 再次设置焦点，确保焦点在 webview 上
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
                .on_menu_event({
                    let should_stop_clone = should_stop.clone();
                    move |app, event| {
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
                                            // 添加小延迟确保窗口完全显示
                                            std::thread::sleep(std::time::Duration::from_millis(50));
                                            // 再次设置焦点，确保焦点在 webview 上
                                            let _ = window.set_focus();
                                        }
                                        Err(_) => {
                                            let _ = window.show();
                                            let _ = window.set_focus();
                                            // 添加小延迟确保窗口完全显示
                                            std::thread::sleep(std::time::Duration::from_millis(50));
                                            // 再次设置焦点，确保焦点在 webview 上
                                            let _ = window.set_focus();
                                        }
                                    }
                                }
                            }
                            "quit" => {
                                // 停止剪贴板监听器
                                should_stop_clone.store(true, Ordering::Relaxed);
                                println!("正在停止剪贴板监听器...");
                                
                                // 等待一小段时间让监听器线程停止
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                
                                app.exit(0);
                            }
                            _ => {}
                        }
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
        .invoke_handler(tauri::generate_handler![greet, save_settings, load_settings, register_shortcut, set_auto_start, get_auto_start_status, cleanup_history, paste_to_clipboard, reset_database, load_image_file, clear_memory_cache, force_memory_cleanup, stop_clipboard_watcher, start_new_clipboard_watcher])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
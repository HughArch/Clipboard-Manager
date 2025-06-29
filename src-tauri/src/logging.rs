use std::fs;
use std::path::PathBuf;
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{
    fmt::time::LocalTime,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

/// 日志配置结构
#[derive(Clone)]
pub struct LogConfig {
    pub app_name: String,
    pub log_dir: PathBuf,
    pub max_log_files: usize,
    pub is_production: bool,
    pub console_enabled: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            app_name: "clipboard-manager".to_string(),
            log_dir: get_app_log_dir(),
            max_log_files: 30, // 保留30天的日志
            is_production: !cfg!(debug_assertions),
            console_enabled: true, // 总是启用控制台输出以便调试
        }
    }
}

/// 获取应用程序日志目录（位于程序安装目录）
fn get_app_log_dir() -> PathBuf {
    // 尝试获取程序执行路径
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            return exe_dir.join("logs");
        }
    }
    
    // 如果获取执行路径失败，fallback到当前工作目录
    PathBuf::from(".").join("logs")
}

/// 初始化日志系统
pub fn init_logging(config: LogConfig) -> Result<(), Box<dyn std::error::Error>> {
    // 确保日志目录存在
    fs::create_dir_all(&config.log_dir)?;
    
    // 清理旧日志文件
    cleanup_old_logs(&config.log_dir, config.max_log_files)?;

    // 创建文件appender（按日轮转）
    let file_appender = rolling::daily(&config.log_dir, "app.log");
    let (file_writer, guard) = non_blocking(file_appender);
    
    // 注意：必须保持guard存活以确保文件写入器正常工作
    // 在实际应用中，guard应该存储在全局变量中
    std::mem::forget(guard); // 暂时使用forget防止guard被丢弃
    
    // 文件日志层使用更宽泛的过滤器，确保所有日志都被写入
    let file_filter = if config.is_production {
        "info" // 生产环境：所有模块的info级别及以上
    } else {
        "debug" // 开发环境：所有模块的debug级别及以上
    };

    // 控制台日志层使用相同的过滤器
    let console_filter = file_filter;

    // 创建文件日志层
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // 文件中不使用颜色
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new(file_filter));

    let mut layers = Vec::new();
    layers.push(file_layer.boxed());

    // 如果启用控制台输出，添加控制台层
    if config.console_enabled {
        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true) // 控制台使用颜色
            .with_timer(LocalTime::rfc_3339())
            .with_target(true)
            .compact()
            .with_filter(EnvFilter::new(console_filter));
        layers.push(console_layer.boxed());
    }

    // 初始化订阅器
    tracing_subscriber::registry()
        .with(layers)
        .try_init()?;

    tracing::info!(
        app_name = %config.app_name,
        log_dir = %config.log_dir.display(),
        is_production = config.is_production,
        "日志系统初始化完成"
    );

    Ok(())
}

/// 清理旧的日志文件
fn cleanup_old_logs(log_dir: &PathBuf, max_files: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut log_files = Vec::new();
    
    if log_dir.exists() {
        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("app.log") {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(created) = metadata.created() {
                                log_files.push((path, created));
                            }
                        }
                    }
                }
            }
        }
        
        // 按创建时间排序，最新的在前
        log_files.sort_by(|a, b| b.1.cmp(&a.1));
        
        // 删除超过限制的文件
        for (path, _) in log_files.iter().skip(max_files) {
            if let Err(e) = fs::remove_file(path) {
                tracing::warn!("删除日志文件失败 {}: {}", path.display(), e);
            } else {
                tracing::info!("已删除旧日志文件: {}", path.display());
            }
        }
    }
    
    Ok(())
}

/// 获取日志目录路径
pub fn get_log_dir() -> PathBuf {
    get_app_log_dir()
}

/// 获取当前日志文件路径
pub fn get_current_log_file() -> PathBuf {
    let log_dir = get_log_dir();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    log_dir.join(format!("app.log.{}", today))
}

/// 获取日志文件列表
pub fn get_log_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let log_dir = get_log_dir();
    let mut files = Vec::new();
    
    if log_dir.exists() {
        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("app.log") {
                        files.push(path);
                    }
                }
            }
        }
    }
    
    // 按文件名排序
    files.sort();
    Ok(files)
}

/// 重定向标准输出和错误输出到日志
#[cfg(windows)]
pub fn redirect_stdio_to_log() -> Result<(), Box<dyn std::error::Error>> {
    use winapi::um::wincon::{FreeConsole, GetConsoleWindow};
    use winapi::um::winuser::{ShowWindow, SW_HIDE};

    unsafe {
        // 隐藏控制台窗口（如果存在）
        let console_window = GetConsoleWindow();
        if !console_window.is_null() {
            ShowWindow(console_window, SW_HIDE);
        }
        
        // 释放现有控制台
        FreeConsole();
    }
    
    Ok(())
}

#[cfg(not(windows))]
pub fn redirect_stdio_to_log() -> Result<(), Box<dyn std::error::Error>> {
    // 在非Windows平台上，stdio重定向由系统处理
    Ok(())
} 
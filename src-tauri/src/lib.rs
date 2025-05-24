use tauri::Emitter;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use dirs_next::config_dir;
use arboard::Clipboard;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle};
use base64::{engine::general_purpose, Engine as _};
use tauri_plugin_sql::{Migration, MigrationKind};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub max_history_items: usize,
    pub max_history_time: u64,
    pub hotkey: String,
    pub auto_start: bool,
}

const SETTINGS_FILE: &str = "clipboard_settings.json";

fn settings_file_path() -> Result<PathBuf, String> {
    let dir = config_dir().ok_or("无法获取设置文件路径")?;
    Ok(dir.join(SETTINGS_FILE))
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    let path = settings_file_path()?;
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn load_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_file_path()?;
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let settings: AppSettings = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn start_clipboard_watcher(app: AppHandle) {
    thread::spawn(move || {
        let mut clipboard = Clipboard::new().unwrap();
        let mut last_text = String::new();
        let mut last_image_hash = 0u64;

        loop {
            // 检查文本
            if let Ok(text) = clipboard.get_text() {
                if text != last_text {
                    last_text = text.clone();
                    app.emit("clipboard-text", text).ok();
                }
            }
            // 检查图片
            if let Ok(image) = clipboard.get_image() {
                let hash = image.bytes.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64));
                if hash != last_image_hash {
                    last_image_hash = hash;
                    // 转为 base64
                    let img = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.to_vec()).unwrap();
                    let mut buf = vec![];
                    image::codecs::png::PngEncoder::new(&mut buf)
                        .encode(&img, img.width(), img.height(), image::ColorType::Rgba8)
                        .unwrap();
                    let b64 = general_purpose::STANDARD.encode(&buf);
                    let data_url = format!("data:image/png;base64,{}", b64);
                    app.emit("clipboard-image", data_url).ok();
                }
            }
            thread::sleep(Duration::from_millis(800));
        }
    });
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
        .setup(|app| {
            let app_handle = app.handle().clone();
            start_clipboard_watcher(app_handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, save_settings, load_settings])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
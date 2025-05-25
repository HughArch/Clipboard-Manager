fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "save_settings", 
                "load_settings", 
                "greet", 
                "register_shortcut", 
                "set_auto_start", 
                "get_auto_start_status", 
                "cleanup_history", 
                "paste_to_clipboard"
            ]))
    ).unwrap();
}

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["save_settings", "load_settings", "greet", "register_shortcut"]))
    ).unwrap();
}

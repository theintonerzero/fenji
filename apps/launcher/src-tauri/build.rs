fn main() {
    // The view may call only the app's own two commands (launcher spec §3.9, plan LP10), through
    // the `view` permission set the main window's capability grants.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["intent", "snapshot"])),
    )
    .expect("tauri's build step");
}

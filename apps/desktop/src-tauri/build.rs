fn main() {
    // The app's own commands, so capabilities can grant them (to the editor's origin only).
    let commands = &[
        "desktop_info",
        "lab_up",
        "lab_down",
        "lab_status",
        "capture_start",
        "capture_stop",
        "machine_console",
        "machine_action",
        "snapshot_list",
        "snapshot_take",
        "snapshot_use",
        "snapshot_delete",
        "update_check",
        "update_install",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("tauri build");
}

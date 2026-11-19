#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    // The egui bridge is an explicit one-shot CLI mode: never start a second
    // Tauri window, touch startup recovery or take the app single-instance
    // lock when the receipt is being committed through this protocol.
    if std::env::args().any(|arg| arg == "--bando-egui-reception-commit") {
        std::process::exit(bando_hub_tauri_lib::run_egui_reception_commit());
    }
    if std::env::args().any(|arg| arg == "--bando-egui-settings-patch") {
        std::process::exit(bando_hub_tauri_lib::run_egui_settings_patch());
    }
    bando_hub_tauri_lib::run();
}

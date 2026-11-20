#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    // Reception remains an explicit one-shot CLI mode: never start a second
    // Tauri window, touch startup recovery or take the app single-instance
    // lock when the receipt is being committed through this protocol.
    bando_hub_tauri_lib::run();
}

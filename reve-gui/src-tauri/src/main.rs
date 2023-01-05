#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod configuration;
mod utils;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            utils::get_version,
            utils::replace_file_suffix,
            utils::load_configuration,
            utils::write_configuration,
            utils::write_log,
            commands::upscale_single_video,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

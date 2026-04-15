#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(md_editor_lib::AppState::new())
        .invoke_handler(tauri::generate_handler![
            md_editor_lib::commands::open_file,
            md_editor_lib::commands::save_file,
            md_editor_lib::commands::create_file,
            md_editor_lib::commands::create_dir,
            md_editor_lib::commands::rename_file,
            md_editor_lib::commands::delete_file,
            md_editor_lib::commands::list_vault,
            md_editor_lib::commands::search_vault,
            md_editor_lib::commands::set_vault_root,
            md_editor_lib::commands::get_backlinks,
            md_editor_lib::commands::get_sys_config,
            md_editor_lib::commands::set_sys_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

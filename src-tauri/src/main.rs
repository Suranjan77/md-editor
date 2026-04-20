#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Force the app to use X11/XWayland instead of native Wayland
        std::env::set_var("GDK_BACKEND", "x11");

        // std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

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
            md_editor_lib::tracker_commands::get_tracker_sessions,
            md_editor_lib::tracker_commands::add_tracker_session,
            md_editor_lib::tracker_commands::delete_tracker_session,
            md_editor_lib::tracker_commands::get_tracker_activities,
            md_editor_lib::tracker_commands::add_tracker_activity,
            md_editor_lib::tracker_commands::get_tracker_kv,
            md_editor_lib::tracker_commands::set_tracker_kv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

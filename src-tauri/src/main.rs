#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Force the app to use X11/XWayland instead of native Wayland
        std::env::set_var("GDK_BACKEND", "x11");

        // std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    tauri::Builder::default()
        .setup(|app| {
            if let Ok(resource_dir) = app.path().resource_dir() {
                std::env::set_var("PDFIUM_RESOURCE_DIR", resource_dir);
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .register_asynchronous_uri_scheme_protocol("md-pdf", |ctx, request, responder| {
            let app_handle = ctx.app_handle().clone();

            tauri::async_runtime::spawn_blocking(move || {
                // Parse URI: md-pdf://localhost/page_index/scale/render_generation
                let uri = request.uri().path();
                let parts: Vec<&str> = uri.trim_start_matches('/').split('/').collect();

                if parts.len() >= 2 {
                    if let (Ok(page_index), Ok(scale_int)) =
                        (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                    {
                        let scale = scale_int as f32 / 100.0;
                        let generation = parts.get(2).and_then(|part| part.parse::<u64>().ok());

                        match md_editor_lib::pdf_commands::get_pdf_page_bytes(
                            &app_handle,
                            page_index,
                            scale,
                            generation,
                        ) {
                            Ok(bytes) => {
                                let response = tauri::http::Response::builder()
                                    .header("Content-Type", "image/png")
                                    .header("Access-Control-Allow-Origin", "*")
                                    .body(bytes)
                                    .unwrap();
                                responder.respond(response);
                                return;
                            }
                            Err(e) => {
                                eprintln!("Error rendering PDF page: {}", e);
                                let response = tauri::http::Response::builder()
                                    .status(500)
                                    .body(Vec::new())
                                    .unwrap();
                                responder.respond(response);
                                return;
                            }
                        }
                    }
                }

                eprintln!("Invalid PDF request URI: {}", uri);

                let response = tauri::http::Response::builder()
                    .status(400)
                    .body(Vec::new())
                    .unwrap();

                responder.respond(response);
            });
        })
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
            md_editor_lib::pdf_commands::open_pdf,
            md_editor_lib::pdf_commands::close_pdf,
            md_editor_lib::pdf_commands::set_pdf_render_generation,
            md_editor_lib::pdf_commands::get_pdf_page_bitmap,
            md_editor_lib::pdf_commands::get_page_links,
            md_editor_lib::pdf_commands::get_link_preview,
            md_editor_lib::pdf_commands::search_pdf,
            md_editor_lib::pdf_commands::get_pdfium_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

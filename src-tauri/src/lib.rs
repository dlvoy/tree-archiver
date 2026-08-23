pub mod archive;
pub mod commands;
pub mod events;
pub mod fsutil;
pub mod model;
pub mod naming;
pub mod plan;
pub mod roots;
pub mod scan;
pub mod settings;

use commands::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            // Start the session in the state the user left it in. A missing or
            // unreadable settings file yields defaults rather than an error.
            let saved = settings::load(app.handle());
            let state = app.state::<AppState>();
            if let Ok(mut tree) = state.tree.lock() {
                tree.sort = saved.sort;
                tree.output = saved.output;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_paths,
            commands::remove_node,
            commands::clear_all,
            commands::get_children,
            commands::set_sort,
            commands::set_checked,
            commands::set_all_checked,
            commands::get_summary,
            commands::get_state,
            commands::get_issues,
            commands::save_plan,
            commands::load_plan,
            commands::set_output,
            commands::estimate,
            commands::suggested_output_name,
            commands::start_archive,
            commands::cancel_archive,
            commands::cancel_scan,
            commands::save_log,
            commands::output_extension,
            commands::restore_view,
            commands::path_mode_options,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tree Archiver");
}

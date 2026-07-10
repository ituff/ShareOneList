pub mod auth;
pub mod config;
pub mod errors;
pub mod graph;
pub mod models;
pub mod tools;
pub mod transfer;

use auth::AuthModule;
use config::ConfigManager;
use tauri::Manager;
use tokio::sync::Mutex;
use transfer::download::DownloadEngine;
use transfer::upload::UploadEngine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");
            let config_manager = ConfigManager::new(app_data_dir);
            app.manage(config_manager);
            app.manage(Mutex::new(AuthModule::new()));
            app.manage(Mutex::new(DownloadEngine::new(app.handle().clone())));
            app.manage(Mutex::new(UploadEngine::new(app.handle().clone())));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            config::commands::get_config,
            config::commands::save_config,
            config::commands::get_accounts,
            auth::commands::login,
            auth::commands::logout,
            graph::commands::list_files,
            graph::commands::get_drive,
            graph::commands::get_drive_quota,
            graph::commands::search_files,
            graph::commands::rename_item,
            graph::commands::delete_item,
            graph::commands::create_folder,
            graph::commands::create_share_link,
            graph::commands::convert_format,
            graph::commands::get_preview_url,
            graph::commands::get_item_properties,
            graph::commands::get_sharepoint_sites,
            graph::commands::get_site_drives,
            graph::commands::get_shared_drives,
            transfer::commands::download_file,
            transfer::commands::download_folder,
            transfer::commands::pause_download,
            transfer::commands::resume_download,
            transfer::commands::cancel_download,
            transfer::commands::open_containing_folder,
            transfer::commands::upload_files,
            transfer::commands::upload_folder,
            transfer::commands::cancel_upload,
            tools::commands::parse_sharepoint_url,
            tools::commands::push_to_downloader,
            tools::commands::check_update,
            tools::commands::perform_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to ShareOneList.", name)
}

//! Meetbase desktop application — privacy-first AI meeting notetaker.

mod commands;
mod db;
mod error;
mod export;
mod recording;
mod settings;
mod state;
mod worker;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,meetbase=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db_path = data_dir.join("meetbase.db");
            let pool = tauri::async_runtime::block_on(db::open(&db_path))?;
            app.manage(AppState::new(pool));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_meetings,
            commands::get_meeting,
            commands::rename_meeting,
            commands::delete_meeting,
            commands::start_recording,
            commands::stop_recording,
            commands::recording_status,
            commands::import_media,
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::diarization_status,
            commands::enable_diarization,
            commands::list_audio_devices,
            commands::get_settings,
            commands::set_settings,
            commands::list_templates,
            commands::list_ollama_models,
            commands::generate_summary,
            commands::export_markdown,
            commands::save_markdown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running meetbase");
}

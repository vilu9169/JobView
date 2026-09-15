pub mod domain;
pub mod error;
pub mod integrations;
pub mod repository;
pub mod services;

#[cfg(feature = "desktop")]
mod commands;

#[cfg(feature = "desktop")]
pub fn run() -> tauri::Result<()> {
    use tauri::Manager;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_directory)?;
            let log_directory = app.path().app_log_dir()?;
            std::fs::create_dir_all(&log_directory)?;
            let writer = tracing_appender::rolling::daily(log_directory, "mailview.log");
            let _ = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_env_filter("mailview_lib=info")
                .with_writer(writer)
                .try_init();
            let repository = tauri::async_runtime::block_on(repository::Repository::open(
                data_directory.join("mailview.sqlite3"),
            ))?;
            app.manage(integrations::gmail::GmailIntegration::new(
                repository.clone(),
            )?);
            app.manage(integrations::ai::AiIntegration::new(repository.clone())?);
            app.manage(repository);
            tracing::info!("JobView local workspace initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_workspace,
            commands::save_application,
            commands::load_fixtures,
            commands::link_email,
            commands::set_email_disposition,
            commands::rebuild_classifications,
            commands::resolve_action,
            commands::save_settings,
            commands::get_gmail_status,
            commands::import_google_oauth_client,
            commands::connect_gmail,
            commands::cancel_gmail_connection,
            commands::disconnect_gmail,
            commands::sync_gmail,
            commands::open_gmail_setup,
            commands::open_google_account_connections,
            commands::get_ai_status,
            commands::save_gemini_key,
            commands::remove_gemini_key,
            commands::save_ai_settings,
            commands::test_gemini_connection,
            commands::classify_with_ai,
            commands::cancel_ai_classification,
            commands::open_gemini_setup,
        ])
        .run(tauri::generate_context!())
}

use std::io::Read;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use crate::{
    domain::{AppSettings, ApplicationInput, EmailDisposition, WorkspaceSnapshot},
    error::AppError,
    error::AppResult,
    integrations::{
        ai::{AiIntegration, AiStatus},
        gmail::{GmailIntegration, GmailStatus},
    },
    repository::{AiSettings, Repository},
};

fn report<T>(result: AppResult<T>, operation: &'static str) -> AppResult<T> {
    if let Err(error) = &result {
        error.log_safe(operation);
    }
    result
}

#[tauri::command]
pub async fn get_workspace(repository: State<'_, Repository>) -> AppResult<WorkspaceSnapshot> {
    report(repository.snapshot().await, "get_workspace")
}

#[tauri::command]
pub async fn save_application(
    repository: State<'_, Repository>,
    input: ApplicationInput,
) -> AppResult<WorkspaceSnapshot> {
    report(repository.save_application(input).await, "save_application")
}

#[tauri::command]
pub async fn load_fixtures(repository: State<'_, Repository>) -> AppResult<WorkspaceSnapshot> {
    report(repository.load_fixtures().await, "load_fixtures")
}

#[tauri::command]
pub async fn link_email(
    repository: State<'_, Repository>,
    email_id: String,
    application_id: String,
) -> AppResult<WorkspaceSnapshot> {
    report(
        repository.link_email(&email_id, &application_id).await,
        "link_email",
    )
}

#[tauri::command]
pub async fn set_email_disposition(
    repository: State<'_, Repository>,
    email_id: String,
    disposition: EmailDisposition,
) -> AppResult<WorkspaceSnapshot> {
    report(
        repository
            .set_email_disposition(&email_id, disposition)
            .await,
        "set_email_disposition",
    )
}

#[tauri::command]
pub async fn rebuild_classifications(
    repository: State<'_, Repository>,
) -> AppResult<WorkspaceSnapshot> {
    report(
        repository.rebuild_classifications().await,
        "rebuild_classifications",
    )
}

#[tauri::command]
pub async fn resolve_action(
    repository: State<'_, Repository>,
    application_id: Option<String>,
    email_id: Option<String>,
) -> AppResult<WorkspaceSnapshot> {
    report(
        repository
            .resolve_action(application_id.as_deref(), email_id.as_deref())
            .await,
        "resolve_action",
    )
}

#[tauri::command]
pub async fn save_settings(
    ai: State<'_, AiIntegration>,
    settings: AppSettings,
) -> AppResult<WorkspaceSnapshot> {
    report(ai.save_workspace_settings(settings).await, "save_settings")
}

#[tauri::command]
pub async fn get_gmail_status(gmail: State<'_, GmailIntegration>) -> AppResult<GmailStatus> {
    report(gmail.status().await, "get_gmail_status")
}

#[tauri::command]
pub async fn import_google_oauth_client(
    app: tauri::AppHandle,
    gmail: State<'_, GmailIntegration>,
) -> AppResult<GmailStatus> {
    // Rust opens the picker and reads only the explicitly selected file. Neither
    // arbitrary filesystem access nor client contents are exposed to JavaScript.
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Google Desktop OAuth client", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| AppError::Integration("The OAuth file picker could not be opened.".into()))?;
    let Some(selected) = selected else {
        return gmail.status().await;
    };
    let path = selected.into_path().map_err(|_| {
        AppError::Integration("Choose a local Google OAuth client JSON file.".into())
    })?;
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(AppError::Integration(
            "The OAuth client file is too large. Choose the downloaded Google Desktop client JSON."
                .into(),
        ));
    }
    let json = std::str::from_utf8(&bytes).map_err(|_| {
        AppError::Integration("The OAuth client file is not valid UTF-8 JSON.".into())
    })?;
    report(gmail.configure(json).await, "import_google_oauth_client")
}

#[tauri::command]
pub async fn connect_gmail(gmail: State<'_, GmailIntegration>) -> AppResult<GmailStatus> {
    report(gmail.connect().await, "connect_gmail")
}

#[tauri::command]
pub async fn cancel_gmail_connection(gmail: State<'_, GmailIntegration>) -> AppResult<GmailStatus> {
    report(gmail.cancel_connection().await, "cancel_gmail_connection")
}

#[tauri::command]
pub async fn disconnect_gmail(gmail: State<'_, GmailIntegration>) -> AppResult<GmailStatus> {
    report(gmail.disconnect().await, "disconnect_gmail")
}

#[tauri::command]
pub async fn sync_gmail(
    gmail: State<'_, GmailIntegration>,
    ai: State<'_, AiIntegration>,
    repository: State<'_, Repository>,
) -> AppResult<WorkspaceSnapshot> {
    report(gmail.sync().await, "sync_gmail")?;
    ai.after_sync().await;
    report(repository.snapshot().await, "sync_gmail_snapshot")
}

#[tauri::command]
pub async fn get_ai_status(ai: State<'_, AiIntegration>) -> AppResult<AiStatus> {
    report(ai.status().await, "get_ai_status")
}

#[tauri::command]
pub async fn save_gemini_key(ai: State<'_, AiIntegration>, api_key: String) -> AppResult<AiStatus> {
    // The one-time password input crosses IPC only to reach the OS credential
    // store. Never add tracing instrumentation to this argument.
    report(
        ai.save_key(Zeroizing::new(api_key)).await,
        "save_gemini_key",
    )
}

#[tauri::command]
pub async fn remove_gemini_key(ai: State<'_, AiIntegration>) -> AppResult<AiStatus> {
    report(ai.remove_key().await, "remove_gemini_key")
}

#[tauri::command]
pub async fn save_ai_settings(
    ai: State<'_, AiIntegration>,
    settings: AiSettings,
) -> AppResult<AiStatus> {
    report(ai.save_configuration(settings).await, "save_ai_settings")
}

#[tauri::command]
pub async fn test_gemini_connection(ai: State<'_, AiIntegration>) -> AppResult<AiStatus> {
    report(ai.test_connection().await, "test_gemini_connection")
}

#[tauri::command]
pub async fn classify_with_ai(ai: State<'_, AiIntegration>, force: bool) -> AppResult<AiStatus> {
    report(ai.classify(force).await, "classify_with_ai")
}

#[tauri::command]
pub async fn cancel_ai_classification(ai: State<'_, AiIntegration>) -> AppResult<AiStatus> {
    report(ai.cancel().await, "cancel_ai_classification")
}

#[tauri::command]
pub fn open_gemini_setup() -> AppResult<()> {
    webbrowser::open("https://aistudio.google.com/api-keys").map_err(|_| {
        AppError::Integration(
            "The system browser could not be opened. Visit Google AI Studio to create an API key."
                .into(),
        )
    })
}

#[tauri::command]
pub fn open_gmail_setup() -> AppResult<()> {
    webbrowser::open("https://console.cloud.google.com/apis/library/gmail.googleapis.com").map_err(
        |_| {
            AppError::Integration(
                "The system browser could not be opened. See the Gmail setup guide in the README."
                    .into(),
            )
        },
    )
}

#[tauri::command]
pub fn open_google_account_connections() -> AppResult<()> {
    webbrowser::open("https://myaccount.google.com/connections")
        .map_err(|_| AppError::Integration("The system browser could not be opened. Visit Google Account connections in your browser.".into()))
}

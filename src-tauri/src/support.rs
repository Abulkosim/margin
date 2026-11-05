use crate::{
    core::{install, report::ProblemReport},
    runtime::Runtime,
};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

/// The report last shown to the user. Opening reuses it, so the issue matches
/// the preview and the interface cannot supply a URL or its contents.
#[derive(Default)]
pub struct ReportPreview(Mutex<Option<ProblemReport>>);

#[tauri::command]
pub fn get_install_warning() -> bool {
    let executable = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    let home = dirs::home_dir().and_then(|home| std::fs::canonicalize(home).ok());
    install::warns_about_location(std::env::consts::OS, executable.as_deref(), home.as_deref())
}

#[tauri::command]
pub async fn get_problem_report(
    runtime: State<'_, Arc<Runtime>>,
    preview: State<'_, ReportPreview>,
) -> Result<ProblemReport, String> {
    let accounts = runtime.inner.lock().await.view.accounts.clone();
    let os_version = os_info::get().version().to_string();
    let report = ProblemReport::build(std::env::consts::OS, &os_version, &accounts);
    *preview
        .0
        .lock()
        .map_err(|_| "Could not prepare the report.")? = Some(report.clone());
    Ok(report)
}

#[tauri::command]
pub fn open_problem_report(
    app: AppHandle,
    preview: State<'_, ReportPreview>,
) -> Result<(), String> {
    let report = preview
        .0
        .lock()
        .map_err(|_| "Could not prepare the report.")?
        .clone()
        .ok_or("Review the report first.")?;
    app.opener()
        .open_url(report.url()?, None::<&str>)
        .map_err(|_| "Could not open your browser.".into())
}

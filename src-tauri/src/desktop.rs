use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Serialize)]
pub struct DesktopError {
    code: &'static str,
    detail: String,
}

fn error(detail: impl ToString) -> DesktopError {
    DesktopError {
        code: "nativeOperationFailed",
        detail: detail.to_string(),
    }
}

#[tauri::command]
pub async fn import_cookie_file(
    app: tauri::AppHandle,
    window: tauri::Window,
) -> Result<Option<String>, DesktopError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_parent(&window)
        .add_filter("Cookie", &["txt"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(file) = receiver.await.map_err(error)? else {
        return Ok(None);
    };
    let path = file.into_path().map_err(error)?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_to_string(path).map(Some).map_err(error)
    })
    .await
    .map_err(error)?
}

#[tauri::command]
pub async fn select_download_directory(
    app: tauri::AppHandle,
    window: tauri::Window,
) -> Result<Option<String>, DesktopError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_parent(&window)
        .pick_folder(move |path| {
            let _ = sender.send(path);
        });
    receiver
        .await
        .map_err(error)?
        .map(|file| {
            file.into_path()
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(error)
        })
        .transpose()
}

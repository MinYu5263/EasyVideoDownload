use crate::database::{Storage, StorageError};
use std::path::{Path, PathBuf};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, PartialEq)]
enum FolderTarget {
    Reveal(PathBuf),
    Open(PathBuf),
}

fn classify_file_error(error: std::io::Error) -> StorageError {
    if error.kind() == std::io::ErrorKind::NotFound {
        StorageError::new("historyFileMissing", "The output file is missing")
    } else {
        StorageError::new("historyOpenFailed", error)
    }
}
fn file_target(path: Option<&str>) -> Result<PathBuf, StorageError> {
    let path = path
        .filter(|p| Path::new(p).is_absolute())
        .ok_or_else(|| StorageError::new("historyFileMissing", "The output file is unavailable"))?;
    let path = Path::new(path)
        .canonicalize()
        .map_err(classify_file_error)?;
    if !std::fs::metadata(&path)
        .map_err(classify_file_error)?
        .is_file()
    {
        return Err(StorageError::new(
            "historyFileMissing",
            "The output file is unavailable",
        ));
    }
    Ok(path)
}
fn folder_target(output: Option<&str>, directory: &str) -> Result<FolderTarget, StorageError> {
    match file_target(output) {
        Ok(file) => return Ok(FolderTarget::Reveal(file)),
        Err(e) if e.code == "historyFileMissing" => {}
        Err(e) => return Err(e),
    }
    let path = Path::new(directory);
    if !path.is_absolute() {
        return Err(StorageError::new(
            "historyDirectoryMissing",
            "The download directory is unavailable",
        ));
    }
    let path = path.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StorageError::new(
                "historyDirectoryMissing",
                "The download directory is unavailable",
            )
        } else {
            StorageError::new("historyOpenFailed", e)
        }
    })?;
    if !path.is_dir() {
        return Err(StorageError::new(
            "historyDirectoryMissing",
            "The download directory is unavailable",
        ));
    }
    Ok(FolderTarget::Open(path))
}
fn source_target(source: &str) -> Result<String, StorageError> {
    let url = url::Url::parse(source)
        .map_err(|_| StorageError::new("historyOpenFailed", "Invalid source link"))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(StorageError::new(
            "historyOpenFailed",
            "Invalid source link",
        ));
    }
    crate::database::download_records::sanitize_source_link(url.as_str())
        .map_err(|_| StorageError::new("historyOpenFailed", "Invalid source link"))
}

#[tauri::command]
pub async fn delete_download_record(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    tauri::async_runtime::spawn_blocking(move || db.delete_download_record(id))
        .await
        .map_err(|e| StorageError::new("saveFailed", e))??;
    let _ = app.emit("download-records-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn restore_download_record(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    tauri::async_runtime::spawn_blocking(move || db.restore_download_record(id))
        .await
        .map_err(|e| StorageError::new("saveFailed", e))??;
    let _ = app.emit("download-records-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn purge_download_record(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        db.purge_download_record(id, super::permanent::delete_output_file)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    result
}

#[tauri::command]
pub async fn empty_download_record_trash(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        db.empty_download_record_trash(super::permanent::delete_output_file)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    result
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecycleFileResult {
    file_recycled: bool,
}

#[tauri::command]
pub async fn delete_download_record_and_file(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<RecycleFileResult, StorageError> {
    let db = storage.database()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        db.recycle_download_record_file(id, super::recycle::recycle_output_file)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?;
    // Even a partial success changes file availability and must refresh observers.
    if result.is_ok()
        || result
        .as_ref()
        .err()
        .is_some_and(|e| e.code == "historyFileRecycledSaveFailed")
    {
        let _ = app.emit("download-records-changed", ());
    }
    result.map(|file_recycled| RecycleFileResult { file_recycled })
}

#[tauri::command]
pub async fn open_download_record_file(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let path = tauri::async_runtime::spawn_blocking(move || {
        let record = db.get_download_record(id)?;
        let checked = file_target(record.output_path.as_deref());
        match &checked {
            Ok(_) => db.mark_download_output_availability(&record, "present")?,
            Err(e) if e.code == "historyFileMissing" && record.output_path.is_some() => {
                db.mark_download_output_availability(&record, "missing")?
            }
            _ => {}
        }
        checked
    })
        .await
        .map_err(|e| StorageError::new("historyOpenFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    let path = path?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|e| StorageError::new("historyOpenFailed", e))
}
#[tauri::command]
pub async fn open_download_record_folder(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let target = tauri::async_runtime::spawn_blocking(move || {
        let record = db.get_download_record(id)?;
        let directory = record.successful_output.as_ref().map(|s| s.directory.as_str()).unwrap_or(&record.download_directory);
        let missing = record.output_path.is_some() && matches!(file_target(record.output_path.as_deref()),Err(e) if e.code=="historyFileMissing");
        if missing { db.mark_download_output_availability(&record, "missing")?; }
        let target = folder_target(record.output_path.as_deref(), directory).map_err(|e| if missing && e.code == "historyDirectoryMissing" { StorageError::new("historyFileMissing", "The output file and its directory are missing") } else { e })?;
        if !missing { db.mark_download_output_availability(&record, if record.output_path.is_some() { "present" } else { "unknown" })?; }
        Ok::<_, StorageError>((target, missing))
    })
        .await
        .map_err(|e| StorageError::new("historyOpenFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    let (target, missing) = target?;
    let result = match target {
        FolderTarget::Reveal(path) => app.opener().reveal_item_in_dir(path),
        FolderTarget::Open(path) => app
            .opener()
            .open_path(path.to_string_lossy().into_owned(), None::<String>),
    };
    result.map_err(|e| StorageError::new("historyOpenFailed", e))?;
    if missing {
        let _ = app.emit("download-records-changed", ());
        return Err(StorageError::new(
            "historyFileMissing",
            "Opened the folder; the output file is missing",
        ));
    }
    Ok(())
}
#[tauri::command]
pub async fn open_download_record_source(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let source = tauri::async_runtime::spawn_blocking(move || {
        source_target(&db.get_download_record(id)?.source_link)
    })
        .await
        .map_err(|e| StorageError::new("historyOpenFailed", e))??;
    app.opener()
        .open_url(source, None::<String>)
        .map_err(|e| StorageError::new("historyOpenFailed", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_inaccessible_paths_have_distinct_errors() {
        assert_eq!(
            classify_file_error(std::io::Error::from(std::io::ErrorKind::NotFound)).code,
            "historyFileMissing"
        );
        assert_eq!(
            classify_file_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).code,
            "historyOpenFailed"
        );
        assert_eq!(
            classify_file_error(std::io::Error::from(std::io::ErrorKind::Other)).code,
            "historyOpenFailed"
        );
    }
    #[test]
    fn missing_file_keeps_folder_available_and_rejects_non_http_sources() {
        let dir = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary native history action directory: {}",
            dir.path().display()
        );
        let missing = dir.path().join("removed.mp4");
        assert_eq!(
            file_target(Some(missing.to_str().unwrap()))
                .unwrap_err()
                .code,
            "historyFileMissing"
        );
        assert_eq!(
            folder_target(
                Some(missing.to_str().unwrap()),
                dir.path().to_str().unwrap()
            )
                .unwrap(),
            FolderTarget::Open(dir.path().canonicalize().unwrap())
        );
        std::fs::write(&missing, b"video").unwrap();
        assert_eq!(
            folder_target(
                Some(missing.to_str().unwrap()),
                dir.path().to_str().unwrap()
            )
                .unwrap(),
            FolderTarget::Reveal(missing.canonicalize().unwrap())
        );
        for source in [
            "file:///C:/secret",
            "javascript:alert(1)",
            "https://user:pass@example.com/",
        ] {
            assert!(source_target(source).is_err());
        }
        assert!(source_target("https://www.youtube.com/watch?v=abc").is_ok());
        assert_eq!(
            source_target("https://www.youtube.com/watch?v=abc&access_token=private").unwrap(),
            "https://www.youtube.com/watch?v=abc"
        );
    }
}

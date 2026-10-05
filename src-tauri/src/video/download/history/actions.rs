use crate::database::{Database, Storage, StorageError};
use std::path::{Path, PathBuf};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

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
fn checked_record_file(db: &Database, id: i64) -> Result<PathBuf, StorageError> {
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
        db.purge_download_record(id, super::permanent::delete_download_files)
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
        db.empty_download_record_trash(super::permanent::delete_download_files)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    result
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFileResult {
    file_deleted: bool,
}

pub(crate) fn delete_record_files(db: &Database, id: i64) -> Result<bool, StorageError> {
    let mut file_deleted = false;
    db.delete_download_record_and_file(id, |record, protected| {
        let fragments_deleted = super::permanent::delete_download_fragments(record, protected)?;
        file_deleted = super::permanent::delete_output_file(record, protected)?;
        Ok(file_deleted || fragments_deleted)
    })?;
    Ok(file_deleted)
}

#[tauri::command]
pub async fn delete_download_record_and_file(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<DeleteFileResult, StorageError> {
    let db = storage.database()?;
    let result = tauri::async_runtime::spawn_blocking(move || delete_record_files(&db, id))
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?;
    // Even a partial success changes file availability and must refresh observers.
    if result.is_ok()
        || result
        .as_ref()
        .err()
        .is_some_and(|e| e.code == "historyFileDeletedSaveFailed")
    {
        let _ = app.emit("download-records-changed", ());
    }
    match &result {
        Ok(deleted) => {
            log::info!("operation=delete_record_and_file record_id={id} file_deleted={deleted}")
        }
        Err(error) => log::warn!(
            "operation=delete_record_and_file record_id={id} code={}",
            error.code
        ),
    }
    result.map(|file_deleted| DeleteFileResult { file_deleted })
}

#[tauri::command]
pub async fn open_download_record_file(
    app: tauri::AppHandle,
    storage: tauri::State<'_, Storage>,
    id: i64,
) -> Result<(), StorageError> {
    let db = storage.database()?;
    let path = tauri::async_runtime::spawn_blocking(move || checked_record_file(&db, id))
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
    let target = tauri::async_runtime::spawn_blocking(move || checked_record_file(&db, id))
        .await
        .map_err(|e| StorageError::new("historyOpenFailed", e))?;
    let _ = app.emit("download-records-changed", ());
    let result = target.and_then(|path| {
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| StorageError::new("historyOpenFailed", e))
    });
    match &result {
        Ok(()) => log::info!("operation=open_record_folder record_id={id}"),
        Err(error) => log::warn!(
            "operation=open_record_folder record_id={id} code={}",
            error.code
        ),
    };
    result
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
    fn missing_file_blocks_folder_opening_and_existing_file_can_be_revealed() {
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
        assert_eq!(file_target(None).unwrap_err().code, "historyFileMissing");
        std::fs::write(&missing, b"video").unwrap();
        assert_eq!(
            file_target(Some(missing.to_str().unwrap())).unwrap(),
            missing.canonicalize().unwrap()
        );
        assert_eq!(
            file_target(dir.path().to_str()).unwrap_err().code,
            "historyFileMissing"
        );
        let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
        let mut page = crate::database::persistence_tests::page();
        page.download_directory = dir.path().to_string_lossy().into();
        let id = db
            .begin_download_record(
                "folder-check",
                &crate::database::download_records::DownloadSnapshot { page },
                "2026-10-04 10:00:00",
            )
            .unwrap();
        db.finish_download_record(
            "folder-check",
            &crate::database::download_records::DownloadRecordOutcome::Completed {
                path: missing.to_string_lossy().into(),
                size: 5,
                extension: Some("mp4".into()),
            },
        )
            .unwrap();
        assert_eq!(
            checked_record_file(&db, id).unwrap(),
            missing.canonicalize().unwrap()
        );
        std::fs::remove_file(&missing).unwrap();
        assert_eq!(
            checked_record_file(&db, id).unwrap_err().code,
            "historyFileMissing"
        );
        assert_eq!(
            db.get_download_record(id).unwrap().file_availability,
            "missing"
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

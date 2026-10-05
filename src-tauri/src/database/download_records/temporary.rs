use super::*;
use std::{fs::File, path::PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct TemporaryDirectory {
    pub directory: String,
    pub path: String,
    pub identity: String,
}

pub(super) fn read(
    connection: &Connection,
    record_id: i64,
) -> Result<Vec<TemporaryDirectory>, StorageError> {
    connection.prepare("SELECT directory,path,identity FROM download_temporary_directories WHERE record_id=?1 ORDER BY rowid")
        .map_err(|e| StorageError::new("loadFailed", e))?
        .query_map([record_id], |r| Ok(TemporaryDirectory { directory: r.get(0)?, path: r.get(1)?, identity: r.get(2)? }))
        .map_err(|e| StorageError::new("loadFailed", e))?
        .collect::<Result<Vec<_>, _>>().map_err(|e| StorageError::new("loadFailed", e))
}

fn io_failure(e: std::io::Error) -> StorageError {
    StorageError::new(
        if crate::video::download::failure::file_is_occupied(&e) {
            "historyFileOccupied"
        } else {
            "historyFileDeleteFailed"
        },
        e,
    )
}
fn unsafe_path(detail: impl ToString) -> StorageError {
    StorageError::new("historyFileUnsafe", detail)
}

// Pin the directory without following reparse points. Its identity excludes timestamps
// and length, which change as fragments are downloaded and removed.
fn open_directory(path: &Path) -> Result<File, StorageError> {
    crate::video::download::history::recycle::reject_links(path)?;
    let mut options = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .access_mode(FILE_READ_ATTRIBUTES.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    #[cfg(not(windows))]
    options.read(true);
    let file = options.open(path).map_err(io_failure)?;
    if !file.metadata().map_err(io_failure)?.is_dir() {
        return Err(unsafe_path("Temporary path is not a directory"));
    }
    Ok(file)
}
fn directory_identity(file: &File) -> Result<String, StorageError> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|e| unsafe_path(e))?;
        Ok(format!(
            "{}:{}:{}:{}:{}",
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
            info.ftCreationTime.dwHighDateTime,
            info.ftCreationTime.dwLowDateTime
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata().map_err(io_failure)?;
        Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Err(unsafe_path("Directory identity is unavailable"))
    }
}
pub(crate) fn validate(directory: &TemporaryDirectory) -> Result<File, StorageError> {
    let path = Path::new(&directory.path);
    let home = Path::new(&directory.directory);
    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
    if !path.is_absolute()
        || !home.is_absolute()
        || path.parent() != Some(home)
        || !name
        .strip_prefix(".evd-")
        .and_then(|v| v.strip_suffix(".partial"))
        .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok())
    {
        return Err(unsafe_path(
            "Temporary directory is outside its recorded download directory",
        ));
    }
    let file = open_directory(path)?;
    if directory_identity(&file)? != directory.identity {
        return Err(StorageError::new(
            "historyFileChanged",
            "The owned temporary directory was replaced",
        ));
    }
    Ok(file)
}

impl Database {
    pub(crate) fn prepare_download_temporary_directory(
        &self,
        request_id: &str,
        home: &Path,
    ) -> Result<TemporaryDirectory, StorageError> {
        crate::video::download::history::recycle::reject_links(home)?;
        let home = home.canonicalize().map_err(io_failure)?;
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let record_id: i64 = transaction.query_row("SELECT id FROM download_records WHERE request_id=?1 AND status='running' AND deleted_at IS NULL", [request_id], |r| r.get(0))
            .optional().map_err(|e| StorageError::new("loadFailed", e))?.ok_or_else(|| StorageError::new("recordNotFound", "Active download record not found"))?;
        let home_text = home.to_string_lossy().into_owned();
        for existing in read(&transaction, record_id)?
            .into_iter()
            .rev()
            .filter(|v| v.directory == home_text)
        {
            // A missing old directory needs a new identity; never adopt a replacement.
            if std::fs::symlink_metadata(&existing.path)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                continue;
            }
            validate(&existing)?;
            transaction
                .commit()
                .map_err(|e| StorageError::new("saveFailed", e))?;
            return Ok(existing);
        }
        let path = home.join(format!(".evd-{}.partial", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(io_failure)?;
        let mut created_directory = None;
        let created = (|| {
            let identity = directory_identity(&open_directory(&path)?)?;
            let directory = TemporaryDirectory {
                directory: home_text,
                path: path.to_string_lossy().into_owned(),
                identity,
            };
            created_directory = Some(directory.clone());
            transaction.execute("INSERT INTO download_temporary_directories(record_id,directory,path,identity) VALUES (?1,?2,?3,?4)", params![record_id,directory.directory,directory.path,directory.identity])
                .map_err(|e| StorageError::new("saveFailed", e))?;
            transaction
                .commit()
                .map_err(|e| StorageError::new("saveFailed", e))?;
            Ok(directory)
        })();
        if created.is_err() {
            // No downloader has used this directory. Verify its opened identity even
            // when unwinding a failed database write; never delete a substituted path.
            if let Some(directory) = created_directory {
                let _ = remove_empty(&directory);
            }
        }
        created
    }
}

#[cfg(windows)]
fn delete_empty_directory(directory: &TemporaryDirectory) -> Result<(), StorageError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let path = Path::new(&directory.path);
    crate::video::download::history::recycle::reject_links(path)?;
    let file = std::fs::OpenOptions::new()
        .access_mode(DELETE.0 | FILE_READ_ATTRIBUTES.0)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(io_failure)?;
    if !file.metadata().map_err(io_failure)?.is_dir()
        || directory_identity(&file)? != directory.identity
    {
        return Err(StorageError::new(
            "historyFileChanged",
            "The temporary directory changed before removal",
        ));
    }
    let actual = crate::video::download::history::permanent::native_file_path(&file)?;
    if actual != path.canonicalize().map_err(io_failure)? {
        return Err(StorageError::new(
            "historyFileChanged",
            "The temporary directory moved before removal",
        ));
    }
    crate::video::download::history::recycle::reject_links(path)?;
    // The kernel checks emptiness atomically and deletes this exact opened directory.
    crate::video::download::history::permanent::delete_open_file(&file)
}

pub(crate) fn remove_empty(directory: &TemporaryDirectory) -> Result<(), StorageError> {
    let guard = validate(directory)?;
    if std::fs::read_dir(&directory.path)
        .map_err(io_failure)?
        .next()
        .is_none()
    {
        drop(guard);
        #[cfg(windows)]
        delete_empty_directory(directory)?;
    }
    Ok(())
}

pub(crate) fn delete(directory: &TemporaryDirectory) -> Result<bool, StorageError> {
    crate::video::download::history::recycle::reject_links(Path::new(&directory.path))?;
    match std::fs::symlink_metadata(&directory.path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_failure(e)),
        Ok(_) => {}
    }
    let guard = validate(directory)?;
    #[cfg(not(windows))]
    {
        let _ = guard;
        return Err(StorageError::new(
            "historyFileDeletionUnsupported",
            "Safe temporary file deletion is unavailable on this operating system",
        ));
    }
    #[cfg(windows)]
    {
        let root = PathBuf::from(&directory.path)
            .canonicalize()
            .map_err(io_failure)?;
        let paths = std::fs::read_dir(&root)
            .map_err(io_failure)?
            .map(|entry| {
                let entry = entry.map_err(io_failure)?;
                let path = entry.path();
                crate::video::download::history::recycle::reject_links(&path)?;
                if !entry.file_type().map_err(io_failure)?.is_file() {
                    return Err(unsafe_path(
                        "Unexpected directory or link inside a temporary download",
                    ));
                }
                Ok(path)
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let mut deleted = false;
        for path in paths {
            deleted |= crate::video::download::history::permanent::delete_validated_file(
                &path,
                None,
                None,
                || {
                    validate(directory)?;
                    crate::video::download::history::recycle::reject_links(&path)
                },
            )?;
        }
        drop(guard);
        // Never recurse: an unexpected newly added entry keeps the record for retry.
        delete_empty_directory(directory)?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests;

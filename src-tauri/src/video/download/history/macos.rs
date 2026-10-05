use super::permanent::io_failure;
use crate::database::{
    download_records::{identity, DownloadRecord},
    StorageError,
};
use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::{
    NSError, NSFileManager, NSFileNoSuchFileError, NSFileReadNoPermissionError,
    NSFileReadNoSuchFileError, NSFileWriteNoPermissionError, NSFileWriteVolumeReadOnlyError, NSURL,
};
use std::{
    ffi::{CStr, CString, OsString},
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    ptr::NonNull,
};

fn changed() -> StorageError {
    StorageError::new(
        "historyFileChanged",
        "The completed output moved, was replaced or modified",
    )
}

fn c_path(path: &Path) -> Result<CString, StorageError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|e| StorageError::new("historyFileUnsafe", e))
}

// Hold the file to prevent inode reuse and capture a full leaf file-reference URL.
// Foundation receives that object reference, never the original mutable pathname.
// Recheck location and identity before mutation; no reference is stored across runs.
struct CheckedOutput {
    parent: File,
    name: CString,
    path: PathBuf,
    identity: String,
    _file: File,
    reference: Retained<NSURL>,
}

impl CheckedOutput {
    fn open(record: &DownloadRecord, path: &Path) -> Result<Option<Self>, StorageError> {
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path.parent().ok_or_else(changed)?)
            .map_err(io_failure)?;
        let name = c_path(Path::new(path.file_name().ok_or_else(changed)?))?;
        let file = match open_at(&parent, &name) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_failure(e)),
        };
        let actual = identity::from_file(&file).map_err(io_failure)?;
        let metadata = file.metadata().map_err(io_failure)?;
        if record
            .output_identity
            .as_ref()
            .is_some_and(|expected| expected != &actual)
            || record
                .file_size_bytes
                .is_some_and(|size| metadata.len() != size)
        {
            return Err(changed());
        }
        let checked = Self {
            parent,
            name,
            path: path.to_owned(),
            identity: actual,
            reference: file_reference(path)?,
            _file: file,
        };
        checked.recheck(record)?;
        Ok(Some(checked))
    }

    fn delete_native(&self) -> Result<bool, StorageError> {
        if self.unlinked()? {
            return Ok(false);
        }
        autoreleasepool(
            |_| match NSFileManager::new().removeItemAtURL_error(&self.reference) {
                Ok(()) => Ok(true),
                Err(error) => {
                    if self.unlinked()? {
                        return Ok(false);
                    }
                    let error = foundation_failure(&error, "historyFileDeleteFailed");
                    if error.code == "historyFileMissing" {
                        Ok(false)
                    } else {
                        Err(error)
                    }
                }
            },
        )
    }

    fn recycle_native(&self) -> Result<(bool, Option<PathBuf>), StorageError> {
        if self.unlinked()? {
            return Ok((false, None));
        }
        autoreleasepool(|_| {
            let mut destination = None;
            match NSFileManager::new()
                .trashItemAtURL_resultingItemURL_error(&self.reference, Some(&mut destination))
            {
                Ok(()) => Ok((true, destination.map(|url| url_path(&url)))),
                Err(error) => {
                    if self.unlinked()? {
                        return Ok((false, None));
                    }
                    let error = foundation_failure(&error, "historyRecycleFailed");
                    if error.code == "historyFileMissing" {
                        Ok((false, None))
                    } else {
                        Err(error)
                    }
                }
            }
        })
    }

    fn recheck(&self, record: &DownloadRecord) -> Result<(), StorageError> {
        if self.unlinked()? {
            return Err(changed());
        }
        let directory = record
            .successful_output
            .as_ref()
            .map(|output| output.directory.as_str())
            .unwrap_or(&record.download_directory);
        if super::recycle::validated_output(
            Path::new(record.output_path.as_deref().ok_or_else(changed)?),
            Path::new(directory),
            record.file_size_bytes,
        )?
        .as_deref()
            != Some(self.path.as_path())
        {
            return Err(changed());
        }
        let file = open_at(&self.parent, &self.name).map_err(io_failure)?;
        if identity::from_file(&file).map_err(io_failure)? != self.identity {
            return Err(changed());
        }
        // Verify the resolved leaf reference against the held file before giving
        // it to Foundation. A partial reference with a mutable suffix is rejected.
        let resolved = autoreleasepool(|_| self.reference.filePathURL().map(|url| url_path(&url)))
            .ok_or_else(changed)?;
        if resolved != self.path {
            return Err(changed());
        }
        let absolute = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&resolved)
            .map_err(io_failure)?;
        if identity::from_file(&absolute).map_err(io_failure)? != self.identity {
            return Err(changed());
        }
        Ok(())
    }

    fn unlinked(&self) -> Result<bool, StorageError> {
        // A held descriptor proves whether the selected inode has disappeared,
        // even when Foundation returns an unresolvable-reference URL error.
        // Object references cannot select one of several hard-link entries safely.
        let links = self._file.metadata().map_err(io_failure)?.nlink();
        if links > 1 {
            return Err(StorageError::new(
                "historyFileUnsafe",
                "Hard-linked outputs cannot be deleted using an object reference",
            ));
        }
        Ok(links == 0)
    }
}

fn open_at(parent: &File, name: &CStr) -> std::io::Result<File> {
    // SAFETY: name is NUL-terminated, parent stays open, and successful descriptors
    // are transferred to File exactly once. O_NONBLOCK avoids waiting on a replaced FIFO.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn delete(record: &DownloadRecord, path: &Path) -> Result<bool, StorageError> {
    let Some(output) = CheckedOutput::open(record, path)? else {
        return Ok(false);
    };
    output.recheck(record)?;
    output.delete_native()
}

pub(super) fn recycle(
    record: &DownloadRecord,
    path: &Path,
) -> Result<(bool, Option<PathBuf>), StorageError> {
    let Some(output) = CheckedOutput::open(record, path)? else {
        return Ok((false, None));
    };
    output.recheck(record)?;
    output.recycle_native()
}

fn foundation_failure(error: &NSError, fallback: &str) -> StorageError {
    let code = match (error.domain().to_string().as_str(), error.code()) {
        ("NSPOSIXErrorDomain", code) if code == libc::ENOENT as isize => "historyFileMissing",
        ("NSPOSIXErrorDomain", code) => {
            let mut error = io_failure(std::io::Error::from_raw_os_error(code as i32));
            if error.code == "historyFileDeleteFailed" {
                error.code = fallback.into();
            }
            return error;
        }
        ("NSCocoaErrorDomain", code)
            if code == NSFileReadNoPermissionError || code == NSFileWriteNoPermissionError =>
        {
            "historyFilePermissionDenied"
        }
        ("NSCocoaErrorDomain", code) if code == NSFileWriteVolumeReadOnlyError => {
            "historyFileReadOnly"
        }
        ("NSCocoaErrorDomain", code)
            if code == NSFileNoSuchFileError || code == NSFileReadNoSuchFileError =>
        {
            "historyFileMissing"
        }
        _ => fallback,
    };
    StorageError::new(code, error.localizedDescription())
}

fn url_path(url: &NSURL) -> PathBuf {
    // SAFETY: NSURL owns a valid NUL-terminated representation during this copy.
    let bytes = unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) };
    PathBuf::from(OsString::from_vec(bytes.to_bytes().to_vec()))
}

fn file_reference(path: &Path) -> Result<Retained<NSURL>, StorageError> {
    let bytes = c_path(path)?;
    autoreleasepool(|_| {
        // SAFETY: bytes is a valid NUL-terminated filesystem path and lives until
        // NSURL has copied it. Preserve non-UTF-8 names instead of lossy conversion.
        let url = unsafe {
            NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                NonNull::new(bytes.as_ptr().cast_mut()).unwrap(),
                false,
                None,
            )
        };
        let reference = url.fileReferenceURL().ok_or_else(changed)?;
        let text = reference.absoluteString().ok_or_else(changed)?.to_string();
        let parsed = url::Url::parse(&text).map_err(|_| changed())?;
        let leaf = parsed
            .path()
            .trim_end_matches('/')
            .strip_prefix("/.file/id=");
        if !reference.isFileReferenceURL()
            || !leaf.is_some_and(|id| !id.is_empty() && !id.contains('/'))
        {
            return Err(StorageError::new(
                "historyFileUnsafe",
                "A complete file-object reference is unavailable on this volume",
            ));
        }
        Ok(reference)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        download_records::{DownloadRecordOutcome, DownloadSnapshot},
        Database,
    };

    fn fixture() -> (tempfile::TempDir, Database, DownloadRecord) {
        let root = tempfile::Builder::new()
            .prefix("evd-macos-native-")
            .tempdir()
            .unwrap();
        eprintln!("owned macOS native fixture: {}", root.path().display());
        let directory = root.path().canonicalize().unwrap();
        let database =
            Database::open(&directory.join("app.db"), &directory.join("legacy")).unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        let mut page = crate::database::persistence_tests::page();
        page.download_directory = directory.to_string_lossy().into();
        let id = database
            .begin_download_record(
                &request,
                &DownloadSnapshot { page },
                &crate::datetime::now(),
            )
            .unwrap();
        let output = directory.join(format!("evd-owned-{request} 视频 #.mp4"));
        std::fs::write(&output, b"owned native video").unwrap();
        database
            .finish_download_record(
                &request,
                &DownloadRecordOutcome::Completed {
                    path: output.to_string_lossy().into(),
                    size: 18,
                    extension: Some("mp4".into()),
                },
            )
            .unwrap();
        let record = database.get_download_record(id).unwrap();
        (root, database, record)
    }

    // Record only the exact URL returned for our own fixture. Never enumerate or
    // clear the user's Trash, and refuse cleanup if the file's identity changed.
    struct OwnedTrashedOutput {
        path: PathBuf,
        identity: String,
    }
    impl Drop for OwnedTrashedOutput {
        fn drop(&mut self) {
            if identity::capture(&self.path).as_deref() == Some(&self.identity) {
                eprintln!(
                    "cleaning exact owned Trash fixture: {}",
                    self.path.display()
                );
                if let Err(error) = std::fs::remove_file(&self.path) {
                    eprintln!(
                        "owned Trash fixture cleanup failed: {}: {error}",
                        self.path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn native_recycle_moves_only_the_final_file_and_updates_history_after_success() {
        let (root, database, record) = fixture();
        let output = Path::new(record.output_path.as_deref().unwrap());
        let sibling = root.path().join("cover.jpg");
        std::fs::write(&sibling, b"keep cover").unwrap();
        let mut cleanup = None;
        let result = database
            .recycle_download_record_file(record.id, |record| {
                let (recycled, destination) = recycle(record, output)?;
                cleanup = Some(OwnedTrashedOutput {
                    path: destination
                        .expect("Foundation should return the owned Trash destination"),
                    identity: record.output_identity.clone().unwrap(),
                });
                eprintln!(
                    "exact owned Trash fixture: {}",
                    cleanup.as_ref().unwrap().path.display()
                );
                Ok(recycled)
            })
            .unwrap();
        assert!(result);
        assert!(!output.exists());
        assert_eq!(
            std::fs::read(&cleanup.as_ref().unwrap().path).unwrap(),
            b"owned native video"
        );
        assert_eq!(std::fs::read(sibling).unwrap(), b"keep cover");
        let after = database.get_download_record(record.id).unwrap();
        assert!(after.deleted_at.is_some());
        assert!(after.file_deleted_at.is_some());
        assert_eq!(after.output_path, record.output_path);
        assert_eq!(
            serde_json::to_value(after.successful_output).unwrap(),
            serde_json::to_value(record.successful_output).unwrap()
        );
        let trashed = cleanup.as_ref().unwrap().path.clone();
        drop(cleanup);
        assert!(
            !trashed.exists(),
            "owned Trash fixture must be cleaned: {}",
            trashed.display()
        );
    }

    #[test]
    fn native_delete_does_not_delete_a_replacement_after_the_final_check() {
        for directory in [false, true] {
            let (root, _database, record) = fixture();
            let path = Path::new(record.output_path.as_deref().unwrap());
            let checked = CheckedOutput::open(&record, path).unwrap().unwrap();
            std::fs::rename(path, root.path().join("moved-original.mp4")).unwrap();
            let replacement = if directory {
                std::fs::create_dir(path).unwrap();
                path.join("keep.txt")
            } else {
                path.to_owned()
            };
            std::fs::write(&replacement, b"replacement video!").unwrap();
            assert!(checked.delete_native().unwrap());
            assert_eq!(std::fs::read(replacement).unwrap(), b"replacement video!");
        }
    }

    #[test]
    fn native_recycle_does_not_trash_a_replacement_after_the_final_check() {
        for directory in [false, true] {
            let (root, _database, record) = fixture();
            let path = Path::new(record.output_path.as_deref().unwrap());
            let checked = CheckedOutput::open(&record, path).unwrap().unwrap();
            std::fs::rename(path, root.path().join("moved-original.mp4")).unwrap();
            let replacement = if directory {
                std::fs::create_dir(path).unwrap();
                path.join("keep.txt")
            } else {
                path.to_owned()
            };
            std::fs::write(&replacement, b"replacement video!").unwrap();
            let (recycled, destination) = checked.recycle_native().unwrap();
            let destination = destination.unwrap();
            eprintln!(
                "exact owned post-check Trash fixture: {}",
                destination.display()
            );
            let cleanup = OwnedTrashedOutput {
                identity: identity::capture(&destination).unwrap(),
                path: destination,
            };
            assert!(recycled);
            assert_eq!(std::fs::read(&cleanup.path).unwrap(), b"owned native video");
            assert_eq!(std::fs::read(replacement).unwrap(), b"replacement video!");
            let trashed = cleanup.path.clone();
            drop(cleanup);
            assert!(!trashed.exists());
        }
    }

    #[test]
    fn native_delete_keeps_an_ancestor_replacement_after_the_final_check() {
        let (root, _database, mut record) = fixture();
        let original = PathBuf::from(record.output_path.as_deref().unwrap());
        let directory = root.path().canonicalize().unwrap().join("download");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join(original.file_name().unwrap());
        std::fs::rename(&original, &path).unwrap();
        record.output_path = Some(path.to_string_lossy().into());
        let checked = CheckedOutput::open(&record, &path).unwrap().unwrap();
        let moved = directory.with_file_name("moved-download");
        std::fs::rename(&directory, &moved).unwrap();
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(&path, b"replacement video!").unwrap();
        assert!(checked.delete_native().unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement video!");
        assert!(!moved.join(original.file_name().unwrap()).exists());
    }

    #[test]
    fn native_operations_allow_the_checked_file_to_disappear() {
        let (_root, _database, record) = fixture();
        let path = Path::new(record.output_path.as_deref().unwrap());
        let checked = CheckedOutput::open(&record, path).unwrap().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!checked.delete_native().unwrap());
        assert_eq!(checked.recycle_native().unwrap(), (false, None));
    }

    #[test]
    fn native_operations_reject_hard_links_without_removing_either_entry() {
        let (root, _database, record) = fixture();
        let path = Path::new(record.output_path.as_deref().unwrap());
        let alias = root.path().join("linked.mp4");
        std::fs::hard_link(path, &alias).unwrap();
        assert_eq!(delete(&record, path).unwrap_err().code, "historyFileUnsafe");
        assert_eq!(
            recycle(&record, path).unwrap_err().code,
            "historyFileUnsafe"
        );
        assert_eq!(std::fs::read(path).unwrap(), b"owned native video");
        assert_eq!(std::fs::read(alias).unwrap(), b"owned native video");
    }

    #[test]
    fn native_recycle_rejects_a_replacement_without_removing_the_record() {
        let (root, database, record) = fixture();
        let output = Path::new(record.output_path.as_deref().unwrap());
        std::fs::rename(output, root.path().join("original.mp4")).unwrap();
        std::fs::write(output, b"replacement video!").unwrap();
        let result = database.recycle_download_record_file(record.id, |record| {
            super::super::recycle::recycle_output_file(record)
        });
        assert_eq!(result.unwrap_err().code, "historyFileChanged");
        assert_eq!(std::fs::read(output).unwrap(), b"replacement video!");
        let after = database.get_download_record(record.id).unwrap();
        assert!(after.deleted_at.is_none());
        assert!(after.file_deleted_at.is_none());
    }

    #[test]
    fn native_delete_permission_failure_preserves_file_and_record() {
        use std::os::unix::fs::PermissionsExt;
        let (root, database, record) = fixture();
        database.delete_download_record(record.id).unwrap();
        let directory = root.path();
        let permissions = std::fs::metadata(directory).unwrap().permissions();
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result =
            database.purge_download_record(record.id, super::super::permanent::delete_output_file);
        std::fs::set_permissions(directory, permissions).unwrap();
        assert_eq!(result.unwrap_err().code, "historyFilePermissionDenied");
        assert_eq!(
            std::fs::read(record.output_path.unwrap()).unwrap(),
            b"owned native video"
        );
        assert!(database
            .get_download_record(record.id)
            .unwrap()
            .deleted_at
            .is_some());
    }

    #[test]
    fn native_empty_trash_removes_files_and_missing_records() {
        let (_root, database, record) = fixture();
        database.delete_download_record(record.id).unwrap();
        database
            .empty_download_record_trash(super::super::permanent::delete_output_file)
            .unwrap();
        assert!(!Path::new(record.output_path.as_deref().unwrap()).exists());
        assert_eq!(
            database.get_download_record(record.id).unwrap_err().code,
            "recordNotFound"
        );
    }
}

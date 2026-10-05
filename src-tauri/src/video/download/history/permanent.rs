use crate::database::{download_records::DownloadRecord, StorageError};
use std::path::{Path, PathBuf};

pub(crate) fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

fn failure(error: impl ToString) -> StorageError {
    StorageError::new("historyFileDeleteFailed", error)
}

pub(super) fn io_failure(error: std::io::Error) -> StorageError {
    if super::super::failure::file_is_occupied(&error) {
        StorageError::new("historyFileOccupied", error)
    } else if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageError::new("historyFilePermissionDenied", error)
    } else {
        #[cfg(target_os = "macos")]
        match error.raw_os_error() {
            Some(libc::EROFS) => return StorageError::new("historyFileReadOnly", error),
            Some(libc::EBUSY) => return StorageError::new("historyFileOccupied", error),
            _ => {}
        }
        failure(error)
    }
}

fn validate(record: &DownloadRecord) -> Result<Option<PathBuf>, StorageError> {
    let Some(output) = record.output_path.as_deref() else {
        return Ok(None);
    };
    if record.successful_output.is_none() && record.status != "completed" {
        return Err(StorageError::new(
            "historyFileUnsafe",
            "Only confirmed final output files may be deleted",
        ));
    }
    super::recycle::validated_output(
        Path::new(output),
        Path::new(
            record
                .successful_output
                .as_ref()
                .map(|s| s.directory.as_str())
                .unwrap_or(&record.download_directory),
        ),
        record.file_size_bytes,
    )
        .map_err(|error| {
            if error.code == "historyRecycleFailed" {
                failure(error.detail)
            } else {
                error
            }
        })
}

pub(crate) fn delete_output_file(
    record: &DownloadRecord,
    protected: &[String],
) -> Result<bool, StorageError> {
    let Some(path) = validate(record)? else {
        return Ok(false);
    };
    // A later normal record may refer to this same output. Do not delete its video
    // while purging an older trashed record, including alternate Windows path spelling.
    if protected.iter().any(|other| {
        Path::new(other)
            .canonicalize()
            .is_ok_and(|other| other == path)
    }) {
        return Err(StorageError::new(
            "historyFileInUse",
            "The output is also referenced by a record outside trash",
        ));
    }
    #[cfg(windows)]
    {
        delete_windows(record, &path)
    }
    #[cfg(target_os = "macos")]
    {
        super::macos::delete(record, &path)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err(StorageError::new(
            "historyFileDeletionUnsupported",
            "Safe permanent file deletion is unavailable on this operating system",
        ))
    }
}

#[cfg(windows)]
fn delete_windows(record: &DownloadRecord, path: &Path) -> Result<bool, StorageError> {
    use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows::{
        Wdk::Storage::FileSystem::{
            FileDispositionInformation, NtSetInformationFile, FILE_DISPOSITION_INFORMATION,
        },
        Win32::{
            Foundation::{RtlNtStatusToDosError, HANDLE, STATUS_CANNOT_DELETE},
            Storage::FileSystem::{
                GetFinalPathNameByHandleW, DELETE, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_READ_ATTRIBUTES, VOLUME_NAME_DOS,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };
    // Hold the actual file exclusively: readers must not defer deletion until after restart.
    // Delete via this handle, never by launching a shell or resolving the name again.
    let file = match std::fs::OpenOptions::new()
        .access_mode(DELETE.0 | FILE_READ_ATTRIBUTES.0)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_failure(error)),
    };
    let metadata = file.metadata().map_err(io_failure)?;
    if !metadata.is_file()
        || record
        .file_size_bytes
        .is_some_and(|size| metadata.len() != size)
    {
        return Err(StorageError::new(
            "historyFileChanged",
            "The opened output does not match the completed download",
        ));
    }
    if let Some(expected) = &record.output_identity {
        let actual =
            crate::database::download_records::identity::from_file(&file).map_err(io_failure)?;
        if &actual != expected {
            return Err(StorageError::new(
                "historyFileChanged",
                "The completed file was replaced or modified",
            ));
        }
    }
    let handle = HANDLE(file.as_raw_handle());
    let needed = unsafe { GetFinalPathNameByHandleW(handle, &mut [], VOLUME_NAME_DOS) };
    if needed == 0 {
        return Err(io_failure(std::io::Error::last_os_error()));
    }
    let mut buffer = vec![0u16; needed as usize + 1];
    let written = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_DOS) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(io_failure(std::io::Error::last_os_error()));
    }
    let actual = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written as usize]));
    if actual != path || validate(record)?.as_deref() != Some(path) {
        return Err(StorageError::new(
            "historyFileChanged",
            "The output moved or changed before deletion",
        ));
    }
    let disposition = FILE_DISPOSITION_INFORMATION { DeleteFile: true };
    let mut io_status = IO_STATUS_BLOCK::default();
    // Preserve NTSTATUS: Win32 collapses a live mapped view and permission denial
    // into ERROR_ACCESS_DENIED. Do not bypass read-only attributes or sharing rules.
    let status = unsafe {
        NtSetInformationFile(
            handle,
            &mut io_status,
            (&disposition as *const FILE_DISPOSITION_INFORMATION).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFORMATION>() as u32,
            FileDispositionInformation,
        )
    };
    if status.0 < 0 {
        let detail =
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32);
        if status == STATUS_CANNOT_DELETE {
            let readonly = file
                .metadata()
                .map_err(io_failure)?
                .permissions()
                .readonly();
            return Err(StorageError::new(
                if readonly {
                    "historyFileReadOnly"
                } else {
                    "historyFileOccupied"
                },
                detail,
            ));
        }
        return Err(io_failure(detail));
    }
    drop(file);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::download_records::DownloadRecord;

    fn fixture() -> (tempfile::TempDir, DownloadRecord) {
        let dir = tempfile::Builder::new()
            .prefix("evd-history-permanent-")
            .tempdir()
            .unwrap();
        eprintln!("owned permanent-delete fixture: {}", dir.path().display());
        let root = dir.path().canonicalize().unwrap();
        let output = root.join("video.mp4");
        std::fs::write(&output, b"owned video!").unwrap();
        let record = DownloadRecord {
            id: 1,
            request_id: "fixture".into(),
            platform: "youtube".into(),
            video_id: "fixture".into(),
            file_availability: "unknown".into(),
            successful_output: None,
            source_link: "https://youtu.be/fixture".into(),
            title: "Owned fixture".into(),
            format_id: "a".into(),
            download_directory: root.to_string_lossy().into(),
            output_path: Some(output.to_string_lossy().into()),
            status: "completed".into(),
            size_approximate: false,
            cookie_fallback: false,
            file_size_bytes: Some(12),
            output_identity: None,
            started_at: "2026-10-04 10:00:00".into(),
            updated_at: "2026-10-04 10:01:00".into(),
            deleted_at: Some("2026-10-04 10:02:00".into()),
            thumbnail_url: None,
            thumbnail_cache_path: None,
            duration_seconds: None,
            format_extension: Some("mp4".into()),
            height: None,
            fps: None,
            selected_size_bytes: None,
            output_extension: Some("mp4".into()),
            error_code: None,
            error_detail: None,
            error_stage: None,
            failure_kind: None,
            finished_at: None,
            file_deleted_at: None,
        };
        (dir, record)
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn permanent_native_delete_removes_only_the_owned_final_file_and_missing_is_idempotent() {
        assert!(supported());
        let (dir, record) = fixture();
        let sibling = dir.path().join("cover.jpg");
        std::fs::write(&sibling, b"keep cover").unwrap();
        assert!(delete_output_file(&record, &[]).unwrap());
        assert!(!dir.path().join("video.mp4").exists());
        assert_eq!(std::fs::read(sibling).unwrap(), b"keep cover");
        assert!(!delete_output_file(&record, &[]).unwrap());
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn unsupported_native_deletion_keeps_existing_files_and_allows_missing_record_cleanup() {
        let (dir, record) = fixture();
        assert!(!supported());
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileDeletionUnsupported"
        );
        let output = dir.path().join("video.mp4");
        assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
        std::fs::remove_file(&output).unwrap();
        assert!(!delete_output_file(&record, &[]).unwrap());
    }

    #[test]
    fn permanent_native_delete_rejects_changed_files_and_live_history_references() {
        let (dir, mut record) = fixture();
        let output = dir.path().join("video.mp4");
        assert_eq!(
            delete_output_file(
                &record,
                &[output.canonicalize().unwrap().to_string_lossy().into()]
            )
                .unwrap_err()
                .code,
            "historyFileInUse"
        );
        record.file_size_bytes = Some(13);
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileChanged"
        );
        assert_eq!(std::fs::read(output).unwrap(), b"owned video!");
    }

    #[test]
    fn permanent_native_delete_never_deletes_directories_or_outputs_outside_the_recorded_directory()
    {
        let (dir, mut record) = fixture();
        let downloads = dir.path().join("downloads");
        std::fs::create_dir(&downloads).unwrap();
        record.download_directory = downloads.to_string_lossy().into();
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileUnsafe"
        );
        record.output_path = Some(downloads.to_string_lossy().into());
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileUnsafe"
        );
        assert_eq!(
            std::fs::read(dir.path().join("video.mp4")).unwrap(),
            b"owned video!"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_native_delete_preserves_a_replacement_with_the_same_size() {
        let (dir, mut record) = fixture();
        let output = dir.path().join("video.mp4");
        record.output_identity = crate::database::download_records::identity::capture(&output);
        std::fs::rename(&output, dir.path().join("original.mp4")).unwrap();
        std::fs::write(&output, b"replacement!").unwrap();
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileChanged"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"replacement!");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_native_delete_rejects_a_symlink_and_classifies_permission_errors() {
        let (dir, record) = fixture();
        let output = dir.path().join("video.mp4");
        let saved = dir.path().join("original.mp4");
        std::fs::rename(&output, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &output).unwrap();
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileUnsafe"
        );
        assert_eq!(std::fs::read(&saved).unwrap(), b"owned video!");
        for code in [libc::EACCES, libc::EPERM] {
            assert_eq!(
                io_failure(std::io::Error::from_raw_os_error(code)).code,
                "historyFilePermissionDenied"
            );
        }
        assert_eq!(
            io_failure(std::io::Error::from_raw_os_error(libc::EROFS)).code,
            "historyFileReadOnly"
        );
    }

    #[cfg(windows)]
    #[test]
    fn permanent_native_delete_preserves_locked_files_until_the_handle_is_closed() {
        use std::os::windows::fs::OpenOptionsExt;
        let (dir, record) = fixture();
        let output = dir.path().join("video.mp4");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&output)
            .unwrap();
        assert_eq!(
            delete_output_file(&record, &[]).unwrap_err().code,
            "historyFileOccupied"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
        drop(lock);
        assert!(delete_output_file(&record, &[]).unwrap());
        assert!(!output.exists());
    }

    #[cfg(windows)]
    #[test]
    fn permanent_native_delete_does_not_mistake_read_only_or_permission_denial_for_occupation() {
        let (dir, record) = fixture();
        let output = dir.path().join("video.mp4");
        let original = std::fs::metadata(&output).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&output, readonly).unwrap();
        let result = delete_output_file(&record, &[]);
        std::fs::set_permissions(&output, original).unwrap();
        assert_eq!(result.unwrap_err().code, "historyFileReadOnly");
        assert_eq!(
            io_failure(std::io::Error::from_raw_os_error(5)).code,
            "historyFilePermissionDenied"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
        assert!(delete_output_file(&record, &[]).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn permanent_native_delete_identifies_a_mapped_view_after_the_player_closes_its_file_handle() {
        let (dir, record) = fixture();
        let output = dir.path().join("video.mp4");
        let mapped = crate::video::download::tests::MappedFileRead::new(&output);
        let failure = delete_output_file(&record, &[]).unwrap_err();
        eprintln!(
            "mapped-view delete error: {} {}",
            failure.code, failure.detail
        );
        assert_eq!(failure.code, "historyFileOccupied");
        assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
        drop(mapped);
        assert!(delete_output_file(&record, &[]).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn permanent_native_delete_refuses_players_that_allow_delete_sharing() {
        use std::os::windows::fs::OpenOptionsExt;
        let (dir, record) = fixture();
        let output = dir.path().join("video.mp4");
        let player = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(7)
            .open(&output)
            .unwrap();
        let failure = delete_output_file(&record, &[])
            .expect_err("a reader must never leave the original file pending deletion");
        assert_eq!(failure.code, "historyFileOccupied");
        assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
        drop(player);
        assert!(delete_output_file(&record, &[]).unwrap());
        assert!(!output.exists());
    }
}

#[cfg(test)]
mod shared_path_review_tests {
    use super::*;
    #[test]
    fn review_purging_one_trashed_reference_keeps_the_other_recoverable_file() {
        use crate::database::{
            download_records::{DownloadRecordOutcome, DownloadSnapshot},
            persistence_tests::page,
            Database,
        };
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary shared trashed output directory: {}",
            root.path().display()
        );
        let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
        let output = root.path().join("shared.webm");
        std::fs::write(&output, b"video").unwrap();
        let mut ids = Vec::new();
        for video in ["first", "second"] {
            let mut state = page();
            state.video_id = Some(video.into());
            state.input_link = format!("https://youtu.be/{video}");
            state.download_directory = root.path().to_string_lossy().into();
            let id = db
                .begin_download_record(
                    video,
                    &DownloadSnapshot { page: state },
                    "2026-10-04 10:00:00",
                )
                .unwrap();
            db.finish_download_record(
                video,
                &DownloadRecordOutcome::Completed {
                    path: output.to_string_lossy().into(),
                    size: 5,
                    extension: Some("webm".into()),
                },
            )
                .unwrap();
            db.delete_download_record(id).unwrap();
            ids.push(id);
        }
        assert_eq!(
            db.purge_download_record(ids[0], delete_output_file)
                .unwrap_err()
                .code,
            "historyFileInUse"
        );
        assert!(output.exists());
        assert!(db.get_download_record(ids[1]).is_ok());
    }
}

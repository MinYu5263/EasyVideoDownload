use crate::database::StorageError;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    core::{implement, Error, Ref, HRESULT, PCWSTR},
    Win32::{
        Foundation::E_ABORT,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::Shell::{
            FileOperation, IFileOperation, IFileOperationProgressSink,
            IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName,
            FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO,
            FOF_NO_CONNECTED_ELEMENTS, FOF_NO_UI, TSF_DELETE_RECYCLE_IF_POSSIBLE,
        },
    },
};

fn failure(error: impl ToString) -> StorageError {
    StorageError::new("historyRecycleFailed", error)
}

pub(super) fn missing_suffix_contains(
    parent: &Path,
    directory: &[std::ffi::OsString],
    output: &[std::ffi::OsString],
) -> bool {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Globalization::{CompareStringOrdinal, CSTR_EQUAL},
        Storage::FileSystem::{
            FileCaseSensitiveInfo, GetFileInformationByHandleEx, FILE_CASE_SENSITIVE_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        },
    };
    // Missing suffixes use the existing parent directory's name comparison rules. When
    // the filesystem cannot expose that policy, exact spelling is the conservative choice.
    let ignore_case = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode(7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(parent)
        .ok()
        .and_then(|file| {
            let mut info = FILE_CASE_SENSITIVE_INFO::default();
            unsafe {
                GetFileInformationByHandleEx(
                    HANDLE(file.as_raw_handle()),
                    FileCaseSensitiveInfo,
                    (&mut info as *mut FILE_CASE_SENSITIVE_INFO).cast(),
                    std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
                )
                    .ok()
                    .map(|_| info.Flags & 1 == 0)
            }
        })
        .unwrap_or(false);
    directory.iter().zip(output).all(|(directory, output)| {
        let directory: Vec<u16> = directory.encode_wide().collect();
        let output: Vec<u16> = output.encode_wide().collect();
        unsafe { CompareStringOrdinal(&directory, &output, ignore_case) == CSTR_EQUAL }
    })
}

struct Apartment;
impl Apartment {
    fn enter() -> Result<Self, StorageError> {
        // Every operation gets a dedicated STA; it never relies on Tauri's worker apartment.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
                .ok()
                .map_err(failure)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) fn recycle(
    path: PathBuf,
    directory: PathBuf,
    size: Option<u64>,
    expected_identity: Option<String>,
) -> Result<(), StorageError> {
    std::thread::Builder::new()
        .name("history-file-recycle".into())
        .spawn(move || {
            let _apartment = Apartment::enter()?;
            recycle_on_sta(&path, &directory, size, expected_identity.as_deref()).map(|_| ())
        })
        .map_err(failure)?
        .join()
        .map_err(|_| failure("The native recycle worker stopped unexpectedly"))?
}

fn shell_item(path: &Path) -> Result<IShellItem, StorageError> {
    use std::os::windows::ffi::OsStrExt;
    let path = super::ordinary_path(path)?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(super::unsafe_file(
            "The output path contains a null character",
        ));
    }
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(failure) }
}

fn operation() -> Result<IFileOperation, StorageError> {
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        // RECYCLEONDELETE never permits the permanent-delete fallback. The sink additionally
        // vetoes operations without the shell's recycle transfer flag.
        operation
            .SetOperationFlags(
                FOFX_RECYCLEONDELETE
                    | FOFX_ADDUNDORECORD
                    | FOF_ALLOWUNDO
                    | FOF_NO_UI
                    | FOFX_EARLYFAILURE
                    | FOF_NO_CONNECTED_ELEMENTS,
            )
            .map_err(failure)?;
        Ok(operation)
    }
}

#[derive(Default)]
struct DeleteState {
    recycled: Option<IShellItem>,
    error: Option<StorageError>,
}

#[implement(IFileOperationProgressSink)]
struct RecycleSink {
    state: Rc<RefCell<DeleteState>>,
    path: PathBuf,
    directory: PathBuf,
    size: Option<u64>,
    expected_identity: Option<String>,
}

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for RecycleSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, hrresult: HRESULT) -> windows::core::Result<()> {
        if let Err(error) = hrresult.ok() {
            self.state
                .borrow_mut()
                .error
                .get_or_insert_with(|| failure(error));
        }
        Ok(())
    }
    fn PreDeleteItem(&self, flags: u32, _item: Ref<IShellItem>) -> windows::core::Result<()> {
        #[cfg(test)]
        eprintln!("native owned fixture pre-delete flags: {flags:#x}");
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.state.borrow_mut().error = Some(failure(
                "The system proposed permanent deletion; recycling was cancelled",
            ));
            return Err(Error::from_hresult(E_ABORT));
        }
        let checked =
            super::validated_output(&self.path, &self.directory, self.size).and_then(|path| {
                if let Some(path) = &path {
                    super::validate_identity(path, self.expected_identity.as_deref())?;
                }
                Ok(path)
            });
        match checked {
            Ok(Some(path)) if path == self.path => Ok(()),
            result => {
                self.state.borrow_mut().error = Some(match result {
                    Err(error) => error,
                    _ => failure("The output changed before the recycle operation"),
                });
                Err(Error::from_hresult(E_ABORT))
            }
        }
    }
    fn PostDeleteItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        result: HRESULT,
        recycled: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        let mut state = self.state.borrow_mut();
        match result.ok() {
            Ok(()) => match recycled.cloned() {
                Some(item) => state.recycled = Some(item),
                None => state.error = Some(failure("The system did not confirm a recycled item")),
            },
            Err(error) => {
                state.error.get_or_insert_with(|| failure(error));
            }
        }
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_ABORT))
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_ABORT))
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.state.borrow_mut().error =
            Some(failure("Unexpected move callback in recycle operation"));
        Err(Error::from_hresult(E_ABORT))
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_ABORT))
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.state.borrow_mut().error =
            Some(failure("Unexpected copy callback in recycle operation"));
        Err(Error::from_hresult(E_ABORT))
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_ABORT))
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        self.state.borrow_mut().error =
            Some(failure("Unexpected new-item callback in recycle operation"));
        Err(Error::from_hresult(E_ABORT))
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Err(Error::from_hresult(E_ABORT))
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

fn recycle_on_sta(
    path: &Path,
    directory: &Path,
    size: Option<u64>,
    identity: Option<&str>,
) -> Result<IShellItem, StorageError> {
    let directory = directory.canonicalize().map_err(failure)?;
    let path = super::validated_output(path, &directory, size)?
        .ok_or_else(|| failure("The output disappeared before recycling"))?;
    super::validate_identity(&path, identity)?;
    let item = shell_item(&path)?;
    let operation = operation()?;
    let state = Rc::new(RefCell::new(DeleteState::default()));
    let sink: IFileOperationProgressSink = RecycleSink {
        state: state.clone(),
        path,
        directory,
        size,
        expected_identity: identity.map(str::to_owned),
    }
        .into();
    unsafe {
        operation.DeleteItem(&item, &sink).map_err(failure)?;
        let performed = operation.PerformOperations();
        let aborted = operation.GetAnyOperationsAborted();
        // A successful PostDelete with a receipt is the actual filesystem outcome, even
        // when later shell bookkeeping fails. The database must then record partial success.
        if let Some(receipt) = state.borrow_mut().recycled.take() {
            return Ok(receipt);
        }
        if let Some(error) = state.borrow_mut().error.take() {
            return Err(error);
        }
        performed.map_err(failure)?;
        if aborted.map_err(failure)?.as_bool() {
            return Err(failure("The system cancelled recycling"));
        }
        Err(failure(
            "The system did not confirm that the file entered its recycle bin",
        ))
    }
}

#[cfg(test)]
#[test]
#[ignore = "Performs a native recycle and restores only its owned fixture receipt"]
fn native_recycle_owned_fixture_and_restore() {
    let directory = tempfile::Builder::new()
        .prefix("evd-native-recycle-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned native recycle fixture: {}",
        directory.path().display()
    );
    let path = directory.path().join("only-owned-fixture.mp4");
    std::fs::write(&path, b"EasyVideoDownload owned recycle fixture").unwrap();
    let path = path.canonicalize().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let worker_path = path.clone();
    std::thread::spawn(move || -> Result<(), StorageError> {
        let _apartment = Apartment::enter()?;
        let identity = crate::database::download_records::identity::capture(&worker_path)
            .expect("the owned native recycle fixture has a file identity");
        let receipt = recycle_on_sta(&worker_path, &root, Some(39), Some(&identity))?;
        if worker_path.exists() {
            return Err(failure(
                "The recycled fixture is still at its original path",
            ));
        }
        let operation = operation()?;
        let folder = shell_item(&root)?;
        use std::os::windows::ffi::OsStrExt;
        let name: Vec<u16> = worker_path
            .file_name()
            .unwrap()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Move only the exact receipt returned by our operation; never enumerate user trash.
        unsafe {
            operation
                .MoveItem(&receipt, &folder, PCWSTR(name.as_ptr()), None)
                .map_err(failure)?;
            operation.PerformOperations().map_err(failure)?;
            if operation
                .GetAnyOperationsAborted()
                .map_err(failure)?
                .as_bool()
            {
                return Err(failure("Owned fixture restoration was cancelled"));
            }
        }
        Ok(())
    })
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"EasyVideoDownload owned recycle fixture"
    );
}

#[cfg(test)]
#[test]
#[ignore = "Exercises a native delete failure using only an exclusively locked owned fixture"]
fn native_recycle_locked_owned_fixture_preserves_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::Builder::new()
        .prefix("evd-locked-recycle-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned locked recycle fixture: {}",
        directory.path().display()
    );
    let path = directory.path().join("locked-owned-fixture.mp4");
    std::fs::write(&path, b"original owned content").unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let result = recycle(path.clone(), directory.path().to_path_buf(), Some(22), None);
    assert_eq!(result.unwrap_err().code, "historyRecycleFailed");
    drop(lock);
    assert_eq!(std::fs::read(path).unwrap(), b"original owned content");
}

#[cfg(test)]
#[test]
fn native_recycle_refuses_same_size_replacement_before_shell_operation() {
    use crate::database::{
        download_records::{DownloadRecordOutcome, DownloadSnapshot},
        persistence_tests::page,
        Database,
    };
    use std::os::windows::fs::OpenOptionsExt;

    let root = tempfile::Builder::new()
        .prefix("evd-recycle-identity-")
        .tempdir()
        .unwrap();
    eprintln!("owned recycle identity fixture: {}", root.path().display());
    let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let path = root.path().join("video.mp4");
    std::fs::write(&path, b"original").unwrap();
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    let id = db
        .begin_download_record(
            "recycle-identity",
            &DownloadSnapshot { page: state },
            "2026-10-04 10:00:00",
        )
        .unwrap();
    db.finish_download_record(
        "recycle-identity",
        &DownloadRecordOutcome::Completed {
            path: path.to_string_lossy().into(),
            size: 8,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    let record = db.get_download_record(id).unwrap();
    assert!(record.output_identity.is_some());
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"replaced").unwrap();
    // The baseline must never move this owned replacement into the system recycle bin.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let failure = super::recycle_output_file(&record).unwrap_err();
    drop(lock);
    assert_eq!(failure.code, "historyFileChanged");
    assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
}

#[cfg(test)]
#[test]
fn native_recycle_predelete_rechecks_identity_and_allows_legacy_records() {
    let root = tempfile::Builder::new()
        .prefix("evd-recycle-predelete-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned recycle pre-delete fixture: {}",
        root.path().display()
    );
    let path = root.path().join("video.mp4");
    std::fs::write(&path, b"original").unwrap();
    let expected_identity = crate::database::download_records::identity::capture(&path).unwrap();
    let directory = root.path().canonicalize().unwrap();
    let path = path.canonicalize().unwrap();
    let _apartment = Apartment::enter().unwrap();
    let item = shell_item(&path).unwrap();
    let state = Rc::new(RefCell::new(DeleteState::default()));
    let sink: IFileOperationProgressSink = RecycleSink {
        state: state.clone(),
        path: path.clone(),
        directory: directory.clone(),
        size: Some(8),
        expected_identity: Some(expected_identity),
    }
        .into();
    let flags = TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32;
    // Exercise the real COM callback without asking the shell to delete any file.
    unsafe { sink.PreDeleteItem(flags, &item) }.unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"replaced").unwrap();
    assert_eq!(
        unsafe { sink.PreDeleteItem(flags, &item) }
            .unwrap_err()
            .code(),
        E_ABORT
    );
    assert_eq!(
        state.borrow().error.as_ref().unwrap().code,
        "historyFileChanged"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"replaced");

    let state = Rc::new(RefCell::new(DeleteState::default()));
    let legacy: IFileOperationProgressSink = RecycleSink {
        state: state.clone(),
        path: path.clone(),
        directory,
        size: Some(8),
        expected_identity: None,
    }
        .into();
    unsafe { legacy.PreDeleteItem(flags, &item) }.unwrap();
    assert!(state.borrow().error.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
}

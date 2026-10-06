use super::{directory_identity, io_failure, unsafe_path, validate, TemporaryDirectory};
use crate::database::StorageError;
use crate::video::download::history::recycle::reject_links;
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

// Verify CoreServices object references against held file identities. Unlike
// Foundation's recursive removal, FSUnlinkObject only removes
// files or empty directories, preserving entries added during cleanup.
// The SDK marks these APIs deprecated, but they remain available on macOS.
#[repr(C)]
struct FSRef {
    hidden: [u8; 80],
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn FSPathMakeRefWithOptions(
        path: *const u8,
        options: u32,
        reference: *mut FSRef,
        is_directory: *mut u8,
    ) -> i32;
    fn FSRefMakePath(reference: *const FSRef, path: *mut u8, maximum: u32) -> i32;
    fn FSUnlinkObject(reference: *const FSRef) -> i16;
}

fn native_failure(status: i32) -> StorageError {
    let code = match status {
        -43 | -120 | -1401 => "historyFileMissing",
        -44 | -46 => "historyFileReadOnly",
        -45 | -54 | -61 => "historyFilePermissionDenied",
        -47 => "historyFileOccupied",
        _ => "historyFileDeleteFailed",
    };
    StorageError::new(
        code,
        format!("macOS temporary file operation failed (OSStatus {status})"),
    )
}

struct CheckedEntry {
    file: File,
    reference: FSRef,
    directory: bool,
}

impl CheckedEntry {
    fn open(path: &Path, directory: bool) -> Result<Self, StorageError> {
        reject_links(path)?;
        let mut flags = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        if directory {
            flags |= libc::O_DIRECTORY;
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path)
            .map_err(io_failure)?;
        let metadata = file.metadata().map_err(io_failure)?;
        if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
            return Err(unsafe_path(
                "Unexpected directory or link inside a temporary download",
            ));
        }
        if !directory && metadata.nlink() != 1 {
            return Err(unsafe_path(
                "Hard-linked temporary files cannot be deleted using an object reference",
            ));
        }
        let path_bytes = CString::new(path.as_os_str().as_bytes()).map_err(unsafe_path)?;
        let mut reference = FSRef { hidden: [0; 80] };
        let mut is_directory = 0;
        // SAFETY: the path is NUL-terminated and all output buffers are valid.
        // kFSPathMakeRefDoNotFollowLeafSymlink = 1, from CoreServices/Files.h.
        let status = unsafe {
            FSPathMakeRefWithOptions(
                path_bytes.as_ptr().cast(),
                1,
                &mut reference,
                &mut is_directory,
            )
        };
        if status != 0 {
            return Err(native_failure(status));
        }
        let checked = Self {
            file,
            reference,
            directory,
        };
        if (is_directory != 0) != directory {
            return Err(unsafe_path("The temporary entry changed during validation"));
        }
        checked.recheck(path)?;
        Ok(checked)
    }

    fn recheck(&self, path: &Path) -> Result<(), StorageError> {
        reject_links(path)?;
        let actual = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(io_failure)?;
        if directory_identity(&actual)? != directory_identity(&self.file)? {
            return Err(StorageError::new(
                "historyFileChanged",
                "The temporary entry was replaced",
            ));
        }
        let mut resolved = [0u8; libc::PATH_MAX as usize];
        // SAFETY: reference was initialized by CoreServices; buffer size matches maximum.
        let status = unsafe {
            FSRefMakePath(
                &self.reference,
                resolved.as_mut_ptr(),
                resolved.len() as u32,
            )
        };
        if status != 0 {
            return Err(native_failure(status));
        }
        let length = resolved
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| unsafe_path("Invalid temporary object reference"))?;
        let resolved = Path::new(std::ffi::OsStr::from_bytes(&resolved[..length]));
        if resolved != path
            || directory_identity(
                &OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(resolved)
                    .map_err(io_failure)?,
            )? != directory_identity(&self.file)?
        {
            return Err(StorageError::new(
                "historyFileChanged",
                "The temporary object reference changed",
            ));
        }
        Ok(())
    }

    fn delete_native(&self) -> Result<bool, StorageError> {
        let links = self.file.metadata().map_err(io_failure)?.nlink();
        if links == 0 {
            return Ok(false);
        }
        if !self.directory && links != 1 {
            return Err(unsafe_path(
                "Hard-linked temporary files cannot be deleted using an object reference",
            ));
        }
        // SAFETY: the initialized reference selects the object pinned by file.
        // The kernel refuses nonempty directories; this operation never recurses.
        let status = unsafe { FSUnlinkObject(&self.reference) } as i32;
        if status == 0 {
            return Ok(true);
        }
        if self.file.metadata().map_err(io_failure)?.nlink() == 0 {
            return Ok(false);
        }
        Err(native_failure(status))
    }
}

pub(super) fn delete_empty(directory: &TemporaryDirectory) -> Result<(), StorageError> {
    let guard = validate(directory)?;
    let checked = CheckedEntry::open(Path::new(&directory.path), true)?;
    if directory_identity(&checked.file)? != directory_identity(&guard)? {
        return Err(unsafe_path("The owned temporary directory changed"));
    }
    checked.delete_native()?;
    Ok(())
}

pub(super) fn delete(directory: &TemporaryDirectory, guard: File) -> Result<bool, StorageError> {
    let root = Path::new(&directory.path);
    let checked_root = CheckedEntry::open(root, true)?;
    if directory_identity(&checked_root.file)? != directory_identity(&guard)? {
        return Err(unsafe_path("The owned temporary directory changed"));
    }
    // Validate every entry before mutating anything. Unexpected directories,
    // links and hard links retain the cancelled record and all its fragments.
    let entries = std::fs::read_dir(root)
        .map_err(io_failure)?
        .map(|entry| {
            let path = entry.map_err(io_failure)?.path();
            let checked = CheckedEntry::open(&path, false)?;
            Ok((path, checked))
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let mut deleted = false;
    for (path, entry) in entries {
        validate(directory)?;
        entry.recheck(&path)?;
        deleted |= entry.delete_native()?;
    }
    validate(directory)?;
    checked_root.recheck(root)?;
    checked_root.delete_native()?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::Builder::new()
            .prefix("evd-macos-partial-")
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        eprintln!("owned macOS partial fixture: {}", root.path().display());
        root
    }

    #[test]
    fn native_unlink_preserves_a_replacement_after_the_final_check() {
        for directory in [false, true] {
            let root = fixture();
            let original = root.path().join("owned");
            if directory {
                std::fs::create_dir(&original).unwrap();
            } else {
                std::fs::write(&original, b"fragment").unwrap();
            }
            let checked = CheckedEntry::open(&original, directory).unwrap();
            let moved = root.path().join("moved");
            std::fs::rename(&original, &moved).unwrap();
            std::fs::create_dir(&original).unwrap();
            let replacement = original.join("keep.txt");
            std::fs::write(&replacement, b"keep replacement").unwrap();
            assert!(checked.delete_native().unwrap());
            assert!(!moved.exists());
            assert_eq!(std::fs::read(replacement).unwrap(), b"keep replacement");
        }
    }

    #[test]
    fn native_unlink_keeps_new_entries_in_a_previously_empty_directory() {
        let root = fixture();
        let directory = root.path().join("owned");
        std::fs::create_dir(&directory).unwrap();
        let checked = CheckedEntry::open(&directory, true).unwrap();
        let unexpected = directory.join("keep.txt");
        std::fs::write(&unexpected, b"new entry").unwrap();
        assert!(checked.delete_native().is_err());
        assert_eq!(std::fs::read(unexpected).unwrap(), b"new entry");
    }

    #[test]
    fn native_unlink_rejects_an_added_hard_link() {
        let root = fixture();
        let original = root.path().join("owned.part");
        std::fs::write(&original, b"fragment").unwrap();
        let checked = CheckedEntry::open(&original, false).unwrap();
        let link = root.path().join("keep.part");
        std::fs::hard_link(&original, &link).unwrap();
        assert_eq!(
            checked.delete_native().unwrap_err().code,
            "historyFileUnsafe"
        );
        assert_eq!(std::fs::read(original).unwrap(), b"fragment");
        assert_eq!(std::fs::read(link).unwrap(), b"fragment");
    }
}

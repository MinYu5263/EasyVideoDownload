use std::path::Path;

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

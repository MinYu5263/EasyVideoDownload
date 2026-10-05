use crate::database::StorageError;
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows;

fn unsafe_file(detail: impl ToString) -> StorageError {
    StorageError::new("historyFileUnsafe", detail)
}

fn access_failure(error: std::io::Error) -> StorageError {
    #[cfg(target_os = "macos")]
    {
        return super::permanent::io_failure(error);
    }
    #[cfg(not(target_os = "macos"))]
    StorageError::new(
        if super::super::failure::file_is_occupied(&error) {
            "historyFileOccupied"
        } else {
            "historyFileDeleteFailed"
        },
        error,
    )
}

fn metadata(path: &Path) -> Result<Option<std::fs::Metadata>, StorageError> {
    match std::fs::symlink_metadata(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(access_failure(error)),
    }
}

pub(crate) fn reject_links(path: &Path) -> Result<(), StorageError> {
    let mut ancestor = PathBuf::new();
    for component in path.components() {
        if matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        ) {
            return Err(unsafe_file("Ambiguous output path"));
        }
        ancestor.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        if let Some(value) = metadata(&ancestor)? {
            if value.file_type().is_symlink() || is_reparse_point(&value) {
                return Err(unsafe_file(
                    "Symbolic links and reparse points cannot be deleted",
                ));
            }
        }
    }
    Ok(())
}

fn is_reparse_point(value: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        value.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        false
    }
}

fn ordinary_path(path: &Path) -> Result<PathBuf, StorageError> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        let mut normalized = match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", drive as char)),
                Prefix::VerbatimUNC(server, share) => PathBuf::from(format!(
                    "\\\\{}\\{}",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                )),
                Prefix::Disk(_) | Prefix::UNC(_, _) => PathBuf::from(prefix.as_os_str()),
                _ => return Err(unsafe_file("Device paths cannot be recycled")),
            },
            _ => return Ok(path.to_path_buf()),
        };
        for component in components {
            normalized.push(component);
        }
        Ok(normalized)
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_path_buf())
    }
}

fn missing_directory_contains(directory: &Path, output: &Path) -> Result<bool, StorageError> {
    fn existing_ancestor(path: &Path) -> Result<(PathBuf, Vec<std::ffi::OsString>), StorageError> {
        for ancestor in path.ancestors() {
            match ancestor.canonicalize() {
                Ok(existing) => {
                    let suffix = path
                        .strip_prefix(ancestor)
                        .map_err(unsafe_file)?
                        .components()
                        .map(|part| part.as_os_str().to_owned())
                        .collect();
                    return Ok((existing, suffix));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(access_failure(error)),
            }
        }
        Err(unsafe_file(
            "Cannot establish the missing download directory's parent",
        ))
    }
    let (directory_parent, directory_suffix) = existing_ancestor(directory)?;
    let (output_parent, output_suffix) = existing_ancestor(output)?;
    if directory_parent != output_parent || output_suffix.len() <= directory_suffix.len() {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        Ok(windows::missing_suffix_contains(
            &directory_parent,
            &directory_suffix,
            &output_suffix,
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(output_suffix.starts_with(&directory_suffix))
    }
}

pub(super) fn validated_output(
    output: &Path,
    directory: &Path,
    size: Option<u64>,
) -> Result<Option<PathBuf>, StorageError> {
    let output = ordinary_path(output)?;
    let directory = ordinary_path(directory)?;
    if !output.is_absolute() || !directory.is_absolute() || output == directory {
        return Err(unsafe_file(
            "Output must be an absolute file inside the recorded download directory",
        ));
    }
    reject_links(&directory)?;
    reject_links(&output)?;
    let value = metadata(&output)?;
    let canonical_directory = match directory.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && value.is_none() => {
            return if missing_directory_contains(&directory, &output)? {
                Ok(None)
            } else {
                Err(unsafe_file(
                    "Missing output escapes the recorded download directory",
                ))
            };
        }
        Err(error) => return Err(access_failure(error)),
    };
    let Some(value) = value else {
        // Canonicalize the nearest existing parent so missing outputs still cannot escape
        // the recorded directory, while accepting Windows' case-insensitive path spelling.
        for ancestor in output.ancestors().skip(1) {
            match ancestor.canonicalize() {
                Ok(parent) if parent.starts_with(&canonical_directory) => return Ok(None),
                Ok(_) => {
                    return Err(unsafe_file(
                        "Missing output escapes the recorded download directory",
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(access_failure(error)),
            }
        }
        return Err(unsafe_file(
            "Cannot establish the missing output's parent directory",
        ));
    };
    if !value.is_file() {
        return Err(unsafe_file("The recorded output is not a regular file"));
    }
    if size.is_some_and(|size| value.len() != size) {
        return Err(StorageError::new(
            "historyFileChanged",
            "The output size differs from the completed download",
        ));
    }
    let output = output.canonicalize().map_err(access_failure)?;
    if !output.starts_with(&canonical_directory) || output == canonical_directory {
        return Err(unsafe_file(
            "Output escapes the recorded download directory",
        ));
    }
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::Builder::new()
            .prefix("evd-history-recycle-")
            .tempdir()
            .unwrap();
        eprintln!("owned recycle fixture: {}", directory.path().display());
        directory
    }

    #[test]
    fn recycle_validation_rejects_relative_and_outside_paths() {
        let fixture = fixture();
        let root = fixture.path().join("downloads");
        std::fs::create_dir(&root).unwrap();
        let outside = fixture.path().join("outside.mp4");
        std::fs::write(&outside, b"owned fixture").unwrap();
        for (output, directory) in [
            (Path::new("relative.mp4"), root.as_path()),
            (outside.as_path(), root.as_path()),
            (outside.as_path(), Path::new("relative")),
        ] {
            assert_eq!(
                validated_output(output, directory, None).unwrap_err().code,
                "historyFileUnsafe"
            );
        }
        assert_eq!(std::fs::read(outside).unwrap(), b"owned fixture");
    }

    #[test]
    fn recycle_validation_rejects_directory_and_changed_size() {
        let fixture = fixture();
        let output = fixture.path().join("video.mp4");
        std::fs::write(&output, b"original").unwrap();
        assert_eq!(
            validated_output(fixture.path(), fixture.path(), None)
                .unwrap_err()
                .code,
            "historyFileUnsafe"
        );
        assert_eq!(
            validated_output(&output, fixture.path(), Some(9))
                .unwrap_err()
                .code,
            "historyFileChanged"
        );
        assert_eq!(std::fs::read(output).unwrap(), b"original");
    }

    #[test]
    fn recycle_validation_reports_only_missing_file_as_absent() {
        let fixture = fixture();
        let output = fixture.path().join("absent.mp4");
        assert!(validated_output(&output, fixture.path(), Some(8))
            .unwrap()
            .is_none());
        std::fs::write(&output, b"original").unwrap();
        assert_eq!(
            validated_output(&output, fixture.path(), Some(8)).unwrap(),
            Some(output.canonicalize().unwrap())
        );
    }

    #[cfg(windows)]
    #[test]
    fn recycle_validation_accepts_canonical_output_with_ordinary_download_directory() {
        let fixture = fixture();
        let output = fixture.path().join("video.mp4");
        std::fs::write(&output, b"original").unwrap();
        let canonical = output.canonicalize().unwrap();
        assert_eq!(
            validated_output(&canonical, fixture.path(), Some(8)).unwrap(),
            Some(canonical)
        );
    }

    #[cfg(windows)]
    #[test]
    fn recycle_validation_uses_canonical_windows_directory_case() {
        let fixture = fixture();
        let root = fixture.path().join("DownloadsMixedCase");
        std::fs::create_dir(&root).unwrap();
        let output = root.join("video.mp4");
        std::fs::write(&output, b"original").unwrap();
        let recorded = PathBuf::from(root.to_string_lossy().to_ascii_lowercase());
        assert_eq!(
            validated_output(&output, &recorded, Some(8)).unwrap(),
            Some(output.canonicalize().unwrap())
        );
        assert!(validated_output(&root.join("missing.mp4"), &recorded, None)
            .unwrap()
            .is_none());
    }

    #[cfg(windows)]
    #[test]
    fn recycle_validation_allows_missing_directory_with_windows_case_spelling() {
        let fixture = fixture();
        let recorded = fixture.path().join("DOWNLOADS");
        let missing = fixture.path().join("Downloads").join("video.mp4");
        assert!(validated_output(&missing, &recorded, Some(8))
            .unwrap()
            .is_none());
        assert_eq!(
            validated_output(
                &fixture.path().join("other").join("video.mp4"),
                &recorded,
                None
            )
                .unwrap_err()
                .code,
            "historyFileUnsafe"
        );
    }

    #[cfg(windows)]
    #[test]
    fn recycle_validation_rejects_junction_ancestor() {
        let fixture = fixture();
        let real = fixture.path().join("real");
        let linked = fixture.path().join("linked");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("video.mp4"), b"original").unwrap();
        // Junctions do not require symlink privileges; this owned link points only at our fixture.
        let result = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&linked)
            .arg(&real)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            validated_output(&linked.join("video.mp4"), fixture.path(), None)
                .unwrap_err()
                .code,
            "historyFileUnsafe"
        );
        std::fs::remove_dir(&linked).unwrap();
        assert_eq!(std::fs::read(real.join("video.mp4")).unwrap(), b"original");
    }
}

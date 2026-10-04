use super::*;

pub(super) fn complete(directory: &Path) -> bool {
    ["yt-dlp", "_internal/Python", "_internal/base_library.zip"]
        .iter()
        .all(|name| std::fs::symlink_metadata(directory.join(name))
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0))
}

pub(super) async fn download(
    client: &reqwest::Client,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    let filename = asset("macos", std::env::consts::ARCH)?;
    let tag = latest_release_tag(client, "yt-dlp/yt-dlp").await?;
    parse_version("yt-dlp", &tag)?;
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{tag}");
    let sums = read_response(get(client, &format!("{base}/SHA2-256SUMS")).await?, 64 * 1024).await?;
    let expected = checksum(std::str::from_utf8(&sums).map_err(invalid)?, filename)?;
    let package = destination.parent().unwrap().join(filename);
    archive(client, &format!("{base}/{filename}"), Some(&expected), &package, progress).await?;
    progress(ConfigureProgress { phase: "extracting", downloaded: 0, total: None });
    extract_zip_layout(&package, destination, &[], cancellation, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn package(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn bundle_keeps_nested_runtime_and_renames_only_the_entrypoint() {
        let root = tempfile::tempdir().unwrap();
        eprintln!("bundle extraction test: {}", root.path().display());
        let package_path = root.path().join("bundle.zip");
        let destination = root.path().join("new");
        package(&package_path, &[
            ("yt-dlp_macos", b"entrypoint"),
            ("_internal/Python", b"runtime"),
            ("_internal/base_library.zip", b"stdlib"),
            ("_internal/certifi/cacert.pem", b"certificates"),
            ("_internal/certifi/py.typed", b""),
            ("_internal/Python.framework/Versions/3.14/Python", b"framework"),
        ]);
        let (_sender, receiver) = watch::channel(false);
        extract_zip_layout(&package_path, &destination, &[], &receiver, true).unwrap();
        assert!(complete(&destination));
        assert_eq!(std::fs::read(destination.join("yt-dlp")).unwrap(), b"entrypoint");
        assert_eq!(std::fs::read(destination.join("_internal/certifi/cacert.pem")).unwrap(), b"certificates");
        assert!(!destination.join("yt-dlp_macos").exists());
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(std::fs::metadata(destination.join("yt-dlp")).unwrap().permissions().mode() & 0o100, 0);
        }
        root.close().unwrap();
    }

    #[test]
    fn incomplete_unsafe_or_cancelled_bundle_is_not_accepted() {
        for entries in [
            vec![("yt-dlp_macos", b"binary".as_slice())],
            vec![("../outside", b"bad".as_slice())],
            vec![("_internal/Python", b"runtime".as_slice()), ("yt-dlp", b"unexpected".as_slice())],
            vec![("_internal/../../outside", b"bad".as_slice())],
        ] {
            let root = tempfile::tempdir().unwrap();
            eprintln!("invalid bundle test: {}", root.path().display());
            let path = root.path().join("bundle.zip");
            package(&path, &entries);
            let (_sender, receiver) = watch::channel(false);
            assert!(extract_zip_layout(&path, &root.path().join("new"), &[], &receiver, true).is_err());
            assert!(!root.path().join("outside").exists());
            root.close().unwrap();
        }
        let root = tempfile::tempdir().unwrap();
        eprintln!("cancelled bundle test: {}", root.path().display());
        let path = root.path().join("bundle.zip");
        package(&path, &[("yt-dlp_macos", b"binary")]);
        let (_sender, receiver) = watch::channel(true);
        assert_eq!(extract_zip_layout(&path, &root.path().join("new"), &[], &receiver, true).unwrap_err().code, "configureCancelled");
        root.close().unwrap();
    }

    #[test]
    fn bundle_rejects_symbolic_links() {
        let root = tempfile::tempdir().unwrap();
        eprintln!("bundle link test: {}", root.path().display());
        let path = root.path().join("bundle.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.add_symlink("_internal/Python", "../../outside", SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
        let (_sender, receiver) = watch::channel(false);
        assert_eq!(extract_zip_layout(&path, &root.path().join("new"), &[], &receiver, true).unwrap_err().code, "downloadInvalid");
        root.close().unwrap();
    }
}

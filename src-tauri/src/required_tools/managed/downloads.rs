use super::*;
use std::io::Read;

const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 256 * 1024 * 1024;

fn invalid(detail: impl ToString) -> RequiredToolError {
    error("downloadInvalid", "", detail)
}

fn digest(text: &str) -> Result<Vec<u8>, RequiredToolError> {
    // The official Windows Deno release uses formatted Get-FileHash output.
    let fields: Vec<_> = text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect();
    let algorithms: Vec<_> = fields
        .iter()
        .filter(|(key, _)| *key == "Algorithm")
        .map(|(_, value)| *value)
        .collect();
    let hashes: Vec<_> = fields
        .iter()
        .filter(|(key, _)| *key == "Hash")
        .map(|(_, value)| *value)
        .collect();
    let value = if !algorithms.is_empty() || !hashes.is_empty() {
        if algorithms != ["SHA256"] || hashes.len() != 1 {
            return Err(invalid("invalid checksum fields"));
        }
        hashes[0]
    } else {
        text.split_whitespace()
            .next()
            .ok_or_else(|| invalid("missing checksum"))?
    };
    checksum(&format!("{value}  asset"), "asset")
}

fn deno_asset(os: &str, arch: &str) -> Result<String, RequiredToolError> {
    let platform = match (os, arch) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return Err(error("automaticUnsupported", "deno", "")),
    };
    Ok(format!("deno-{platform}.zip"))
}

// Extract only expected executables into a flat, application-owned directory.
// Reject traversal, links, duplicate binaries and excessive expansion.
fn extract_zip(
    archive: &Path,
    destination: &Path,
    names: &[String],
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    let file = std::fs::File::open(archive).map_err(|e| error("configureWriteFailed", "", e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(invalid)?;
    if zip.len() > 4096 {
        return Err(invalid("too many archive entries"));
    }
    let mut found = std::collections::BTreeSet::new();
    for index in 0..zip.len() {
        if *cancellation.borrow() {
            return Err(error("configureCancelled", "", ""));
        }
        let mut entry = zip.by_index(index).map_err(invalid)?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| invalid("unsafe archive path"))?;
        let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
        if kind != 0 && kind != 0o100000 && kind != 0o040000 {
            return Err(invalid("archive link or special file"));
        }
        if entry.is_dir() {
            continue;
        }
        let Some(name) = enclosed.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !names.iter().any(|expected| expected == name) {
            continue;
        }
        if !found.insert(name.to_owned()) {
            return Err(invalid("duplicate executable"));
        }
        if entry.size() == 0 || entry.size() > MAX_EXECUTABLE {
            return Err(invalid("invalid executable size"));
        }
        let path = destination.join(name);
        let mut output =
            std::fs::File::create(&path).map_err(|e| error("configureWriteFailed", "", e))?;
        let mut copied = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if *cancellation.borrow() {
                return Err(error("configureCancelled", "", ""));
            }
            let count = entry.read(&mut buffer).map_err(invalid)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > MAX_EXECUTABLE {
                return Err(invalid("executable too large"));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|e| error("configureWriteFailed", "", e))?;
        }
        output
            .sync_all()
            .map_err(|e| error("configureWriteFailed", "", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| error("configureWriteFailed", "", e))?;
        }
    }
    if found.len() != names.len() {
        return Err(invalid("missing executable"));
    }
    Ok(())
}

async fn archive(
    client: &reqwest::Client,
    url: &str,
    expected: Option<&[u8]>,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
) -> Result<(), RequiredToolError> {
    let mut response = get(client, url).await?;
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_ARCHIVE) {
        return Err(invalid("archive too large"));
    }
    let mut output =
        std::fs::File::create(destination).map_err(|e| error("configureWriteFailed", "", e))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0;
    let mut last = std::time::Instant::now();
    progress(ConfigureProgress {
        phase: "downloading",
        downloaded,
        total,
    });
    while let Some(chunk) = response.chunk().await.map_err(download_error)? {
        downloaded += chunk.len() as u64;
        if downloaded > MAX_ARCHIVE {
            return Err(invalid("archive too large"));
        }
        output
            .write_all(&chunk)
            .map_err(|e| error("configureWriteFailed", "", e))?;
        hasher.update(&chunk);
        if last.elapsed() >= Duration::from_millis(100) {
            progress(ConfigureProgress {
                phase: "downloading",
                downloaded,
                total,
            });
            last = std::time::Instant::now();
        }
    }
    output
        .sync_all()
        .map_err(|e| error("configureWriteFailed", "", e))?;
    drop(output);
    progress(ConfigureProgress {
        phase: "verifying",
        downloaded,
        total,
    });
    if downloaded == 0 || expected.is_some_and(|sum| hasher.finalize()[..] != *sum) {
        return Err(invalid("checksum mismatch"));
    }
    Ok(())
}

async fn deno(
    client: &reqwest::Client,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    let tag = latest_release_tag(client, "denoland/deno").await?;
    parse_version("deno", &format!("deno {}", tag.trim_start_matches('v')))?;
    let filename = deno_asset(std::env::consts::OS, std::env::consts::ARCH)?;
    let base = format!("https://github.com/denoland/deno/releases/download/{}", tag);
    let sum = read_response(
        get(client, &format!("{base}/{filename}.sha256sum")).await?,
        4096,
    )
        .await?;
    let expected = digest(std::str::from_utf8(&sum).map_err(invalid)?)?;
    let package = destination.parent().unwrap().join("deno.zip");
    archive(
        client,
        &format!("{base}/{filename}"),
        Some(&expected),
        &package,
        progress,
    )
        .await?;
    progress(ConfigureProgress {
        phase: "extracting",
        downloaded: 0,
        total: None,
    });
    extract_zip(
        &package,
        destination,
        &[executable_name("deno")],
        cancellation,
    )
}

fn ffmpeg_windows_package_url(url: &url::Url) -> Result<String, RequiredToolError> {
    if url.scheme() != "https" || url.host_str() != Some("www.gyan.dev") {
        return Err(invalid("unexpected FFmpeg source"));
    }
    let version = url
        .path()
        .strip_prefix("/ffmpeg/builds/packages/ffmpeg-")
        .and_then(|name| name.strip_suffix("-essentials_build.zip.sha256"))
        .filter(|version| {
            version.split('.').count() >= 2
                && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .ok_or_else(|| invalid("invalid checksum URL"))?;
    // Use Gyan's GitHub mirror of exactly the build covered by this checksum.
    Ok(format!(
        "https://github.com/GyanD/codexffmpeg/releases/download/{version}/ffmpeg-{version}-essentials_build.zip"
    ))
}

async fn ffmpeg_windows(
    client: &reqwest::Client,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    // Resolve the checksum redirect first to pin binary and checksum to one release.
    let response = get(
        client,
        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256",
    )
        .await?;
    let binary = ffmpeg_windows_package_url(response.url())?;
    let sum = read_response(response, 4096).await?;
    let expected = digest(std::str::from_utf8(&sum).map_err(invalid)?)?;
    let package = destination.parent().unwrap().join("ffmpeg.zip");
    archive(client, &binary, Some(&expected), &package, progress).await?;
    progress(ConfigureProgress {
        phase: "extracting",
        downloaded: 0,
        total: None,
    });
    extract_zip(
        &package,
        destination,
        &[executable_name("ffmpeg"), executable_name("ffprobe")],
        cancellation,
    )
}

async fn ffmpeg_macos(
    client: &reqwest::Client,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    #[derive(Deserialize)]
    struct Package {
        url: String,
    }
    #[derive(Deserialize)]
    struct Formats {
        zip: Package,
    }
    #[derive(Deserialize)]
    struct Info {
        name: String,
        version: String,
        download: Formats,
    }
    let mut infos = Vec::new();
    for name in ["ffmpeg", "ffprobe"] {
        let info: Info = serde_json::from_slice(
            &read_response(
                get(
                    client,
                    &format!("https://evermeet.cx/ffmpeg/info/{name}/release"),
                )
                    .await?,
                64 * 1024,
            )
                .await?,
        )
            .map_err(invalid)?;
        let url = url::Url::parse(&info.download.zip.url).map_err(invalid)?;
        if info.name != name
            || url.scheme() != "https"
            || url.host_str() != Some("evermeet.cx")
            || !url.path().starts_with("/ffmpeg/")
            || !url.path().ends_with(".zip")
        {
            return Err(invalid("unexpected FFmpeg source"));
        }
        parse_version(name, &format!("{name} version {}", info.version))?;
        infos.push(info);
    }
    if infos[0].version != infos[1].version {
        return Err(invalid("FFmpeg release mismatch"));
    }
    for info in infos {
        let package = destination
            .parent()
            .unwrap()
            .join(format!("{}.zip", info.name));
        // This provider publishes GPG signatures, not SHA-256 sums. HTTPS, ZIP CRC
        // and the native version/identity checks are used without requiring GPG.
        archive(client, &info.download.zip.url, None, &package, progress).await?;
        progress(ConfigureProgress {
            phase: "extracting",
            downloaded: 0,
            total: None,
        });
        extract_zip(
            &package,
            destination,
            &[executable_name(&info.name)],
            cancellation,
        )?;
    }
    Ok(())
}

pub(super) async fn download_tool(
    id: RequiredToolId,
    client: &reqwest::Client,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
    cancellation: &watch::Receiver<bool>,
) -> Result<(), RequiredToolError> {
    progress(ConfigureProgress {
        phase: "preparing",
        downloaded: 0,
        total: None,
    });
    match id {
        RequiredToolId::Ytdlp => {
            download_ytdlp(
                client,
                asset(std::env::consts::OS, std::env::consts::ARCH)?,
                &destination.join(executable_name("yt-dlp")),
                progress,
            )
                .await
        }
        RequiredToolId::Deno => deno(client, destination, progress, cancellation).await,
        RequiredToolId::Ffmpeg if cfg!(windows) => {
            ffmpeg_windows(client, destination, progress, cancellation).await
        }
        RequiredToolId::Ffmpeg => ffmpeg_macos(client, destination, progress, cancellation).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    #[test]
    fn ffmpeg_github_download_is_pinned_to_the_checksum_release() {
        for (source, expected) in [
            (
                "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip.sha256",
                "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
            ),
            (
                "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-8.1-essentials_build.zip.sha256",
                "https://github.com/GyanD/codexffmpeg/releases/download/8.1/ffmpeg-8.1-essentials_build.zip",
            ),
        ] {
            assert_eq!(
                ffmpeg_windows_package_url(&url::Url::parse(source).unwrap()).unwrap(),
                expected,
            );
        }
    }

    #[test]
    fn ffmpeg_github_download_rejects_unexpected_checksum_sources() {
        for source in [
            "https://example.com/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip.sha256",
            "https://www.gyan.dev/other/ffmpeg-9.0.2-essentials_build.zip.sha256",
            "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-full_build.zip.sha256",
            "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2%2Fother-essentials_build.zip.sha256",
            "http://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip.sha256",
        ] {
            assert_eq!(
                ffmpeg_windows_package_url(&url::Url::parse(source).unwrap())
                    .unwrap_err()
                    .code,
                "downloadInvalid",
            );
        }
    }

    fn package(entries: &[(&str, &[u8])], path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    #[test]
    fn windows_deno_checksum_accepts_the_official_powershell_format() {
        let content = format!(
            "\r\nAlgorithm : SHA256\r\nHash      : {}\r\nPath      : C:\\build\\deno.zip\r\n",
            "ab".repeat(32)
        );
        assert_eq!(digest(&content).unwrap(), vec![0xab; 32]);
        assert!(digest(&format!("{content}Hash : {}\n", "cd".repeat(32))).is_err());
        assert!(digest(&content.replace("SHA256", "SHA1")).is_err());
    }

    #[test]
    fn zip_extracts_both_ffmpeg_programs_without_other_package_files() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("download.zip");
        let destination = root.path().join("new");
        std::fs::create_dir(&destination).unwrap();
        package(
            &[
                ("release/bin/ffmpeg", b"ffmpeg"),
                ("release/bin/ffprobe", b"ffprobe"),
                ("release/README", b"readme"),
            ],
            &archive,
        );
        let (_sender, receiver) = watch::channel(false);
        extract_zip(
            &archive,
            &destination,
            &["ffmpeg".into(), "ffprobe".into()],
            &receiver,
        )
            .unwrap();
        assert_eq!(
            std::fs::read(destination.join("ffprobe")).unwrap(),
            b"ffprobe"
        );
        assert_eq!(std::fs::read_dir(destination).unwrap().count(), 2);
    }
    #[test]
    fn zip_rejects_traversal_duplicates_missing_programs_and_cancellation() {
        for entries in [
            vec![("../outside", b"bad".as_slice())],
            vec![
                ("one/deno", b"one".as_slice()),
                ("two/deno", b"two".as_slice()),
            ],
            vec![("README", b"none".as_slice())],
        ] {
            let root = tempfile::tempdir().unwrap();
            let archive = root.path().join("download.zip");
            package(&entries, &archive);
            let (_sender, receiver) = watch::channel(false);
            assert_eq!(
                extract_zip(&archive, root.path(), &["deno".into()], &receiver)
                    .unwrap_err()
                    .code,
                "downloadInvalid"
            );
        }
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("download.zip");
        package(&[("deno", b"valid")], &archive);
        let (_sender, receiver) = watch::channel(true);
        assert_eq!(
            extract_zip(&archive, root.path(), &["deno".into()], &receiver)
                .unwrap_err()
                .code,
            "configureCancelled"
        );
        assert!(!root.path().join("deno").exists());
    }
    #[test]
    fn zip_rejects_symbolic_links() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("download.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .add_symlink("deno", "../outside", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        let (_sender, receiver) = watch::channel(false);
        assert_eq!(
            extract_zip(&archive, root.path(), &["deno".into()], &receiver)
                .unwrap_err()
                .code,
            "downloadInvalid"
        );
        assert!(!root.path().join("deno").exists());
    }

    #[test]
    fn deno_assets_match_desktop_architectures() {
        assert_eq!(
            deno_asset("macos", "aarch64").unwrap(),
            "deno-aarch64-apple-darwin.zip"
        );
        assert_eq!(
            deno_asset("windows", "x86_64").unwrap(),
            "deno-x86_64-pc-windows-msvc.zip"
        );
        assert!(deno_asset("linux", "x86_64").is_err());
    }
}

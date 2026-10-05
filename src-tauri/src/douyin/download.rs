use super::cancel::Cancellation;
use super::{Candidate, LabError};
use serde::Serialize;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedMedia {
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub duration: Option<f64>,
    pub bitrate: Option<u64>,
    pub file_size: u64,
}
fn observed(
    json: &serde_json::Value,
    candidate: &Candidate,
    size: u64,
) -> Result<ObservedMedia, LabError> {
    let stream = json["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| LabError::new("verifyFailed", "No video stream in downloaded file"))?;
    let width = stream["width"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| LabError::new("verifyFailed", "Invalid video width"))?;
    let height = stream["height"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| LabError::new("verifyFailed", "Invalid video height"))?;
    let codec = stream["codec_name"].as_str().unwrap_or("unknown");
    let f = &candidate.format;
    if f.width.is_some_and(|w| w != width)
        || f.height.is_some_and(|h| h != height)
        || (f.codec != "unknown" && f.codec != codec)
    {
        return Err(LabError::new(
            "formatMismatch",
            "Actual media does not match the selected format; parse again",
        ));
    }
    if size == 0 {
        return Err(LabError::new("verifyFailed", "Downloaded file is empty"));
    }
    Ok(ObservedMedia {
        width,
        height,
        codec: codec.into(),
        duration: json["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.0),
        bitrate: json["format"]["bit_rate"]
            .as_str()
            .and_then(|s| s.parse().ok()),
        file_size: size,
    })
}
async fn transfer(
    client: &reqwest::Client,
    urls: &[String],
    path: &Path,
    cancel: &Cancellation,
    progress: impl Fn(u64, Option<u64>),
) -> Result<(), LabError> {
    let mut last = LabError::new("downloadFailed", "All media mirrors failed");
    let mut expired = false;
    for url in urls.iter().take(8) {
        cancel.check()?;
        let attempt = async {
            let mut response = cancel
                .run(async {
                    tokio::select! {
                        result = client.get(url).header("Referer", "https://www.douyin.com/").send() =>
                            result.map_err(|_| LabError::new("downloadFailed", "Media request failed")),
                        _ = cancel.active_timeout(std::time::Duration::from_secs(30)) =>
                            Err(LabError::new("timeout", "Media request timed out")),
                    }
                })
                .await?;
            if [401, 403, 410].contains(&response.status().as_u16()) {
                return Err(LabError::new("signatureExpired", "Media URL expired"));
            }
            if !response.status().is_success() {
                return Err(LabError::new(
                    "downloadFailed",
                    "Media mirror rejected request",
                ));
            }
            let total = response.content_length();
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .await
                .map_err(|_| LabError::new("fileFailed", "Unable to write temporary file"))?;
            let mut received = 0u64;
            progress(0, total);
            let mut last_progress = std::time::Instant::now();
            loop {
                let chunk = cancel
                    .run(async {
                        tokio::select! {
                            result = response.chunk() => result.map_err(|_| LabError::new("downloadFailed", "Media transfer interrupted")),
                            _ = cancel.active_timeout(std::time::Duration::from_secs(30)) => Err(LabError::new("timeout", "Media transfer stalled")),
                        }
                    })
                    .await?;
                let Some(chunk) = chunk else {
                    break;
                };
                cancel.wait_until_resumed().await?;
                file.write_all(&chunk)
                    .await
                    .map_err(|_| LabError::new("fileFailed", "Temporary file write failed"))?;
                received += chunk.len() as u64;
                if last_progress.elapsed() >= std::time::Duration::from_millis(150) {
                    progress(received, total);
                    last_progress = std::time::Instant::now();
                }
            }
            cancel.check()?;
            file.flush()
                .await
                .map_err(|_| LabError::new("fileFailed", "Temporary file flush failed"))?;
            file.sync_all()
                .await
                .map_err(|_| LabError::new("fileFailed", "Temporary file sync failed"))?;
            if received == 0 || total.is_some_and(|n| n != received) {
                return Err(LabError::new("downloadFailed", "Incomplete media transfer"));
            }
            progress(received, total);
            Ok(())
        }
            .await;
        match attempt {
            Ok(()) => return Ok(()),
            Err(e) if e.code == "cancelled" || e.code == "fileFailed" => return Err(e),
            Err(e) => {
                expired |= e.code == "signatureExpired";
                last = e;
            }
        }
    }
    if expired {
        Err(LabError::new("signatureExpired", "Media URLs expired"))
    } else {
        Err(last)
    }
}

async fn probe(
    path: &Path,
    program: &Path,
    cancel: &Cancellation,
) -> Result<serde_json::Value, LabError> {
    let mut command = tokio::process::Command::new(program);
    command
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let (mut child, tree) = crate::required_tools::process_tree::spawn(&mut command)
        .await
        .map_err(|_| LabError::new("verifyFailed", "Unable to start ffprobe"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LabError::new("verifyFailed", "Unable to read ffprobe output"))?;
    let reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(LabError::cancelled()),
        result = tokio::time::timeout(std::time::Duration::from_secs(30),child.wait()) => match result {
            Ok(Ok(status)) if status.success() => Ok(()),
            _ => Err(LabError::new("verifyFailed","ffprobe failed or timed out")),
        }
    };
    drop(tree);
    if status.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let bytes = reader
        .await
        .map_err(|_| LabError::new("verifyFailed", "ffprobe reader failed"))?
        .map_err(|_| LabError::new("verifyFailed", "ffprobe output unavailable"))?;
    status?;
    if bytes.len() > 1024 * 1024 {
        return Err(LabError::new(
            "verifyFailed",
            "ffprobe output exceeds limit",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| LabError::new("verifyFailed", "Invalid ffprobe output"))
}

fn publish(
    mut temporary: tempfile::NamedTempFile,
    directory: &Path,
    stem: &str,
) -> Result<String, LabError> {
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            format!("{stem}.mp4")
        } else {
            format!("{stem} ({suffix}).mp4")
        };
        let path = directory.join(name);
        match temporary.persist_noclobber(&path) {
            Ok(_) => return Ok(path.to_string_lossy().into_owned()),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.file
            }
            Err(_) => {
                return Err(LabError::new(
                    "fileFailed",
                    "Unable to publish downloaded file",
                ))
            }
        }
    }
    Err(LabError::new(
        "fileFailed",
        "Too many files with the same name",
    ))
}

pub(crate) async fn download(
    client: &reqwest::Client,
    parsed: &super::ParsedResult,
    candidate: &Candidate,
    directory: &Path,
    temporary_directory: &Path,
    ffprobe: &Path,
    cancel: &Cancellation,
    progress: impl Fn(u64, Option<u64>),
    verifying: impl Fn(),
) -> Result<(String, ObservedMedia), LabError> {
    if !directory.is_absolute() || !temporary_directory.is_absolute() {
        return Err(LabError::new(
            "fileFailed",
            "Choose an absolute download directory",
        ));
    }
    cancel.check()?;
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|_| LabError::new("fileFailed", "Unable to create download directory"))?;
    let temporary = tempfile::Builder::new()
        .prefix(&format!(".douyin-lab-{}-", uuid::Uuid::new_v4()))
        .suffix(".part")
        .tempfile_in(temporary_directory)
        .map_err(|_| LabError::new("fileFailed", "Unable to create temporary file"))?;
    let mut selected = candidate.clone();
    if let Err(error) = transfer(client, &selected.urls, temporary.path(), cancel, &progress).await
    {
        if error.code != "signatureExpired" {
            return Err(error);
        }
        let refreshed = cancel
            .run(super::http::fetch_detail(client, &parsed.video.video_id))
            .await?;
        selected = refreshed_candidate(&refreshed, &selected)?;
        transfer(client, &selected.urls, temporary.path(), cancel, &progress).await?;
    }
    cancel.check()?;
    verifying();
    cancel.wait_until_resumed().await?;
    let json = probe(temporary.path(), ffprobe, cancel).await?;
    let size = temporary
        .as_file()
        .metadata()
        .map_err(|_| LabError::new("fileFailed", "Unable to inspect downloaded file"))?
        .len();
    let media = observed(&json, &selected, size)?;
    let title: String = parsed
        .video
        .title
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .take(70)
        .collect();
    let stem = format!(
        "{} [{}] {}x{}_{}_{}",
        title.trim_matches([' ', '.']),
        parsed.video.video_id,
        media.width,
        media.height,
        media.codec,
        &candidate.format.id[..12]
    );
    let path = cancel.commit(|| publish(temporary, directory, &stem))?;
    Ok((path, media))
}

fn refreshed_candidate(
    refreshed: &super::ParsedResult,
    selected: &Candidate,
) -> Result<Candidate, LabError> {
    // Unknown quality fields cannot prove equivalence after the platform changes its identity.
    refreshed
        .candidates
        .iter()
        .find(|c| c.format.id == selected.format.id)
        .cloned()
        .ok_or_else(|| LabError::new("formatExpired", "Selected format changed; parse again"))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn native_http_transfer_preserves_partial_file_while_paused_then_continues() {
        use std::{
            io::{Read, Write},
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pause.part");
        eprintln!("owned HTTP pause test file: {}", path.display());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/media", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream.write_all(&vec![7u8; 65536]).unwrap();
        });
        let token = Cancellation::default();
        let control = token.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let observed_pause = paused.clone();
        let output = path.clone();
        let worker = tokio::spawn(async move {
            transfer(&reqwest::Client::new(), &[url], &output, &token, |_, _| {
                if !observed_pause.swap(true, Ordering::SeqCst) {
                    token.set_paused(true);
                }
            })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !paused.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert!(!worker.is_finished());
        control.set_paused(false);
        tokio::time::timeout(std::time::Duration::from_secs(3), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), vec![7u8; 65536]);
        server.join().unwrap();
        root.close().unwrap();
    }
    #[test]
    fn verifies_actual_media_against_selected_dimensions() {
        let parsed = super::super::parse_detail(&json!({"aweme_detail":{"aweme_id":"1","video":{"bit_rate":[{"bit_rate":1000,"play_addr":{"width":1920,"height":1080,"url_list":["https://media.test/a"]}}]}}})).unwrap();
        let c = &parsed.candidates[0];
        let mut probe = json!({"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080}],"format":{"duration":"170.13","bit_rate":"2220338"}});
        let result = observed(&probe, c, 47219203).unwrap();
        assert_eq!(
            (result.width, result.height, result.file_size),
            (1920, 1080, 47219203)
        );
        probe["streams"][0]["width"] = json!(1280);
        assert_eq!(observed(&probe, c, 1).unwrap_err().code, "formatMismatch");
    }

    #[test]
    fn signature_refresh_rejects_unknown_quality_with_a_changed_id() {
        let detail = |key: &str| json!({"aweme_detail":{"aweme_id":"1","video":{"play_addr_h264":{"width":1920,"height":1080,"url_key":key,"url_list":["https://media.test/a"]}}}});
        let old = super::super::parse_detail(&detail("original-quality")).unwrap();
        let new = super::super::parse_detail(&detail("different-quality")).unwrap();
        assert_eq!(
            refreshed_candidate(&new, &old.candidates[0])
                .err()
                .unwrap()
                .code,
            "formatExpired"
        );
    }
    #[test]
    fn signature_refresh_prefers_exact_id_over_ambiguous_unknown_metadata() {
        let addr = |key: &str| json!({"width":1920,"height":1080,"url_key":key,"url_list":["https://media.test/a"]});
        let old = super::super::parse_detail(&json!({"aweme_detail":{"aweme_id":"1","video":{"play_addr_h264":addr("original-quality")}}})).unwrap();
        let new = super::super::parse_detail(&json!({"aweme_detail":{"aweme_id":"1","video":{"play_addr_h264":addr("original-quality"),"bit_rate":[{"play_addr":addr("different-quality")}]}}})).unwrap();
        assert_eq!(
            refreshed_candidate(&new, &old.candidates[0])
                .unwrap()
                .format
                .id,
            old.candidates[0].format.id
        );
    }
    #[tokio::test]
    async fn transfers_real_bytes_with_mirror_fallback_and_progress() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/video", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nvideo",
                )
                .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        eprintln!("douyin transfer test directory: {}", root.path().display());
        let path = root.path().join("owned.part");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let progress = std::sync::Mutex::new(Vec::new());
        let result = transfer(
            &client,
            &["http://127.0.0.1:1/fail".into(), url],
            &path,
            &super::super::cancel::Cancellation::default(),
            |n, total| progress.lock().unwrap().push((n, total)),
        )
            .await;
        if result.is_ok() {
            server.join().unwrap();
        } else {
            drop(server);
        }
        result.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"video");
        assert_eq!(progress.lock().unwrap().last(), Some(&(5, Some(5))));
    }

    #[test]
    fn publication_preserves_existing_file_and_cancelled_file_is_not_published() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "douyin publication test directory: {}",
            root.path().display()
        );
        std::fs::write(root.path().join("test.mp4"), b"original").unwrap();
        let mut temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        temporary.write_all(b"new").unwrap();
        let path = publish(temporary, root.path(), "test").unwrap();
        assert_eq!(
            std::fs::read(root.path().join("test.mp4")).unwrap(),
            b"original"
        );
        assert_eq!(std::fs::read(path).unwrap(), b"new");
        let temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        let owned = temporary.path().to_owned();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            cancel
                .commit(|| publish(temporary, root.path(), "cancelled"))
                .unwrap_err()
                .code,
            "cancelled"
        );
        assert!(!owned.exists());
        assert!(!root.path().join("cancelled.mp4").exists());
    }

    #[tokio::test]
    async fn cancelling_real_transfer_removes_only_its_temporary_file() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/slow", listener.local_addr().unwrap());
        let (sent, received) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut buf = [0; 4096];
            stream.read(&mut buf).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\nx",
                )
                .unwrap();
            let _ = sent.send(());
            let _ = stream.read(&mut buf);
        });
        let parsed = super::super::parse_detail(&json!({"aweme_detail":{"aweme_id":"1","desc":"cancel test","video":{"bit_rate":[{"play_addr":{"url_list":[url]}}]}}})).unwrap();
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "douyin cancellation test directory: {}",
            root.path().display()
        );
        let keep = root.path().join("user.mp4");
        std::fs::write(&keep, b"keep").unwrap();
        let temporary_directory = root.path().join("owned temporary files");
        std::fs::create_dir(&temporary_directory).unwrap();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let trigger = tokio::spawn(async move {
            received.await.unwrap();
            token.cancel();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = download(
            &client,
            &parsed,
            &parsed.candidates[0],
            root.path(),
            &temporary_directory,
            Path::new("unused-ffprobe"),
            &cancel,
            |_, _| {},
            || {},
        )
            .await;
        assert_eq!(result.unwrap_err().code, "cancelled");
        trigger.await.unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&keep).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&temporary_directory).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

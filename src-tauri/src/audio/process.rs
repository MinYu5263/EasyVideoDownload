use super::{media::AudioTrack, AudioError};
use crate::database::download_records::identity;
use std::path::{Path, PathBuf};
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::sync::watch;

async fn bounded_read(
    mut pipe: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    let mut truncated = false;
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
    Ok((bytes, truncated))
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    while !*cancel.borrow_and_update() {
        if cancel.changed().await.is_err() {
            break;
        }
    }
}

async fn progress_read(
    pipe: impl AsyncRead + Unpin,
    duration: Option<f64>,
    progress: &(impl Fn(f64) + Sync),
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    loop {
        line.clear();
        // FFmpeg's progress records are short; cap each read as well as retained diagnostics.
        let count = (&mut reader).take(4096).read_line(&mut line).await?;
        if count == 0 {
            break;
        }
        if count == 4096 {
            return Err(std::io::Error::other(
                "FFmpeg progress record exceeded its size limit",
            ));
        }
        if let (Some(time), Some(total)) = (
            line.trim()
                .strip_prefix("out_time_us=")
                .and_then(|s| s.parse::<f64>().ok()),
            duration,
        ) {
            if time.is_finite() {
                progress((time / 1_000_000.0 / total * 100.0).clamp(0.0, 99.0));
            }
        }
    }
    Ok((Vec::new(), false))
}

async fn run(
    program: &Path,
    args: &[OsString],
    cancel: &mut watch::Receiver<bool>,
    duration: Option<f64>,
    progress: Option<&(impl Fn(f64) + Sync)>,
) -> Result<Vec<u8>, AudioError> {
    if *cancel.borrow() {
        return Err(AudioError::new(
            "cancelled",
            "Audio operation was cancelled",
        ));
    }
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, tree) = crate::required_tools::process_tree::spawn(&mut command)
        .await
        .map_err(|e| AudioError::new("processFailed", e))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let operation = async {
        let read_stdout = async {
            match progress {
                Some(callback) => progress_read(stdout, duration, callback).await,
                None => bounded_read(stdout, 1024 * 1024).await,
            }
        };
        let (status, (output, overflow), (diagnostic, _)) =
            tokio::try_join!(child.wait(), read_stdout, bounded_read(stderr, 16 * 1024))
                .map_err(|e| AudioError::new("processFailed", e))?;
        if !status.success() {
            let detail = String::from_utf8_lossy(&diagnostic);
            return Err(AudioError::new(
                if progress.is_some() {
                    "extractFailed"
                } else {
                    "probeFailed"
                },
                if detail.trim().is_empty() {
                    format!("Media process exited with {status}")
                } else {
                    detail.trim().to_owned()
                },
            ));
        }
        if overflow {
            return Err(AudioError::new(
                "probeFailed",
                "FFprobe output exceeded its size limit",
            ));
        }
        Ok(output)
    };
    let result = tokio::select! {
        result = async {
            if progress.is_none() { tokio::time::timeout(Duration::from_secs(30), operation).await.map_err(|_| AudioError::new("probeFailed", "FFprobe timed out"))? }
            else { operation.await }
        } => result,
        _ = cancelled(cancel) => Err(AudioError::new("cancelled", "Audio operation was cancelled")),
    };
    drop(tree);
    if result.is_err() {
        let _ = child.start_kill();
    }
    let _ = child.wait().await;
    result
}

pub(super) async fn probe(
    program: &Path,
    source: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<serde_json::Value, AudioError> {
    let mut args: Vec<OsString> = [
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
        "-i",
    ]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(source.as_os_str().to_owned());
    let bytes = run(program, &args, cancel, None, None::<&fn(f64)>).await?;
    serde_json::from_slice(&bytes).map_err(|e| AudioError::new("probeFailed", e))
}

fn check_source(source: &Path, expected: &str) -> Result<(), AudioError> {
    let current =
        identity::capture_checked(source).map_err(|e| AudioError::new("sourceChanged", e))?;
    if current != expected {
        return Err(AudioError::new(
            "sourceChanged",
            "The selected video changed; select it again to parse its current audio tracks",
        ));
    }
    Ok(())
}

pub(super) async fn extract(
    ffmpeg: &Path,
    ffprobe: &Path,
    source: &Path,
    identity: &str,
    track: &AudioTrack,
    cancel: &mut watch::Receiver<bool>,
    progress: impl Fn(f64) + Sync,
) -> Result<PathBuf, AudioError> {
    check_source(source, identity)?;
    let parent = source
        .parent()
        .ok_or_else(|| AudioError::new("invalidPath", "Video has no parent directory"))?;
    let (extension, muxer) = super::media::output_format(&track.codec);
    let temporary = tempfile::Builder::new()
        .prefix(".evd-audio-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(parent)
        .map_err(|e| AudioError::new("outputFailed", e))?
        .into_temp_path();
    let mut args: Vec<OsString> = [
        "-nostdin",
        "-v",
        "error",
        "-y",
        "-protocol_whitelist",
        "file,pipe",
        "-i",
    ]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(source.as_os_str().to_owned());
    args.extend(["-map".into(), format!("0:{}", track.index).into()]);
    args.extend(
        [
            "-vn",
            "-sn",
            "-dn",
            "-map_chapters",
            "-1",
            "-c:a",
            "copy",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            muxer,
        ]
            .iter()
            .map(OsString::from),
    );
    args.push(temporary.as_os_str().to_owned());
    run(ffmpeg, &args, cancel, track.duration, Some(&progress)).await?;
    if *cancel.borrow() {
        return Err(AudioError::new(
            "cancelled",
            "Audio operation was cancelled",
        ));
    }
    check_source(source, identity)?;
    if std::fs::metadata(&temporary)
        .map_err(|e| AudioError::new("outputFailed", e))?
        .len()
        == 0
    {
        return Err(AudioError::new(
            "outputFailed",
            "FFmpeg produced an empty audio file",
        ));
    }
    let verified = probe(ffprobe, &temporary, cancel).await?;
    let streams = verified["streams"].as_array().ok_or_else(|| {
        AudioError::new("outputFailed", "Extracted audio has no readable streams")
    })?;
    let tracks = super::media::parse_tracks(&verified)?;
    if streams.len() != 1 || tracks.len() != 1 || tracks[0].codec != track.codec {
        return Err(AudioError::new(
            "outputFailed",
            "Extracted audio did not match the selected audio stream",
        ));
    }
    if let (Some(expected), Some(actual)) = (track.duration, tracks[0].duration) {
        if (expected - actual).abs() > 2.0_f64.max(expected * 0.02) {
            return Err(AudioError::new(
                "outputFailed",
                "Extracted audio duration did not match the selected stream",
            ));
        }
    }
    check_source(source, identity)?;
    if *cancel.borrow() {
        return Err(AudioError::new(
            "cancelled",
            "Audio operation was cancelled",
        ));
    }
    let stem = source
        .file_stem()
        .ok_or_else(|| AudioError::new("invalidPath", "Video has no file name"))?;
    let mut temporary = temporary;
    for suffix in 0..10_000 {
        let mut name = stem.to_os_string();
        if suffix != 0 {
            name.push(format!(" ({suffix})"));
        }
        name.push(format!(".{extension}"));
        let output = parent.join(name);
        // Atomic no-clobber publication also protects races with another application.
        match temporary.persist_noclobber(&output) {
            Ok(()) => return Ok(output),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.path
            }
            Err(error) => return Err(AudioError::new("outputFailed", error.error)),
        }
    }
    Err(AudioError::new(
        "outputFailed",
        "No unused audio output file name was available",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires installed FFmpeg/FFprobe; explicitly exercised during audio feature validation"]
    async fn real_ffmpeg_selects_one_track_and_never_overwrites() {
        let ffmpeg =
            which::which("ffmpeg").expect("FFmpeg must be available for this integration test");
        let ffprobe =
            which::which("ffprobe").expect("FFprobe must be available for this integration test");
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("访谈 test.mkv");
        let mut command = tokio::process::Command::new(&ffmpeg);
        command
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=32x32:rate=10:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=1",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-c:v",
                "mpeg4",
                "-c:a:0",
                "aac",
                "-c:a:1",
                "flac",
                "-disposition:a:0",
                "0",
                "-disposition:a:1",
                "default",
            ])
            .arg(&source);
        let (mut child, tree) = crate::required_tools::process_tree::spawn(&mut command)
            .await
            .unwrap();
        assert!(child.wait().await.unwrap().success());
        drop(tree);
        let (_sender, mut cancel) = watch::channel(false);
        let parsed = probe(&ffprobe, &source, &mut cancel).await.unwrap();
        let tracks = super::super::media::parse_tracks(&parsed).unwrap();
        assert_eq!(tracks.len(), 2);
        assert!(tracks[1].is_default);
        let identity =
            crate::database::download_records::identity::capture_checked(&source).unwrap();
        let existing = source.with_extension("flac");
        std::fs::write(&existing, b"existing user file").unwrap();
        let output = extract(
            &ffmpeg,
            &ffprobe,
            &source,
            &identity,
            &tracks[1],
            &mut cancel,
            |_| {},
        )
            .await
            .unwrap();
        assert_eq!(output, root.path().join("访谈 test (1).flac"));
        assert_eq!(std::fs::read(&existing).unwrap(), b"existing user file");
        let output_probe = probe(&ffprobe, &output, &mut cancel).await.unwrap();
        assert_eq!(output_probe["streams"].as_array().unwrap().len(), 1);
        assert_eq!(output_probe["streams"][0]["codec_name"], "flac");
        assert!(std::fs::metadata(&source).unwrap().len() > 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }

    #[tokio::test]
    #[ignore = "requires installed FFprobe; explicitly exercised during audio feature validation"]
    async fn corrupt_media_returns_real_probe_error() {
        let program = which::which("ffprobe").unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"invalid video").unwrap();
        let (_sender, mut cancel) = watch::channel(false);
        let error = probe(&program, file.path(), &mut cancel).await.unwrap_err();
        assert_eq!(error.code, "probeFailed");
        assert!(error.detail.contains("Invalid data") || error.detail.contains("invalid"));
    }

    #[tokio::test]
    #[ignore = "requires installed FFmpeg/FFprobe; verifies chaptered video extraction"]
    async fn real_ffmpeg_drops_chapter_streams_from_m4a() {
        let ffmpeg = which::which("ffmpeg").unwrap();
        let ffprobe = which::which("ffprobe").unwrap();
        let root = tempfile::tempdir().unwrap();
        let chapter = root.path().join("chapters.txt");
        std::fs::write(
            &chapter,
            ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Introduction\n",
        )
            .unwrap();
        let source = root.path().join("chaptered.mkv");
        let mut command = tokio::process::Command::new(&ffmpeg);
        command
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=32x32:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1",
                "-f",
                "ffmetadata",
                "-i",
            ])
            .arg(&chapter)
            .args([
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map_chapters",
                "2",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ])
            .arg(&source);
        let (mut child, tree) = crate::required_tools::process_tree::spawn(&mut command)
            .await
            .unwrap();
        assert!(child.wait().await.unwrap().success());
        drop(tree);
        let (_sender, mut cancel) = watch::channel(false);
        let value = probe(&ffprobe, &source, &mut cancel).await.unwrap();
        let track = super::super::media::parse_tracks(&value).unwrap().remove(0);
        let identity = identity::capture_checked(&source).unwrap();
        let output = extract(
            &ffmpeg,
            &ffprobe,
            &source,
            &identity,
            &track,
            &mut cancel,
            |_| {},
        )
            .await
            .unwrap();
        let verified = probe(&ffprobe, &output, &mut cancel).await.unwrap();
        assert_eq!(verified["streams"].as_array().unwrap().len(), 1);
        assert_eq!(verified["streams"][0]["codec_name"], "aac");
    }

    #[tokio::test]
    async fn changed_source_is_rejected_before_any_output_is_created() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("changed.mp4");
        std::fs::write(&source, b"original").unwrap();
        let identity = identity::capture_checked(&source).unwrap();
        std::fs::write(&source, b"replacement video").unwrap();
        let track = super::super::media::parse_tracks(
            &serde_json::json!({"streams":[{"index":1,"codec_type":"audio","codec_name":"aac"}]}),
        )
            .unwrap()
            .remove(0);
        let (_sender, mut cancel) = watch::channel(false);
        let error = extract(
            Path::new("unused"),
            Path::new("unused"),
            &source,
            &identity,
            &track,
            &mut cancel,
            |_| {},
        )
            .await
            .unwrap_err();
        assert_eq!(error.code, "sourceChanged");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn cancellation_reaps_process_and_cleans_temporary_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("video.mp4");
        std::fs::write(&source, b"video").unwrap();
        let identity = identity::capture_checked(&source).unwrap();
        let track = super::super::media::parse_tracks(
            &serde_json::json!({"streams":[{"index":1,"codec_type":"audio","codec_name":"aac"}]}),
        )
            .unwrap()
            .remove(0);
        let (sender, mut cancel) = watch::channel(false);
        sender.send(true).unwrap();
        let error = extract(
            Path::new("unused"),
            Path::new("unused"),
            &source,
            &identity,
            &track,
            &mut cancel,
            |_| {},
        )
            .await
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cancellation_stops_an_actual_native_child() {
        let program = which::which("powershell").unwrap();
        let args: Vec<OsString> = [
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]
            .iter()
            .map(OsString::from)
            .collect();
        let (sender, mut cancel) = watch::channel(false);
        let trigger = async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            sender.send(true).unwrap();
        };
        let (result, _) = tokio::join!(
            run(&program, &args, &mut cancel, None, None::<&fn(f64)>),
            trigger
        );
        assert_eq!(result.unwrap_err().code, "cancelled");
    }
}

use super::*;
use serde_json::Value;

fn test_directory() -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("evd-download-test-")
        .tempdir()
        .unwrap();
    eprintln!(
        "temporary download test directory: {}",
        directory.path().display()
    );
    directory
}
#[test]
fn default_paths_use_system_video_directory_and_separate_platforms() {
    let directory = test_directory();
    let video = directory.path().join("redirected videos");
    let paths = default_directories(Some(video.clone()), Some(directory.path().into())).unwrap();
    assert_eq!(
        Path::new(&paths["youtube"]),
        video.join("EasyVideoDownload/youtube")
    );
    assert_eq!(
        Path::new(&paths["douyin"]),
        video.join("EasyVideoDownload/douyin")
    );
    assert!(
        !video.exists(),
        "reading defaults must not create directories"
    );
    let fallback = default_directories(None, Some(directory.path().into())).unwrap();
    assert!(Path::new(&fallback["bilibili"]).ends_with("EasyVideoDownload/bilibili"));
    assert_eq!(
        default_directories(None, None).unwrap_err().code,
        "defaultDirectoryFailed"
    );
}
#[test]
fn native_total_progress_handles_single_stream_and_missing_sizes() {
    let mut tracker = ProgressTracker::default();
    tracker
        .update(r#"__EVD_PLAN__{"formats":[],"formatId":"video","size":0}"#)
        .unwrap();
    let unknown = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":50,"speed":25}}"#).unwrap();
    assert_eq!(
        unknown.percent,
        Some(0.0),
        "unknown totals must not invent forward movement"
    );
    assert_eq!(unknown.speed, Some(25.0));
    let measured = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":50,"total_bytes":100}}"#).unwrap();
    assert_eq!(measured.percent, Some(47.5));
    let finished = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"finished","total_bytes":100}}"#).unwrap();
    assert_eq!(finished.phase, "processing");
    assert_eq!(finished.percent, Some(95.0));
    assert!(tracker.update("unrelated diagnostic").is_none());
}
#[test]
fn late_plan_retries_and_estimate_changes_cannot_reset_task_progress() {
    let mut tracker = ProgressTracker::default();
    let before_plan = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":50,"total_bytes":100}}"#).unwrap();
    assert_eq!(before_plan.percent, Some(0.0));
    let planned = tracker
        .update(r#"__EVD_PLAN__{"formats":[],"formatId":"video","size":100}"#)
        .unwrap();
    assert_eq!(planned.percent, Some(47.5));
    let retry = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":1,"total_bytes":200}}"#).unwrap();
    assert_eq!(retry.percent, Some(47.5));
    let finalizing = tracker.finalizing();
    assert_eq!(finalizing.percent, Some(99.0));
    let stale = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":2,"total_bytes":200}}"#).unwrap();
    assert_eq!(stale.phase, "processing");
    assert_eq!(stale.percent, Some(99.0));
}
#[test]
fn combined_downloader_does_not_double_count_video_and_audio() {
    let mut tracker = ProgressTracker::default();
    tracker.update(r#"__EVD_PLAN__{"formats":[{"format_id":"video","filesize":900},{"format_id":"audio","filesize":100}],"formatId":"video+audio","size":0}"#).unwrap();
    let combined = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video+audio","progress":{"status":"downloading","downloaded_bytes":500,"total_bytes":1000}}"#).unwrap();
    assert_eq!(combined.percent, Some(47.5));
}
#[test]
fn output_confirmation_rejects_missing_empty_and_outside_files() {
    let directory = test_directory();
    let output = directory.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let video = output.join("video.mp4");
    assert!(confirmed_file(&video, &output).is_err());
    std::fs::write(&video, []).unwrap();
    assert!(confirmed_file(&video, &output).is_err());
    std::fs::write(&video, "bytes").unwrap();
    assert!(confirmed_file(&video, &output).is_ok());
    let outside = directory.path().join("outside.mp4");
    std::fs::write(&outside, "bytes").unwrap();
    assert!(confirmed_file(&outside, &output).is_err());
}
#[test]
fn cancellation_is_scoped_and_the_single_download_slot_is_released() {
    let manager = DownloadManager::default();
    let (active, cancel) = manager.begin("first".into()).unwrap();
    assert!(matches!(manager.begin("second".into()), Err(e) if e.code == "downloadBusy"));
    manager.cancel("second").unwrap();
    assert!(!*cancel.borrow());
    manager.cancel("first").unwrap();
    assert!(*cancel.borrow());
    drop(active);
    assert!(manager.begin("second".into()).is_ok());
}
fn fixture(mode: &str, path: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "video::download::tests::process_fixture",
            "--nocapture",
        ])
        .env("EVD_DOWNLOAD_FIXTURE", mode)
        .env("EVD_DOWNLOAD_PATH", path);
    command
}
#[test]
#[ignore = "native child process fixture"]
fn process_fixture() {
    let path = PathBuf::from(std::env::var_os("EVD_DOWNLOAD_PATH").unwrap());
    match std::env::var("EVD_DOWNLOAD_FIXTURE").unwrap().as_str() {
        mode @ ("already" | "cached_parts" | "unmarked" | "already_missing" | "already_empty"
        | "already_failure") => {
            if mode != "already_missing" {
                std::fs::write(
                    &path,
                    if mode == "already_empty" {
                        ""
                    } else {
                        "native output"
                    },
                )
                .unwrap();
            }
            if mode == "cached_parts" {
                eprintln!("[download] audio.f251.webm has already been downloaded");
            }
            let mut output = serde_json::json!({"filepath": path});
            if mode != "unmarked" {
                output["__real_download"] = (mode == "cached_parts").into();
            }
            println!("__EVD_FILE__{output}");
            if mode == "already_failure" {
                std::process::exit(3);
            }
        }
        "pipeline" => {
            std::fs::write(&path, "native output").unwrap();
            println!(
                r#"__EVD_PLAN__{{"formats":[{{"format_id":"video","filesize":900}},{{"format_id":"audio","filesize":100}}],"formatId":"video+audio","size":0}}"#
            );
            for line in [
                r#"{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":450,"total_bytes":900}}"#,
                r#"{"formatId":"video","progress":{"status":"finished","downloaded_bytes":900,"total_bytes":900}}"#,
                r#"{"formatId":"audio","progress":{"status":"downloading","downloaded_bytes":50,"total_bytes":100}}"#,
                r#"{"formatId":"audio","progress":{"status":"finished","downloaded_bytes":100,"total_bytes":100}}"#,
            ] {
                println!("__EVD_PROGRESS__{line}");
            }
            println!(r#"__EVD_PROCESSING__{{"status":"started","postprocessor":"Merger"}}"#);
            println!(r#"__EVD_PROCESSING__{{"status":"finished","postprocessor":"Merger"}}"#);
            println!(
                "__EVD_FILE__{}",
                serde_json::json!({"filepath": path, "__real_download": true})
            );
        }
        "success" => {
            std::fs::write(&path, "native output").unwrap();
            println!(r#"__EVD_PLAN__{{"formats":[],"formatId":"video","size":10}}"#);
            println!(
                r#"__EVD_PROGRESS__{{"formatId":"video","progress":{{"status":"downloading","downloaded_bytes":5,"total_bytes":10}}}}"#
            );
            println!(r#"__EVD_PROCESSING__{{"status":"started","postprocessor":"Merger"}}"#);
            println!(
                "__EVD_FILE__{}",
                serde_json::json!({"filepath": path, "__real_download": true})
            );
        }
        "missing" => println!("__EVD_PROCESSING__"),
        "failure" => {
            eprintln!("HTTP 403: unavailable");
            std::process::exit(3);
        }
        "cookie" => {
            eprintln!("Cookie error: secret-value");
            std::process::exit(3);
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(path, "orphaned process").unwrap();
        }
        "flood" => println!("{}", "x".repeat(70_000)),
        _ => panic!("unknown fixture"),
    }
}
#[tokio::test]
async fn existing_final_file_is_reported_without_mistaking_cached_streams_for_a_duplicate() {
    let directory = test_directory();
    for (mode, expected) in [
        ("already", true),
        ("cached_parts", false),
        ("unmarked", false),
    ] {
        let path = directory.path().join(format!("{mode}.mp4"));
        let (_sender, cancel) = watch::channel(false);
        let result = run_download(
            fixture(mode, &path),
            directory.path(),
            cancel,
            Duration::from_secs(5),
            |_| Ok(()),
        )
        .await
        .unwrap();
        let response = serde_json::to_value(result).unwrap();
        assert_eq!(response["alreadyDownloaded"], expected, "{mode}");
        assert!(Path::new(response["path"].as_str().unwrap()).is_file());
    }
    for (mode, code) in [
        ("already_missing", "downloadResultMissing"),
        ("already_empty", "downloadResultMissing"),
        ("already_failure", "downloadFailed"),
    ] {
        let path = directory.path().join(format!("{mode}.mp4"));
        let (_sender, cancel) = watch::channel(false);
        let failure = run_download(
            fixture(mode, &path),
            directory.path(),
            cancel,
            Duration::from_secs(5),
            |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert_eq!(failure.code, code, "{mode}");
    }
}
#[tokio::test]
async fn native_pipeline_combines_video_audio_and_processing_without_resetting() {
    let directory = test_directory();
    let path = directory.path().join("pipeline.mp4");
    let (_sender, cancel) = watch::channel(false);
    let updates = Mutex::new(Vec::new());
    run_download(
        fixture("pipeline", &path),
        directory.path(),
        cancel,
        Duration::from_secs(5),
        |update| {
            updates.lock().unwrap().push(update);
            Ok(())
        },
    )
    .await
    .unwrap();
    let updates = updates.lock().unwrap();
    let percentages: Vec<_> = updates.iter().filter_map(|update| update.percent).collect();
    assert_eq!(
        percentages,
        [0.0, 42.75, 85.5, 90.25, 95.0, 95.0, 95.0, 99.0]
    );
    assert_eq!(
        updates[2].phase, "downloading",
        "finishing video must not switch to merging before audio"
    );
    assert!(percentages.windows(2).all(|pair| pair[1] >= pair[0]));
}
#[tokio::test]
async fn native_process_requires_both_successful_exit_and_an_existing_output() {
    let directory = test_directory();
    let path = directory.path().join("video.mp4");
    let (_sender, cancel) = watch::channel(false);
    let updates = Mutex::new(Vec::new());
    let result = run_download(
        fixture("success", &path),
        directory.path(),
        cancel.clone(),
        Duration::from_secs(5),
        |update| {
            updates.lock().unwrap().push(update);
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(Path::new(&result.path).is_file());
    assert!(updates
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.percent == Some(47.5)));
    for (mode, code) in [
        ("missing", "downloadResultMissing"),
        ("failure", "downloadFailed"),
        ("cookie", "downloadFailed"),
        ("flood", "outputTooLarge"),
    ] {
        let failure = run_download(
            fixture(mode, &path),
            directory.path(),
            cancel.clone(),
            Duration::from_secs(5),
            |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert_eq!(failure.code, code);
        assert!(!failure.detail.contains("secret-value"));
    }
}
#[tokio::test]
async fn cancellation_and_timeout_stop_native_processes() {
    let directory = test_directory();
    for cancelled in [true, false] {
        let marker = directory
            .path()
            .join(if cancelled { "cancelled" } else { "timeout" });
        let (sender, cancel) = watch::channel(false);
        let signal = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if cancelled {
                sender.send(true).unwrap();
            }
        };
        let download = run_download(
            fixture("sleep", &marker),
            directory.path(),
            cancel,
            Duration::from_millis(250),
            |_| Ok(()),
        );
        let (result, _) = tokio::join!(download, signal);
        assert_eq!(
            result.unwrap_err().code,
            if cancelled {
                "downloadCancelled"
            } else {
                "downloadTimeout"
            }
        );
    }
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(!directory.path().join("cancelled").exists());
    assert!(!directory.path().join("timeout").exists());
}

#[tokio::test]
#[ignore = "explicit network smoke test; set EVD_LIVE_URL and EVD_LIVE_PLATFORM"]
async fn live_download() {
    use crate::required_tools::{
        Program, RequiredToolConfig, RequiredToolId, RequiredToolSettings, RequiredToolSource,
    };
    let input = std::env::var("EVD_LIVE_URL").expect("set EVD_LIVE_URL");
    let platform = match std::env::var("EVD_LIVE_PLATFORM").unwrap().as_str() {
        "youtube" => CookiePlatform::Youtube,
        "douyin" => CookiePlatform::Douyin,
        "bilibili" => CookiePlatform::Bilibili,
        _ => panic!("unknown platform"),
    };
    let mut settings = RequiredToolSettings::default();
    for (id, name) in [
        (RequiredToolId::Ytdlp, "yt-dlp"),
        (RequiredToolId::Ffmpeg, "ffmpeg"),
        (RequiredToolId::Deno, "deno"),
    ] {
        let path = which::which(name).unwrap();
        settings.tools.insert(
            id,
            RequiredToolConfig {
                source: RequiredToolSource::Manual,
                manual_path: path.to_string_lossy().into_owned(),
                programs: vec![Program {
                    name: name.into(),
                    path,
                    version: "live-test".into(),
                }],
                checked_at: "2026-10-03 00:00:00".into(),
            },
        );
    }
    let directory = test_directory();
    let store_path = std::env::var_os("EVD_LIVE_DATA_DIRECTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.path().into());
    let store = CookieStore::new(&store_path);
    let original = std::fs::read(store.path(platform)).ok();
    let video = super::super::parse_with_tools(settings.clone(), store.clone(), platform, &input)
        .await
        .unwrap();
    // Keep the live probe small, but still exercise separate video/audio merging where available.
    let requested_format = std::env::var("EVD_LIVE_FORMAT_ID").ok();
    let format = if let Some(id) = requested_format.as_ref() {
        video
            .formats
            .iter()
            .find(|format| &format.format_id == id)
            .expect("requested live format is unavailable")
    } else {
        video
            .formats
            .iter()
            .filter(|format| format.height.is_some_and(|h| h <= 360))
            .last()
            .unwrap_or(&video.formats[0])
    };
    let alternative = if requested_format.is_some() {
        None
    } else {
        video
            .formats
            .iter()
            .filter(|candidate| {
                candidate.extension == format.extension
                    && candidate.height < format.height
                    && candidate.height.is_some()
            })
            .last()
    };
    for format in std::iter::once(format).chain(alternative) {
        let options: DownloadCommandOptions = serde_json::from_value(
            serde_json::json!({"directory": directory.path(), "formatId": format.format_id,
        "container": format.extension.as_deref().filter(|ext| *ext == "mp4"),
        "cookieFallback": video.cookie_fallback}),
        )
        .unwrap();
        let copy = if video.cookie_fallback {
            None
        } else {
            cookie_snapshot(&store, platform).unwrap()
        };
        let command = download_command(
            &settings,
            &normalize_link(&input, platform).unwrap(),
            copy.as_ref().map(|f| f.path()),
            &options,
        )
        .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let observed = Mutex::new(Vec::new());
        let result = run_download(
            command,
            directory.path(),
            cancel,
            Duration::from_secs(180),
            |update| {
                eprintln!("native download: {} {:?}%", update.phase, update.percent);
                observed.lock().unwrap().push(update);
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(
            !result.already_downloaded,
            "a new video was marked as already downloaded"
        );
        let observed = observed.lock().unwrap();
        let percentages: Vec<_> = observed
            .iter()
            .filter_map(|progress| progress.percent)
            .collect();
        assert!(
            percentages.windows(2).all(|pair| pair[1] >= pair[0]),
            "live total progress moved backwards: {percentages:?}"
        );
        assert!(
            percentages.iter().all(|percent| *percent < 100.0),
            "native pipeline claimed completion before file confirmation"
        );
        assert!(
            observed
                .iter()
                .any(|progress| progress.phase == "downloading"
                    && progress.percent.is_some_and(|p| p > 0.0 && p < 95.0)),
            "live stream downloads did not contribute to total progress"
        );
        assert!(observed
            .iter()
            .any(|progress| progress.phase == "processing" && progress.percent == Some(95.0)));
        assert_eq!(percentages.last(), Some(&99.0));
        assert_eq!(
            std::fs::read(store.path(platform)).ok(),
            original,
            "download changed the managed Cookie file"
        );
        let probe = std::process::Command::new(which::which("ffprobe").unwrap())
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,height:format=format_name",
                "-of",
                "json",
            ])
            .arg(&result.path)
            .output()
            .unwrap();
        assert!(
            probe.status.success(),
            "final output is not a playable media file"
        );
        let metadata: Value = serde_json::from_slice(&probe.stdout).unwrap();
        if let Some(container) = options.container.as_deref() {
            assert_eq!(
                Path::new(&result.path)
                    .extension()
                    .and_then(|ext| ext.to_str()),
                Some(container)
            );
            if container == "mp4" {
                assert!(
                    metadata["format"]["format_name"]
                        .as_str()
                        .unwrap()
                        .contains("mp4"),
                    "output is not an MP4 container"
                );
            }
        }
        assert!(metadata["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stream| stream["codec_type"] == "video"));
        if let Some(height) = format.height {
            assert!(
                metadata["streams"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|stream| stream["codec_type"] == "video"
                        && stream["height"].as_u64() == Some(height as u64)),
                "output does not match the selected {}p format: {}",
                height,
                metadata
            );
        }
        eprintln!(
            "live download succeeded: {} ({} bytes), streams={}",
            result.path,
            std::fs::metadata(&result.path).unwrap().len(),
            metadata["streams"]
        );
        let before = std::fs::metadata(&result.path).unwrap();
        let repeat_command = download_command(
            &settings,
            &normalize_link(&input, platform).unwrap(),
            copy.as_ref().map(|file| file.path()),
            &options,
        )
        .unwrap();
        let (_sender, repeat_cancel) = watch::channel(false);
        let repeated = run_download(
            repeat_command,
            directory.path(),
            repeat_cancel,
            Duration::from_secs(180),
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert!(
            repeated.already_downloaded,
            "the existing final video was not recognized"
        );
        assert_eq!(repeated.path, result.path);
        let after = std::fs::metadata(&repeated.path).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(
            after.modified().unwrap(),
            before.modified().unwrap(),
            "repeat download overwrote the video"
        );
        eprintln!(
            "live repeat correctly reused the existing file: {}",
            repeated.path
        );
    }
}

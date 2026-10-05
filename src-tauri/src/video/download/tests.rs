use super::*;

#[test]
fn two_slots_dispatch_fifo_and_cancelled_queue_never_starts() {
    let mut queue = tasks::scheduler::Scheduler::default();
    queue.set_limit(2);
    assert_eq!(queue.enqueue("a".into()), vec!["a"]);
    assert_eq!(queue.enqueue("b".into()), vec!["b"]);
    assert!(queue.enqueue("c".into()).is_empty());
    assert!(queue.enqueue("d".into()).is_empty());
    assert!(queue.cancel_queued("c"));
    assert_eq!(queue.finish("a"), vec!["d"]);
    assert_eq!(queue.running_count(), 2);
    queue.stop();
    assert!(queue.finish("b").is_empty());
}
use serde_json::Value;

#[tokio::test]
async fn deleted_download_tools_are_reported_before_starting_a_transfer() {
    use crate::required_tools::{Program, RequiredToolConfig, RequiredToolSource};
    let root = test_directory();
    let mut settings = RequiredToolSettings::default();
    for (id, names) in [
        (RequiredToolId::Ytdlp, vec!["yt-dlp"]),
        (RequiredToolId::Ffmpeg, vec!["ffmpeg", "ffprobe"]),
        (RequiredToolId::Deno, vec!["deno"]),
    ] {
        let programs: Vec<_> = names
            .into_iter()
            .map(|name| {
                let path = root.path().join(if cfg!(windows) {
                    format!("{name}.exe")
                } else {
                    name.into()
                });
                std::fs::write(&path, b"availability fixture; never executed").unwrap();
                Program {
                    name: name.into(),
                    path,
                    version: "9.0.2".into(),
                }
            })
            .collect();
        settings.tools.insert(
            id,
            RequiredToolConfig {
                source: RequiredToolSource::Manual,
                manual_path: if id == RequiredToolId::Ffmpeg {
                    root.path().to_string_lossy().into()
                } else {
                    programs[0].path.to_string_lossy().into()
                },
                programs,
                checked_at: crate::datetime::now(),
            },
        );
    }
    check_download_tool_files(&settings, CookiePlatform::Bilibili)
        .await
        .unwrap();
    for (id, name, code) in [
        (RequiredToolId::Ytdlp, "yt-dlp", "toolMissing"),
        (RequiredToolId::Ffmpeg, "ffmpeg", "ffmpegMissing"),
        (RequiredToolId::Ffmpeg, "ffprobe", "ffmpegMissing"),
        (RequiredToolId::Deno, "deno", "denoMissing"),
    ] {
        let path = &settings.tools[&id]
            .programs
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .path;
        std::fs::remove_file(path).unwrap();
        let failure = check_download_tool_files(&settings, CookiePlatform::Youtube)
            .await
            .unwrap_err();
        assert_eq!(failure.code, code);
        assert!(failure.detail.starts_with(name));
        assert!(failure.detail.contains(&path.to_string_lossy().to_string()));
        if id == RequiredToolId::Deno {
            check_download_tool_files(&settings, CookiePlatform::Bilibili)
                .await
                .unwrap();
        }
        // A directory in place of an executable is not a usable tool either.
        std::fs::create_dir(path).unwrap();
        assert_eq!(
            check_download_tool_files(&settings, CookiePlatform::Youtube)
                .await
                .unwrap_err()
                .code,
            code
        );
        std::fs::remove_dir(path).unwrap();
        std::fs::write(path, b"restored availability fixture; never executed").unwrap();
    }
    settings.tools.remove(&RequiredToolId::Ffmpeg);
    assert_eq!(
        check_download_tool_files(&settings, CookiePlatform::Bilibili)
            .await
            .unwrap_err()
            .code,
        "ffmpegMissing"
    );
}

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
fn native_speed_keeps_latest_measurement_when_a_sample_has_no_speed() {
    let mut tracker = ProgressTracker::default();
    tracker
        .update(r#"__EVD_PLAN__{"formats":[],"formatId":"video","size":1000}"#)
        .unwrap();
    let sample = |speed: Value| {
        format!(
            "__EVD_PROGRESS__{}",
            serde_json::json!({"formatId":"video","progress":{"status":"downloading","downloaded_bytes":10,"speed":speed}})
        )
    };
    assert_eq!(
        tracker.update(&sample(Value::Null)).unwrap().speed,
        None,
        "no speed is displayed before the first measurement"
    );
    assert_eq!(
        tracker
            .update(&sample(serde_json::json!(2048)))
            .unwrap()
            .speed,
        Some(2048.0)
    );
    for missing in [
        Value::Null,
        serde_json::json!("unknown"),
        serde_json::json!(-1),
    ] {
        assert_eq!(
            tracker.update(&sample(missing)).unwrap().speed,
            Some(2048.0),
            "an unavailable speed sample must not erase the last measurement"
        );
    }
    let omitted = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":20}}"#).unwrap();
    assert_eq!(omitted.speed, Some(2048.0));
    assert_eq!(
        tracker.update(&sample(serde_json::json!(0))).unwrap().speed,
        Some(0.0),
        "a measured zero is a valid speed, not a missing sample"
    );
    assert_eq!(
        tracker.update(&sample(Value::Null)).unwrap().speed,
        Some(0.0)
    );
    assert_eq!(
        tracker
            .update(&sample(serde_json::json!(4096)))
            .unwrap()
            .speed,
        Some(4096.0)
    );
}
#[test]
fn native_speed_survives_stream_transitions_until_processing() {
    let mut tracker = ProgressTracker::default();
    tracker.update(r#"__EVD_PLAN__{"formats":[{"format_id":"video","filesize":900},{"format_id":"audio","filesize":100}],"formatId":"video+audio","size":0}"#).unwrap();
    let first = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","downloaded_bytes":450,"speed":2048}}"#).unwrap();
    assert_eq!(first.speed, Some(2048.0));
    let video_finished = tracker.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"finished","downloaded_bytes":900}}"#).unwrap();
    assert_eq!(video_finished.phase, "downloading");
    assert_eq!(video_finished.speed, Some(2048.0));
    let audio_started = tracker.update(r#"__EVD_PROGRESS__{"formatId":"audio","progress":{"status":"downloading","downloaded_bytes":1,"speed":null}}"#).unwrap();
    assert_eq!(audio_started.speed, Some(2048.0));
    let audio_speed = tracker.update(r#"__EVD_PROGRESS__{"formatId":"audio","progress":{"status":"downloading","downloaded_bytes":50,"speed":1024}}"#).unwrap();
    assert_eq!(audio_speed.speed, Some(1024.0));
    let audio_finished = tracker.update(r#"__EVD_PROGRESS__{"formatId":"audio","progress":{"status":"finished","downloaded_bytes":100}}"#).unwrap();
    assert_eq!(audio_finished.phase, "processing");
    assert_eq!(audio_finished.speed, None);
    assert_eq!(tracker.finalizing().speed, None);
    let stale = tracker.update(r#"__EVD_PROGRESS__{"formatId":"audio","progress":{"status":"downloading","speed":4096}}"#).unwrap();
    assert_eq!(stale.phase, "processing");
    assert_eq!(stale.speed, None);
    let mut next = ProgressTracker::default();
    let new_download = next.update(r#"__EVD_PROGRESS__{"formatId":"video","progress":{"status":"downloading","speed":null}}"#).unwrap();
    assert_eq!(
        new_download.speed, None,
        "measurements never leak into another download"
    );
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

pub(super) fn fixture(mode: &str, path: &Path) -> Command {
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
        "buffered_entry" => {
            eprintln!(
                "__EVD_PLAN__{}",
                serde_json::json!({"filepath":path,"formatId":"video","formats":[],"size":10})
            );
            let directory = path.parent().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while !directory.join("begin.signal").exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            println!("[debug] Invoking http downloader on \"https://private.example/signed\"");
            std::fs::write(directory.join("entry.ready"), b"entry written").unwrap();
            std::thread::sleep(Duration::from_secs(10));
        }
        mode @ ("early_failure" | "early_cancel") => {
            println!(
                "__EVD_PLAN__{}",
                serde_json::json!({"filepath":path,"formats":[],"size":10})
            );
            eprintln!("[debug] Invoking http downloader on \"https://private.example/signed\"");
            if mode == "early_cancel" {
                std::thread::sleep(Duration::from_secs(10));
            }
            eprintln!("HTTP 403: transfer failed");
            std::process::exit(3);
        }
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
        mode @ ("controlled" | "pausable") => {
            if mode == "pausable" {
                use std::io::Write;
                println!(r#"__EVD_PLAN__{{"formats":[],"formatId":"video","size":10}}"#);
                println!(
                    r#"__EVD_PROGRESS__{{"formatId":"video","progress":{{"status":"downloading","downloaded_bytes":5,"total_bytes":10}}}}"#
                );
                std::io::stdout().flush().unwrap();
            }
            let marker = path.with_extension("started");
            std::fs::write(marker, b"started").unwrap();
            let release = path.with_extension("release");
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !release.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(&path, b"native task output").unwrap();
            println!(
                "__EVD_FILE__{}",
                serde_json::json!({"filepath":path,"__real_download":true})
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
async fn native_history_distinguishes_actual_failures_skips_and_missing_evidence() {
    use super::history::DownloadRecordSession;
    use crate::database::download_records::DownloadSnapshot;
    use crate::database::{persistence_tests::page, Database};
    let dir = test_directory();
    let db = std::sync::Arc::new(
        Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap(),
    );
    for (mode, expected) in [
        ("success", Some("completed")),
        ("early_failure", Some("failed")),
        ("failure", Some("failed")),
        ("already", Some("completed")),
        ("cached_parts", Some("completed")),
        ("unmarked", Some("completed")),
    ] {
        let path = dir.path().join(format!("{mode}.mp4"));
        let mut state = page();
        state.download_directory = dir.path().to_string_lossy().into();
        let request = uuid::Uuid::new_v4().to_string();
        let session = DownloadRecordSession::new(
            db.clone(),
            dir.path(),
            request.clone(),
            DownloadSnapshot { page: state },
            None,
        )
            .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let result = run_download_with_history(
            fixture(mode, &path),
            dir.path(),
            cancel,
            Duration::from_secs(5),
            |_| Ok(()),
            Some(&session),
        )
            .await;
        session.finish(&result).await;
        let record = db
            .list_download_records(None, 200)
            .unwrap()
            .records
            .into_iter()
            .find(|r| r.request_id == request);
        assert_eq!(
            record.as_ref().map(|r| r.status.as_str()),
            expected,
            "{mode}"
        );
        if let Some(record) = record {
            assert!(record
                .error_detail
                .as_deref()
                .is_none_or(|v| !v.contains("private.example")));
        }
    }
}
#[tokio::test]
async fn history_records_cancel_before_first_packet_and_during_preparation() {
    use crate::database::{download_records::DownloadSnapshot, persistence_tests::page, Database};
    let dir = test_directory();
    let db = std::sync::Arc::new(
        Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap(),
    );
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let id = uuid::Uuid::new_v4().to_string();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        id.clone(),
        DownloadSnapshot {
            page: state.clone(),
        },
        None,
    )
        .unwrap();
    let (sender, cancel) = watch::channel(false);
    let path = dir.path().join("cancel.mp4");
    let (result, ()) = tokio::join!(
        run_download_with_history(
            fixture("early_cancel", &path),
            dir.path(),
            cancel,
            Duration::from_secs(5),
            |_| Ok(()),
            Some(&session)
        ),
        async {
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while !db.record_exists(&id).unwrap() && std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            sender.send(true).unwrap();
        }
    );
    assert_eq!(result.as_ref().unwrap_err().code, "downloadCancelled");
    assert!(session.finish(&result).await.is_none());
    assert_eq!(
        db.list_download_records(None, 10).unwrap().records[0].status,
        "cancelled"
    );
    assert!(!path.exists());
    let id = uuid::Uuid::new_v4().to_string();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        id,
        DownloadSnapshot { page: state },
        None,
    )
        .unwrap();
    let (_sender, cancel) = watch::channel(true);
    let result = run_download_with_history(
        fixture("success", &path),
        dir.path(),
        cancel,
        Duration::from_secs(5),
        |_| Ok(()),
        Some(&session),
    )
        .await;
    session.finish(&result).await;
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records.len(), 1);
    assert!(records.iter().all(|record| record.status == "cancelled"));
    assert!(!path.exists());
}
#[tokio::test]
async fn history_storage_failure_keeps_the_real_successful_file() {
    use crate::database::{download_records::DownloadSnapshot, persistence_tests::page, Database};
    let dir = test_directory();
    let dbpath = dir.path().join("app.db");
    let db = std::sync::Arc::new(Database::open(&dbpath, &dir.path().join("legacy")).unwrap());
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db,
        dir.path(),
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page: state },
        None,
    )
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let path = dir.path().join("success.mp4");
    let broken = std::sync::atomic::AtomicBool::new(false);
    let result = run_download_with_history(
        fixture("success", &path),
        dir.path(),
        cancel,
        Duration::from_secs(5),
        |progress| {
            if progress.phase == "downloading"
                && !broken.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                rusqlite::Connection::open(&dbpath)
                    .unwrap()
                    .execute("DROP TABLE download_records", [])
                    .unwrap();
            }
            Ok(())
        },
        Some(&session),
    )
        .await;
    assert!(Path::new(&result.as_ref().unwrap().path).is_file());
    assert_eq!(session.finish(&result).await.unwrap().code, "saveFailed");
}

#[test]
fn persisted_download_errors_do_not_contain_signed_urls() {
    let failure = download_failure(
        "HTTP 403: https://private.example/video?token=secret-value unavailable\nConnection failed",
    );
    assert!(!failure.detail.contains("secret-value"));
    assert!(!failure.detail.contains("private.example"));
    assert!(failure.detail.contains("HTTP 403"));
}

#[tokio::test]
async fn cancellation_drains_already_written_downloader_entry_before_settlement() {
    use crate::database::{download_records::DownloadSnapshot, persistence_tests::page, Database};
    let dir = test_directory();
    let db = std::sync::Arc::new(
        Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap(),
    );
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page: state },
        None,
    )
        .unwrap();
    let (sender, cancel) = watch::channel(false);
    let path = dir.path().join("cancel.mp4");
    let result = run_download_with_history(
        fixture("buffered_entry", &path),
        dir.path(),
        cancel,
        Duration::from_secs(5),
        |update| {
            if update.phase == "preparing" {
                std::fs::write(dir.path().join("begin.signal"), b"enter downloader").unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while !dir.path().join("entry.ready").exists()
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(dir.path().join("entry.ready").exists());
                sender.send(true).unwrap();
            }
            Ok(())
        },
        Some(&session),
    )
        .await;
    assert_eq!(result.as_ref().unwrap_err().code, "downloadCancelled");
    session.finish(&result).await;
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, "cancelled");
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
    let video =
        super::super::parse_with_tools(settings.clone(), store.clone(), platform, &input, None)
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
            .rfind(|format| format.height.is_some_and(|h| h <= 360))
            .unwrap_or(&video.formats[0])
    };
    let alternative = if requested_format.is_some() {
        None
    } else {
        video.formats.iter().rfind(|candidate| {
            candidate.extension == format.extension
                && candidate.height < format.height
                && candidate.height.is_some()
        })
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
            None,
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
        let observed = observed.lock().unwrap().clone();
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
            None,
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

#[cfg(windows)]
#[test]
fn output_verification_preserves_occupation_errors_without_guessing_from_access_denied() {
    for code in [32, 33, 303, 1224] {
        assert_eq!(
            output_access_failure(std::io::Error::from_raw_os_error(code)).code,
            "historyFileOccupied"
        );
    }
    for code in [2, 3, 5] {
        assert_eq!(
            output_access_failure(std::io::Error::from_raw_os_error(code)).code,
            "downloadResultMissing"
        );
    }
}

#[cfg(windows)]
pub(super) struct MappedFileRead(windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS);
#[cfg(windows)]
impl MappedFileRead {
    pub(super) fn new(path: &Path) -> Self {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_READ, PAGE_READONLY},
        };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(7)
            .open(path)
            .unwrap();
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READONLY,
                0,
                0,
                std::ptr::null(),
            )
        };
        assert!(!mapping.is_null(), "{}", std::io::Error::last_os_error());
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) };
        unsafe { CloseHandle(mapping) };
        assert!(!view.Value.is_null(), "{}", std::io::Error::last_os_error());
        drop(file);
        Self(view)
    }
}
#[cfg(windows)]
impl Drop for MappedFileRead {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Memory::UnmapViewOfFile(self.0) };
    }
}

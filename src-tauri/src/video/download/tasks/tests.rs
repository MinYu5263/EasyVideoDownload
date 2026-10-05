use super::*;
use commands::RecordRestartMode;

#[tokio::test]
async fn record_redownload_requires_confirmation_after_restoring_successful_output() {
    use crate::database::download_records::DownloadRecordOutcome;
    for failed_retry in [false, true] {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "owned redownload confirmation fixture: {}",
            root.path().display()
        );
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
        let db = storage.database().unwrap();
        let mut original = page();
        original.download_directory = root.path().to_string_lossy().into();
        let id = db
            .begin_download_record(
                "saved-output",
                &DownloadSnapshot {
                    page: original.clone(),
                },
                &crate::datetime::now(),
            )
            .unwrap();
        let output = root.path().join("saved.webm");
        std::fs::write(&output, b"original").unwrap();
        db.finish_download_record(
            "saved-output",
            &DownloadRecordOutcome::Completed {
                path: output.to_string_lossy().into(),
                size: 8,
                extension: Some("webm".into()),
            },
        )
            .unwrap();
        let successful = db.get_download_record(id).unwrap();
        if failed_retry {
            db.restart_download_record(
                "failed-retry",
                id,
                "saved-output",
                &DownloadSnapshot { page: original },
                false,
                |_, _| Ok(None),
            )
                .unwrap();
            db.finish_download_record(
                "failed-retry",
                &DownloadRecordOutcome::Failed {
                    code: "downloadFailed".into(),
                    detail: "previous output retained".into(),
                },
            )
                .unwrap();
        }
        let before = db.get_download_record(id).unwrap();
        let manager = two_slot_manager();
        let tools = RequiredToolManager::new(storage.clone());
        let cookies = CookieStore::new(root.path());
        let result = commands::restart_from_record(
            None,
            id,
            &before.request_id,
            RecordRestartMode::CheckOutput,
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .expect("an existing successful output must be checked before download tools");
        assert_eq!(result.kind, "confirmationRequired");
        assert_eq!(result.record.status, "completed");
        assert_eq!(result.record.file_availability, "present");
        assert_eq!(result.record.finished_at, successful.finished_at);
        assert_eq!(result.record.request_id, before.request_id);
        assert!(result.record.error_code.is_none());
        assert_eq!(db.get_download_record(id).unwrap().status, "completed");
        assert_eq!(std::fs::read(output).unwrap(), b"original");
        assert!(!manager.is_active().unwrap());
    }
}

#[tokio::test]
async fn page_download_restores_trashed_existing_output_without_requiring_download_tools() {
    use crate::database::download_records::DownloadRecordOutcome;
    for failed_retry in [false, true] {
        let root = tempfile::tempdir().unwrap();
        eprintln!("owned trash restore fixture: {}", root.path().display());
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
        let db = storage.database().unwrap();
        let mut original = page();
        original.download_directory = root.path().to_string_lossy().into();
        original.selected_format_id = Some("a".into());
        original.formats[0].height = Some(1080);
        original.formats[0].fps = Some(60.0);
        original.selected_height = Some(1080);
        original.selected_fps = Some(60.0);
        let id = db
            .begin_download_record(
                "saved-output",
                &DownloadSnapshot {
                    page: original.clone(),
                },
                &crate::datetime::now(),
            )
            .unwrap();
        let output = root.path().join("saved.webm");
        std::fs::write(&output, b"original").unwrap();
        db.finish_download_record(
            "saved-output",
            &DownloadRecordOutcome::Completed {
                path: output.to_string_lossy().into(),
                size: 8,
                extension: Some("webm".into()),
            },
        )
            .unwrap();
        if failed_retry {
            let mut retry = original.clone();
            retry.formats[0].height = Some(720);
            retry.formats[0].fps = Some(30.0);
            retry.selected_height = Some(720);
            retry.selected_fps = Some(30.0);
            db.restart_download_record(
                "failed-retry",
                id,
                "saved-output",
                &DownloadSnapshot { page: retry },
                false,
                |_, _| Ok(None),
            )
                .unwrap();
            db.finish_download_record(
                "failed-retry",
                &DownloadRecordOutcome::Failed {
                    code: "downloadFailed".into(),
                    detail: "retained successful output".into(),
                },
            )
                .unwrap();
            // Legacy successful outputs only have scalar format metadata.
            rusqlite::Connection::open(root.path().join("app.db"))
                .unwrap()
                .execute(
                    "UPDATE download_records SET successful_format_snapshot_json=NULL WHERE id=?1",
                    [id],
                )
                .unwrap();
        }
        db.mark_download_file_availability(id, "missing").unwrap();
        db.delete_download_record(id).unwrap();
        let before = db.get_download_record(id).unwrap();
        let mut current = original;
        current.download_directory = root.path().join("new-directory").to_string_lossy().into();
        db.save_download_page_state(&current).unwrap();
        let manager = two_slot_manager();
        let tools = RequiredToolManager::new(storage.clone());
        let cookies = CookieStore::new(root.path());
        let result = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: current,
                restore_trashed: true,
                redownload: false,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        assert_eq!(result.kind, "alreadyDownloaded");
        assert!(result.task.is_none());
        assert_eq!(result.record.id, id);
        assert_eq!(result.record.request_id, before.request_id);
        assert_eq!(result.record.status, "completed");
        assert_eq!(result.record.file_availability, "present");
        assert!(result.record.deleted_at.is_none());
        assert!(result.record.error_code.is_none());
        assert_eq!(result.record.output_path, before.output_path);
        assert_eq!(
            result.record.finished_at,
            before.successful_output.unwrap().finished_at
        );
        assert_eq!(
            result.record.format_snapshot.as_ref().unwrap().height,
            Some(1080)
        );
        assert_eq!(
            result.record.format_snapshot.as_ref().unwrap().fps,
            Some(60.0)
        );
        assert!(!manager.is_active().unwrap());
        assert_eq!(std::fs::read(output).unwrap(), b"original");
        assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 1);
    }
}

#[tokio::test]
async fn page_download_restores_trashed_missing_output_before_download_preflight() {
    use crate::database::download_records::DownloadRecordOutcome;
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned missing trash output fixture: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let mut snapshot = page();
    snapshot.download_directory = root.path().to_string_lossy().into();
    let id = db
        .begin_download_record(
            "missing-output",
            &DownloadSnapshot {
                page: snapshot.clone(),
            },
            &crate::datetime::now(),
        )
        .unwrap();
    let output = root.path().join("missing.webm");
    std::fs::write(&output, b"original").unwrap();
    db.finish_download_record(
        "missing-output",
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    db.delete_download_record(id).unwrap();
    std::fs::remove_file(&output).unwrap();
    let tools = RequiredToolManager::new(storage.clone());
    snapshot.parser_fingerprint = Some(
        crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
            .unwrap(),
    );
    db.save_download_page_state(&snapshot).unwrap();
    let manager = two_slot_manager();
    let cookies = CookieStore::new(root.path());
    let failure = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot: snapshot.clone(),
            restore_trashed: true,
            redownload: false,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.code, "toolMissing");
    let restored = db.get_download_record(id).unwrap();
    assert!(
        restored.deleted_at.is_none(),
        "restoration precedes download preflight"
    );
    assert_eq!(restored.file_availability, "missing");
    assert_eq!(restored.request_id, "missing-output");
    for (tool_id, mut config) in settings().tools {
        for program in &mut config.programs {
            program.version = if tool_id == RequiredToolId::Ytdlp {
                "2026.10.04"
            } else {
                "8.0.0"
            }
                .into();
        }
        db.save_tool(tool_id, &config).unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    snapshot.parser_fingerprint = Some(
        crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
            .unwrap(),
    );
    db.save_download_page_state(&snapshot).unwrap();
    manager.inner.lock().unwrap().scheduler.stop();
    let result = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot,
            restore_trashed: false,
            redownload: false,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    assert_eq!(result.kind, "accepted");
    assert_eq!(result.record.id, id);
    assert_eq!(result.task.unwrap().phase, "queued");
    assert_ne!(result.record.request_id, "missing-output");
    assert!(result.record.deleted_at.is_none());
    manager.cancel(&result.record.request_id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
}

#[tokio::test]
async fn page_download_restores_trashed_parked_pause_after_exit_is_aborted() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned parked trash restore fixture: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    for (id, mut config) in settings().tools {
        for program in &mut config.programs {
            program.version = if id == RequiredToolId::Ytdlp {
                "2026.10.04"
            } else {
                "8.0.0"
            }
                .into();
        }
        db.save_tool(id, &config).unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let manager = two_slot_manager();
    let old = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "parked-restore",
        "pausable",
    );
    until(|| manager.task_for(old.record.id).unwrap().phase == "downloading").await;
    manager.set_paused(&old.record.request_id, true).unwrap();
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    manager.abort_exit();
    db.delete_download_record(old.record.id).unwrap();
    let mut snapshot = page();
    snapshot.video_id = Some("parked-restore".into());
    snapshot.input_link = "https://youtu.be/parked-restore".into();
    snapshot.download_directory = root.path().to_string_lossy().into();
    snapshot.parser_fingerprint = Some(
        crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
            .unwrap(),
    );
    db.save_download_page_state(&snapshot).unwrap();
    manager.inner.lock().unwrap().scheduler.stop();
    let result = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot,
            restore_trashed: true,
            redownload: false,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    // Settle any new native job before assertions, including when the regression fails.
    if result.kind == "accepted" {
        manager.cancel(&result.record.request_id).unwrap();
        until(|| !manager.is_active().unwrap()).await;
    }
    assert_eq!(result.kind, "accepted");
    assert_eq!(result.record.id, old.record.id);
    assert_ne!(result.record.request_id, old.record.request_id);
    assert!(result.record.deleted_at.is_none());
}

#[tokio::test]
async fn page_download_restores_completed_output_and_removes_the_old_native_task_snapshot() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned completed trash snapshot fixture: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let manager = two_slot_manager();
    let old = enqueue(&manager, db.clone(), root.path(), "restore-completed");
    until(|| root.path().join("restore-completed.started").exists()).await;
    std::fs::write(root.path().join("restore-completed.release"), b"release").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(
        db.get_download_record(old.record.id).unwrap().status,
        "completed"
    );
    db.delete_download_record(old.record.id).unwrap();
    let mut snapshot = page();
    snapshot.video_id = Some("restore-completed".into());
    snapshot.input_link = "https://youtu.be/restore-completed".into();
    snapshot.download_directory = root.path().to_string_lossy().into();
    db.save_download_page_state(&snapshot).unwrap();
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let result = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot,
            restore_trashed: true,
            redownload: false,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    assert_eq!(result.kind, "alreadyDownloaded");
    assert_eq!(result.record.id, old.record.id);
    assert_eq!(result.record.status, "completed");
    assert!(
        manager.snapshots().unwrap().is_empty(),
        "reconnection must not load the old task"
    );
}

#[tokio::test]
async fn saved_concurrency_controls_native_workers_and_live_changes_preserve_running_jobs() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned configurable concurrency fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut settings = db.app_settings("en").unwrap();
    settings.max_concurrent_downloads = 1;
    db.save_app_settings(&settings).unwrap();
    let manager = DownloadManager::default();
    let rows: Vec<_> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|id| enqueue(&manager, db.clone(), root.path(), id))
        .collect();
    until(|| root.path().join("a.started").exists()).await;
    assert_eq!(manager.inner.lock().unwrap().scheduler.running_count(), 1);
    assert!(!root.path().join("b.started").exists());

    // A real database failure cannot release queued work or change the saved limit.
    let connection = rusqlite::Connection::open(root.path().join("app.db")).unwrap();
    connection.execute_batch(
        "CREATE TRIGGER reject_limit BEFORE UPDATE ON app_settings WHEN NEW.setting_key='max_concurrent_downloads' BEGIN SELECT RAISE(ABORT, 'owned concurrency save failure'); END;"
    ).unwrap();
    settings.max_concurrent_downloads = 3;
    assert_eq!(
        manager.save_settings(&db, &settings).unwrap_err().code,
        "saveFailed"
    );
    assert_eq!(db.download_limit().unwrap(), 1);
    assert_eq!(manager.inner.lock().unwrap().scheduler.running_count(), 1);
    connection
        .execute_batch("DROP TRIGGER reject_limit;")
        .unwrap();

    manager.save_settings(&db, &settings).unwrap();
    until(|| root.path().join("b.started").exists() && root.path().join("c.started").exists())
        .await;
    settings.max_concurrent_downloads = 1;
    manager.save_settings(&db, &settings).unwrap();
    assert_eq!(manager.inner.lock().unwrap().scheduler.running_count(), 3);
    for (index, id) in ["a", "b"].into_iter().enumerate() {
        std::fs::write(root.path().join(format!("{id}.release")), b"release").unwrap();
        until(|| {
            db.get_download_record(rows[index].record.id)
                .unwrap()
                .status
                == "completed"
        })
            .await;
        assert!(!root.path().join("d.started").exists());
    }
    std::fs::write(root.path().join("c.release"), b"release").unwrap();
    until(|| root.path().join("d.started").exists()).await;
    std::fs::write(root.path().join("d.release"), b"release").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert!(rows
        .iter()
        .all(|row| db.get_download_record(row.record.id).unwrap().status == "completed"));
}

// Existing pause/cancel fixtures deliberately exercise a full two-slot queue.
fn two_slot_manager() -> DownloadManager {
    let manager = DownloadManager::default();
    {
        let mut state = manager.inner.lock().unwrap();
        state.scheduler.set_limit(2);
        state.limit_loaded = true;
    }
    manager
}

#[tokio::test]
async fn published_progress_is_finite_and_cannot_resurrect_terminal_tasks() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned progress boundary fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    // Keep it queued so no worker races the injected native samples.
    manager.inner.lock().unwrap().scheduler.stop();
    let task = enqueue(&manager, db, root.path(), "progress-boundary");
    manager.publish_progress(
        &task.record.request_id,
        DownloadProgress {
            phase: "downloading",
            percent: Some(150.0),
            speed: Some(f64::INFINITY),
            eta: Some(-1.0),
        },
    );
    let sample = manager.task_for(task.record.id).unwrap();
    manager.cancel(&task.record.request_id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
    let terminal = manager.task_for(task.record.id).unwrap();
    manager.publish_progress(
        &task.record.request_id,
        DownloadProgress {
            phase: "downloading",
            percent: Some(10.0),
            speed: Some(1.0),
            eta: Some(2.0),
        },
    );
    let late = manager.task_for(task.record.id).unwrap();
    assert_eq!(sample.percent, Some(100.0));
    assert_eq!(sample.speed, None);
    assert_eq!(sample.eta, None);
    assert_eq!(late.phase, terminal.phase);
    assert_eq!(late.percent, terminal.percent);
}

#[tokio::test]
async fn explicit_cancel_of_queued_redownload_preserves_previous_completed_video() {
    use crate::database::download_records::DownloadRecordOutcome;
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned cancel previous output fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    let id = db
        .begin_download_record("completed-before", &snapshot, &crate::datetime::now())
        .unwrap();
    let output = root.path().join("finished.mp4");
    std::fs::write(&output, b"previous complete video").unwrap();
    db.finish_download_record(
        "completed-before",
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 23,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    db.restart_download_record(
        "retry-cancel",
        id,
        "completed-before",
        &snapshot,
        false,
        |_, _| Ok(None),
    )
        .unwrap();
    db.finish_download_record("retry-cancel", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    two_slot_manager()
        .cancel_and_remove("retry-cancel", db.clone())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), b"previous complete video");
    assert!(db.find_request_record("retry-cancel").unwrap().is_none());
}

#[tokio::test]
#[cfg(windows)]
async fn explicit_cancel_keeps_record_on_occupied_fragment_and_can_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned occupied cancel fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    db.begin_download_record(
        "occupied",
        &DownloadSnapshot { page: state },
        &crate::datetime::now(),
    )
        .unwrap();
    let directory = db
        .prepare_download_temporary_directory("occupied", root.path())
        .unwrap();
    let partial = Path::new(&directory.path).join("video.part");
    std::fs::write(&partial, b"fragment").unwrap();
    db.finish_download_record(
        "occupied",
        &crate::database::download_records::DownloadRecordOutcome::Paused,
    )
        .unwrap();
    let guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&partial)
        .unwrap();
    let manager = two_slot_manager();
    assert!(manager
        .cancel_and_remove("occupied", db.clone())
        .await
        .is_err());
    assert!(db.find_request_record("occupied").unwrap().is_some());
    assert!(partial.exists());
    drop(guard);
    manager
        .cancel_and_remove("occupied", db.clone())
        .await
        .unwrap();
    assert!(db.find_request_record("occupied").unwrap().is_none());
    assert!(!Path::new(&directory.path).exists());
}

#[tokio::test]
async fn explicit_cancel_removes_live_task_and_owned_fragments() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned cancel fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "cancel-cleanup",
        "pausable",
    );
    until(|| root.path().join("cancel-cleanup.started").exists()).await;
    let directory = db
        .prepare_download_temporary_directory(&task.record.request_id, root.path())
        .unwrap();
    std::fs::write(Path::new(&directory.path).join("video.part"), b"fragment").unwrap();
    manager
        .cancel_and_remove(&task.record.request_id, db.clone())
        .await
        .unwrap();
    assert!(!Path::new(&directory.path).exists());
    assert!(db
        .find_request_record(&task.record.request_id)
        .unwrap()
        .is_none());
    assert!(manager.task_for(task.record.id).is_none());
}

#[tokio::test]
async fn explicit_cancel_after_restart_removes_paused_record_and_fragments() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned restored cancel fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    db.begin_download_record(
        "restored",
        &DownloadSnapshot { page: state },
        &crate::datetime::now(),
    )
        .unwrap();
    let directory = db
        .prepare_download_temporary_directory("restored", root.path())
        .unwrap();
    std::fs::write(Path::new(&directory.path).join("video.part"), b"fragment").unwrap();
    db.finish_download_record(
        "restored",
        &crate::database::download_records::DownloadRecordOutcome::Paused,
    )
        .unwrap();
    drop(db);
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    two_slot_manager()
        .cancel_and_remove("restored", db.clone())
        .await
        .unwrap();
    assert!(!Path::new(&directory.path).exists());
    assert!(db.find_request_record("restored").unwrap().is_none());
}
use crate::{
    database::{persistence_tests::page, Database},
    required_tools::{Program, RequiredToolConfig, RequiredToolSource},
};
fn settings() -> RequiredToolSettings {
    let path = std::env::current_exe().unwrap();
    RequiredToolSettings {
        tools: [
            (RequiredToolId::Ytdlp, vec!["yt-dlp"]),
            (RequiredToolId::Ffmpeg, vec!["ffmpeg", "ffprobe"]),
            (RequiredToolId::Deno, vec!["deno"]),
        ]
            .into_iter()
            .map(|(id, names)| {
                (
                    id,
                    RequiredToolConfig {
                        source: RequiredToolSource::Manual,
                        manual_path: path.to_string_lossy().into(),
                        programs: names
                            .into_iter()
                            .map(|name| Program {
                                name: name.into(),
                                path: path.clone(),
                                version: "test".into(),
                            })
                            .collect(),
                        checked_at: crate::datetime::now(),
                    },
                )
            })
            .collect(),
    }
}
fn enqueue(
    manager: &DownloadManager,
    database: Arc<Database>,
    root: &Path,
    name: &str,
) -> DownloadTaskSnapshot {
    enqueue_with_mode(manager, database, root, name, "controlled")
}
fn enqueue_with_mode(
    manager: &DownloadManager,
    database: Arc<Database>,
    root: &Path,
    name: &str,
    mode: &str,
) -> DownloadTaskSnapshot {
    let mut page = page();
    page.video_id = Some(name.into());
    page.input_link = format!("https://youtu.be/{name}");
    page.download_directory = root.to_string_lossy().into();
    let (acceptance, history) = DownloadRecordSession::accept(
        database.clone(),
        root,
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page: page.clone() },
        None,
        false,
        false,
    )
        .unwrap();
    let RecordAcceptance::Accepted(record) = acceptance else {
        panic!()
    };
    let settings = settings();
    let lease = crate::required_tools::usage::ToolUsageRegistry::default()
        .acquire(&[])
        .unwrap();
    let execution = super::super::task_snapshot::TaskExecutionSnapshot {
        page,
        settings,
        platform: CookiePlatform::Youtube,
        runner: super::super::task_snapshot::DownloadExecution::Ytdlp(
            super::super::tests::fixture(mode, &root.join(format!("{name}.mp4"))),
        ),
        network_diagnostic: crate::video::diagnostics::network_summary("youtube", None),
        command_options: None,
        _cookie: None,
        _tools: lease,
    };
    manager
        .accept_internal(
            None,
            record,
            Job {
                restart_output: None,
                execution: Ok(execution),
                history: history.unwrap(),
                database,
            },
        )
        .unwrap()
}
async fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    while !predicate() {
        assert!(
            std::time::Instant::now() < deadline,
            "native task did not settle"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn two_paused_tasks_release_slots_for_a_third_native_download() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned paused queue fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue_with_mode(&manager, db.clone(), root.path(), "paused-a", "pausable");
    let b = enqueue_with_mode(&manager, db.clone(), root.path(), "paused-b", "pausable");
    until(|| {
        [&a, &b]
            .iter()
            .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
            && root.path().join("paused-a.started").exists()
            && root.path().join("paused-b.started").exists()
    })
        .await;
    manager.set_paused(&a.record.request_id, true).unwrap();
    manager.set_paused(&b.record.request_id, true).unwrap();
    let c = enqueue_with_mode(&manager, db.clone(), root.path(), "third", "pausable");
    let started = tokio::time::timeout(Duration::from_secs(2), async {
        until(|| root.path().join("third.started").exists()).await;
    })
        .await
        .is_ok();
    let phase = manager.task_for(c.record.id).unwrap().phase;
    let slots = manager.inner.lock().unwrap().scheduler.running_count();
    // Stop owned children before asserting so a regression never leaves suspended processes.
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert!(
        started,
        "the third native process must start while both other tasks are paused"
    );
    assert_eq!(phase, "downloading");
    assert_eq!(slots, 1);
    assert_eq!(
        db.get_download_record(a.record.id).unwrap().status,
        "paused"
    );
    assert_eq!(
        db.get_download_record(b.record.id).unwrap().status,
        "paused"
    );
}

#[tokio::test]
async fn resumed_native_process_waits_behind_existing_queue_without_exceeding_two_slots() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned resume queue fixture: {}", root.path().display());
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue_with_mode(&manager, db.clone(), root.path(), "resume-a", "pausable");
    let b = enqueue_with_mode(&manager, db.clone(), root.path(), "resume-b", "pausable");
    until(|| {
        [&a, &b]
            .iter()
            .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
            && root.path().join("resume-a.started").exists()
            && root.path().join("resume-b.started").exists()
    })
        .await;
    let c = enqueue_with_mode(&manager, db.clone(), root.path(), "resume-c", "pausable");
    let d = enqueue_with_mode(&manager, db.clone(), root.path(), "resume-d", "pausable");
    manager.set_paused(&a.record.request_id, true).unwrap();
    let started = tokio::time::timeout(Duration::from_secs(2), async {
        until(|| manager.task_for(c.record.id).unwrap().phase == "downloading").await;
    })
        .await
        .is_ok();
    if !started {
        manager.begin_exit().unwrap();
        until(|| !manager.is_active().unwrap()).await;
        panic!("pausing a task must dispatch the first queued native process");
    }
    let resumed = manager.set_paused(&a.record.request_id, false).unwrap();
    let duplicate = manager.set_paused(&a.record.request_id, false).unwrap();
    std::fs::write(root.path().join("resume-a.release"), b"release").unwrap();
    // Buffered progress from before suspension must not turn a queued resume into downloading.
    manager.publish_progress(
        &a.record.request_id,
        DownloadProgress {
            phase: "downloading",
            percent: Some(70.0),
            speed: Some(100.0),
            eta: Some(1.0),
        },
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    let queued = manager.task_for(a.record.id).unwrap();
    let frozen = !root.path().join("resume-a.mp4").exists();
    let full_slots = manager.inner.lock().unwrap().scheduler.running_count();
    std::fs::write(root.path().join("resume-b.release"), b"release").unwrap();
    until(|| manager.task_for(d.record.id).unwrap().phase == "downloading").await;
    let fifo_phase = manager.task_for(a.record.id).unwrap().phase;
    std::fs::write(root.path().join("resume-c.release"), b"release").unwrap();
    until(|| manager.task_for(a.record.id).unwrap().phase == "completed").await;
    let completed = manager.task_for(a.record.id).unwrap();
    std::fs::write(root.path().join("resume-d.release"), b"release").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(resumed.phase, "queued");
    assert_eq!(
        duplicate.revision, resumed.revision,
        "duplicate resumes cannot enqueue twice"
    );
    assert_eq!(queued.phase, "queued");
    assert_eq!(queued.percent, resumed.percent);
    assert!(
        frozen,
        "a queued resume must keep the original OS process suspended"
    );
    assert_eq!(full_slots, 2);
    assert_eq!(
        fifo_phase, "queued",
        "earlier queued tasks must start before the resumed task"
    );
    assert_eq!(completed.record.request_id, a.record.request_id);
    assert_eq!(completed.record.id, a.record.id);
    assert_eq!(
        db.get_download_record(a.record.id).unwrap().status,
        "completed"
    );
}

#[tokio::test]
async fn queued_resume_can_be_cancelled_or_preserved_on_exit() {
    for exiting in [false, true] {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "owned pending resume cleanup fixture: {}",
            root.path().display()
        );
        let db = Arc::new(
            Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap(),
        );
        let manager = two_slot_manager();
        let a = enqueue_with_mode(&manager, db.clone(), root.path(), "pending-a", "pausable");
        let b = enqueue_with_mode(&manager, db.clone(), root.path(), "pending-b", "pausable");
        until(|| {
            [&a, &b]
                .iter()
                .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
        })
            .await;
        manager.set_paused(&a.record.request_id, true).unwrap();
        let c = enqueue_with_mode(&manager, db.clone(), root.path(), "pending-c", "pausable");
        until(|| manager.task_for(c.record.id).unwrap().phase == "downloading").await;
        let resumed = manager.set_paused(&a.record.request_id, false).unwrap();
        assert_eq!(resumed.phase, "queued");
        assert!(!db.download_pause_requested(&a.record.request_id).unwrap());
        if !exiting {
            manager.cancel(&a.record.request_id).unwrap();
            until(|| manager.task_for(a.record.id).unwrap().phase == "cancelled").await;
        }
        manager.begin_exit().unwrap();
        until(|| !manager.is_active().unwrap()).await;
        assert_eq!(
            manager.task_for(a.record.id).unwrap().phase,
            if exiting { "paused" } else { "cancelled" }
        );
        assert_eq!(
            db.get_download_record(a.record.id).unwrap().status,
            if exiting { "paused" } else { "cancelled" }
        );
        assert!(!root.path().join("pending-a.mp4").exists());
    }
}

#[tokio::test]
async fn failed_queued_resume_keeps_process_suspended_and_does_not_change_slots() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned pending resume save failure fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue_with_mode(&manager, db.clone(), root.path(), "save-a", "pausable");
    let b = enqueue_with_mode(&manager, db.clone(), root.path(), "save-b", "pausable");
    until(|| {
        [&a, &b]
            .iter()
            .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
    })
        .await;
    manager.set_paused(&a.record.request_id, true).unwrap();
    let c = enqueue_with_mode(&manager, db.clone(), root.path(), "save-c", "pausable");
    until(|| manager.task_for(c.record.id).unwrap().phase == "downloading").await;
    let connection = rusqlite::Connection::open(root.path().join("app.db")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_queued_resume BEFORE UPDATE OF pause_requested ON download_records WHEN OLD.pause_requested=1 AND NEW.pause_requested=0 BEGIN SELECT RAISE(ABORT,'owned queued resume storage failure'); END;").unwrap();
    let result = manager.set_paused(&a.record.request_id, false);
    let phase = manager.task_for(a.record.id).unwrap().phase;
    let slots = manager.inner.lock().unwrap().scheduler.running_count();
    let pause_requested = db.download_pause_requested(&a.record.request_id).unwrap();
    std::fs::write(root.path().join("save-a.release"), b"release").unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let frozen = !root.path().join("save-a.mp4").exists();
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(result.err().unwrap().code, "resumeFailed");
    assert_eq!(phase, "paused");
    assert_eq!(slots, 2);
    assert!(pause_requested);
    assert!(frozen);
}

#[tokio::test]
async fn an_old_dispatch_cannot_unfreeze_a_task_that_has_rejoined_the_queue() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned stale resume dispatch fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue_with_mode(&manager, db.clone(), root.path(), "stale-a", "pausable");
    let b = enqueue_with_mode(&manager, db.clone(), root.path(), "stale-b", "pausable");
    until(|| {
        [&a, &b]
            .iter()
            .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
    })
        .await;
    manager.set_paused(&a.record.request_id, true).unwrap();
    // Capture a dispatch before launch, as can happen when event emission is delayed.
    let stale = {
        let mut state = manager.inner.lock().unwrap();
        let entry = state.entries.get_mut(&a.record.request_id).unwrap();
        entry
            .set_process_paused(&a.record.request_id, false)
            .unwrap();
        entry.suspended = false;
        entry.snapshot.phase = "downloading".into();
        state.scheduler.enqueue(a.record.request_id.clone())
    };
    let c = enqueue_with_mode(&manager, db, root.path(), "stale-c", "pausable");
    manager.set_paused(&a.record.request_id, true).unwrap();
    until(|| manager.task_for(c.record.id).unwrap().phase == "downloading").await;
    let resumed = manager.set_paused(&a.record.request_id, false).unwrap();
    manager.launch(stale);
    std::fs::write(root.path().join("stale-a.release"), b"release").unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let phase = manager.task_for(a.record.id).unwrap().phase;
    let frozen = !root.path().join("stale-a.mp4").exists();
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(resumed.phase, "queued");
    assert_eq!(phase, "queued");
    assert!(
        frozen,
        "a stale dispatch must not resume a third native process"
    );
}

#[tokio::test]
async fn explicit_cancel_followed_by_exit_cannot_turn_a_queued_resume_into_paused() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned queued resume cancel/exit race fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "cancel-exit-a",
        "pausable",
    );
    let b = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "cancel-exit-b",
        "pausable",
    );
    until(|| {
        [&a, &b]
            .iter()
            .all(|task| manager.task_for(task.record.id).unwrap().phase == "downloading")
    })
        .await;
    manager.set_paused(&a.record.request_id, true).unwrap();
    let c = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "cancel-exit-c",
        "pausable",
    );
    until(|| manager.task_for(c.record.id).unwrap().phase == "downloading").await;
    manager.set_paused(&a.record.request_id, false).unwrap();
    // Hold settlement in SQLite so exit observes the explicit cancellation in flight.
    let connection = rusqlite::Connection::open(root.path().join("app.db")).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
    manager.cancel(&a.record.request_id).unwrap();
    manager.begin_exit().unwrap();
    connection.execute_batch("COMMIT;").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(manager.task_for(a.record.id).unwrap().phase, "cancelled");
    assert_eq!(
        db.get_download_record(a.record.id).unwrap().status,
        "cancelled"
    );
}

#[tokio::test]
async fn pause_freezes_native_process_and_resume_completes_the_same_task() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned pause/resume test directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue_with_mode(&manager, db.clone(), root.path(), "pause", "pausable");
    let id = task.record.request_id.clone();
    until(|| manager.task_for(task.record.id).unwrap().phase == "downloading").await;
    let before = manager.task_for(task.record.id).unwrap();
    let paused = manager.set_paused(&id, true).unwrap();
    assert_eq!(paused.phase, "paused");
    assert_eq!(paused.percent, before.percent);
    assert!(db.download_pause_requested(&id).unwrap());
    assert!(manager.is_active().unwrap());
    std::fs::write(root.path().join("pause.release"), b"release").unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !root.path().join("pause.mp4").exists(),
        "paused OS process must not write output"
    );
    assert_eq!(manager.task_for(task.record.id).unwrap().phase, "paused");
    assert_eq!(manager.set_paused(&id, false).unwrap().phase, "downloading");
    assert!(!db.download_pause_requested(&id).unwrap());
    until(|| !manager.is_active().unwrap()).await;
    let completed = manager.task_for(task.record.id).unwrap();
    assert_eq!(completed.phase, "completed");
    assert_eq!(completed.record.request_id, id);
    assert_eq!(
        db.get_download_record(task.record.id).unwrap().status,
        "completed"
    );
}

#[tokio::test]
async fn exiting_preserves_a_paused_task_across_reopen_without_leaving_a_process_running() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned paused exit test directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue_with_mode(&manager, db.clone(), root.path(), "pause-exit", "pausable");
    until(|| manager.task_for(task.record.id).unwrap().phase == "downloading").await;
    manager.set_paused(&task.record.request_id, true).unwrap();
    manager.begin_exit().unwrap();
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(manager.task_for(task.record.id).unwrap().phase, "paused");
    assert_eq!(
        db.get_download_record(task.record.id).unwrap().status,
        "paused"
    );
    let reopened =
        Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    super::super::history::recover(&reopened, root.path()).unwrap();
    super::super::history::recover(&reopened, root.path()).unwrap();
    let saved = reopened.get_download_record(task.record.id).unwrap();
    assert_eq!(saved.status, "paused");
    assert!(saved.finished_at.is_none());
    assert!(saved.error_code.is_none());
    assert!(!root.path().join("pause-exit.mp4").exists());
}

#[tokio::test]
async fn reopened_paused_task_resumes_with_its_saved_format_and_directory() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned reopened pause/resume fixture: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let original = two_slot_manager();
    let task = enqueue_with_mode(&original, db.clone(), root.path(), "reopen", "pausable");
    until(|| original.task_for(task.record.id).unwrap().phase == "downloading").await;
    original.set_paused(&task.record.request_id, true).unwrap();
    original.begin_exit().unwrap();
    until(|| !original.is_active().unwrap()).await;
    for (id, mut config) in settings().tools {
        for program in &mut config.programs {
            program.version = if id == RequiredToolId::Ytdlp {
                "2026.10.04".into()
            } else {
                "8.0.0".into()
            };
        }
        db.save_tool(id, &config).unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let reopened = two_slot_manager();
    let a = enqueue(&reopened, db.clone(), root.path(), "resume-block-a");
    let b = enqueue(&reopened, db.clone(), root.path(), "resume-block-b");
    until(|| {
        root.path().join("resume-block-a.started").exists()
            && root.path().join("resume-block-b.started").exists()
    })
        .await;
    let result = commands::resume_from_request(
        None,
        &task.record.request_id,
        &reopened,
        &tools,
        &cookies,
        &storage,
    )
        .await;
    // Always stop owned native children before asserting on the resumed result.
    if let Ok(resumed) = &result {
        reopened.cancel(&resumed.record.request_id).unwrap();
    }
    reopened.cancel(&a.record.request_id).unwrap();
    reopened.cancel(&b.record.request_id).unwrap();
    until(|| !reopened.is_active().unwrap()).await;
    let resumed = result.unwrap();
    assert_eq!(resumed.phase, "queued");
    assert_eq!(resumed.record.id, task.record.id);
    assert_ne!(resumed.record.request_id, task.record.request_id);
    assert_eq!(resumed.record.format_id, task.record.format_id);
    assert_eq!(
        resumed.record.download_directory,
        task.record.download_directory
    );
    assert_eq!(resumed.record.source_link, task.record.source_link);
    assert!(!db
        .download_pause_requested(&resumed.record.request_id)
        .unwrap());
}

#[tokio::test]
async fn resume_restores_processing_that_arrived_at_the_pause_boundary() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned pause boundary test directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue_with_mode(&manager, db, root.path(), "boundary", "pausable");
    let id = &task.record.request_id;
    until(|| manager.task_for(task.record.id).unwrap().phase == "downloading").await;
    manager.set_paused(id, true).unwrap();
    manager.publish_progress(
        id,
        DownloadProgress {
            phase: "processing",
            percent: None,
            speed: None,
            eta: None,
        },
    );
    assert_eq!(manager.task_for(task.record.id).unwrap().phase, "paused");
    assert_eq!(manager.set_paused(id, false).unwrap().phase, "processing");
    assert_eq!(
        manager.set_paused(id, false).unwrap().phase,
        "processing",
        "duplicate resumes are idempotent"
    );
    assert_eq!(
        manager.set_paused(id, true).err().unwrap().code,
        "pauseFailed"
    );
    manager.cancel(id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
}

#[tokio::test]
async fn explicitly_cancelling_a_paused_task_still_records_cancellation() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned paused cancellation fixture: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue_with_mode(
        &manager,
        db.clone(),
        root.path(),
        "pause-cancel",
        "pausable",
    );
    until(|| manager.task_for(task.record.id).unwrap().phase == "downloading").await;
    manager.set_paused(&task.record.request_id, true).unwrap();
    manager.cancel(&task.record.request_id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(
        db.get_download_record(task.record.id).unwrap().status,
        "cancelled"
    );
    assert!(!db
        .download_pause_requested(&task.record.request_id)
        .unwrap());
}

#[tokio::test]
async fn storage_failure_rolls_back_native_pause_control() {
    for (name, was_paused, code, phase) in [
        ("pause-save", false, "pauseFailed", "downloading"),
        ("resume-save", true, "resumeFailed", "paused"),
    ] {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "owned pause control save failure fixture: {}",
            root.path().display()
        );
        let db = Arc::new(
            Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap(),
        );
        let manager = two_slot_manager();
        let task = enqueue_with_mode(&manager, db.clone(), root.path(), name, "pausable");
        until(|| manager.task_for(task.record.id).unwrap().phase == "downloading").await;
        if was_paused {
            manager.set_paused(&task.record.request_id, true).unwrap();
        }
        let connection = rusqlite::Connection::open(root.path().join("app.db")).unwrap();
        let trigger = if was_paused {
            "CREATE TRIGGER reject_control BEFORE UPDATE OF pause_requested ON download_records WHEN NEW.pause_requested=0 AND OLD.pause_requested=1 BEGIN SELECT RAISE(ABORT,'owned resume storage failure'); END;"
        } else {
            "CREATE TRIGGER reject_control BEFORE UPDATE OF pause_requested ON download_records WHEN NEW.pause_requested=1 BEGIN SELECT RAISE(ABORT,'owned pause storage failure'); END;"
        };
        connection.execute_batch(trigger).unwrap();
        let failure = manager
            .set_paused(&task.record.request_id, !was_paused)
            .err()
            .unwrap();
        assert_eq!(failure.code, code);
        assert_eq!(manager.task_for(task.record.id).unwrap().phase, phase);
        assert_eq!(
            manager.inner.lock().unwrap().scheduler.running_count(),
            if was_paused { 0 } else { 1 }
        );
        assert_eq!(
            db.download_pause_requested(&task.record.request_id)
                .unwrap(),
            was_paused
        );
        std::fs::write(root.path().join(format!("{name}.release")), b"release").unwrap();
        if was_paused {
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert!(!root.path().join(format!("{name}.mp4")).exists());
            connection
                .execute_batch("DROP TRIGGER reject_control;")
                .unwrap();
            manager.set_paused(&task.record.request_id, false).unwrap();
        }
        until(|| !manager.is_active().unwrap()).await;
        assert!(root.path().join(format!("{name}.mp4")).is_file());
        assert_eq!(
            db.get_download_record(task.record.id).unwrap().status,
            "completed"
        );
    }
}

#[tokio::test]
#[ignore = "requires authorized imported Douyin Cookie, ffprobe and live platform network"]
async fn live_formal_douyin_parse_queue_and_verify() {
    let cookie_path =
        PathBuf::from(std::env::var_os("EVD_NATIVE_COOKIE_FILE").expect("Cookie path required"));
    let original = std::fs::read(&cookie_path).unwrap();
    let contents = std::str::from_utf8(&original).unwrap();
    let ffprobe = PathBuf::from(std::env::var_os("EVD_NATIVE_FFPROBE").unwrap());
    let ffmpeg = PathBuf::from(std::env::var_os("EVD_NATIVE_FFMPEG").unwrap());
    let input = "https://v.douyin.com/V4Jkr52cl90/";
    let metadata = crate::video::native_douyin::parse(contents, None, input)
        .await
        .unwrap();
    let chosen = metadata
        .formats
        .iter()
        .filter(|f| {
            f.height == Some(1080)
                && f.video_codec.as_deref() == Some("h264")
                && f.watermarked != Some(true)
        })
        .max_by_key(|f| f.bitrate)
        .expect("sample 1080p H264")
        .clone();
    eprintln!(
        "formal Douyin result: {} formats; selected {}x{} H264, {} bytes",
        metadata.formats.len(),
        chosen.width.unwrap_or(0),
        chosen.height.unwrap_or(0),
        chosen.size_bytes.unwrap_or(0)
    );
    let root = tempfile::Builder::new()
        .prefix("evd-formal-douyin-live-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned formal Douyin live directory: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    db.save_tool(
        RequiredToolId::Ffmpeg,
        &RequiredToolConfig {
            source: RequiredToolSource::Manual,
            manual_path: ffmpeg.parent().unwrap().to_string_lossy().into(),
            programs: vec![
                Program {
                    name: "ffmpeg".into(),
                    path: ffmpeg,
                    version: "8.0".into(),
                },
                Program {
                    name: "ffprobe".into(),
                    path: ffprobe,
                    version: "8.0".into(),
                },
            ],
            checked_at: crate::datetime::now(),
        },
    )
        .unwrap();
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    cookies.save(CookiePlatform::Douyin, contents).unwrap();
    let snapshot = crate::database::page_states::DownloadPageState {
        platform: "douyin".into(),
        input_link: input.into(),
        video_id: Some(metadata.id),
        title: Some(metadata.title),
        duration_seconds: metadata.duration,
        extension: Some("mp4".into()),
        formats: metadata.formats,
        selected_format_id: Some(chosen.format_id.clone()),
        selected_height: chosen.height,
        selected_fps: chosen.fps,
        download_directory: root.path().join("output").to_string_lossy().into(),
        parser_fingerprint: Some("douyin-rust-v1".into()),
        parsed_at: Some(crate::datetime::now()),
        ..Default::default()
    };
    db.save_download_page_state(&snapshot).unwrap();
    let manager = two_slot_manager();
    let accepted = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot,
            restore_trashed: false,
            redownload: false,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(180), async {
        while manager.is_active().unwrap() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
        .await
        .unwrap();
    let record = db.get_download_record(accepted.record.id).unwrap();
    assert_eq!(record.status, "completed", "{:?}", record.error_detail);
    assert_eq!(
        std::fs::metadata(record.output_path.unwrap())
            .unwrap()
            .len(),
        record.file_size_bytes.unwrap()
    );
    assert_eq!(
        std::fs::read(cookie_path).unwrap(),
        original,
        "Managed Cookie was modified"
    );
    eprintln!(
        "formal Douyin verified output size: {} bytes",
        record.file_size_bytes.unwrap()
    );
    drop(manager);
    drop(db);
    drop(tools);
    drop(storage);
    root.close().unwrap();
}

#[tokio::test]
#[ignore = "requires installed FFmpeg and ffprobe; serves owned generated media over loopback"]
async fn douyin_native_queue_downloads_verifies_and_cancels_without_ytdlp() {
    use std::io::{Read, Write};
    let ffmpeg =
        PathBuf::from(std::env::var_os("EVD_NATIVE_FFMPEG").expect("FFmpeg path required"));
    let ffprobe =
        PathBuf::from(std::env::var_os("EVD_NATIVE_FFPROBE").expect("ffprobe path required"));
    let root = tempfile::Builder::new()
        .prefix("evd-native-queue-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned native queue smoke directory: {}",
        root.path().display()
    );
    let source = root.path().join("source.mp4");
    let generated = std::process::Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let bytes = std::fs::read(&source).unwrap();
    for cancel in [false, true] {
        let workspace = root.path().join(if cancel { "cancel" } else { "complete" });
        std::fs::create_dir_all(&workspace).unwrap();
        let storage = Storage::new(&workspace.join("app.db"), &workspace.join("legacy"));
        let db = storage.database().unwrap();
        db.save_tool(
            RequiredToolId::Ffmpeg,
            &RequiredToolConfig {
                source: RequiredToolSource::Manual,
                manual_path: ffmpeg.parent().unwrap().to_string_lossy().into(),
                programs: vec![
                    Program {
                        name: "ffmpeg".into(),
                        path: ffmpeg.clone(),
                        version: "8.0".into(),
                    },
                    Program {
                        name: "ffprobe".into(),
                        path: ffprobe.clone(),
                        version: "8.0".into(),
                    },
                ],
                checked_at: crate::datetime::now(),
            },
        )
            .unwrap();
        let tools = RequiredToolManager::new(storage.clone());
        assert!(!tools
            .settings_snapshot()
            .unwrap()
            .tools
            .contains_key(&RequiredToolId::Ytdlp));
        let cookies = CookieStore::new(&workspace);
        cookies
            .save(
                CookiePlatform::Douyin,
                ".douyin.com\tTRUE\t/\tTRUE\t0\tfixture\tsynthetic-cookie\n",
            )
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/media", listener.local_addr().unwrap());
        let served = bytes.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = [0; 4096];
            let n = stream.read(&mut request).unwrap();
            assert!(!String::from_utf8_lossy(&request[..n]).contains("synthetic-cookie"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                served.len()
            )
                .unwrap();
            if cancel {
                let _ = stream.write_all(&served[..128]);
                std::thread::sleep(Duration::from_millis(500));
                let _ = stream.write_all(&served[128..]);
            } else {
                stream.write_all(&served).unwrap();
            }
        });
        let parsed = crate::douyin::parse_detail(&serde_json::json!({"aweme_detail":{"aweme_id":"123","desc":"Native queue test","video":{"bit_rate":[{"FPS":30,"bit_rate":100000,"is_bytevc1":false,"play_addr":{"width":320,"height":180,"data_size":bytes.len(),"url_list":[url]}}]}}})).unwrap();
        let metadata = crate::video::native_douyin::remember(parsed).unwrap();
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("127.0.0.1"));
        let mut snapshot = page();
        snapshot.platform = "douyin".into();
        snapshot.input_link = "https://www.douyin.com/video/123".into();
        snapshot.video_id = Some(metadata.id);
        snapshot.title = Some(metadata.title);
        snapshot.formats = metadata.formats;
        snapshot.selected_format_id = Some(snapshot.formats[0].format_id.clone());
        snapshot.selected_height = Some(180);
        snapshot.selected_fps = Some(30.0);
        snapshot.extension = Some("mp4".into());
        snapshot.parser_fingerprint = Some("douyin-rust-v1".into());
        snapshot.download_directory = workspace.join("output").to_string_lossy().into();
        db.save_download_page_state(&snapshot).unwrap();
        let manager = two_slot_manager();
        let accepted = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot,
                restore_trashed: false,
                redownload: false,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        if cancel {
            until(|| manager.task_for(accepted.record.id).unwrap().phase == "downloading").await;
            manager.cancel(&accepted.record.request_id).unwrap();
        }
        until(|| !manager.is_active().unwrap()).await;
        let record = db.get_download_record(accepted.record.id).unwrap();
        assert_eq!(
            record.status,
            if cancel { "cancelled" } else { "completed" },
            "{:?}",
            record.error_detail
        );
        if cancel {
            assert!(record.output_path.is_none());
            assert_eq!(
                std::fs::read_dir(workspace.join("output")).unwrap().count(),
                0
            );
        } else {
            assert_eq!(
                std::fs::read(record.output_path.as_ref().unwrap()).unwrap(),
                bytes
            );
            assert_eq!(record.file_size_bytes, Some(bytes.len() as u64));
            assert_eq!(
                record
                    .successful_output
                    .unwrap()
                    .format_snapshot
                    .unwrap()
                    .video_codec
                    .as_deref(),
                Some("h264")
            );
        }
        server.join().unwrap();
    }
    root.close().unwrap();
}
#[tokio::test]
async fn two_native_slots_cancel_queue_and_complete_without_event_subscribers() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary native task registry directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let a = enqueue(&manager, db.clone(), root.path(), "a");
    let b = enqueue(&manager, db.clone(), root.path(), "b");
    let c = enqueue(&manager, db.clone(), root.path(), "c");
    let d = enqueue(&manager, db.clone(), root.path(), "d");
    assert!(
        a.submission_order < b.submission_order
            && b.submission_order < c.submission_order
            && c.submission_order < d.submission_order
    );
    until(|| root.path().join("a.started").exists() && root.path().join("b.started").exists())
        .await;
    assert_eq!(manager.task_for(c.record.id).unwrap().phase, "queued");
    assert!(!root.path().join("c.started").exists());
    manager.cancel(&c.record.request_id).unwrap();
    manager.cancel(&a.record.request_id).unwrap();
    until(|| root.path().join("d.started").exists()).await;
    assert!(!root.path().join("c.started").exists());
    assert!(active(&manager.task_for(b.record.id).unwrap().phase));
    assert!(manager.inner.lock().unwrap().scheduler.running_count() <= 2);
    std::fs::write(root.path().join("b.release"), b"release").unwrap();
    std::fs::write(root.path().join("d.release"), b"release").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    for row in [&b, &d] {
        assert_eq!(
            db.get_download_record(row.record.id).unwrap().status,
            "completed"
        );
    }
    for row in [&a, &c] {
        assert_eq!(
            db.get_download_record(row.record.id).unwrap().status,
            "cancelled"
        );
    }
}
#[tokio::test]
async fn exit_drains_running_and_queued_native_jobs() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary native exit task directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let rows = [
        enqueue(&manager, db.clone(), root.path(), "a"),
        enqueue(&manager, db.clone(), root.path(), "b"),
        enqueue(&manager, db.clone(), root.path(), "c"),
    ];
    until(|| root.path().join("a.started").exists() && root.path().join("b.started").exists())
        .await;
    manager.begin_exit().unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert!(manager.inner.lock().unwrap().exiting);
    assert!(!root.path().join("c.started").exists());
    for task in rows {
        assert_eq!(
            db.get_download_record(task.record.id).unwrap().status,
            "cancelled"
        );
    }
    manager.abort_exit();
    assert!(!manager.inner.lock().unwrap().exiting);
}

#[tokio::test]
async fn review_cancel_terminal_interleaving_cannot_resurrect_a_finished_task() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary cancellation race directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = two_slot_manager();
    let task = enqueue(&manager, db.clone(), root.path(), "race");
    until(|| root.path().join("race.started").exists()).await;
    let cancelling = manager.clone();
    let observing = manager.clone();
    let id = task.record.request_id.clone();
    let record_id = task.record.id;
    tokio::task::spawn_blocking(move || {
        cancelling.cancel_at_boundary(&id, || {
            let deadline = std::time::Instant::now() + Duration::from_secs(6);
            while active(&observing.task_for(record_id).unwrap().phase) {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manager.task_for(record_id).unwrap().phase, "cancelled");
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(
        db.get_download_record(record_id).unwrap().status,
        "cancelled"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn record_restart_preflights_before_deletion_and_enters_the_shared_queue() {
    use crate::database::download_records::DownloadRecordOutcome;
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary native record restart directory: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let original_directory = root.path().join("original");
    std::fs::create_dir(&original_directory).unwrap();
    let mut original = page();
    original.download_directory = original_directory.to_string_lossy().into();
    original.cookie_fallback = true;
    db.save_download_page_state(&original).unwrap();
    let uuid = uuid::Uuid::new_v4().to_string();
    let RecordAcceptance::Accepted(row) = db
        .accept_download_record(
            &uuid,
            &DownloadSnapshot {
                page: original.clone(),
            },
            false,
            false,
        )
        .unwrap()
    else {
        panic!()
    };
    let output = original_directory.join("old.webm");
    std::fs::write(&output, b"original").unwrap();
    db.finish_download_record(
        &uuid,
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    let manager = two_slot_manager();
    let cookies = CookieStore::new(root.path());
    cookies
        .save(CookiePlatform::Youtube, "current cookie")
        .unwrap();
    let tools = RequiredToolManager::new(storage.clone());
    let failure = commands::restart_from_record(
        None,
        row.id,
        &uuid,
        RecordRestartMode::ReplaceOutput,
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.code, "toolMissing");
    assert!(output.exists());
    assert_eq!(db.get_download_record(row.id).unwrap().request_id, uuid);
    assert!(!manager.is_active().unwrap());
    for (id, mut config) in settings().tools {
        for program in &mut config.programs {
            program.version = if id == RequiredToolId::Ytdlp {
                "2026.10.04".into()
            } else {
                "8.0.0".into()
            };
        }
        db.save_tool(id, &config).unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    {
        use std::os::windows::fs::OpenOptionsExt;
        let player = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(7)
            .open(&output)
            .unwrap();
        let failure = commands::restart_from_record(
            None,
            row.id,
            &uuid,
            RecordRestartMode::ReplaceOutput,
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .err()
            .expect("deletion must succeed before a record retry enters the queue");
        assert_eq!(failure.code, "historyFileOccupied");
        assert!(manager.task_for(row.id).is_none());
        assert_eq!(db.get_download_record(row.id).unwrap().status, "completed");
        assert_eq!(std::fs::read(&output).unwrap(), b"original");
        assert!(!manager.is_active().unwrap());
        drop(player);
    }
    let blocked_directory = original_directory.clone();
    let a = enqueue(&manager, db.clone(), &blocked_directory, "a");
    let b = enqueue(&manager, db.clone(), &blocked_directory, "b");
    until(|| {
        blocked_directory.join("a.started").exists() && blocked_directory.join("b.started").exists()
    })
        .await;
    let result = commands::restart_from_record(
        None,
        row.id,
        &db.get_download_record(row.id).unwrap().request_id,
        RecordRestartMode::ReplaceOutput,
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    assert_eq!(result.kind, "accepted");
    assert_eq!(result.record.id, row.id);
    assert_ne!(result.record.request_id, uuid);
    assert!(
        !output.exists(),
        "a confirmed record retry deletes its old output before entering the queue"
    );
    assert_eq!(manager.task_for(row.id).unwrap().phase, "queued");
    assert_eq!(
        db.download_page_states().unwrap()[0].selected_format_id,
        original.selected_format_id
    );
    {
        let entry = manager.inner.lock().unwrap();
        let job = entry.entries[&result.record.request_id]
            .job
            .as_ref()
            .unwrap();
        let execution = job.execution.as_ref().unwrap();
        assert_eq!(
            std::fs::read_to_string(execution._cookie.as_ref().unwrap().path()).unwrap(),
            "current cookie"
        );
        assert_eq!(
            execution.page.download_directory,
            original.download_directory
        );
    }
    let again = commands::restart_from_record(
        None,
        row.id,
        &uuid,
        RecordRestartMode::CheckOutput,
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    assert_eq!(again.kind, "existing");
    assert_eq!(again.record.request_id, result.record.request_id);
    manager.cancel(&result.record.request_id).unwrap();
    manager.cancel(&a.record.request_id).unwrap();
    manager.cancel(&b.record.request_id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(db.get_download_record(row.id).unwrap().status, "cancelled");
    assert!(!output.exists());
}

#[cfg(windows)]
#[tokio::test]
async fn duplicate_page_download_preflights_and_restarts_with_current_options() {
    use crate::database::download_records::DownloadRecordOutcome;
    for (trashed, missing) in [(false, false), (false, true), (true, false)] {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary duplicate page download directory: {}",
            root.path().display()
        );
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
        let db = storage.database().unwrap();
        let old_dir = root.path().join("old");
        std::fs::create_dir(&old_dir).unwrap();
        let mut original = page();
        original.download_directory = old_dir.to_string_lossy().into();
        original.selected_format_id = Some("a".into());
        let uuid = uuid::Uuid::new_v4().to_string();
        let RecordAcceptance::Accepted(row) = db
            .accept_download_record(
                &uuid,
                &DownloadSnapshot {
                    page: original.clone(),
                },
                false,
                false,
            )
            .unwrap()
        else {
            panic!()
        };
        let output = old_dir.join("old.webm");
        std::fs::write(&output, b"original").unwrap();
        db.finish_download_record(
            &uuid,
            &DownloadRecordOutcome::Completed {
                path: output.to_string_lossy().into(),
                size: 8,
                extension: Some("webm".into()),
            },
        )
            .unwrap();
        if missing {
            std::fs::remove_file(&output).unwrap();
        }
        if trashed {
            db.delete_download_record(row.id).unwrap();
        }
        let manager = two_slot_manager();
        let cookies = CookieStore::new(root.path());
        let tools = RequiredToolManager::new(storage.clone());
        let mut current = original.clone();
        current.title = Some("Fresh parsed title".into());
        current.formats[0].height = Some(720);
        current.formats[0].fps = Some(30.0);
        current.selected_format_id = Some("a".into());
        current.selected_height = Some(720);
        current.selected_fps = Some(30.0);
        current.download_directory = root.path().join("new").to_string_lossy().into();
        current.parser_fingerprint = Some(
            crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
                .unwrap(),
        );
        db.save_download_page_state(&current).unwrap();
        if trashed {
            let failure = commands::submit_from_page(
                None,
                SubmitDownloadRequest {
                    snapshot: current.clone(),
                    restore_trashed: false,
                    redownload: true,
                },
                &manager,
                &tools,
                &cookies,
                &storage,
            )
                .await
                .err()
                .unwrap();
            assert_eq!(failure.code, "recordTrashed");
            assert!(output.exists());
        }
        let failure = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: current.clone(),
                restore_trashed: trashed,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, "toolMissing");
        let untouched = db.get_download_record(row.id).unwrap();
        assert_eq!(untouched.request_id, uuid);
        assert_eq!(untouched.status, "completed");
        assert_eq!(untouched.deleted_at.is_some(), trashed);
        assert_eq!(output.exists(), !missing);
        assert!(!manager.is_active().unwrap());
        for (id, mut config) in settings().tools {
            for program in &mut config.programs {
                program.version = if id == RequiredToolId::Ytdlp {
                    "2026.10.04".into()
                } else {
                    "8.0.0".into()
                };
            }
            db.save_tool(id, &config).unwrap();
        }
        let tools = RequiredToolManager::new(storage.clone());
        current.parser_fingerprint = Some(
            crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
                .unwrap(),
        );
        db.save_download_page_state(&current).unwrap();
        let blocked = root.path().join("cannot-use-a-file-as-directory");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let mut invalid = current.clone();
        invalid.download_directory = blocked.to_string_lossy().into();
        db.save_download_page_state(&invalid).unwrap();
        let failure = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: invalid,
                restore_trashed: trashed,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, "downloadDirectoryFailed");
        assert_eq!(db.get_download_record(row.id).unwrap().request_id, uuid);
        assert_eq!(
            db.get_download_record(row.id).unwrap().deleted_at.is_some(),
            trashed
        );
        assert_eq!(output.exists(), !missing);
        db.save_download_page_state(&current).unwrap();
        if !missing {
            use std::os::windows::fs::OpenOptionsExt;
            let player = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(7)
                .open(&output)
                .unwrap();
            let accepted = commands::submit_from_page(
                None,
                SubmitDownloadRequest {
                    snapshot: current.clone(),
                    restore_trashed: trashed,
                    redownload: true,
                },
                &manager,
                &tools,
                &cookies,
                &storage,
            )
                .await
                .expect("file deletion belongs to the accepted task, not submission");
            assert_eq!(accepted.kind, "accepted");
            assert_eq!(accepted.task.as_ref().unwrap().phase, "queued");
            assert_eq!(accepted.record.id, row.id);
            until(|| !manager.is_active().unwrap()).await;
            let failed = manager.task_for(row.id).unwrap();
            assert_eq!(failed.phase, "failed");
            assert_eq!(
                failed.record.error_code.as_deref(),
                Some("historyFileOccupied")
            );
            assert_eq!(failed.record.error_stage.as_deref(), Some("preparing"));
            let retained = db.get_download_record(row.id).unwrap();
            assert_eq!(retained.request_id, accepted.record.request_id);
            assert_eq!(retained.status, "failed");
            assert_eq!(retained.file_availability, "present");
            assert!(retained.deleted_at.is_none());
            assert_eq!(retained.successful_output.as_ref().unwrap().format_id, "a");
            assert_eq!(std::fs::read(&output).unwrap(), b"original");
            assert_eq!(db.list_download_records(None, 50).unwrap().records.len(), 1);
            drop(player);
        }
        let a = enqueue(&manager, db.clone(), &old_dir, "a");
        let b = enqueue(&manager, db.clone(), &old_dir, "b");
        until(|| old_dir.join("a.started").exists() && old_dir.join("b.started").exists()).await;
        let result = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: current.clone(),
                restore_trashed: trashed,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        assert_eq!(result.kind, "accepted");
        assert_eq!(result.record.id, row.id);
        assert_ne!(result.record.request_id, uuid);
        assert_eq!(result.record.height, Some(720));
        assert_eq!(result.record.fps, Some(30.0));
        assert_eq!(result.record.format_id, "a");
        assert_eq!(result.record.title, "Fresh parsed title");
        assert!(result.record.deleted_at.is_none());
        assert_eq!(
            result.record.successful_output.as_ref().unwrap().format_id,
            "a"
        );
        assert_eq!(
            output.exists(),
            !missing,
            "a queued task has not started deleting"
        );
        assert_eq!(manager.task_for(row.id).unwrap().phase, "queued");
        {
            let state = manager.inner.lock().unwrap();
            let execution = state.entries[&result.record.request_id]
                .job
                .as_ref()
                .unwrap()
                .execution
                .as_ref()
                .unwrap();
            assert_eq!(execution.page.selected_format_id.as_deref(), Some("a"));
            assert_eq!(
                execution.page.download_directory,
                current.download_directory
            );
            let args = execution
                .ytdlp_command()
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.windows(2).any(|a| a[0] == "--format"
                && a[1] == r#"bestvideo[format_id="a"]+bestaudio/best*[format_id="a"]"#));
        }
        // Another format creates its own task while the first task keeps its captured parameters.
        current.selected_format_id = Some("b".into());
        current.selected_height = Some(1080);
        current.selected_fps = Some(59.94);
        current.download_directory = original.download_directory.clone();
        db.save_download_page_state(&current).unwrap();
        let again = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: current,
                restore_trashed: false,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        assert_eq!(again.kind, "accepted");
        assert_ne!(again.record.id, result.record.id);
        assert_eq!(again.record.format_id, "b");
        assert_eq!(again.record.height, Some(1080));
        {
            let state = manager.inner.lock().unwrap();
            let execution = state.entries[&result.record.request_id]
                .job
                .as_ref()
                .unwrap()
                .execution
                .as_ref()
                .unwrap();
            assert_eq!(execution.page.selected_format_id.as_deref(), Some("a"));
            assert_eq!(
                execution.page.download_directory,
                root.path().join("new").to_string_lossy()
            );
        }
        assert_eq!(
            db.find_format_download_record("youtube", "abc", "a")
                .unwrap()
                .unwrap()
                .id,
            row.id
        );
        manager.cancel(&again.record.request_id).unwrap();
        manager.cancel(&result.record.request_id).unwrap();
        manager.cancel(&a.record.request_id).unwrap();
        manager.cancel(&b.record.request_id).unwrap();
        until(|| !manager.is_active().unwrap()).await;
        assert_eq!(
            output.exists(),
            !missing,
            "cancelling in the queue preserves the old output"
        );
        // After releasing the player, a real native worker deletes first and then downloads.
        let a = enqueue(&manager, db.clone(), &old_dir, "c");
        let b = enqueue(&manager, db.clone(), &old_dir, "d");
        until(|| old_dir.join("c.started").exists() && old_dir.join("d.started").exists()).await;
        let mut retry_page = db
            .download_page_states()
            .unwrap()
            .into_iter()
            .find(|state| state.platform == "youtube")
            .unwrap();
        retry_page.selected_format_id = Some("a".into());
        retry_page.selected_height = Some(720);
        retry_page.selected_fps = Some(30.0);
        retry_page.download_directory = root.path().join("new").to_string_lossy().into();
        db.save_download_page_state(&retry_page).unwrap();
        let retry = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: retry_page,
                restore_trashed: false,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        assert_eq!(retry.record.id, row.id);
        assert_eq!(manager.task_for(row.id).unwrap().phase, "queued");
        assert_eq!(output.exists(), !missing);
        let new_output = root.path().join("new").join("retried.mp4");
        {
            let mut state = manager.inner.lock().unwrap();
            let execution = state
                .entries
                .get_mut(&retry.record.request_id)
                .unwrap()
                .job
                .as_mut()
                .unwrap()
                .execution
                .as_mut()
                .unwrap();
            execution.runner = super::super::task_snapshot::DownloadExecution::Ytdlp(
                super::super::tests::fixture("controlled", &new_output),
            );
            execution.command_options = None;
        }
        manager.cancel(&a.record.request_id).unwrap();
        until(|| new_output.with_extension("started").exists()).await;
        assert!(
            !output.exists(),
            "the native process starts only after deleting the old output"
        );
        std::fs::write(new_output.with_extension("release"), b"release").unwrap();
        until(|| manager.task_for(row.id).unwrap().phase == "completed").await;
        let completed = db.get_download_record(row.id).unwrap();
        assert_eq!(completed.error_code, None);
        assert_eq!(
            completed.output_path.as_deref(),
            Some(new_output.to_string_lossy().as_ref())
        );
        assert_eq!(completed.successful_output.as_ref().unwrap().format_id, "a");
        assert_eq!(completed.height, Some(720));
        assert_eq!(completed.fps, Some(30.0));
        assert_eq!(std::fs::read(&new_output).unwrap(), b"native task output");
        manager.cancel(&b.record.request_id).unwrap();
        until(|| !manager.is_active().unwrap()).await;

        // A destination that becomes invalid in the queue must not delete the successful output.
        let a = enqueue(&manager, db.clone(), &old_dir, "e");
        let b = enqueue(&manager, db.clone(), &old_dir, "f");
        until(|| old_dir.join("e.started").exists() && old_dir.join("f.started").exists()).await;
        let mut invalid = db
            .download_page_states()
            .unwrap()
            .into_iter()
            .find(|state| state.platform == "youtube")
            .unwrap();
        let unavailable = root.path().join("became-unavailable");
        invalid.download_directory = unavailable.to_string_lossy().into();
        db.save_download_page_state(&invalid).unwrap();
        let retry = commands::submit_from_page(
            None,
            SubmitDownloadRequest {
                snapshot: invalid,
                restore_trashed: false,
                redownload: true,
            },
            &manager,
            &tools,
            &cookies,
            &storage,
        )
            .await
            .unwrap();
        assert_eq!(manager.task_for(row.id).unwrap().phase, "queued");
        std::fs::remove_dir(&unavailable).unwrap();
        std::fs::write(&unavailable, b"directory became a file").unwrap();
        manager.cancel(&a.record.request_id).unwrap();
        until(|| manager.task_for(row.id).unwrap().phase == "failed").await;
        let failed = db.get_download_record(row.id).unwrap();
        assert_eq!(failed.request_id, retry.record.request_id);
        assert_eq!(
            failed.error_code.as_deref(),
            Some("downloadDirectoryFailed")
        );
        assert_eq!(failed.file_availability, "present");
        assert_eq!(std::fs::read(&new_output).unwrap(), b"native task output");
        manager.cancel(&b.record.request_id).unwrap();
        until(|| !manager.is_active().unwrap()).await;
    }
}

#[tokio::test]
async fn submitting_a_cleared_parse_returns_an_options_error_without_accepting_a_task() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary cleared parse submission directory: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let mut snapshot = page();
    snapshot.clear_result();
    snapshot.input_link.clear();
    snapshot.download_directory = root.path().to_string_lossy().into();
    db.save_download_page_state(&snapshot).unwrap();
    let manager = two_slot_manager();
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let failure = commands::submit_from_page(
        None,
        SubmitDownloadRequest {
            snapshot,
            restore_trashed: false,
            redownload: true,
        },
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.code, "invalidDownloadOptions");
    assert!(!manager.is_active().unwrap());
    assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 0);
}

#[tokio::test]
async fn missing_record_output_proceeds_to_a_real_native_download_on_the_same_record() {
    use crate::database::download_records::DownloadRecordOutcome;
    let root = tempfile::Builder::new()
        .prefix("evd-missing-record-retry-")
        .tempdir()
        .unwrap();
    eprintln!(
        "owned missing-output retry fixture: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let mut original = page();
    original.download_directory = root.path().to_string_lossy().into();
    let RecordAcceptance::Accepted(row) = db
        .accept_download_record(
            "removed-original",
            &DownloadSnapshot {
                page: original.clone(),
            },
            false,
            false,
        )
        .unwrap()
    else {
        panic!()
    };
    let output = root.path().join("saved.webm");
    std::fs::write(&output, b"original").unwrap();
    db.finish_download_record(
        "removed-original",
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    std::fs::remove_file(&output).unwrap();
    assert_eq!(
        db.get_download_record(row.id).unwrap().file_availability,
        "present",
        "the user deleted the file outside the app"
    );
    for (id, mut config) in settings().tools {
        for program in &mut config.programs {
            program.version = if id == RequiredToolId::Ytdlp {
                "2026.10.04".into()
            } else {
                "8.0.0".into()
            };
        }
        db.save_tool(id, &config).unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let manager = two_slot_manager();
    let a = enqueue(&manager, db.clone(), root.path(), "missing-block-a");
    let b = enqueue(&manager, db.clone(), root.path(), "missing-block-b");
    until(|| {
        root.path().join("missing-block-a.started").exists()
            && root.path().join("missing-block-b.started").exists()
    })
        .await;
    let accepted = commands::restart_from_record(
        None,
        row.id,
        "removed-original",
        RecordRestartMode::CheckOutput,
        &manager,
        &tools,
        &cookies,
        &storage,
    )
        .await
        .unwrap();
    assert_eq!(accepted.record.id, row.id);
    assert_eq!(accepted.task.unwrap().phase, "queued");
    {
        let mut inner = manager.inner.lock().unwrap();
        let execution = inner
            .entries
            .get_mut(&accepted.record.request_id)
            .unwrap()
            .job
            .as_mut()
            .unwrap()
            .execution
            .as_mut()
            .unwrap();
        assert_eq!(
            execution.page.selected_format_id,
            original.selected_format_id
        );
        // Replace only the external downloader boundary with the existing native child fixture.
        execution.runner = super::super::task_snapshot::DownloadExecution::Ytdlp(
            super::super::tests::fixture("controlled", &output),
        );
        execution.command_options = None;
    }
    manager.cancel(&a.record.request_id).unwrap();
    manager.cancel(&b.record.request_id).unwrap();
    until(|| output.with_extension("started").exists()).await;
    // This child has not emitted any transfer samples; do not fabricate a downloading phase.
    assert_eq!(db.get_download_record(row.id).unwrap().status, "running");
    assert!(manager
        .task_for(row.id)
        .unwrap()
        .record
        .error_code
        .is_none());
    std::fs::write(output.with_extension("release"), b"release").unwrap();
    until(|| !manager.is_active().unwrap()).await;
    let completed = db.get_download_record(row.id).unwrap();
    assert_eq!(completed.id, row.id);
    assert_eq!(completed.status, "completed");
    assert!(completed.error_code.is_none());
    assert!(output.is_file());
    assert!(
        completed.file_deleted_at.is_none(),
        "a missing file was not deleted again"
    );
}

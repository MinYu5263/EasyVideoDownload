use super::*;
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
        command: super::super::tests::fixture("controlled", &root.join(format!("{name}.mp4"))),
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
async fn two_native_slots_cancel_queue_and_complete_without_event_subscribers() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary native task registry directory: {}",
        root.path().display()
    );
    let db =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let manager = DownloadManager::default();
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
    let manager = DownloadManager::default();
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
    let manager = DownloadManager::default();
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
    let manager = DownloadManager::default();
    let cookies = CookieStore::new(root.path());
    cookies
        .save(CookiePlatform::Youtube, "current cookie")
        .unwrap();
    let tools = RequiredToolManager::new(storage.clone());
    let failure =
        commands::restart_from_record(None, row.id, &uuid, &manager, &tools, &cookies, &storage)
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
        let accepted = commands::restart_from_record(
            None, row.id, &uuid, &manager, &tools, &cookies, &storage,
        )
            .await
            .expect("record retry must use the same accepted task flow as the download page");
        assert_eq!(accepted.kind, "accepted");
        assert_eq!(accepted.task.unwrap().phase, "queued");
        until(|| !manager.is_active().unwrap()).await;
        let failed = manager.task_for(row.id).unwrap();
        assert_eq!(failed.phase, "failed");
        assert_eq!(
            failed.record.error_code.as_deref(),
            Some("historyFileOccupied")
        );
        assert_eq!(db.get_download_record(row.id).unwrap().status, "failed");
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
        output.exists(),
        "queued record retries must preserve the old output"
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
    let again =
        commands::restart_from_record(None, row.id, &uuid, &manager, &tools, &cookies, &storage)
            .await
            .unwrap();
    assert_eq!(again.kind, "existing");
    assert_eq!(again.record.request_id, result.record.request_id);
    manager.cancel(&result.record.request_id).unwrap();
    manager.cancel(&a.record.request_id).unwrap();
    manager.cancel(&b.record.request_id).unwrap();
    until(|| !manager.is_active().unwrap()).await;
    assert_eq!(db.get_download_record(row.id).unwrap().status, "cancelled");
    assert!(output.exists());
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
        let manager = DownloadManager::default();
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
            assert_eq!(retained.successful_output.as_ref().unwrap().format_id, "b");
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
            "b"
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
                .command
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.windows(2).any(|a| a[0] == "--format"
                && a[1] == r#"bestvideo[format_id="a"]+bestaudio/best*[format_id="a"]"#));
        }
        // Later page edits cannot rewrite the queued command or create a second active task.
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
        assert_eq!(again.kind, "existing");
        assert_eq!(again.record.request_id, result.record.request_id);
        assert_eq!(again.record.height, Some(720));
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
            db.find_download_record("youtube", "abc")
                .unwrap()
                .unwrap()
                .id,
            row.id
        );
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
            execution.command = super::super::tests::fixture("controlled", &new_output);
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
    let manager = DownloadManager::default();
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
async fn record_output_proceeds_to_a_real_native_download_on_the_same_record() {
    let cases: &[bool] = if cfg!(any(windows, target_os = "macos")) {
        &[true, false]
    } else {
        &[true]
    };
    for &missing in cases {
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
        if missing {
            std::fs::remove_file(&output).unwrap();
        }
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
        let manager = DownloadManager::default();
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
            execution.command = super::super::tests::fixture("controlled", &output);
        }
        manager.cancel(&a.record.request_id).unwrap();
        manager.cancel(&b.record.request_id).unwrap();
        until(|| output.with_extension("started").exists()).await;
        assert!(
            !output.exists(),
            "the old output must be gone before the native downloader starts"
        );
        assert_eq!(
            db.get_download_record(row.id)
                .unwrap()
                .file_deleted_at
                .is_some(),
            !missing
        );
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
            "a newly completed output clears the old deletion marker"
        );
    }
}

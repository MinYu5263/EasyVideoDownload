use super::*;

#[test]
fn a_missing_only_restart_never_removes_a_reappeared_successful_output() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned reappeared output fixture: {}", root.path().display());
    let database =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut state = crate::database::persistence_tests::page();
    state.download_directory = root.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    let original_id = uuid::Uuid::new_v4().to_string();
    let (_, session) = DownloadRecordSession::accept(
        database.clone(),
        root.path(),
        original_id.clone(),
        snapshot.clone(),
        None,
        false,
        false,
    )
        .unwrap();
    let output = root.path().join("original.webm");
    std::fs::write(&output, b"original").unwrap();
    database
        .finish_download_record(
            &original_id,
            &crate::database::download_records::DownloadRecordOutcome::Completed {
                path: output.to_string_lossy().into(),
                size: 8,
                extension: Some("webm".into()),
            },
        )
        .unwrap();
    drop(session);
    let previous = database
        .find_download_record("youtube", "abc")
        .unwrap()
        .unwrap();
    let result = DownloadRecordSession::restart(
        database.clone(),
        root.path(),
        uuid::Uuid::new_v4().to_string(),
        RestartTarget {
            record: &previous,
            restore_trashed: false,
            output_policy: RestartOutputPolicy::RequireMissing,
        },
        snapshot,
        None,
    );
    assert!(
        result.is_err(),
        "an unconfirmed missing-only retry must stop when the successful output exists again"
    );
    assert_eq!(
        database.get_download_record(previous.id).unwrap().status,
        "completed"
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"original");
}
use crate::database::persistence_tests::page;
use std::time::{Duration, Instant};

#[test]
fn recovery_keeps_persisted_pause_intent_after_the_worker_lock_is_released() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned paused recovery fixture: {}", root.path().display());
    let database_path = root.path().join("app.db");
    let legacy = root.path().join("legacy");
    let db = Arc::new(Database::open(&database_path, &legacy).unwrap());
    let id = uuid::Uuid::new_v4().to_string();
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db.clone(),
        root.path(),
        id.clone(),
        DownloadSnapshot { page: state },
        None,
    )
        .unwrap();
    db.set_download_paused(&id, true).unwrap();
    recover(&db, root.path()).unwrap();
    assert_eq!(
        db.find_request_record(&id).unwrap().unwrap().status,
        "running"
    );
    let live_paused = db
        .query_download_records(&HistoryQuery {
            cursor: None,
            limit: 10,
            query: String::new(),
            status: Some("paused".into()),
        })
        .unwrap();
    assert_eq!(live_paused.records.len(), 1);
    assert_eq!(live_paused.status_counts["paused"], 1);
    assert_eq!(live_paused.status_counts["running"], 0);
    db.set_download_paused(&id, false).unwrap();
    let resumed = db
        .query_download_records(&HistoryQuery {
            cursor: None,
            limit: 10,
            query: String::new(),
            status: Some("running".into()),
        })
        .unwrap();
    assert_eq!(resumed.records.len(), 1);
    assert_eq!(resumed.status_counts["running"], 1);
    assert_eq!(resumed.status_counts["paused"], 0);
    db.set_download_paused(&id, true).unwrap();
    drop(session);
    let reopened = Database::open(&database_path, &legacy).unwrap();
    recover(&reopened, root.path()).unwrap();
    recover(&reopened, root.path()).unwrap();
    let record = reopened.find_request_record(&id).unwrap().unwrap();
    assert_eq!(record.status, "paused");
    assert!(record.finished_at.is_none());
    assert!(record.error_code.is_none());
    let filtered = reopened
        .query_download_records(&HistoryQuery {
            cursor: None,
            limit: 10,
            query: String::new(),
            status: Some("paused".into()),
        })
        .unwrap();
    assert_eq!(filtered.records.len(), 1);
    assert_eq!(filtered.status_counts["paused"], 1);
}

#[test]
#[ignore = "native lock holder fixture"]
fn lock_holder_fixture() {
    let root = PathBuf::from(std::env::var_os("EVD_LOCK_ROOT").unwrap());
    let id = std::env::var("EVD_LOCK_ID").unwrap();
    let _lock = RequestLock::acquire(&root, &id, false).unwrap().unwrap();
    std::fs::write(root.join("ready"), b"locked").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
fn recovery_respects_another_process_and_recovers_after_it_exits() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!("temporary lock test directory: {}", dir.path().display());
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    db.begin_download_record(
        &id,
        &DownloadSnapshot { page: state },
        &crate::datetime::now(),
    )
        .unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "video::download::history::tests::lock_holder_fixture",
        ])
        .env("EVD_LOCK_ROOT", dir.path())
        .env("EVD_LOCK_ID", &id)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("ready").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let ready = dir.path().join("ready").exists();
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("native child did not obtain its lock");
    }
    recover(&db, dir.path()).unwrap();
    let active = db.list_download_records(None, 10).unwrap().records[0]
        .status
        .clone();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(active, "running");
    recover(&db, dir.path()).unwrap();
    recover(&db, dir.path()).unwrap();
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records[0].status, "interrupted");
    assert!(records[0].finished_at.is_some());
}

#[tokio::test]
async fn accepted_task_is_recorded_before_any_downloader_output() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary accepted task directory: {}",
        dir.path().display()
    );
    let db =
        Arc::new(Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap());
    let mut snapshot = page();
    snapshot.download_directory = dir.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page: snapshot },
        None,
    )
        .unwrap();
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(
        records.len(),
        1,
        "accepted tasks must exist before preparation starts"
    );
    assert_eq!(records[0].status, "running");
    assert!(session
        .finish(&Err(super::super::super::error(
            "toolMissing",
            "yt-dlp unavailable"
        )))
        .await
        .is_none());
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records[0].status, "failed");
    assert_eq!(records[0].error_code.as_deref(), Some("toolMissing"));
}

#[tokio::test]
async fn finalized_session_rejects_late_activity() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary finalized session directory: {}",
        dir.path().display()
    );
    let db =
        Arc::new(Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap());
    let mut page = page();
    page.download_directory = dir.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page },
        None,
    )
        .unwrap();
    session
        .finish(&Err(super::super::super::error("downloadCancelled", "")))
        .await;
    session
        .observe(r#"__EVD_PROGRESS__{"progress":{"status":"downloading"}}"#)
        .await;
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, "cancelled");
}

#[tokio::test]
async fn settlement_waits_for_a_write_whose_reader_was_dropped() {
    use std::{future::Future, sync::mpsc, task::Poll};
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary pending observation directory: {}",
        dir.path().display()
    );
    let db =
        Arc::new(Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap());
    let mut page = page();
    page.download_directory = dir.path().to_string_lossy().into();
    let session = DownloadRecordSession::new(
        db.clone(),
        dir.path(),
        uuid::Uuid::new_v4().to_string(),
        DownloadSnapshot { page },
        None,
    )
        .unwrap();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let inner = session.0.clone();
    let holder = std::thread::spawn(move || {
        let _state = inner.state.lock().unwrap();
        held_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    held_rx.recv().unwrap();
    let mut observation =
        Box::pin(session.observe(r#"__EVD_PROGRESS__{"progress":{"status":"downloading"}}"#));
    std::future::poll_fn(|cx| {
        assert!(observation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
        .await;
    drop(observation);
    let ending = DownloadRecordSession(session.0.clone());
    let finish = tokio::spawn(async move {
        ending
            .finish(&Err(super::super::super::error("downloadCancelled", "")))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !session.0.observations.lock().unwrap().closed {
            tokio::task::yield_now().await;
        }
    })
        .await
        .unwrap();
    assert!(!finish.is_finished());
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(finish.await.unwrap().is_none());
    let records = db.list_download_records(None, 10).unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, "cancelled");
}

#[test]
fn queued_task_is_not_recovered_until_its_runtime_lock_is_released() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary queued recovery directory: {}",
        dir.path().display()
    );
    let db =
        Arc::new(Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap());
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let id = uuid::Uuid::new_v4().to_string();
    let (_, session) = DownloadRecordSession::accept(
        db.clone(),
        dir.path(),
        id.clone(),
        DownloadSnapshot { page: state },
        None,
        false,
        false,
    )
        .unwrap();
    recover(&db, dir.path()).unwrap();
    assert_eq!(
        db.find_request_record(&id).unwrap().unwrap().status,
        "queued"
    );
    drop(session);
    recover(&db, dir.path()).unwrap();
    assert_eq!(
        db.find_request_record(&id).unwrap().unwrap().status,
        "interrupted"
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[tokio::test]
async fn cancellation_while_restart_deletion_waits_for_database_preserves_the_output() {
    use crate::database::download_records::{tasks::RecordAcceptance, DownloadRecordOutcome};
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary deferred cancellation directory: {}",
        root.path().display()
    );
    let database =
        Arc::new(Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap());
    let mut page = page();
    page.download_directory = root.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page };
    let original_id = uuid::Uuid::new_v4().to_string();
    let RecordAcceptance::Accepted(original) = database
        .accept_download_record(&original_id, &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    let output = root.path().join("old.webm");
    std::fs::write(&output, b"original").unwrap();
    database
        .finish_download_record(
            &original_id,
            &DownloadRecordOutcome::Completed {
                path: output.to_string_lossy().into(),
                size: 8,
                extension: Some("webm".into()),
            },
        )
        .unwrap();
    let previous = database.get_download_record(original.id).unwrap();
    let retry_id = uuid::Uuid::new_v4().to_string();
    let (_, history) = DownloadRecordSession::restart(
        database.clone(),
        root.path(),
        retry_id.clone(),
        RestartTarget {
            record: &previous,
            restore_trashed: false,
            output_policy: RestartOutputPolicy::Deferred,
        },
        snapshot,
        None,
    )
        .unwrap();
    let history = history.unwrap();
    database.mark_download_running(&retry_id).unwrap();
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let guard = rusqlite::Connection::open(root.path().join("app.db")).unwrap();
    guard.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let mut preparation =
        tokio::spawn(async move { history.delete_previous_output(previous, receiver).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut preparation)
            .await
            .is_err()
    );
    cancel.send_replace(true);
    guard.execute_batch("ROLLBACK;").unwrap();
    drop(guard);
    let result = preparation.await.unwrap();
    assert_eq!(
        std::fs::read(&output).unwrap(),
        b"original",
        "cancellation after waiting for the database must prevent deletion"
    );
    assert_eq!(result.unwrap_err().code, "downloadCancelled");
    assert_eq!(
        database
            .get_download_record(original.id)
            .unwrap()
            .file_availability,
        "present"
    );
}

use super::*;
use crate::database::persistence_tests::page;

#[test]
fn duplicate_submissions_and_stale_completion_cannot_replace_current_attempt() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary unique task test directory: {}",
        dir.path().display()
    );
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    assert!(db.find_download_record("youtube", "abc").unwrap().is_none());
    let RecordAcceptance::Accepted(first) = db
        .accept_download_record("first", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(first.status, "queued");
    let RecordAcceptance::Existing(existing) = db
        .accept_download_record("duplicate", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(existing.request_id, "first");
    assert!(db.delete_download_record(first.id).is_err());
    db.finish_download_record("first", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    let RecordAcceptance::Accepted(retry) = db
        .accept_download_record("retry", &snapshot, false, true)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(first.id, retry.id);
    db.finish_download_record("first", &DownloadRecordOutcome::Interrupted)
        .unwrap();
    assert_eq!(
        db.get_download_record(first.id).unwrap().request_id,
        "retry"
    );
    assert_eq!(db.get_download_record(first.id).unwrap().status, "queued");
    db.finish_download_record("retry", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    db.delete_download_record(first.id).unwrap();
    assert_eq!(
        db.accept_download_record("restore", &snapshot, false, true)
            .err()
            .unwrap()
            .code,
        "recordTrashed"
    );
    let RecordAcceptance::Accepted(restored) = db
        .accept_download_record("restore", &snapshot, true, true)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(first.id, restored.id);
}

#[test]
fn migration_archives_duplicates_preserves_files_and_id_high_water() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary unique migration directory: {}",
        dir.path().display()
    );
    let path = dir.path().join("app.db");
    let c = Connection::open(&path).unwrap();
    for migration in [
        include_str!("../../../../migrations/001_settings.sql"),
        include_str!("../../../../migrations/002_settings_key_value.sql"),
    ] {
        c.execute_batch(migration).unwrap();
    }
    for migration in [
        include_str!("../../../../migrations/004_automatic_ytdlp.sql"),
        include_str!("../../../../migrations/005_persistence.sql"),
        include_str!("../../../../migrations/006_automatic_tools.sql"),
        include_str!("../../../../migrations/007_single_input_link.sql"),
        include_str!("../../../../migrations/008_download_history_cards.sql"),
        include_str!("../../../../migrations/009_download_history_trash.sql"),
    ] {
        c.execute_batch(migration).unwrap();
    }
    let output = dir.path().join("old.mp4");
    std::fs::write(&output, b"original video").unwrap();
    c.execute("INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,finished_at,output_path,file_size_bytes) VALUES(4,'successful','youtube','abc','https://youtu.be/abc','Old','a',?1,'completed','2026-10-04 10:00:00','2026-10-04 10:00:01',?2,14)", params![dir.path().to_str(),output.to_str()]).unwrap();
    c.execute("INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,finished_at,error_code) VALUES(99,'later-failure','youtube','abc','https://youtu.be/abc','Failed','a',?1,'failed','2026-10-04 11:00:00','2026-10-04 11:00:01','network')", params![dir.path().to_str()]).unwrap();
    c.execute("INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,finished_at,output_path,file_size_bytes,deleted_at) VALUES(100,'trash-success','youtube','abc','https://youtu.be/abc','Trash','a',?1,'completed','2026-10-04 12:00:00','2026-10-04 12:00:01',?2,14,'2026-10-04 12:01:00')", params![dir.path().to_str(),output.to_str()]).unwrap();
    c.pragma_update(None, "user_version", 9).unwrap();
    drop(c);
    let db = Database::open(&path, &dir.path().join("legacy")).unwrap();
    assert_eq!(
        db.find_download_record("youtube", "abc")
            .unwrap()
            .unwrap()
            .id,
        4
    );
    assert_eq!(std::fs::read(output).unwrap(), b"original video");
    assert_eq!(
        db.connection("test")
            .unwrap()
            .query_row(
                "SELECT count(*) FROM download_record_migration_archive",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    let mut state = page();
    state.video_id = Some("new".into());
    state.input_link = "https://youtu.be/new".into();
    state.download_directory = dir.path().to_string_lossy().into();
    let RecordAcceptance::Accepted(new) = db
        .accept_download_record("new", &DownloadSnapshot { page: state }, false, false)
        .unwrap()
    else {
        panic!()
    };
    assert!(new.id > 100);
}

#[test]
fn unrelated_task_directory_does_not_block_history_file_actions() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary unrelated task directory: {}",
        dir.path().display()
    );
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let mut first = page();
    first.download_directory = dir.path().join("finished").to_string_lossy().into();
    let RecordAcceptance::Accepted(row) = db
        .accept_download_record("first", &DownloadSnapshot { page: first }, false, false)
        .unwrap()
    else {
        panic!()
    };
    db.finish_download_record(
        "first",
        &DownloadRecordOutcome::Completed {
            path: dir.path().join("finished/old.mp4").to_string_lossy().into(),
            size: 5,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    let mut other = page();
    other.video_id = Some("other".into());
    other.input_link = "https://youtu.be/other".into();
    other.download_directory = dir.path().join("active").to_string_lossy().into();
    db.accept_download_record("other", &DownloadSnapshot { page: other }, false, false)
        .unwrap();
    assert!(db
        .recycle_download_record_file(row.id, |_| Ok(false))
        .is_ok());
}

#[test]
fn readonly_lookup_and_same_video_identity_are_platform_scoped() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!("temporary scoped video directory: {}", dir.path().display());
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    assert!(db.find_download_record("youtube", "abc").unwrap().is_none());
    assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 0);
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let RecordAcceptance::Accepted(first) = db
        .accept_download_record(
            "youtube",
            &DownloadSnapshot {
                page: state.clone(),
            },
            false,
            false,
        )
        .unwrap()
    else {
        panic!()
    };
    state.platform = "douyin".into();
    state.input_link = "https://douyin.com/video/abc".into();
    let RecordAcceptance::Accepted(second) = db
        .accept_download_record("douyin", &DownloadSnapshot { page: state }, false, false)
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(first.id, second.id);
    assert_eq!(
        db.delete_download_record(first.id).unwrap_err().code,
        "recordRunning"
    );
    let query = HistoryQuery {
        status: Some("running".into()),
        cursor: None,
        limit: 50,
        query: String::new(),
    };
    let result = db.query_download_records(&query).unwrap();
    assert_eq!(result.records.len(), 2);
    assert_eq!(result.matched_count, 2);
}
#[test]
fn migration_failure_rolls_back_version_records_and_archive() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary migration rollback directory: {}",
        dir.path().display()
    );
    let path = dir.path().join("app.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(concat!(
        include_str!("../../../../migrations/001_settings.sql"),
        include_str!("../../../../migrations/002_settings_key_value.sql"),
        include_str!("../../../../migrations/003_beijing_datetime.sql"),
        include_str!("../../../../migrations/004_automatic_ytdlp.sql"),
        include_str!("../../../../migrations/005_persistence.sql"),
        include_str!("../../../../migrations/006_automatic_tools.sql"),
        include_str!("../../../../migrations/007_single_input_link.sql"),
        include_str!("../../../../migrations/008_download_history_cards.sql"),
        include_str!("../../../../migrations/009_download_history_trash.sql"),
        "PRAGMA user_version=9;"
        ))
        .unwrap();
    connection.execute_batch("INSERT INTO download_records(request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at) VALUES('a','youtube','abc','https://youtu.be/abc','A','a','C:/Videos','running','2026-10-04 10:00:00'),('b','youtube','abc','https://youtu.be/abc','B','a','C:/Videos','running','2026-10-04 11:00:00'); CREATE TABLE download_record_migration_archive AS SELECT * FROM download_records WHERE 0;CREATE TRIGGER reject_archive BEFORE INSERT ON download_record_migration_archive BEGIN SELECT RAISE(ABORT,'migration failure'); END;").unwrap();
    drop(connection);
    assert!(Database::open(&path, &dir.path().join("legacy")).is_err());
    let old = Connection::open(path).unwrap();
    assert_eq!(
        old.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        old.query_row("SELECT count(*) FROM download_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        old.query_row(
            "SELECT count(*) FROM download_record_migration_archive",
            [],
            |r| r.get::<_, i64>(0)
        )
            .unwrap(),
        0
    );
}

#[test]
fn missing_successful_output_can_retry_without_creating_a_second_row() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary missing retry directory: {}",
        dir.path().display()
    );
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let mut page = page();
    page.download_directory = dir.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page };
    let RecordAcceptance::Accepted(first) = db
        .accept_download_record("first", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    db.finish_download_record(
        "first",
        &DownloadRecordOutcome::Completed {
            path: dir.path().join("file.webm").to_string_lossy().into(),
            size: 12,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    assert!(matches!(
        db.accept_download_record("duplicate", &snapshot, false, false)
            .unwrap(),
        RecordAcceptance::AlreadyDownloaded(_)
    ));
    db.mark_download_file_availability(first.id, "missing")
        .unwrap();
    let RecordAcceptance::Accepted(retry) = db
        .accept_download_record("retry", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(retry.id, first.id);
    assert!(retry.successful_output.is_some());
}

#[test]
#[cfg(windows)]
fn review_recycle_protects_an_alternate_case_shared_output() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary shared recycle output directory: {}",
        root.path().display()
    );
    let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let output = root.path().join("video.webm");
    std::fs::write(&output, b"video").unwrap();
    let mut ids = Vec::new();
    for (video, path) in [
        ("first", output.to_string_lossy().into_owned()),
        ("second", output.to_string_lossy().to_uppercase()),
    ] {
        let mut state = page();
        state.video_id = Some(video.into());
        state.input_link = format!("https://youtu.be/{video}");
        state.download_directory = root.path().to_string_lossy().into();
        let id = db
            .begin_download_record(
                video,
                &DownloadSnapshot { page: state },
                "2026-10-04 10:00:00",
            )
            .unwrap();
        db.finish_download_record(
            video,
            &DownloadRecordOutcome::Completed {
                path,
                size: 5,
                extension: Some("webm".into()),
            },
        )
            .unwrap();
        ids.push(id);
    }
    let called = std::cell::Cell::new(false);
    let result = db.recycle_download_record_file(ids[0], |_| {
        called.set(true);
        Ok(false)
    });
    assert_eq!(result.unwrap_err().code, "historyFileInUse");
    assert!(!called.get());
    assert!(output.exists());
}

#[cfg(windows)]
fn restart_fixture() -> (
    tempfile::TempDir,
    Database,
    DownloadSnapshot,
    DownloadRecord,
) {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary direct redownload directory: {}",
        dir.path().display()
    );
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    let RecordAcceptance::Accepted(row) = db
        .accept_download_record("original", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    let output = dir.path().join("video.webm");
    std::fs::write(&output, b"original").unwrap();
    db.finish_download_record(
        "original",
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    let record = db.get_download_record(row.id).unwrap();
    (dir, db, snapshot, record)
}

#[cfg(windows)]
#[test]
fn restart_deletes_original_before_acceptance_and_reuses_the_record() {
    let (_dir, db, snapshot, original) = restart_fixture();
    let RecordAcceptance::Accepted(row) = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(row.id, original.id);
    assert_eq!(row.status, "queued");
    assert_eq!(row.file_availability, "missing");
    assert!(!Path::new(original.output_path.as_ref().unwrap()).exists());
    let RecordAcceptance::Existing(existing) = db
        .restart_download_record(
            "duplicate",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |_, _| panic!("active retry must not delete twice"),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(existing.request_id, "restart");
    assert_eq!(db.list_download_records(None, 50).unwrap().records.len(), 1);
}

#[cfg(windows)]
#[test]
fn restart_refuses_shared_changed_trashed_and_stale_outputs() {
    let (_dir, db, mut snapshot, original) = restart_fixture();
    snapshot.page.video_id = Some("other".into());
    let RecordAcceptance::Accepted(other) = db
        .accept_download_record("other", &snapshot, false, false)
        .unwrap()
    else {
        panic!()
    };
    db.finish_download_record(
        "other",
        &DownloadRecordOutcome::Completed {
            path: original.output_path.clone().unwrap(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    snapshot.page.video_id = Some(original.video_id.clone());
    let err = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .err()
        .unwrap();
    assert_eq!(err.code, "historyFileInUse");
    assert!(Path::new(original.output_path.as_ref().unwrap()).exists());
    db.connection("test")
        .unwrap()
        .execute("DELETE FROM download_records WHERE id=?1", [other.id])
        .unwrap();
    let err = db
        .restart_download_record("restart", original.id, "stale", &snapshot, false, |_, _| {
            panic!("stale identity must not delete")
        })
        .err()
        .unwrap();
    assert_eq!(err.code, "historyFileChanged");
    std::fs::write(original.output_path.as_ref().unwrap(), b"changed contents").unwrap();
    let err = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .err()
        .unwrap();
    assert_eq!(err.code, "historyFileChanged");
    db.delete_download_record(original.id).unwrap();
    let err = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |_, _| panic!("trash must not delete"),
        )
        .err()
        .unwrap();
    assert_eq!(err.code, "recordTrashed");
}

#[cfg(windows)]
#[test]
fn restart_reports_file_deleted_save_failure_and_can_retry_a_missing_output() {
    let (_dir, db, snapshot, original) = restart_fixture();
    db.connection("test").unwrap().execute_batch("CREATE TRIGGER reject_restart BEFORE UPDATE ON download_records WHEN NEW.request_id='restart' BEGIN SELECT RAISE(ABORT,'write failed'); END;").unwrap();
    let err = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .err()
        .unwrap();
    assert_eq!(err.code, "historyFileDeletedSaveFailed");
    assert!(!Path::new(original.output_path.as_ref().unwrap()).exists());
    assert_eq!(
        db.get_download_record(original.id).unwrap().status,
        "completed"
    );
    db.connection("test")
        .unwrap()
        .execute_batch("DROP TRIGGER reject_restart;")
        .unwrap();
    let RecordAcceptance::Accepted(row) = db
        .restart_download_record(
            "restart",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(row.id, original.id);
    assert_eq!(row.file_availability, "missing");
}

#[cfg(windows)]
#[test]
fn restart_rejects_a_different_file_with_the_same_size() {
    let (_dir, db, snapshot, row) = restart_fixture();
    let path = Path::new(row.output_path.as_ref().unwrap());
    std::fs::remove_file(path).unwrap();
    std::fs::write(path, b"replaced").unwrap();
    let err = db
        .restart_download_record(
            "retry",
            row.id,
            &row.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .err()
        .expect("replacement must be refused");
    assert_eq!(err.code, "historyFileChanged");
    assert!(path.exists());
    assert_eq!(
        db.get_download_record(row.id).unwrap().request_id,
        row.request_id
    );
}

#[cfg(windows)]
#[test]
fn identity_migration_preserves_legacy_records_and_allows_confirmed_restart() {
    let (dir, db, snapshot, original) = restart_fixture();
    db.connection("test")
        .unwrap()
        .execute_batch(
            "ALTER TABLE download_records DROP COLUMN output_identity; PRAGMA user_version=10;",
        )
        .unwrap();
    drop(db);
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
    let row = db.get_download_record(original.id).unwrap();
    assert_eq!(row.request_id, original.request_id);
    assert!(row.output_identity.is_none());
    assert!(Path::new(row.output_path.as_ref().unwrap()).exists());
    assert_eq!(
        db.connection("test")
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    let RecordAcceptance::Accepted(retry) = db
        .restart_download_record(
            "retry",
            row.id,
            &row.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(retry.id, row.id);
    assert!(!Path::new(row.output_path.as_ref().unwrap()).exists());
}

#[cfg(windows)]
#[test]
fn restart_refuses_an_active_attempt_referencing_its_output_before_deletion() {
    let (_dir, db, mut snapshot, original) = restart_fixture();
    snapshot.page.video_id = Some("other".into());
    db.accept_download_record("other", &snapshot, false, false)
        .unwrap();
    db.finish_download_record(
        "other",
        &DownloadRecordOutcome::Completed {
            path: original.output_path.clone().unwrap(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    db.accept_download_record("other-active", &snapshot, false, true)
        .unwrap();
    snapshot.page.video_id = Some(original.video_id.clone());
    let err = db
        .restart_download_record(
            "retry",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |_, _| panic!("conflicting active output must be refused before deletion"),
        )
        .err()
        .unwrap();
    assert_eq!(err.code, "historyBusy");
    assert!(Path::new(original.output_path.as_ref().unwrap()).exists());
}

#[cfg(windows)]
#[test]
fn restart_from_new_parse_updates_metadata_and_options_without_changing_successful_specs() {
    let (_dir, db, mut snapshot, original) = restart_fixture();
    snapshot.page.title = Some("Refreshed title".into());
    snapshot.page.input_link = "https://youtu.be/abc?token=secret&feature=share".into();
    snapshot.page.formats[0].height = Some(720);
    snapshot.page.formats[0].fps = Some(30.0);
    snapshot.page.selected_format_id = Some("a".into());
    snapshot.page.selected_height = Some(720);
    snapshot.page.selected_fps = Some(30.0);
    let RecordAcceptance::Accepted(row) = db
        .restart_download_record(
            "new-parse",
            original.id,
            &original.request_id,
            &snapshot,
            false,
            |row, protected| {
                crate::video::download::history::permanent::delete_output_file(row, protected)
                    .map(Some)
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(row.id, original.id);
    assert_eq!(row.title, "Refreshed title");
    assert!(!row.source_link.contains("secret"));
    assert_eq!(row.height, Some(720));
    assert_eq!(row.fps, Some(30.0));
    assert_eq!(row.format_id, "a");
    assert_eq!(row.successful_output.as_ref().unwrap().format_id, "b");
    assert_eq!(row.successful_output.as_ref().unwrap().height, Some(1080));
    assert_eq!(row.successful_output.as_ref().unwrap().fps, Some(59.94));
    assert_eq!(row.output_path, original.output_path);
    assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 1);
}

#[cfg(windows)]
#[test]
fn deferred_restart_rechecks_files_and_references_before_deletion() {
    for (case, expected) in [
        ("new-reference", "historyFileInUse"),
        ("new-active-reference", "historyBusy"),
        ("replaced-file", "historyFileChanged"),
        ("stale-request", "historyFileChanged"),
    ] {
        let (_dir, db, snapshot, original) = restart_fixture();
        let RecordAcceptance::Accepted(queued) = db
            .restart_download_record(
                "queued-retry",
                original.id,
                &original.request_id,
                &snapshot,
                false,
                |_, _| Ok(None),
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(queued.file_availability, "present");
        assert!(queued.file_deleted_at.is_none());
        assert_eq!(
            std::fs::read(original.output_path.as_ref().unwrap()).unwrap(),
            b"original"
        );
        db.mark_download_running("queued-retry").unwrap();
        if case.starts_with("new-") {
            let mut other = snapshot.clone();
            other.page.video_id = Some("other-reference".into());
            db.accept_download_record("other-reference", &other, false, false)
                .unwrap();
            db.finish_download_record(
                "other-reference",
                &DownloadRecordOutcome::Completed {
                    path: original.output_path.clone().unwrap(),
                    size: 8,
                    extension: Some("webm".into()),
                },
            )
                .unwrap();
            if case == "new-active-reference" {
                db.accept_download_record("active-reference", &other, false, true)
                    .unwrap();
            }
        } else if case == "replaced-file" {
            std::fs::remove_file(original.output_path.as_ref().unwrap()).unwrap();
            std::fs::write(original.output_path.as_ref().unwrap(), b"replaced").unwrap();
        }
        let request = if case == "stale-request" {
            "old-request"
        } else {
            "queued-retry"
        };
        let failure = db
            .prepare_restart_output(
                request,
                &original,
                crate::video::download::history::permanent::delete_output_file,
            )
            .unwrap_err();
        assert_eq!(failure.code, expected, "{case}");
        let retained = db.get_download_record(original.id).unwrap();
        assert_eq!(retained.request_id, "queued-retry");
        assert_eq!(retained.file_availability, "present");
        assert!(retained.file_deleted_at.is_none());
        assert!(Path::new(original.output_path.as_ref().unwrap()).exists());
    }
}

#[cfg(windows)]
#[test]
fn deferred_restart_reports_post_deletion_write_failure_and_can_resume_missing_output() {
    let (_dir, db, snapshot, original) = restart_fixture();
    db.restart_download_record(
        "queued-retry",
        original.id,
        &original.request_id,
        &snapshot,
        false,
        |_, _| Ok(None),
    )
        .unwrap();
    db.mark_download_running("queued-retry").unwrap();
    db.connection("test")
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_output_removal BEFORE UPDATE ON download_records
         WHEN NEW.file_availability='missing' BEGIN SELECT RAISE(ABORT,'write failed'); END;",
        )
        .unwrap();
    let failure = db
        .prepare_restart_output(
            "queued-retry",
            &original,
            crate::video::download::history::permanent::delete_output_file,
        )
        .unwrap_err();
    assert_eq!(failure.code, "historyFileDeletedSaveFailed");
    assert!(!Path::new(original.output_path.as_ref().unwrap()).exists());
    assert_eq!(
        db.get_download_record(original.id)
            .unwrap()
            .file_availability,
        "present"
    );
    db.connection("test")
        .unwrap()
        .execute_batch("DROP TRIGGER reject_output_removal;")
        .unwrap();
    db.prepare_restart_output(
        "queued-retry",
        &original,
        crate::video::download::history::permanent::delete_output_file,
    )
        .unwrap();
    assert_eq!(
        db.get_download_record(original.id)
            .unwrap()
            .file_availability,
        "missing"
    );
}

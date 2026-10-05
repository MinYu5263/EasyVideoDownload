use super::page_states::DownloadPageState;
use super::*;

#[test]
fn version_twelve_format_upgrade_keeps_history_files_snapshots_and_id_high_water() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned schema twelve format migration directory: {}",
        root.path().display()
    );
    let path = root.path().join("app.db");
    let connection = Connection::open(&path).unwrap();
    for migration in [
        include_str!("../../migrations/001_settings.sql"),
        include_str!("../../migrations/002_settings_key_value.sql"),
        include_str!("../../migrations/003_beijing_datetime.sql"),
        include_str!("../../migrations/004_automatic_ytdlp.sql"),
        include_str!("../../migrations/005_persistence.sql"),
        include_str!("../../migrations/006_automatic_tools.sql"),
        include_str!("../../migrations/007_single_input_link.sql"),
        include_str!("../../migrations/008_download_history_cards.sql"),
        include_str!("../../migrations/009_download_history_trash.sql"),
        include_str!("../../migrations/010_shared_download_tasks.sql"),
        include_str!("../../migrations/011_output_identity.sql"),
        include_str!("../../migrations/012_download_format_snapshot.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    let format = state
        .formats
        .iter()
        .find(|f| Some(&f.format_id) == state.selected_format_id.as_ref())
        .unwrap();
    let snapshot = serde_json::to_string(format).unwrap();
    let output = root.path().join("retained.webm");
    std::fs::write(&output, b"retained media").unwrap();
    connection.execute("INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,finished_at,output_path,file_size_bytes,format_snapshot_json,successful_format_snapshot_json,output_identity,successful_format_id,successful_directory,successful_finished_at) VALUES(41,'retained','youtube','abc','https://youtu.be/abc','Retained',?1,?2,'completed','2026-10-04 10:00:00','2026-10-04 10:01:00',?3,14,?4,?4,'retained-identity',?1,?2,'2026-10-04 10:01:00')", params![format.format_id,state.download_directory,output.to_str(),snapshot]).unwrap();
    connection.execute_batch("UPDATE sqlite_sequence SET seq=128 WHERE name='download_records'; PRAGMA user_version=12;").unwrap();
    drop(connection);
    let db = Database::open(&path, &root.path().join("legacy")).unwrap();
    let retained = db.get_download_record(41).unwrap();
    assert_eq!(retained.request_id, "retained");
    assert_eq!(
        retained.output_identity.as_deref(),
        Some("retained-identity")
    );
    let mut normalized = format.clone();
    crate::video::formats::normalize(&mut normalized);
    assert_eq!(retained.format_snapshot.as_ref(), Some(&normalized));
    assert_eq!(
        retained.successful_output.unwrap().format_snapshot.as_ref(),
        Some(&normalized)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"retained media");
    let other = state
        .formats
        .iter()
        .find(|f| f.format_id != format.format_id)
        .unwrap();
    state.selected_format_id = Some(other.format_id.clone());
    let created = db
        .begin_download_record(
            "other-format",
            &super::download_records::DownloadSnapshot { page: state },
            "2026-10-05 10:00:00",
        )
        .unwrap();
    assert!(created > 128);
    assert_eq!(
        db.video_download_records("youtube", "abc").unwrap().len(),
        2
    );
    drop(db);
    let reopened = Database::open(&path, &root.path().join("legacy")).unwrap();
    assert_eq!(
        reopened
            .video_download_records("youtube", "abc")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn version_eleven_upgrade_preserves_legacy_record_and_output() {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned format migration directory: {}",
        root.path().display()
    );
    let path = root.path().join("app.db");
    let db = Database::open(&path, &root.path().join("legacy")).unwrap();
    let mut snapshot = page();
    snapshot.download_directory = root.path().to_string_lossy().into();
    let row = db
        .begin_download_record(
            "legacy-format",
            &super::download_records::DownloadSnapshot { page: snapshot },
            "2026-10-05 10:00:00",
        )
        .unwrap();
    let output = root.path().join("old.webm");
    std::fs::write(&output, b"original video").unwrap();
    db.finish_download_record(
        "legacy-format",
        &super::download_records::DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 14,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    let before = db.get_download_record(row).unwrap();
    db.connection("test").unwrap().execute_batch("ALTER TABLE download_records DROP COLUMN format_snapshot_json; ALTER TABLE download_records DROP COLUMN successful_format_snapshot_json; PRAGMA user_version=11;").unwrap();
    drop(db);
    let upgraded = Database::open(&path, &root.path().join("legacy")).unwrap();
    let after = upgraded.get_download_record(row).unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.status, "completed");
    assert_eq!(after.output_path, before.output_path);
    assert_eq!(after.format_id, before.format_id);
    assert_eq!(after.format_snapshot.unwrap().quality_label.as_deref(), Some("1080P"));
    assert_eq!(after.successful_output.unwrap().format_snapshot.unwrap().quality_label.as_deref(), Some("1080P"));
    assert_eq!(std::fs::read(&output).unwrap(), b"original video");
}
#[test]
fn format_snapshot_migration_preserves_codec_for_history_retries() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("format snapshot test directory: {}", root.path().display());
    let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let mut p = page();
    let mut json = serde_json::to_value(&p).unwrap();
    json["formats"][1]["videoCodec"] = serde_json::json!("hev1");
    p = serde_json::from_value(json).unwrap();
    p.download_directory = root.path().to_string_lossy().into();
    let id = db
        .begin_download_record(
            "snapshot-codec",
            &super::download_records::DownloadSnapshot { page: p },
            "2026-10-05 12:00:00",
        )
        .unwrap();
    let row = serde_json::to_value(db.get_download_record(id).unwrap()).unwrap();
    assert_eq!(row["formatSnapshot"]["videoCodec"], "hev1");
    assert_eq!(
        db.connection("test")
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        15
    );
}

fn database() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary persistence test directory: {}",
        dir.path().display()
    );
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
    (dir, db)
}

#[test]
fn legacy_page_and_history_get_native_format_labels_without_changing_selection() {
    use super::download_records::DownloadSnapshot;
    let (dir, db) = database();
    let mut state = page();
    state.formats[1].width = Some(1920);
    state.formats[1].height = Some(1080);
    state.formats[1].video_codec = Some("hev1.1".into());
    state.download_directory = dir.path().to_string_lossy().into();
    db.save_download_page_state(&state).unwrap();
    // Simulate an old formats_json, which predates all derived fields.
    db.connection("test").unwrap().execute("UPDATE download_page_states SET formats_json=?1", [serde_json::to_string(&state.formats).unwrap()]).unwrap();
    let restored = db.download_page_states().unwrap().remove(0);
    assert_eq!(restored.selected_format_id, state.selected_format_id);
    let selected = restored.formats.iter().find(|f| f.format_id == "b").unwrap();
    assert_eq!(selected.quality_label.as_deref(), Some("1080P"));
    assert_eq!(selected.codec_label.as_deref(), Some("H.265"));
    let id = db.begin_download_record("legacy-normalized", &DownloadSnapshot { page: state }, "2026-10-04 10:00:00").unwrap();
    db.connection("test").unwrap().execute("UPDATE download_records SET format_snapshot_json=NULL WHERE id=?1", [id]).unwrap();
    let record = db.get_download_record(id).unwrap();
    let format = record.format_snapshot.unwrap();
    assert_eq!(format.format_id, "b");
    assert_eq!(format.quality_label.as_deref(), Some("1080P"));
    assert!(format.codec_label.is_none(), "legacy columns cannot reconstruct codec");
}

#[test]
fn video_record_retry_reuses_identity_and_keeps_the_successful_file() {
    use super::download_records::{DownloadRecordOutcome, DownloadSnapshot};
    let (dir, db) = database();
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    let first = db
        .begin_download_record("unique-first", &snapshot, "2026-10-04 10:00:00")
        .unwrap();
    db.finish_download_record(
        "unique-first",
        &DownloadRecordOutcome::Completed {
            path: dir.path().join("saved.mp4").to_string_lossy().into(),
            size: 12,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    let retry = db
        .begin_download_record("unique-retry", &snapshot, "2026-10-04 11:00:00")
        .unwrap();
    assert_eq!(first, retry, "a video must keep one stable record");
    db.finish_download_record("unique-retry", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    let record = db.get_download_record(first).unwrap();
    assert!(
        record.output_path.is_some(),
        "cancelled retry must retain its previous output"
    );
    assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 1);
}

pub(crate) fn page() -> DownloadPageState {
    serde_json::from_value(serde_json::json!({
        "platform":"youtube", "inputLink":"https://youtu.be/abc",
        "videoId":"abc", "title":"Video", "thumbnailUrl":null, "thumbnailCachePath":null,
        "durationSeconds":30.5, "extension":"mp4",
        "formats":[{"formatId":"a","height":1080,"fps":59.94,"extension":"mp4","sizeBytes":12345,"sizeApproximate":false},
                    {"formatId":"b","height":1080,"fps":59.94,"extension":"webm","sizeBytes":null,"sizeApproximate":false}],
        "selectedFormatId":"b", "selectedHeight":1080,"selectedFps":59.94,"cookieFallback":false,
        "downloadDirectory":"", "directoryCustomized":true, "parserFingerprint":"fingerprint",
        "parsedAt":"2026-10-04 10:00:00", "updatedAt":"2026-10-04 10:00:00"
    })).unwrap()
}

#[test]
fn parsed_metadata_requires_a_valid_current_input_link() {
    let (_dir, db) = database();
    let mut snapshot = page();
    snapshot.input_link = "edited invalid draft".into();
    assert_eq!(
        db.save_download_page_state(&snapshot).unwrap_err().code,
        "invalidSettings"
    );
    snapshot.clear_result();
    assert_eq!(
        db.save_download_page_state(&snapshot).unwrap().input_link,
        "edited invalid draft"
    );
}

#[test]
fn ui_choices_preserve_proxy_and_unknown_keys_and_reopen() {
    let (dir, db) = database();
    db.connection("test").unwrap().execute_batch("INSERT INTO app_settings(setting_key,value_json) VALUES ('proxy','null'),('future','42');").unwrap();
    let mut settings = db.ui_preferences().unwrap();
    settings.download_platform = "youtube".into();
    settings.settings_section = "proxy".into();
    settings.active_page = "history".into();
    db.save_ui_preferences(&settings).unwrap();
    drop(db);
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
    assert_eq!(db.ui_preferences().unwrap(), settings);
    assert_eq!(
        db.connection("test")
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key='future'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "42"
    );
}

#[test]
fn platform_settings_navigation_restores_without_changing_download_platform() {
    let (_dir, db) = database();
    let mut settings = db.ui_preferences().unwrap();
    assert_eq!(settings.settings_platform, "douyin");
    settings.download_platform = "youtube".into();
    settings.settings_section = "platforms".into();
    settings.settings_platform = "bilibili".into();
    db.save_ui_preferences(&settings).unwrap();
    assert_eq!(db.ui_preferences().unwrap(), settings);
}

#[test]
fn page_snapshot_restores_format_order_fractional_fps_and_cleared_directory() {
    let (dir, db) = database();
    let saved = db.save_download_page_state(&page()).unwrap();
    db.save_download_page_state(&page()).unwrap();
    drop(db);
    let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
    let states = db.download_page_states().unwrap();
    assert_eq!(states, vec![saved]);
    assert_eq!(states[0].formats[1].format_id, "b");
    assert_eq!(states[0].selected_fps, Some(59.94));
    assert!(states[0].directory_customized);
    assert_eq!(states[0].input_link, "https://youtu.be/abc");
    assert_eq!(states[0].download_directory, "");
}

#[test]
fn damaged_snapshot_and_choices_are_rejected_without_overwriting() {
    let (_dir, db) = database();
    db.save_download_page_state(&page()).unwrap();
    let mut invalid = page();
    invalid.selected_format_id = Some("missing".into());
    assert_eq!(
        db.save_download_page_state(&invalid).unwrap_err().code,
        "invalidSettings"
    );
    db.connection("test").unwrap().execute_batch("UPDATE download_page_states SET formats_json='[{}]'; INSERT INTO app_settings(setting_key,value_json) VALUES ('download_platform','42');").unwrap();
    assert_eq!(db.download_page_states().unwrap_err().code, "loadFailed");
    assert_eq!(db.ui_preferences().unwrap_err().code, "loadFailed");
    assert_eq!(
        db.connection("test")
            .unwrap()
            .query_row("SELECT formats_json FROM download_page_states", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "[{}]"
    );
}

#[test]
fn real_download_records_are_idempotent_independent_and_paginated() {
    use super::download_records::{DownloadRecordOutcome, DownloadSnapshot};
    let (dir, db) = database();
    let mut state = page();
    state.download_directory = dir.path().to_string_lossy().into();
    let snapshot = DownloadSnapshot { page: state };
    let id = db
        .begin_download_record("request-1", &snapshot, "2026-10-04 10:00:00")
        .unwrap();
    assert_eq!(
        db.begin_download_record("request-1", &snapshot, "2026-10-04 10:00:00")
            .unwrap(),
        id
    );
    let file = dir.path().join("video.mp4");
    std::fs::write(&file, b"actual bytes").unwrap();
    db.finish_download_record(
        "request-1",
        &DownloadRecordOutcome::Completed {
            path: file.to_string_lossy().into(),
            size: 12,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    let mut second = snapshot.clone();
    second.page.video_id = Some("another".into());
    db.begin_download_record("request-2", &second, "2026-10-04 10:00:00")
        .unwrap();
    db.finish_download_record("request-2", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    let first = db.list_download_records(None, 1).unwrap();
    assert_eq!(first.total_count, 2);
    assert_eq!(first.records[0].request_id, "request-2");
    let next = db.list_download_records(first.next_cursor, 1).unwrap();
    assert_eq!(next.records[0].request_id, "request-1");
    assert!(next.next_cursor.is_none());
    assert_eq!(next.records[0].file_size_bytes, Some(12));
    assert_eq!(next.records[0].source_link, snapshot.page.input_link);
    assert_eq!(next.records[0].format_extension, Some("webm".into()));
    let mut changed = page();
    changed.title = Some("changed page".into());
    db.save_download_page_state(&changed).unwrap();
    assert_eq!(
        db.list_download_records(None, 50).unwrap().records[0].title,
        "Video"
    );
    assert!(!db.discard_running_record("request-1").unwrap());
}

#[test]
fn all_platform_states_restore_atomically_without_refreshing_unchanged_timestamps() {
    let (dir, db) = database();
    for (platform, link) in [
        ("douyin", "https://douyin.com/video/1"),
        ("bilibili", "https://bilibili.com/video/BV1"),
        ("youtube", "https://youtu.be/abc"),
    ] {
        let mut snapshot = page();
        snapshot.platform = platform.into();
        snapshot.input_link = link.into();
        db.save_download_page_state(&snapshot).unwrap();
    }
    db.connection("test").unwrap().execute_batch("UPDATE download_page_states SET updated_at='2020-01-01 00:00:00'; CREATE TRIGGER refuse_page BEFORE UPDATE ON download_page_states WHEN NEW.title='refused' BEGIN SELECT RAISE(ABORT,'disk failure fixture'); END;").unwrap();
    let old = db.download_page_states().unwrap();
    assert_eq!(old.len(), 3);
    let unchanged = db.save_download_page_state(&old[2]).unwrap();
    assert_eq!(unchanged.updated_at, "2020-01-01 00:00:00");
    let mut refused = old[2].clone();
    refused.title = Some("refused".into());
    refused.input_link = "https://youtu.be/changed".into();
    assert_eq!(
        db.save_download_page_state(&refused).unwrap_err().code,
        "saveFailed"
    );
    assert_eq!(db.download_page_states().unwrap(), old);
    drop(db);
    let reopened =
        Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
    assert_eq!(reopened.download_page_states().unwrap(), old);
}

#[test]
fn recovery_tolerates_a_task_that_finished_after_the_running_scan() {
    use super::download_records::{DownloadRecordOutcome, DownloadSnapshot};
    let (dir, db) = database();
    let mut page = page();
    page.download_directory = dir.path().to_string_lossy().into();
    db.begin_download_record("scan-race", &DownloadSnapshot { page }, &datetime::now())
        .unwrap();
    let scanned = db.running_request_ids().unwrap();
    db.finish_download_record("scan-race", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    assert!(db
        .finish_download_record(&scanned[0], &DownloadRecordOutcome::Interrupted)
        .is_ok());
    assert_eq!(
        db.list_download_records(None, 10).unwrap().records[0].status,
        "cancelled"
    );
}

#[test]
fn history_cards_migration_adds_nullable_failure_fields_and_preserves_old_records() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary history migration directory: {}",
        dir.path().display()
    );
    let path = dir.path().join("app.db");
    let connection = Connection::open(&path).unwrap();
    for sql in [
        include_str!("../../migrations/001_settings.sql"),
        include_str!("../../migrations/002_settings_key_value.sql"),
        include_str!("../../migrations/003_beijing_datetime.sql"),
        include_str!("../../migrations/004_automatic_ytdlp.sql"),
        include_str!("../../migrations/005_persistence.sql"),
        include_str!("../../migrations/006_automatic_tools.sql"),
        include_str!("../../migrations/007_single_input_link.sql"),
    ] {
        connection.execute_batch(sql).unwrap();
    }
    connection.execute_batch("PRAGMA user_version=7;
        INSERT INTO download_records(request_id,platform,video_id,source_link,title,format_id,download_directory,status,error_code,error_detail,started_at,finished_at,updated_at)
        VALUES ('legacy','youtube','old','https://youtu.be/old','Old video','old','C:/Videos','failed','downloadFailed','old error','2025-01-01 10:00:00','2025-01-01 10:01:00','2025-01-01 10:01:00');").unwrap();
    drop(connection);
    let db = Database::open(&path, &dir.path().join("legacy.json")).unwrap();
    let connection = db.connection("test").unwrap();
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(download_records)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        columns.iter().any(|name| name == "error_stage"),
        "migration must add failure stage"
    );
    assert!(
        columns.iter().any(|name| name == "failure_kind"),
        "migration must add failure kind"
    );
    let old: (String, Option<String>, Option<String>, String) = connection.query_row(
        "SELECT title,error_stage,failure_kind,started_at FROM download_records WHERE request_id='legacy'", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(
        old,
        ("Old video".into(), None, None, "2025-01-01 10:00:00".into())
    );
}

fn history_record(
    db: &Database,
    directory: &Path,
    request: &str,
    title: &str,
    platform: &str,
    status: &str,
) -> i64 {
    use super::download_records::{DownloadRecordOutcome, DownloadSnapshot};
    let mut state = page();
    state.download_directory = directory.to_string_lossy().into();
    state.platform = platform.into();
    state.input_link = match platform {
        "bilibili" => "https://bilibili.com/video/BV1",
        "douyin" => "https://douyin.com/video/1",
        _ => "https://youtu.be/abc",
    }
        .into();
    state.title = Some(title.into());
    state.video_id = Some(request.into());
    let id = db
        .begin_download_record(
            request,
            &DownloadSnapshot { page: state },
            "2026-10-04 10:00:00",
        )
        .unwrap();
    let outcome = match status {
        "completed" => DownloadRecordOutcome::Completed {
            path: directory.join("video.mp4").to_string_lossy().into(),
            size: 12,
            extension: Some("mp4".into()),
        },
        "failed" => DownloadRecordOutcome::Failed {
            code: "downloadFailed".into(),
            detail: "failed".into(),
        },
        "cancelled" => DownloadRecordOutcome::Cancelled,
        "interrupted" => DownloadRecordOutcome::Interrupted,
        "running" => return id,
        _ => panic!("invalid test status"),
    };
    db.finish_download_record(request, &outcome).unwrap();
    id
}

fn history_query(
    query: &str,
    status: Option<&str>,
    limit: u32,
) -> super::download_records::HistoryQuery {
    super::download_records::HistoryQuery {
        cursor: None,
        limit,
        query: query.into(),
        status: status.map(str::to_owned),
    }
}

#[test]
fn history_search_and_status_counts_cover_records_beyond_the_first_fifty() {
    let (dir, db) = database();
    history_record(
        &db,
        dir.path(),
        "old-completed",
        "Target old",
        "youtube",
        "completed",
    );
    history_record(
        &db,
        dir.path(),
        "old-failed",
        "TARGET failed",
        "youtube",
        "failed",
    );
    for index in 0..60 {
        history_record(
            &db,
            dir.path(),
            &format!("filler-{index}"),
            "Unrelated",
            "douyin",
            "cancelled",
        );
    }
    let result = db
        .query_download_records(&history_query(" target ", Some("failed"), 50))
        .unwrap();
    assert_eq!(result.total_count, 62);
    assert_eq!(result.matched_count, 1);
    assert_eq!(result.records[0].request_id, "old-failed");
    assert_eq!(result.status_counts["completed"], 1);
    assert_eq!(result.status_counts["failed"], 1);
    assert_eq!(result.status_counts["cancelled"], 0);
    assert_eq!(result.status_counts.len(), 7);
    assert_eq!(result.status_counts["paused"], 0);
    assert!(result.next_cursor.is_none());
}

#[test]
fn history_search_escapes_literal_wildcards_and_matches_platform_aliases() {
    let (dir, db) = database();
    history_record(
        &db,
        dir.path(),
        "percent",
        "100% finished",
        "youtube",
        "completed",
    );
    history_record(
        &db,
        dir.path(),
        "underscore",
        "file_name",
        "bilibili",
        "failed",
    );
    history_record(
        &db,
        dir.path(),
        "slash",
        "path\\video",
        "douyin",
        "cancelled",
    );
    for (query, request) in [
        ("%", "percent"),
        ("_", "underscore"),
        ("\\", "slash"),
        ("油管", "percent"),
        ("YoUTuBe", "percent"),
        ("B站", "underscore"),
        ("哔哩哔哩", "underscore"),
        ("抖音", "slash"),
        ("DouYin", "slash"),
    ] {
        let result = db
            .query_download_records(&history_query(query, None, 50))
            .unwrap();
        assert_eq!(result.matched_count, 1, "search {query}");
        assert_eq!(result.records[0].request_id, request, "search {query}");
    }
}

#[test]
fn history_filtered_cursor_handles_identical_start_times_without_duplicates_or_gaps() {
    let (dir, db) = database();
    for index in 0..105 {
        history_record(
            &db,
            dir.path(),
            &format!("row-{index}"),
            "Same title",
            "youtube",
            if index % 2 == 0 {
                "failed"
            } else {
                "cancelled"
            },
        );
    }
    let mut query = history_query("same", Some("failed"), 7);
    let mut requests = Vec::new();
    loop {
        let result = db.query_download_records(&query).unwrap();
        assert_eq!(result.matched_count, 53);
        requests.extend(result.records.into_iter().map(|record| record.request_id));
        query.cursor = result.next_cursor;
        if query.cursor.is_none() {
            break;
        }
    }
    assert_eq!(requests.len(), 53);
    assert_eq!(requests.first().unwrap(), "row-104");
    assert_eq!(requests.last().unwrap(), "row-0");
    let unique: std::collections::HashSet<_> = requests.iter().collect();
    assert_eq!(unique.len(), 53);
}

#[test]
fn history_removal_refuses_running_records_and_keeps_completed_files() {
    let (dir, db) = database();
    let running = history_record(&db, dir.path(), "running", "Video", "youtube", "running");
    assert_eq!(
        db.delete_download_record(running).unwrap_err().code,
        "recordRunning"
    );
    assert_eq!(db.get_download_record(running).unwrap().status, "running");
    let completed = history_record(
        &db,
        dir.path(),
        "completed",
        "Video",
        "youtube",
        "completed",
    );
    let file = dir.path().join("video.mp4");
    std::fs::write(&file, b"actual bytes").unwrap();
    db.delete_download_record(completed).unwrap();
    assert_eq!(
        db.get_download_record(completed).unwrap().status,
        "completed"
    );
    db.delete_download_record(completed).unwrap();
    assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 1);
    assert_eq!(std::fs::read(file).unwrap(), b"actual bytes");
}

#[test]
fn history_failure_supplements_are_nullable_validated_and_preserved_after_settlement() {
    use super::download_records::DownloadRecordOutcome;
    let (dir, db) = database();
    let id = history_record(&db, dir.path(), "failure", "Video", "youtube", "running");
    let outcome = DownloadRecordOutcome::Failed {
        code: "spawnFailed".into(),
        detail: "Cannot start".into(),
    };
    assert_eq!(
        db.finish_download_record_with_failure(
            "failure",
            &outcome,
            Some("invented"),
            Some("tools")
        )
            .unwrap_err()
            .code,
        "invalidSettings"
    );
    assert_eq!(db.get_download_record(id).unwrap().status, "running");
    db.finish_download_record_with_failure("failure", &outcome, Some("preparing"), Some("tools"))
        .unwrap();
    db.finish_download_record("failure", &outcome).unwrap();
    let record = db.get_download_record(id).unwrap();
    assert_eq!(record.error_stage.as_deref(), Some("preparing"));
    assert_eq!(record.failure_kind.as_deref(), Some("tools"));
    assert_eq!(record.error_code.as_deref(), Some("spawnFailed"));
    let old = history_record(
        &db,
        dir.path(),
        "legacy-failure",
        "Old",
        "youtube",
        "failed",
    );
    assert!(db.get_download_record(old).unwrap().error_stage.is_none());
    assert!(db.get_download_record(old).unwrap().failure_kind.is_none());
    let invalid = history_query("", Some("imaginary"), 50);
    assert_eq!(
        db.query_download_records(&invalid).unwrap_err().code,
        "invalidSettings"
    );
}

#[test]
fn history_query_keeps_rows_and_counts_consistent_during_external_database_writes() {
    let (dir, db) = database();
    let writer =
        Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
    let directory = dir.path().to_path_buf();
    let writer = std::thread::spawn(move || {
        for index in 0..100 {
            history_record(
                &writer,
                &directory,
                &format!("concurrent-{index}"),
                "Match",
                "youtube",
                "failed",
            );
            std::thread::yield_now();
        }
    });
    for _ in 0..150 {
        let result = db
            .query_download_records(&history_query("match", None, 200))
            .unwrap();
        assert_eq!(result.total_count, result.matched_count);
        assert_eq!(result.records.len() as u64, result.matched_count);
        assert_eq!(
            result.status_counts.values().sum::<u64>(),
            result.total_count
        );
    }
    writer.join().unwrap();
    assert_eq!(
        db.query_download_records(&history_query("match", None, 200))
            .unwrap()
            .total_count,
        100
    );
}

#[test]
fn history_trash_scopes_counts_search_and_pagination_and_restores_full_snapshot() {
    let (dir, db) = database();
    let mut removed = Vec::new();
    for index in 0..65 {
        let id = history_record(
            &db,
            dir.path(),
            &format!("trash-{index}"),
            "Target",
            "youtube",
            "failed",
        );
        db.delete_download_record(id).unwrap();
        removed.push(id);
    }
    history_record(&db, dir.path(), "normal", "Other", "douyin", "completed");
    let normal = db
        .query_download_records(&history_query("", None, 200))
        .unwrap();
    assert_eq!(
        (normal.total_count, normal.matched_count, normal.trash_count),
        (1, 1, 65)
    );
    assert_eq!(normal.status_counts["failed"], 0);
    let mut query = history_query("target", Some("failed"), 7);
    let mut seen = Vec::new();
    loop {
        let result = db.query_download_records_in_scope(&query, true).unwrap();
        assert_eq!(
            (result.total_count, result.matched_count, result.trash_count),
            (65, 65, 65)
        );
        assert_eq!(result.status_counts["failed"], 65);
        assert_eq!(result.status_counts["completed"], 0);
        seen.extend(result.records.into_iter().map(|r| r.id));
        query.cursor = result.next_cursor;
        if query.cursor.is_none() {
            break;
        }
    }
    removed.reverse();
    assert_eq!(seen, removed);
    let id = removed[0];
    let mut before = serde_json::to_value(db.get_download_record(id).unwrap()).unwrap();
    assert!(before["deletedAt"].is_string());
    db.delete_download_record(id).unwrap();
    assert_eq!(
        serde_json::to_value(db.get_download_record(id).unwrap()).unwrap(),
        before
    );
    db.restore_download_record(id).unwrap();
    db.restore_download_record(id).unwrap();
    before["deletedAt"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::to_value(db.get_download_record(id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        db.query_download_records(&history_query("", None, 200))
            .unwrap()
            .trash_count,
        64
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn history_permanent_purge_deletes_native_file_before_record_and_keeps_shared_normal_outputs() {
    use crate::video::download::history::permanent::delete_output_file;
    let (dir, db) = database();
    let output = dir.path().join("video.mp4");
    std::fs::write(&output, b"owned video!").unwrap();
    let id = history_record(
        &db,
        dir.path(),
        "owned-purge",
        "Owned",
        "youtube",
        "completed",
    );
    db.delete_download_record(id).unwrap();
    db.purge_download_record(id, delete_output_file).unwrap();
    assert!(!output.exists());
    assert_eq!(
        db.get_download_record(id).unwrap_err().code,
        "recordNotFound"
    );

    std::fs::write(&output, b"owned video!").unwrap();
    let shared = history_record(
        &db,
        dir.path(),
        "shared-trash",
        "Old",
        "youtube",
        "completed",
    );
    let current = history_record(
        &db,
        dir.path(),
        "shared-normal",
        "Current",
        "youtube",
        "completed",
    );
    db.delete_download_record(shared).unwrap();
    assert_eq!(
        db.purge_download_record(shared, delete_output_file)
            .unwrap_err()
            .code,
        "historyFileInUse"
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
    assert!(db.get_download_record(shared).unwrap().deleted_at.is_some());
    assert!(db
        .get_download_record(current)
        .unwrap()
        .deleted_at
        .is_none());
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn history_permanent_purge_reports_file_deleted_save_failure_and_retries_missing_file() {
    use crate::video::download::history::permanent::delete_output_file;
    let (dir, db) = database();
    let output = dir.path().join("video.mp4");
    std::fs::write(&output, b"owned video!").unwrap();
    let id = history_record(
        &db,
        dir.path(),
        "save-failure-purge",
        "Owned",
        "youtube",
        "completed",
    );
    db.delete_download_record(id).unwrap();
    db.connection("test").unwrap().execute_batch("CREATE TRIGGER block_purge BEFORE DELETE ON download_records BEGIN SELECT RAISE(ABORT, 'owned test failure'); END;").unwrap();
    assert_eq!(
        db.purge_download_record(id, delete_output_file)
            .unwrap_err()
            .code,
        "historyFileDeletedSaveFailed"
    );
    assert!(!output.exists());
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
    db.connection("test")
        .unwrap()
        .execute_batch("DROP TRIGGER block_purge;")
        .unwrap();
    db.purge_download_record(id, delete_output_file).unwrap();
    assert_eq!(
        db.get_download_record(id).unwrap_err().code,
        "recordNotFound"
    );
}

#[cfg(windows)]
#[test]
fn history_permanent_empty_removes_successes_and_keeps_locked_files_for_retry() {
    use crate::video::download::history::permanent::delete_output_file;
    use std::os::windows::fs::OpenOptionsExt;
    let (dir, db) = database();
    let good = dir.path().join("good");
    let locked = dir.path().join("locked");
    std::fs::create_dir(&good).unwrap();
    std::fs::create_dir(&locked).unwrap();
    for root in [&good, &locked] {
        std::fs::write(root.join("video.mp4"), b"owned video!").unwrap();
    }
    let good_id = history_record(&db, &good, "good-purge", "Good", "youtube", "completed");
    let locked_id = history_record(
        &db,
        &locked,
        "locked-purge",
        "Locked",
        "youtube",
        "completed",
    );
    let missing_id = history_record(
        &db,
        dir.path(),
        "missing-purge",
        "Missing",
        "youtube",
        "completed",
    );
    let failed_id = history_record(
        &db,
        dir.path(),
        "failed-purge",
        "Failed",
        "youtube",
        "failed",
    );
    for id in [good_id, locked_id, missing_id, failed_id] {
        db.delete_download_record(id).unwrap();
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(locked.join("video.mp4"))
        .unwrap();
    assert_eq!(
        db.empty_download_record_trash(delete_output_file)
            .unwrap_err()
            .code,
        "historyTrashPartiallyDeleted"
    );
    for id in [good_id, missing_id, failed_id] {
        assert_eq!(
            db.get_download_record(id).unwrap_err().code,
            "recordNotFound"
        );
    }
    assert!(!good.join("video.mp4").exists());
    assert_eq!(
        std::fs::read(locked.join("video.mp4")).unwrap(),
        b"owned video!"
    );
    assert!(db
        .get_download_record(locked_id)
        .unwrap()
        .deleted_at
        .is_some());
    drop(lock);
    db.empty_download_record_trash(delete_output_file).unwrap();
    assert!(!locked.join("video.mp4").exists());
    assert_eq!(
        db.get_download_record(locked_id).unwrap_err().code,
        "recordNotFound"
    );
}

#[test]
fn history_permanent_operations_reject_active_downloads_before_deleting_any_file() {
    use crate::video::download::history::permanent::delete_output_file;
    let (dir, db) = database();
    let output = dir.path().join("video.mp4");
    std::fs::write(&output, b"owned video!").unwrap();
    let id = history_record(
        &db,
        dir.path(),
        "busy-purge",
        "Owned",
        "youtube",
        "completed",
    );
    db.delete_download_record(id).unwrap();
    history_record(
        &db,
        dir.path(),
        "active-purge",
        "Active",
        "youtube",
        "running",
    );
    assert_eq!(
        db.purge_download_record(id, delete_output_file)
            .unwrap_err()
            .code,
        "historyBusy"
    );
    assert_eq!(
        db.empty_download_record_trash(delete_output_file)
            .unwrap_err()
            .code,
        "historyBusy"
    );
    assert_eq!(std::fs::read(output).unwrap(), b"owned video!");
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
}

#[test]
fn history_purge_and_empty_only_remove_nonrunning_trash() {
    let (dir, db) = database();
    let running = history_record(&db, dir.path(), "running", "Video", "youtube", "running");
    let normal = history_record(&db, dir.path(), "normal", "Video", "youtube", "completed");
    let trash = history_record(&db, dir.path(), "trash", "Video", "youtube", "failed");
    assert_eq!(
        db.purge_download_record(normal, |_, _| Ok(false))
            .unwrap_err()
            .code,
        "recordNotTrashed"
    );
    assert_eq!(
        db.purge_download_record(running, |_, _| Ok(false))
            .unwrap_err()
            .code,
        "recordRunning"
    );
    assert_eq!(
        db.restore_download_record(99999).unwrap_err().code,
        "recordNotFound"
    );
    assert_eq!(
        db.delete_download_record(99999).unwrap_err().code,
        "recordNotFound"
    );
    assert_eq!(
        db.purge_download_record(99999, |_, _| Ok(false))
            .unwrap_err()
            .code,
        "recordNotFound"
    );
    db.delete_download_record(trash).unwrap();
    db.purge_download_record(trash, |_, _| Ok(false)).unwrap();
    db.finish_download_record(
        "running",
        &super::download_records::DownloadRecordOutcome::Cancelled,
    )
        .unwrap();
    assert_eq!(
        db.get_download_record(trash).unwrap_err().code,
        "recordNotFound"
    );
    let another = history_record(
        &db,
        dir.path(),
        "another-trash",
        "Video",
        "youtube",
        "cancelled",
    );
    db.delete_download_record(another).unwrap();
    db.empty_download_record_trash(|_, _| Ok(false)).unwrap();
    db.empty_download_record_trash(|_, _| Ok(false)).unwrap();
    assert_eq!(
        db.get_download_record(another).unwrap_err().code,
        "recordNotFound"
    );
    assert_eq!(db.get_download_record(normal).unwrap().status, "completed");
    assert_eq!(db.get_download_record(running).unwrap().status, "cancelled");
}

#[test]
fn history_delete_record_and_file_success_and_missing_leave_no_record_in_trash() {
    for recycled in [true, false] {
        let (dir, db) = database();
        let id = history_record(
            &db,
            dir.path(),
            "completed",
            "Video",
            "youtube",
            "completed",
        );
        let before = db.get_download_record(id).unwrap();
        assert_eq!(
            db.delete_download_record_and_file(id, |record, _| {
                assert_eq!(record.output_path, before.output_path);
                Ok(recycled)
            })
                .unwrap(),
            recycled
        );
        assert_eq!(
            db.get_download_record(id).unwrap_err().code,
            "recordNotFound"
        );
        assert_eq!(db.list_download_records(None, 50).unwrap().total_count, 0);
        let query = super::download_records::HistoryQuery {
            cursor: None,
            limit: 50,
            query: String::new(),
            status: None,
        };
        assert_eq!(db.query_download_records_in_scope(&query, true).unwrap().records.len(), 0);
    }
}

#[cfg(windows)]
#[test]
fn history_delete_record_and_file_uses_native_permanent_deletion_and_preserves_locked_outputs() {
    use crate::video::download::history::permanent::delete_output_file;
    use std::os::windows::fs::OpenOptionsExt;
    for present in [true, false] {
        let (dir, db) = database();
        let output = dir.path().join("video.mp4");
        let sibling = dir.path().join("cover.jpg");
        std::fs::write(&sibling, b"keep").unwrap();
        if present { std::fs::write(&output, b"owned video!").unwrap(); }
        let id = history_record(&db, dir.path(), "native-delete", "Owned", "youtube", "completed");
        if present {
            let player = std::fs::OpenOptions::new().read(true).share_mode(7).open(&output).unwrap();
            assert_eq!(db.delete_download_record_and_file(id, delete_output_file).unwrap_err().code, "historyFileOccupied");
            assert!(db.get_download_record(id).unwrap().deleted_at.is_none());
            assert_eq!(std::fs::read(&output).unwrap(), b"owned video!");
            drop(player);
        }
        assert_eq!(db.delete_download_record_and_file(id, delete_output_file).unwrap(), present);
        assert!(!output.exists());
        assert_eq!(db.get_download_record(id).unwrap_err().code, "recordNotFound");
        assert_eq!(std::fs::read(sibling).unwrap(), b"keep");
    }
}

#[cfg(windows)]
#[test]
fn history_delete_record_and_file_removes_owned_fragments_before_dropping_their_ownership() {
    use crate::video::download::history::actions::delete_record_files;
    for present in [true, false] {
        let (dir, db) = database();
        let output = dir.path().join("video.mp4");
        let sibling = dir.path().join("cover.jpg");
        std::fs::write(&sibling, b"keep").unwrap();
        if present { std::fs::write(&output, b"owned video!").unwrap(); }
        let id = history_record(&db, dir.path(), "fragment-delete", "Owned", "youtube", "running");
        let fragments = db.prepare_download_temporary_directory("fragment-delete", dir.path()).unwrap();
        eprintln!("owned deletion fragments: {}", fragments.path);
        std::fs::write(std::path::Path::new(&fragments.path).join("video.part"), b"fragment").unwrap();
        db.finish_download_record("fragment-delete", &super::download_records::DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 12,
            extension: Some("mp4".into()),
        }).unwrap();
        assert_eq!(delete_record_files(&db, id).unwrap(), present);
        assert!(!output.exists());
        assert!(!std::path::Path::new(&fragments.path).exists());
        assert_eq!(db.get_download_record(id).unwrap_err().code, "recordNotFound");
        assert_eq!(std::fs::read(sibling).unwrap(), b"keep");
    }
}

#[test]
fn history_delete_record_file_validates_before_callback_and_rolls_back_callback_failure() {
    let (dir, db) = database();
    let id = history_record(
        &db,
        dir.path(),
        "completed",
        "Video",
        "youtube",
        "completed",
    );
    let failed = history_record(&db, dir.path(), "failed", "Video", "youtube", "failed");
    let running = history_record(&db, dir.path(), "running", "Video", "youtube", "running");
    assert_eq!(
        db.delete_download_record_and_file(id, |_, _| panic!("busy callback"))
            .unwrap_err()
            .code,
        "historyBusy"
    );
    assert_eq!(
        db.delete_download_record_and_file(running, |_, _| panic!("running callback"))
            .unwrap_err()
            .code,
        "recordRunning"
    );
    assert_eq!(
        db.delete_download_record_and_file(failed, |_, _| panic!("failed callback"))
            .unwrap_err()
            .code,
        "invalidSettings"
    );
    db.finish_download_record(
        "running",
        &super::download_records::DownloadRecordOutcome::Cancelled,
    )
        .unwrap();
    let before = serde_json::to_value(db.get_download_record(id).unwrap()).unwrap();
    assert_eq!(
        db.delete_download_record_and_file(id, |_, _| Err(StorageError::new(
            "historyFileDeleteFailed",
            "fixture"
        )))
            .unwrap_err()
            .code,
        "historyFileDeleteFailed"
    );
    assert_eq!(
        serde_json::to_value(db.get_download_record(id).unwrap()).unwrap(),
        before
    );
    db.delete_download_record(id).unwrap();
    assert_eq!(
        db.delete_download_record_and_file(id, |_, _| panic!("trash callback"))
            .unwrap_err()
            .code,
        "recordTrashed"
    );
    assert_eq!(
        db.delete_download_record_and_file(99999, |_, _| panic!("missing callback"))
            .unwrap_err()
            .code,
        "recordNotFound"
    );
}

#[test]
fn history_delete_record_file_distinguishes_file_success_from_history_save_failure() {
    for recycled in [true, false] {
        let (dir, db) = database();
        let id = history_record(
            &db,
            dir.path(),
            "completed",
            "Video",
            "youtube",
            "completed",
        );
        db.connection("test").unwrap().execute_batch("CREATE TRIGGER refuse_delete BEFORE DELETE ON download_records BEGIN SELECT RAISE(ABORT,'disk failure fixture'); END;").unwrap();
        let error = db
            .delete_download_record_and_file(id, |_, _| Ok(recycled))
            .unwrap_err();
        assert_eq!(
            error.code,
            if recycled {
                "historyFileDeletedSaveFailed"
            } else {
                "saveFailed"
            }
        );
        let after = db.get_download_record(id).unwrap();
        assert!(after.deleted_at.is_none());
        assert!(after.file_deleted_at.is_none());
        assert_eq!(after.status, "completed");
    }
}

#[test]
fn history_delete_record_file_holds_writer_lock_before_calling_native_file_operation() {
    let (dir, db) = database();
    let id = history_record(
        &db,
        dir.path(),
        "completed",
        "Video",
        "youtube",
        "completed",
    );
    let writer = Connection::open(dir.path().join("app.db")).unwrap();
    writer.busy_timeout(Duration::ZERO).unwrap();
    db.delete_download_record_and_file(id, |_, _| {
        let error = writer
            .execute(
                "UPDATE download_records SET title='Changed' WHERE id=?1",
                [id],
            )
            .unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        Ok(true)
    })
        .unwrap();
    assert_eq!(db.get_download_record(id).unwrap_err().code, "recordNotFound");
    let another = history_record(&db, dir.path(), "another", "Video", "youtube", "completed");
    assert_eq!(
        writer
            .execute(
                "UPDATE download_records SET title='After' WHERE id=?1",
                [another]
            )
            .unwrap(),
        1
    );
}

#[test]
fn history_delete_record_file_commit_failure_reports_partial_success_and_rolls_back_history() {
    let (dir, db) = database();
    let id = history_record(
        &db,
        dir.path(),
        "completed",
        "Video",
        "youtube",
        "completed",
    );
    // A deferred constraint lets DELETE succeed and rejects only COMMIT.
    db.connection("test").unwrap().execute_batch(
        "CREATE TABLE recycle_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE recycle_child(parent_id INTEGER REFERENCES recycle_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER reject_recycle_commit AFTER DELETE ON download_records
        BEGIN INSERT INTO recycle_child(parent_id) VALUES (1); END;"
    ).unwrap();
    let error = db
        .delete_download_record_and_file(id, |_, _| Ok(true))
        .unwrap_err();
    assert_eq!(error.code, "historyFileDeletedSaveFailed");
    let record = db.get_download_record(id).unwrap();
    assert!(record.deleted_at.is_none());
    assert!(record.file_deleted_at.is_none());
    assert_eq!(
        db.connection("test")
            .unwrap()
            .query_row("SELECT count(*) FROM recycle_child", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn history_trash_migration_from_eight_preserves_history_and_old_null_markers() {
    let (dir, db) = database();
    let id = history_record(
        &db,
        dir.path(),
        "legacy",
        "Saved title",
        "youtube",
        "failed",
    );
    let path = dir.path().join("app.db");
    db.connection("test").unwrap().execute_batch("DROP INDEX idx_download_records_normal_started; DROP INDEX idx_download_records_trash_started; ALTER TABLE download_records DROP COLUMN deleted_at; ALTER TABLE download_records DROP COLUMN file_deleted_at; PRAGMA user_version=8;").unwrap();
    drop(db);
    let upgraded = Database::open(&path, &dir.path().join("legacy.json")).unwrap();
    let record = upgraded.get_download_record(id).unwrap();
    assert_eq!(record.title, "Saved title");
    assert_eq!(record.started_at, "2026-10-04 10:00:00");
    assert_eq!(record.status, "failed");
    assert!(record.deleted_at.is_none());
    assert!(record.file_deleted_at.is_none());
    assert_eq!(
        upgraded
            .connection("test")
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        15
    );
    assert_eq!(upgraded.connection("test").unwrap().query_row(
        "SELECT count(*) FROM pragma_index_list('download_records') WHERE partial=1 AND name IN ('idx_download_records_normal_started','idx_download_records_trash_started')", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
}

#[test]
fn history_scope_pagination_uses_partial_indexes_without_sorting() {
    let (_dir, db) = database();
    let connection = db.connection("test").unwrap();
    for (scope, expected_index) in [
        ("deleted_at IS NULL", "idx_download_records_normal_started"),
        (
            "deleted_at IS NOT NULL",
            "idx_download_records_trash_started",
        ),
    ] {
        let details: Vec<String> = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT * FROM download_records WHERE {scope}
            AND (title LIKE ?1 ESCAPE '\\' OR platform LIKE ?1 ESCAPE '\\')
            AND (?2 IS NULL OR status=?2)
            AND (?3 IS NULL OR started_at<?3 OR (started_at=?3 AND id<?4))
            ORDER BY started_at DESC,id DESC LIMIT ?5"
            ))
            .unwrap()
            .query_map(
                params![
                    "%video%",
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<i64>::None,
                    50
                ],
                |row| row.get(3),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            details.iter().any(|detail| detail.contains(expected_index)),
            "{scope}: {details:?}"
        );
        assert!(
            !details.iter().any(|detail| detail.contains("TEMP B-TREE")),
            "{scope}: {details:?}"
        );
        let partial: bool = connection
            .query_row(
                "SELECT partial FROM pragma_index_list('download_records') WHERE name=?1",
                [expected_index],
                |row| row.get(0),
            )
            .unwrap();
        assert!(partial);
    }
}

#[test]
fn history_purged_ids_are_not_reused_across_connections_or_stale_file_confirmations() {
    for empty_trash in [false, true] {
        let (dir, db) = database();
        let writer =
            Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
        let old_id = history_record(&db, dir.path(), "old", "Old video", "youtube", "completed");
        db.delete_download_record(old_id).unwrap();
        if empty_trash {
            db.empty_download_record_trash(|_, _| Ok(false)).unwrap();
        } else {
            db.purge_download_record(old_id, |_, _| Ok(false)).unwrap();
        }
        let new_id = history_record(
            &writer,
            dir.path(),
            "new",
            "New video",
            "youtube",
            "completed",
        );
        assert!(new_id > old_id, "purged {old_id} was reused as {new_id}");
        assert_eq!(
            db.delete_download_record_and_file(old_id, |_, _| panic!(
                "stale confirmation must not reach a new file"
            ))
                .unwrap_err()
                .code,
            "recordNotFound"
        );
        assert_eq!(
            db.restore_download_record(old_id).unwrap_err().code,
            "recordNotFound"
        );
        let new_record = writer.get_download_record(new_id).unwrap();
        assert_eq!(new_record.request_id, "new");
        assert!(new_record.deleted_at.is_none());
        assert!(new_record.file_deleted_at.is_none());
    }
}

#[test]
fn history_eight_to_nine_rebuild_preserves_every_column_and_advances_legacy_ids() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary schema eight recycle migration directory: {}",
        dir.path().display()
    );
    let path = dir.path().join("app.db");
    let old = Connection::open(&path).unwrap();
    old.execute_batch(concat!(
    include_str!("../../migrations/001_settings.sql"),
    include_str!("../../migrations/002_settings_key_value.sql"),
    include_str!("../../migrations/003_beijing_datetime.sql"),
    include_str!("../../migrations/004_automatic_ytdlp.sql"),
    include_str!("../../migrations/005_persistence.sql"),
    include_str!("../../migrations/006_automatic_tools.sql"),
    include_str!("../../migrations/007_single_input_link.sql"),
    include_str!("../../migrations/008_download_history_cards.sql"),
    "PRAGMA user_version=8;"
    ))
        .unwrap();
    old.execute_batch(
        "INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,error_code,error_detail,started_at,finished_at,updated_at,error_stage,failure_kind)
        VALUES (7,'legacy-failed','youtube','saved','https://youtu.be/saved','Saved title','format','C:/Videos','failed','downloadFailed','Original error','2026-10-04 10:00:00','2026-10-04 10:01:00','2026-10-04 10:02:00','processing','processing');
        INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,finished_at,updated_at,output_path,output_extension,file_size_bytes)
        VALUES (32,'legacy-completed','bilibili','saved','https://bilibili.com/video/saved','Completed title','format','C:/Videos','completed','2026-10-04 09:00:00','2026-10-04 09:01:00','2026-10-04 09:02:00','C:/Videos/video.mp4','mp4',456);
        INSERT INTO download_records(id,request_id,platform,video_id,source_link,title,format_id,download_directory,status,started_at,updated_at)
        VALUES (20,'legacy-running','douyin','saved','https://douyin.com/video/saved','Running title','format','C:/Videos','running','2026-10-04 08:00:00','2026-10-04 08:01:00');
        UPDATE download_records SET thumbnail_url='https://example.com/cover.jpg',thumbnail_cache_path='C:/Cache/cover.jpg',duration_seconds=30.5,format_extension='webm',height=1080,fps=59.94,selected_size_bytes=123,size_approximate=1,cookie_fallback=1;"
    ).unwrap();
    let mut statement = old
        .prepare("SELECT * FROM download_records ORDER BY id")
        .unwrap();
    let column_count = statement.column_count();
    let before: Vec<Vec<rusqlite::types::Value>> = statement
        .query_map([], |row| {
            (0..column_count).map(|index| row.get(index)).collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(statement);
    drop(old);
    let upgraded = Database::open(&path, &dir.path().join("legacy.json")).unwrap();
    let connection = upgraded.connection("test").unwrap();
    let after: Vec<Vec<rusqlite::types::Value>> = connection
        .prepare("SELECT * FROM download_records ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            (0..column_count).map(|index| row.get(index)).collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        after, before,
        "Every original column, ID and timestamp must survive the rebuild"
    );
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='index' AND name LIKE 'idx_download_records_%'", [], |row| row.get::<_, i64>(0)).unwrap(), 5);
    assert!(connection
        .execute(
            "UPDATE download_records SET output_path=NULL WHERE id=32",
            []
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE download_records SET error_stage='processing' WHERE id=32",
            []
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE download_records SET deleted_at='2026-10-04 10:00:00' WHERE id=20",
            []
        )
        .is_err());
    assert_eq!(connection.query_row("SELECT count(*) FROM download_records WHERE deleted_at IS NULL AND file_deleted_at IS NULL", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    drop(connection);
    upgraded.delete_download_record(32).unwrap();
    assert_eq!(
        upgraded
            .purge_download_record(32, |_, _| panic!("legacy active download deletion"))
            .unwrap_err()
            .code,
        "historyBusy"
    );
    upgraded
        .finish_download_record(
            "legacy-running",
            &super::download_records::DownloadRecordOutcome::Interrupted,
        )
        .unwrap();
    upgraded
        .purge_download_record(32, |_, _| Ok(false))
        .unwrap();
    drop(upgraded);
    let reopened = Database::open(&path, &dir.path().join("legacy.json")).unwrap();
    let next = history_record(
        &reopened,
        dir.path(),
        "new-after-migration",
        "New",
        "youtube",
        "failed",
    );
    assert!(
        next > 32,
        "The deleted highest legacy ID must never be reused"
    );
}

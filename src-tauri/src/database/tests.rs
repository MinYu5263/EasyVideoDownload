use super::*;

#[test]
fn zero_baseline_reopens_without_reinitializing_or_importing_legacy_tools() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut settings = db.app_settings("en").unwrap();
    settings.theme = "dark".into();
    db.save_app_settings(&settings).unwrap();
    db.connection("test")
        .unwrap()
        .pragma_update(None, "application_id", 0)
        .unwrap();
    drop(db);
    std::fs::write(&legacy, "invalid legacy configuration").unwrap();

    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.app_settings("zh-CN").unwrap(), settings);
    assert_eq!(
        reopened.connection("test").unwrap()
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        reopened.connection("test").unwrap()
            .pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0))
            .unwrap(),
        DATABASE_APPLICATION_ID
    );
}

#[test]
fn incomplete_baseline_cannot_be_opened_or_renumbered() {
    for version in [0, 15] {
        let (_dir, path, legacy) = paths();
        let db = Database::open(&path, &legacy).unwrap();
        let settings = db.app_settings("en").unwrap();
        let connection = db.connection("test").unwrap();
        connection.execute_batch("DROP TABLE download_page_states;").unwrap();
        connection.pragma_update(None, "application_id", 0).unwrap();
        connection.pragma_update(None, "user_version", version).unwrap();
        drop(connection);
        drop(db);

        assert!(Database::open(&path, &legacy).is_err());
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(),
            version
        );
        assert_eq!(read_app_settings(&connection, "zh-CN").unwrap(), settings);
        assert_eq!(connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0)).unwrap(), 0);
    }
}

#[test]
fn baseline_with_missing_runtime_columns_is_rejected_without_changing_metadata() {
    for version in [0, 15] {
        for (table, column) in [
            ("download_page_states", "title"),
            ("download_records", "source_link"),
        ] {
            let (_dir, path, legacy) = paths();
            let db = Database::open(&path, &legacy).unwrap();
            let settings = db.app_settings("en").unwrap();
            let connection = db.connection("test").unwrap();
            connection.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column};")).unwrap();
            connection.pragma_update(None, "application_id", 0).unwrap();
            connection.pragma_update(None, "user_version", version).unwrap();
            drop(connection);
            drop(db);

            assert!(Database::open(&path, &legacy).is_err(), "version {version}, missing {table}.{column}");
            let connection = Connection::open(&path).unwrap();
            assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), version);
            assert_eq!(connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0)).unwrap(), 0);
            assert_eq!(read_app_settings(&connection, "zh-CN").unwrap(), settings);
        }
    }
}

#[test]
fn future_schema_fifteen_is_rejected_without_renumbering() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let settings = db.app_settings("en").unwrap();
    let connection = db.connection("test").unwrap();
    connection.pragma_update(None, "application_id", 0x45564431_i32).unwrap();
    connection.pragma_update(None, "user_version", 15).unwrap();
    drop(connection);
    drop(db);

    assert!(Database::open(&path, &legacy).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 15);
    assert_eq!(connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0)).unwrap(), DATABASE_APPLICATION_ID);
    assert_eq!(read_app_settings(&connection, "zh-CN").unwrap(), settings);
}

#[test]
fn first_release_initialization_creates_only_runtime_tables() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let connection = db.connection("test").unwrap();
    let tables: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(tables, [
        "app_settings",
        "download_page_states",
        "download_records",
        "download_temporary_directories",
        "required_tools",
    ]);
    assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0)).unwrap(), DATABASE_APPLICATION_ID);
}

#[test]
fn failed_initialization_rolls_back_schema_and_can_be_retried() {
    let (_dir, path, legacy) = paths();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = Connection::open(&path).unwrap();
    // No business tables exist: initialization creates app_settings, then fails
    // on the conflicting view. Its partial DDL and header changes must roll back.
    connection.execute_batch("CREATE VIEW required_tools AS SELECT 'keep' AS sentinel;").unwrap();
    drop(connection);
    assert!(Database::open(&path, &legacy).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0)).unwrap(), 0);
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name='app_settings'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.query_row("SELECT sentinel FROM required_tools", [], |row| row.get::<_, String>(0)).unwrap(), "keep");
    connection.execute_batch("DROP VIEW required_tools;").unwrap();
    drop(connection);
    assert_eq!(Database::open(&path, &legacy).unwrap().app_settings("en").unwrap().locale, "en");
}

#[test]
fn unsupported_schema_versions_are_rejected_without_changing_saved_data() {
    for version in (1..15).rev() {
        let (_dir, path, legacy) = paths();
        let db = Database::open(&path, &legacy).unwrap();
        let mut settings = db.app_settings("en").unwrap();
        settings.theme = "dark".into();
        db.save_app_settings(&settings).unwrap();
        db.connection("test")
            .unwrap()
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(db);

        let error = Database::open(&path, &legacy)
            .err()
            .expect("development schemas must be rejected");
        assert_eq!(error.code, "loadFailed");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            version
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT value_json FROM app_settings WHERE setting_key='theme'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "\"dark\""
        );
    }
}

#[test]
fn download_limit_rejects_invalid_values_without_changing_saved_preferences() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut settings = db.app_settings("en").unwrap();
    settings.max_concurrent_downloads = 6;
    db.save_app_settings(&settings).unwrap();
    for limit in [0, 7, usize::MAX] {
        settings.max_concurrent_downloads = limit;
        assert_eq!(db.save_app_settings(&settings).unwrap_err().code, "invalidSettings");
        assert_eq!(db.download_limit().unwrap(), 6);
    }
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.download_limit().unwrap(), 6);
}

#[test]
fn missing_download_limit_defaults_to_three_and_is_persisted() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let settings = serde_json::to_value(db.app_settings("en").unwrap()).unwrap();
    assert_eq!(settings["maxConcurrentDownloads"], 3);
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(serde_json::to_value(reopened.app_settings("en").unwrap()).unwrap()["maxConcurrentDownloads"], 3);
}
use crate::required_tools::{Program, RequiredToolSource};

#[test]
fn automatic_ffmpeg_and_deno_configurations_restore_as_complete_groups() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut expected = Vec::new();
    for (id, version) in [
        (RequiredToolId::Ffmpeg, "9.0.2"),
        (RequiredToolId::Deno, "2.9.7"),
    ] {
        let mut configured = config(id, version);
        configured.source = RequiredToolSource::Automatic;
        configured.manual_path.clear();
        db.save_tool(id, &configured).unwrap();
        expected.push((id, configured));
    }
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    for (id, configured) in expected {
        assert_eq!(reopened.tools().unwrap().tools[&id], configured);
    }
}

#[test]
fn automatic_ytdlp_configuration_restores_after_reopening() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut configured = config(RequiredToolId::Ytdlp, "2026.09.25");
    configured.source = serde_json::from_str("\"automatic\"")
        .expect("yt-dlp should support application-managed configuration");
    configured.manual_path.clear();
    db.save_tool(RequiredToolId::Ytdlp, &configured).unwrap();
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(
        reopened.tools().unwrap().tools[&RequiredToolId::Ytdlp],
        configured
    );
    assert_eq!(reopened.tools().unwrap().tools.len(), 1);
}

#[test]
fn proxy_settings_are_saved_as_one_record_and_restore_after_reopening() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    assert_eq!(db.proxy_settings().unwrap(), None);
    let proxy = crate::proxy::ProxySettings {
        protocol: "socks5".into(),
        address: "127.0.0.1".into(),
        port: 7890,
    };
    db.save_proxy_settings(Some(&proxy)).unwrap();
    db.save_app_settings(&db.app_settings("zh-CN").unwrap())
        .unwrap();
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.proxy_settings().unwrap(), Some(proxy));
    reopened.save_proxy_settings(None).unwrap();
    assert_eq!(reopened.proxy_settings().unwrap(), None);
}

#[test]
fn invalid_proxy_and_failed_writes_keep_the_applied_configuration() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let saved = crate::proxy::ProxySettings {
        protocol: "http".into(),
        address: "127.0.0.1".into(),
        port: 7890,
    };
    db.save_proxy_settings(Some(&saved)).unwrap();
    let invalid = crate::proxy::ProxySettings {
        port: 0,
        ..saved.clone()
    };
    assert_eq!(
        db.save_proxy_settings(Some(&invalid)).unwrap_err().code,
        "invalidSettings"
    );
    db.connection("test").unwrap().execute_batch(
        "CREATE TRIGGER reject_proxy BEFORE UPDATE ON app_settings WHEN NEW.setting_key = 'proxy'
         BEGIN SELECT RAISE(ABORT, 'test failure'); END;"
    ).unwrap();
    // Unchanged settings must not write or update their timestamp.
    db.save_proxy_settings(Some(&saved)).unwrap();
    let changed = crate::proxy::ProxySettings {
        port: 8080,
        ..saved.clone()
    };
    assert_eq!(
        db.save_proxy_settings(Some(&changed)).unwrap_err().code,
        "saveFailed"
    );
    assert_eq!(db.proxy_settings().unwrap(), Some(saved));
}

#[test]
fn corrupt_proxy_is_reported_instead_of_replaced_with_direct_connection() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.connection("test")
        .unwrap()
        .execute(
            "INSERT INTO app_settings (setting_key, value_json) VALUES ('proxy', ?1)",
            [r#"{"protocol":"http","address":"http://wrong","port":7890}"#],
        )
        .unwrap();
    assert_eq!(db.proxy_settings().unwrap_err().code, "loadFailed");
}

fn paths() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("EasyVideoDownload/app.db");
    let legacy = dir.path().join("required-tools.json");
    (dir, database, legacy)
}

fn config(id: RequiredToolId, version: &str) -> RequiredToolConfig {
    let root = std::env::current_dir().unwrap().join("fixture tools");
    let names: &[&str] = match id {
        RequiredToolId::Ytdlp => &["yt-dlp"],
        RequiredToolId::Ffmpeg => &["ffmpeg", "ffprobe"],
        RequiredToolId::Deno => &["deno"],
    };
    RequiredToolConfig {
        source: RequiredToolSource::Manual,
        manual_path: if id == RequiredToolId::Ffmpeg {
            root.to_string_lossy().into()
        } else {
            root.join(names[0]).to_string_lossy().into()
        },
        programs: names
            .iter()
            .map(|name| Program {
                name: (*name).into(),
                path: root.join(name),
                version: version.into(),
            })
            .collect(),
        checked_at: "1970-01-01 08:02:03".into(),
    }
}

#[test]
fn preferences_restore_after_reopening_without_overwriting_saved_locale() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let defaults = db.app_settings("zh-CN").unwrap();
    assert_eq!(
        defaults,
        AppSettings {
            locale: "zh-CN".into(),
            theme: "system".into(),
            notify_on_completion: true,
            close_action: "ask".into(),
            max_concurrent_downloads: 3,
        }
    );
    let preferences = AppSettings {
        locale: "en".into(),
        theme: "dark".into(),
        notify_on_completion: false,
        close_action: "tray".into(),
        max_concurrent_downloads: 4,
    };
    db.save_app_settings(&preferences).unwrap();
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.app_settings("zh-CN").unwrap(), preferences);
}

#[test]
fn retired_failure_notification_preference_is_ignored_and_not_exposed() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.connection.lock().unwrap().execute_batch(
        "INSERT INTO app_settings (setting_key, value_json) VALUES ('notify_on_failure', 'false');",
    ).unwrap();
    let settings = db.app_settings("en").unwrap();
    assert!(settings.notify_on_completion);
    assert!(serde_json::to_value(&settings).unwrap().get("notifyOnFailure").is_none());
    // Even an invalid retired value must not block loading the remaining preferences.
    db.connection.lock().unwrap().execute_batch(
        "UPDATE app_settings SET value_json = '\"invalid\"' WHERE setting_key = 'notify_on_failure';",
    ).unwrap();
    assert_eq!(db.app_settings("en").unwrap(), settings);
}

#[test]
fn legacy_proxy_addresses_remain_editable_but_cannot_be_saved_or_used() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.connection("test").unwrap().execute(
        "INSERT INTO app_settings (setting_key, value_json) VALUES ('proxy', ?1)",
        [r#"{"protocol":"http","address":"anything","port":7890}"#],
    ).unwrap();
    let restored = db.proxy_settings_for_editing().unwrap().unwrap();
    assert_eq!(restored.address, "anything");
    assert_eq!(db.proxy_settings().unwrap_err().code, "loadFailed");
    assert_eq!(db.save_proxy_settings(Some(&restored)).unwrap_err().code, "invalidSettings");
    assert_eq!(db.proxy_settings_for_editing().unwrap(), Some(restored.clone()));
    let corrected = crate::proxy::ProxySettings { address: "127.0.0.1".into(), ..restored };
    db.save_proxy_settings(Some(&corrected)).unwrap();
    assert_eq!(db.proxy_settings().unwrap(), Some(corrected));
}

#[test]
fn tool_groups_restore_after_reopening_without_running_executables() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.save_tool(
        RequiredToolId::Ffmpeg,
        &config(RequiredToolId::Ffmpeg, "8.0"),
    )
    .unwrap();
    db.save_tool(RequiredToolId::Deno, &config(RequiredToolId::Deno, "2.5.0"))
        .unwrap();
    drop(db);
    let restored = Database::open(&path, &legacy).unwrap().tools().unwrap();
    assert_eq!(restored.tools.len(), 2);
    assert_eq!(
        restored.tools[&RequiredToolId::Ffmpeg],
        config(RequiredToolId::Ffmpeg, "8.0")
    );
    assert_eq!(
        restored.tools[&RequiredToolId::Deno],
        config(RequiredToolId::Deno, "2.5.0")
    );
}

#[test]
fn legacy_tool_files_are_imported_once_and_preserved() {
    for (filename, field, id) in [
        ("required-tools.json", "tools", "deno"),
        ("program-dependencies.json", "dependencies", "deno"),
        ("tools.json", "tools", "runtime"),
    ] {
        let (dir, path, legacy) = paths();
        let source = dir.path().join(filename);
        let original = serde_json::to_vec(&serde_json::json!({field: {
            id: config(RequiredToolId::Deno, "2.5.0")
        }}))
        .unwrap();
        std::fs::write(&source, &original).unwrap();
        let db = Database::open(&path, &legacy).unwrap();
        assert_eq!(
            db.tools().unwrap().tools[&RequiredToolId::Deno].programs[0].version,
            "2.5.0"
        );
        db.save_tool(RequiredToolId::Deno, &config(RequiredToolId::Deno, "2.6.0"))
            .unwrap();
        drop(db);
        let reopened = Database::open(&path, &legacy).unwrap();
        assert_eq!(
            reopened.tools().unwrap().tools[&RequiredToolId::Deno].programs[0].version,
            "2.6.0"
        );
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}

#[test]
fn corrupt_legacy_file_is_preserved_and_migration_can_be_retried() {
    let (_dir, path, legacy) = paths();
    std::fs::write(&legacy, "not JSON").unwrap();
    assert!(Database::open(&path, &legacy).is_err());
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "not JSON");
    std::fs::write(&legacy, "{\"tools\":{}}").unwrap();
    assert!(Database::open(&path, &legacy)
        .unwrap()
        .tools()
        .unwrap()
        .tools
        .is_empty());
}

#[test]
fn failed_program_write_rolls_back_the_entire_tool_group() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let old = config(RequiredToolId::Ffmpeg, "8.0");
    db.save_tool(RequiredToolId::Ffmpeg, &old).unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_program BEFORE INSERT ON required_tools
         WHEN NEW.program_name = 'ffprobe' BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
        )
        .unwrap();
    assert!(db
        .save_tool(
            RequiredToolId::Ffmpeg,
            &config(RequiredToolId::Ffmpeg, "9.0")
        )
        .is_err());
    assert_eq!(db.tools().unwrap().tools[&RequiredToolId::Ffmpeg], old);
}

#[test]
fn incomplete_or_relative_tool_configuration_cannot_replace_saved_config() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let old = config(RequiredToolId::Ffmpeg, "8.0");
    db.save_tool(RequiredToolId::Ffmpeg, &old).unwrap();
    let mut incomplete = old.clone();
    incomplete.programs.pop();
    assert!(db.save_tool(RequiredToolId::Ffmpeg, &incomplete).is_err());
    let mut relative = old.clone();
    relative.programs[0].path = "relative/ffmpeg".into();
    assert!(db.save_tool(RequiredToolId::Ffmpeg, &relative).is_err());
    assert_eq!(db.tools().unwrap().tools[&RequiredToolId::Ffmpeg], old);
}

#[test]
fn invalid_preferences_do_not_replace_saved_values() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let old = db.app_settings("en").unwrap();
    let mut invalid = old.clone();
    invalid.theme = "unknown".into();
    assert!(db.save_app_settings(&invalid).is_err());
    assert_eq!(db.app_settings("zh-CN").unwrap(), old);
}

#[test]
fn newer_schema_and_corrupt_database_are_not_overwritten() {
    let (dir, path, legacy) = paths();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE future_data (value TEXT); INSERT INTO future_data VALUES ('keep'); PRAGMA user_version=99;").unwrap();
    drop(connection);
    assert!(Database::open(&path, &legacy).is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT value FROM future_data", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        99
    );
    let corrupt = dir.path().join("corrupt.db");
    std::fs::write(&corrupt, "broken database").unwrap();
    assert!(Database::open(&corrupt, &legacy).is_err());
    assert_eq!(std::fs::read_to_string(corrupt).unwrap(), "broken database");
}

#[test]
fn concurrent_tool_and_preference_writes_do_not_lose_each_other() {
    let (_dir, path, legacy) = paths();
    let db = std::sync::Arc::new(Database::open(&path, &legacy).unwrap());
    let mut preferences = db.app_settings("en").unwrap();
    preferences.theme = "light".into();
    let mut jobs = Vec::new();
    for (id, version) in [
        (RequiredToolId::Ytdlp, "2026.09.25"),
        (RequiredToolId::Deno, "2.5.0"),
        (RequiredToolId::Ffmpeg, "8.0"),
    ] {
        let db = db.clone();
        jobs.push(std::thread::spawn(move || {
            db.save_tool(id, &config(id, version)).unwrap()
        }));
    }
    let prefs_db = db.clone();
    jobs.push(std::thread::spawn(move || {
        prefs_db.save_app_settings(&preferences).unwrap()
    }));
    for job in jobs {
        job.join().unwrap();
    }
    assert_eq!(db.tools().unwrap().tools.len(), 3);
    assert_eq!(db.app_settings("en").unwrap().theme, "light");
}

#[test]
fn new_rust_writes_and_sql_defaults_use_fixed_beijing_time_without_a_suffix() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.app_settings("en").unwrap();
    let connection = db.connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO app_settings (setting_key, value_json) VALUES ('future_setting', 'true')",
            [],
        )
        .unwrap();
    let mut statement = connection.prepare(
        "SELECT updated_at, abs(unixepoch(updated_at) - unixepoch('now', '+8 hours')) FROM app_settings",
    ).unwrap();
    for row in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
    {
        let (date, seconds) = row.unwrap();
        assert_eq!(date.len(), 19);
        assert!(seconds <= 5, "date is not Beijing time: {date}");
    }
}

#[test]
fn new_settings_can_be_added_without_schema_changes_and_unknown_keys_survive_saves() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut settings = db.app_settings("en").unwrap();
    db.connection.lock().unwrap().execute_batch(
        "INSERT INTO app_settings (setting_key, value_json) VALUES ('future_setting', '{\"limit\":4}');
         DELETE FROM app_settings WHERE setting_key = 'theme';",
    ).unwrap();
    assert_eq!(db.app_settings("zh-CN").unwrap().theme, "system");
    settings.theme = "dark".into();
    db.save_app_settings(&settings).unwrap();
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.app_settings("zh-CN").unwrap(), settings);
    assert_eq!(
        reopened
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key = 'future_setting'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "{\"limit\":4}"
    );
}

#[test]
fn saving_preferences_updates_only_changed_keys_and_their_dates() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let mut settings = db.app_settings("en").unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute_batch("UPDATE app_settings SET updated_at = '2020-01-01 00:00:00';")
        .unwrap();
    settings.theme = "light".into();
    db.save_app_settings(&settings).unwrap();
    let connection = db.connection.lock().unwrap();
    let mut statement = connection
        .prepare("SELECT setting_key, updated_at FROM app_settings")
        .unwrap();
    for row in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
    {
        let (key, date) = row.unwrap();
        assert_eq!(date.len(), 19);
        assert!(datetime::is_valid(&date));
        if key == "theme" {
            assert_ne!(date, "2020-01-01 00:00:00");
        } else {
            assert_eq!(date, "2020-01-01 00:00:00");
        }
    }
}

#[test]
fn invalid_stored_preference_types_and_values_are_reported_without_overwriting_them() {
    for (key, value) in [("theme", "\"unknown\""), ("notify_on_completion", "\"true\"")] {
        let (_dir, path, legacy) = paths();
        let db = Database::open(&path, &legacy).unwrap();
        db.app_settings("en").unwrap();
        db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE app_settings SET value_json = ?1 WHERE setting_key = ?2",
                params![value, key],
            )
            .unwrap();
        assert!(db.app_settings("zh-CN").is_err());
        assert_eq!(
            db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT value_json FROM app_settings WHERE setting_key = ?1",
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            value
        );
    }
}

#[test]
fn failed_preference_write_rolls_back_all_changed_keys() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    let old = db.app_settings("en").unwrap();
    db.connection.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_preference BEFORE INSERT ON app_settings
         WHEN NEW.setting_key = 'notify_on_completion' BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
    ).unwrap();
    let mut next = old.clone();
    next.theme = "dark".into();
    next.notify_on_completion = false;
    assert!(db.save_app_settings(&next).is_err());
    // Remove the injected failure so initialization can read without triggering it.
    db.connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_preference;")
        .unwrap();
    assert_eq!(db.app_settings("zh-CN").unwrap(), old);
}

#[test]
fn inconsistent_program_metadata_cannot_be_loaded_as_a_tool_group() {
    let (_dir, path, legacy) = paths();
    let db = Database::open(&path, &legacy).unwrap();
    db.save_tool(
        RequiredToolId::Ffmpeg,
        &config(RequiredToolId::Ffmpeg, "8.0"),
    )
    .unwrap();
    db.connection.lock().unwrap().execute_batch(
        "UPDATE required_tools SET checked_at = '2026-10-03 08:30:00' WHERE program_name = 'ffprobe';",
    ).unwrap();
    assert!(db.tools().is_err());
}

#[test]
fn legacy_integer_detection_times_are_converted_and_invalid_dates_are_rejected() {
    let (_dir, path, legacy) = paths();
    let mut value = serde_json::to_value(config(RequiredToolId::Deno, "2.5.0")).unwrap();
    value["checkedAt"] = serde_json::json!(123);
    std::fs::write(
        &legacy,
        serde_json::to_vec(&serde_json::json!({"tools": {"deno": value}})).unwrap(),
    )
    .unwrap();
    let db = Database::open(&path, &legacy).unwrap();
    let restored = db.tools().unwrap().tools[&RequiredToolId::Deno].clone();
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["checkedAt"],
        "1970-01-01 08:02:03"
    );
    for invalid in [
        "2026-02-30 00:00:00+00:00",
        "2026-02-30 00:00:00",
        "2026-10-03 08:30:60",
        "2026-10-03 08:30:00+08:00",
        "not a date",
    ] {
        let mut value = serde_json::to_value(&restored).unwrap();
        value["checkedAt"] = serde_json::json!(invalid);
        if let Ok(config) = serde_json::from_value::<RequiredToolConfig>(value) {
            assert!(db.save_tool(RequiredToolId::Deno, &config).is_err());
        }
    }
    assert_eq!(db.tools().unwrap().tools[&RequiredToolId::Deno], restored);
}

#[test]
fn legacy_json_utc_text_is_converted_and_beijing_text_is_not_shifted() {
    for value in ["2026-10-03 06:33:51+00:00", "2026-10-03 14:33:51"] {
        let (_dir, path, legacy) = paths();
        let mut expected = config(RequiredToolId::Deno, "2.5.0");
        expected.checked_at = "2026-10-03 14:33:51".into();
        let mut tool = serde_json::to_value(&expected).unwrap();
        tool["checkedAt"] = serde_json::json!(value);
        let original = serde_json::to_vec(&serde_json::json!({"tools": {"deno": tool}})).unwrap();
        std::fs::write(&legacy, &original).unwrap();
        let db = Database::open(&path, &legacy).unwrap();
        assert_eq!(db.tools().unwrap().tools[&RequiredToolId::Deno], expected);
        assert_eq!(std::fs::read(&legacy).unwrap(), original);
    }
}

#[test]
fn data_directory_remains_available_when_database_loading_fails() {
    let (_dir, path, legacy) = paths();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "broken database").unwrap();
    let storage = Storage::new(&path, &legacy);
    assert!(storage.database().is_err());
    assert_eq!(
        storage.prepare_data_directory().unwrap(),
        path.parent().unwrap()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken database");
}

#[test]
fn previous_database_location_is_imported_with_wal_data_and_preserved() {
    let (dir, path, legacy) = paths();
    let previous = dir
        .path()
        .join("com.minyu.easyvideodownload/EasyVideoDownload/app.db");
    let old = Database::open(&previous, &legacy).unwrap();
    old.connection
        .lock()
        .unwrap()
        .execute_batch("PRAGMA journal_mode=WAL;")
        .unwrap();
    let mut preferences = old.app_settings("en").unwrap();
    preferences.theme = "dark".into();
    old.save_app_settings(&preferences).unwrap();
    old.save_tool(RequiredToolId::Deno, &config(RequiredToolId::Deno, "2.5.0"))
        .unwrap();
    let storage = Storage::new_with_previous(&path, &legacy, &previous);
    let restored = storage.database().unwrap();
    assert_eq!(restored.app_settings("zh-CN").unwrap(), preferences);
    assert_eq!(
        restored.tools().unwrap().tools[&RequiredToolId::Deno],
        config(RequiredToolId::Deno, "2.5.0")
    );
    assert!(previous.is_file());
    assert_eq!(old.app_settings("zh-CN").unwrap(), preferences);
}

#[test]
fn existing_destination_database_is_never_overwritten_by_path_migration() {
    let (dir, path, legacy) = paths();
    let previous = dir.path().join("previous/app.db");
    let old = Database::open(&previous, &legacy).unwrap();
    let mut old_preferences = old.app_settings("en").unwrap();
    old_preferences.theme = "dark".into();
    old.save_app_settings(&old_preferences).unwrap();
    let destination = Database::open(&path, &legacy).unwrap();
    let saved = destination.app_settings("zh-CN").unwrap();
    drop(destination);
    let storage = Storage::new_with_previous(&path, &legacy, &previous);
    assert_eq!(
        storage.database().unwrap().app_settings("en").unwrap(),
        saved
    );
}

#[test]
fn failed_path_migration_does_not_leave_an_empty_destination_database() {
    let (dir, path, legacy) = paths();
    let previous = dir.path().join("previous/app.db");
    std::fs::create_dir_all(previous.parent().unwrap()).unwrap();
    std::fs::write(&previous, "broken database").unwrap();
    let storage = Storage::new_with_previous(&path, &legacy, &previous);
    assert!(storage.database().is_err());
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(previous).unwrap(),
        "broken database"
    );
}

use super::*;
use crate::required_tools::{Program, RequiredToolSource};

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
            notify_on_failure: true,
            close_action: "ask".into(),
        }
    );
    let preferences = AppSettings {
        locale: "en".into(),
        theme: "dark".into(),
        notify_on_completion: false,
        notify_on_failure: false,
        close_action: "tray".into(),
    };
    db.save_app_settings(&preferences).unwrap();
    drop(db);
    let reopened = Database::open(&path, &legacy).unwrap();
    assert_eq!(reopened.app_settings("zh-CN").unwrap(), preferences);
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

fn version_one_database(path: &Path) -> Connection {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(include_str!("../../migrations/001_settings.sql"))
        .unwrap();
    connection
        .execute_batch("PRAGMA user_version = 1;")
        .unwrap();
    connection
}

fn version_two_database(path: &Path) -> Connection {
    let connection = version_one_database(path);
    connection
        .execute_batch(include_str!("../../migrations/002_settings_key_value.sql"))
        .unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    connection
}

#[test]
fn version_two_times_become_beijing_time_once_without_changing_configuration() {
    let (_dir, path, legacy) = paths();
    let connection = version_two_database(&path);
    connection.execute_batch(
        "INSERT INTO app_settings VALUES ('theme', '\"dark\"', '2026-10-03 20:15:30+00:00');
         INSERT INTO app_settings VALUES ('future_setting', '{\"limit\":4}', '2026-10-03 06:33:51+00:00');",
    ).unwrap();
    let mut expected = config(RequiredToolId::Ffmpeg, "8.0");
    expected.checked_at = "2026-10-03 14:33:51".into();
    for program in &expected.programs {
        connection.execute(
            "INSERT INTO required_tools VALUES ('ffmpeg', ?1, 'manual', ?2, ?3, ?4, '2026-10-03 06:33:51+00:00')",
            params![program.name, expected.manual_path, program.path.to_string_lossy(), program.version],
        ).unwrap();
    }
    drop(connection);

    for _ in 0..2 {
        let db = Database::open(&path, &legacy).unwrap();
        assert_eq!(db.tools().unwrap().tools[&RequiredToolId::Ffmpeg], expected);
        assert_eq!(db.app_settings("en").unwrap().theme, "dark");
        let connection = db.connection.lock().unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT updated_at FROM app_settings WHERE setting_key = 'theme'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "2026-10-04 04:15:30"
        );
        assert_eq!(connection.query_row(
            "SELECT value_json, updated_at FROM app_settings WHERE setting_key = 'future_setting'", [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).unwrap(), ("{\"limit\":4}".into(), "2026-10-03 14:33:51".into()));
    }
}

#[test]
fn invalid_version_two_times_roll_back_the_whole_migration() {
    for table in ["app_settings", "required_tools"] {
        for invalid in ["2026-02-30 00:00:00+00:00", "9999-12-31 23:00:00+00:00"] {
            let (_dir, path, legacy) = paths();
            let connection = version_two_database(&path);
            connection.execute_batch(
                "INSERT INTO app_settings VALUES ('theme', '\"dark\"', '2026-10-03 06:33:51+00:00');",
            ).unwrap();
            let tool = config(RequiredToolId::Deno, "2.5.0");
            connection.execute(
                "INSERT INTO required_tools VALUES ('deno', 'deno', 'manual', ?1, ?2, '2.5.0', '2026-10-03 06:33:51+00:00')",
                params![tool.manual_path, tool.programs[0].path.to_string_lossy()],
            ).unwrap();
            let update = match table {
                "app_settings" => "UPDATE app_settings SET updated_at = ?1",
                _ => "UPDATE required_tools SET checked_at = ?1",
            };
            connection.execute(update, [invalid]).unwrap();
            drop(connection);
            assert!(
                Database::open(&path, &legacy).is_err(),
                "{table}: {invalid}"
            );
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                2
            );
            let dates: (String, String) = connection
                .query_row(
                    "SELECT updated_at, checked_at FROM app_settings, required_tools",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let valid = "2026-10-03 06:33:51+00:00";
            assert_eq!(
                dates,
                if table == "app_settings" {
                    (invalid.into(), valid.into())
                } else {
                    (valid.into(), invalid.into())
                }
            );
            connection.execute(update, [valid]).unwrap();
            drop(connection);
            assert_eq!(
                Database::open(&path, &legacy)
                    .unwrap()
                    .tools()
                    .unwrap()
                    .tools[&RequiredToolId::Deno]
                    .checked_at,
                "2026-10-03 14:33:51"
            );
        }
    }
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
fn version_one_migration_preserves_preferences_programs_and_original_times() {
    let (_dir, path, legacy) = paths();
    let connection = version_one_database(&path);
    connection
        .execute_batch(
            "INSERT INTO app_settings VALUES (1, 'en', 'dark', 0, 1, 'tray', 1700000000);",
        )
        .unwrap();
    for (id, name, version) in [
        (RequiredToolId::Ytdlp, "ytdlp", "2026.09.25"),
        (RequiredToolId::Ffmpeg, "ffmpeg", "8.0"),
        (RequiredToolId::Deno, "deno", "2.5.0"),
    ] {
        let config = config(id, version);
        connection
            .execute(
                "INSERT INTO required_tools VALUES (?1, 'manual', ?2, 123)",
                params![name, config.manual_path],
            )
            .unwrap();
        for program in config.programs {
            connection
                .execute(
                    "INSERT INTO required_tool_programs VALUES (?1, ?2, ?3, ?4)",
                    params![
                        name,
                        program.name,
                        program.path.to_string_lossy(),
                        program.version
                    ],
                )
                .unwrap();
        }
    }
    drop(connection);

    let db = Database::open(&path, &legacy).unwrap();
    let settings = db.app_settings("zh-CN").unwrap();
    assert_eq!(settings.locale, "en");
    assert_eq!(settings.theme, "dark");
    assert!(!settings.notify_on_completion);
    assert!(settings.notify_on_failure);
    assert_eq!(settings.close_action, "tray");
    let tools = db.tools().unwrap();
    assert_eq!(tools.tools.len(), 3);
    assert_eq!(tools.tools[&RequiredToolId::Ffmpeg].programs.len(), 2);
    assert_eq!(
        serde_json::to_value(&tools.tools[&RequiredToolId::Ffmpeg]).unwrap()["checkedAt"],
        "1970-01-01 08:02:03"
    );
    let connection = db.connection.lock().unwrap();
    let dates = connection
        .prepare("SELECT updated_at FROM app_settings")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(dates, vec!["2023-11-15 06:13:20"; 5]);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM required_tools", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='required_tool_programs'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(connection);
    drop(db);
    assert_eq!(
        Database::open(&path, &legacy)
            .unwrap()
            .app_settings("zh-CN")
            .unwrap(),
        settings
    );
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
    for (key, value) in [("theme", "\"unknown\""), ("notify_on_failure", "\"true\"")] {
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
         WHEN NEW.setting_key = 'notify_on_failure' BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
    ).unwrap();
    let mut next = old.clone();
    next.theme = "dark".into();
    next.notify_on_failure = false;
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
fn failed_version_one_migration_keeps_original_schema_and_can_be_retried() {
    let (_dir, path, legacy) = paths();
    let connection = version_one_database(&path);
    connection
        .execute_batch(
            "INSERT INTO app_settings VALUES (1, 'en', 'dark', 0, 1, 'tray', 9223372036854775807);",
        )
        .unwrap();
    drop(connection);
    assert!(Database::open(&path, &legacy).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT theme FROM app_settings WHERE id = 1", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "dark"
    );
    connection
        .execute_batch("UPDATE app_settings SET updated_at = 1700000000;")
        .unwrap();
    drop(connection);
    assert_eq!(
        Database::open(&path, &legacy)
            .unwrap()
            .app_settings("zh-CN")
            .unwrap()
            .theme,
        "dark"
    );
}

#[test]
fn incomplete_version_one_tool_group_blocks_migration_without_losing_the_group() {
    let (_dir, path, legacy) = paths();
    let connection = version_one_database(&path);
    connection
        .execute_batch("INSERT INTO required_tools VALUES ('ffmpeg', 'path', NULL, 123);")
        .unwrap();
    drop(connection);
    assert!(Database::open(&path, &legacy).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM required_tools WHERE tool_id='ffmpeg'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
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

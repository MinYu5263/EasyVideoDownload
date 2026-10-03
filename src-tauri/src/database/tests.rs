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
        checked_at: 123,
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
            "CREATE TRIGGER fail_program BEFORE INSERT ON required_tool_programs
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

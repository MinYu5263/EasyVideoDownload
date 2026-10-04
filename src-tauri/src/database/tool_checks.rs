use super::*;
use crate::required_tools::RequiredToolCheckState;
use std::collections::BTreeMap;

fn key(id: RequiredToolId) -> &'static str {
    match id {
        RequiredToolId::Ytdlp => "tool_check.ytdlp",
        RequiredToolId::Ffmpeg => "tool_check.ffmpeg",
        RequiredToolId::Deno => "tool_check.deno",
    }
}

fn validate(state: &RequiredToolCheckState, code: &str) -> Result<(), StorageError> {
    if (state.source != RequiredToolSource::Manual && !state.manual_path.is_empty())
        || state.manual_path != state.manual_path.trim()
        || state
        .error
        .as_ref()
        .is_some_and(|error| error.code.is_empty())
    {
        return Err(StorageError::new(code, "Invalid stored tool check"));
    }
    Ok(())
}

fn read(
    connection: &Connection,
) -> Result<BTreeMap<RequiredToolId, RequiredToolCheckState>, StorageError> {
    let mut checks = BTreeMap::new();
    for id in [
        RequiredToolId::Ytdlp,
        RequiredToolId::Ffmpeg,
        RequiredToolId::Deno,
    ] {
        let value: Option<String> = connection
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key=?1",
                [key(id)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        if let Some(value) = value {
            let state: RequiredToolCheckState = serde_json::from_str(&value)
                .map_err(|_| StorageError::new("loadFailed", "Invalid stored tool check"))?;
            validate(&state, "loadFailed")?;
            checks.insert(id, state);
        }
    }
    Ok(checks)
}

impl Database {
    pub fn tool_checks(
        &self,
    ) -> Result<BTreeMap<RequiredToolId, RequiredToolCheckState>, StorageError> {
        let connection = self.connection("loadFailed")?;
        read(&connection)
    }

    // Commit the last check and any successful configuration together.
    pub fn save_tool_check(
        &self,
        id: RequiredToolId,
        state: &RequiredToolCheckState,
        config: Option<&RequiredToolConfig>,
    ) -> Result<(), StorageError> {
        validate(state, "saveFailed")?;
        if let Some(config) = config {
            if state.error.is_some()
                || state.source != config.source
                || state.manual_path != config.manual_path
            {
                return Err(StorageError::new(
                    "saveFailed",
                    "Inconsistent tool check and configuration",
                ));
            }
            required_tools::validate_settings(&RequiredToolSettings {
                tools: [(id, config.clone())].into(),
            })
                .map_err(|e| StorageError::new("saveFailed", e.detail))?;
        }
        let value = serde_json::to_string(state).map_err(|e| StorageError::new("saveFailed", e))?;
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        // Preserve malformed records instead of silently replacing them.
        read(&transaction)?;
        if let Some(config) = config {
            write_tool(&transaction, id, config).map_err(|e| StorageError::new("saveFailed", e))?;
        }
        transaction.execute(
            "INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES (?1,?2,?3)
             ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at
             WHERE app_settings.value_json<>excluded.value_json",
            params![key(id), value, datetime::now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        transaction
            .commit()
            .map_err(|e| StorageError::new("saveFailed", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_configuration_and_last_check_roll_back_together() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "tool-check rollback test directory: {}",
            root.path().display()
        );
        let database = Database::open(
            &root.path().join("app.db"),
            &root.path().join("legacy.json"),
        )
            .unwrap();
        let original = RequiredToolConfig {
            source: RequiredToolSource::Path,
            manual_path: String::new(),
            checked_at: datetime::now(),
            programs: vec![Program {
                name: "yt-dlp".into(),
                path: root.path().join("old.exe"),
                version: "2026.09.25".into(),
            }],
        };
        let check = RequiredToolCheckState {
            source: original.source,
            manual_path: String::new(),
            error: None,
        };
        database
            .save_tool_check(RequiredToolId::Ytdlp, &check, Some(&original))
            .unwrap();
        let before = database.tool_checks().unwrap();
        database.connection("test").unwrap().execute_batch(
            "CREATE TRIGGER reject_check BEFORE UPDATE ON app_settings
             WHEN NEW.setting_key='tool_check.ytdlp' BEGIN SELECT RAISE(FAIL, 'test failure'); END;",
        ).unwrap();
        let next = RequiredToolConfig {
            source: RequiredToolSource::Automatic,
            ..original.clone()
        };
        let check = RequiredToolCheckState {
            source: next.source,
            manual_path: String::new(),
            error: None,
        };
        assert!(database
            .save_tool_check(RequiredToolId::Ytdlp, &check, Some(&next))
            .is_err());
        assert_eq!(
            database.tools().unwrap().tools[&RequiredToolId::Ytdlp],
            original
        );
        assert_eq!(database.tool_checks().unwrap(), before);
    }

    #[test]
    fn corrupt_tool_checks_are_reported_and_preserved() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "corrupt-tool-check test directory: {}",
            root.path().display()
        );
        let database = Database::open(
            &root.path().join("app.db"),
            &root.path().join("legacy.json"),
        )
            .unwrap();
        let corrupt = r#"{"source":"unknown","manualPath":"","error":null}"#;
        database
            .connection("test")
            .unwrap()
            .execute(
                "INSERT INTO app_settings(setting_key,value_json) VALUES ('tool_check.deno',?1)",
                [corrupt],
            )
            .unwrap();
        assert_eq!(database.tool_checks().unwrap_err().code, "loadFailed");
        let check = RequiredToolCheckState {
            source: RequiredToolSource::Path,
            manual_path: String::new(),
            error: None,
        };
        assert!(database
            .save_tool_check(RequiredToolId::Deno, &check, None)
            .is_err());
        let preserved: String = database
            .connection("test")
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key='tool_check.deno'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(preserved, corrupt);
    }
}

use crate::required_tools::{
    self, Program, RequiredToolConfig, RequiredToolId, RequiredToolSettings, RequiredToolSource,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize)]
pub struct StorageError {
    pub code: String,
    pub detail: String,
}

impl StorageError {
    pub fn new(code: &str, detail: impl ToString) -> Self {
        Self {
            code: code.into(),
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub locale: String,
    pub theme: String,
    pub notify_on_completion: bool,
    pub notify_on_failure: bool,
    pub close_action: String,
}

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path, legacy: &Path) -> Result<Self, StorageError> {
        let initialize = || -> Result<Connection, StorageError> {
            let parent = path
                .parent()
                .ok_or_else(|| StorageError::new("loadFailed", "missing database directory"))?;
            std::fs::create_dir_all(parent).map_err(|e| StorageError::new("loadFailed", e))?;
            let mut connection =
                Connection::open(path).map_err(|e| StorageError::new("loadFailed", e))?;
            connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(|e| StorageError::new("loadFailed", e))?;
            connection
                .pragma_update(None, "foreign_keys", true)
                .map_err(|e| StorageError::new("loadFailed", e))?;
            // Inspect and migrate under the same lock so simultaneous launches cannot both import JSON.
            let transaction = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|e| StorageError::new("loadFailed", e))?;
            let version: i64 = transaction
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|e| StorageError::new("loadFailed", e))?;
            match version {
                0 => {
                    let old = required_tools::load_settings(legacy)
                        .map_err(|e| StorageError::new("loadFailed", e.detail))?;
                    transaction
                        .execute_batch(include_str!("../migrations/001_settings.sql"))
                        .map_err(|e| StorageError::new("loadFailed", e))?;
                    for (id, config) in old.tools {
                        write_tool(&transaction, id, &config)
                            .map_err(|e| StorageError::new("loadFailed", e))?;
                    }
                    transaction
                        .pragma_update(None, "user_version", 1)
                        .map_err(|e| StorageError::new("loadFailed", e))?;
                }
                1 => {}
                _ => {
                    return Err(StorageError::new(
                        "loadFailed",
                        format!("unsupported database schema version: {version}"),
                    ))
                }
            }
            transaction
                .commit()
                .map_err(|e| StorageError::new("loadFailed", e))?;
            Ok(connection)
        };
        Ok(Self {
            connection: Mutex::new(initialize()?),
        })
    }

    fn connection(&self, code: &str) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|e| StorageError::new(code, e))
    }

    pub fn app_settings(&self, initial_locale: &str) -> Result<AppSettings, StorageError> {
        if !matches!(initial_locale, "zh-CN" | "en") {
            return Err(StorageError::new(
                "invalidSettings",
                "invalid initial locale",
            ));
        }
        let mut connection = self.connection("loadFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let existing = transaction.query_row(
            "SELECT locale, theme, notify_on_completion, notify_on_failure, close_action FROM app_settings WHERE id = 1",
            [], read_app_settings,
        ).optional().map_err(|e| StorageError::new("loadFailed", e))?;
        let settings = existing.unwrap_or_else(|| AppSettings {
            locale: initial_locale.into(),
            theme: "system".into(),
            notify_on_completion: true,
            notify_on_failure: true,
            close_action: "ask".into(),
        });
        validate_preferences(&settings)?;
        transaction.execute(
            "INSERT INTO app_settings (id, locale, updated_at) VALUES (1, ?1, ?2) ON CONFLICT (id) DO NOTHING",
            params![initial_locale, now()],
        ).map_err(|e| StorageError::new("loadFailed", e))?;
        transaction
            .commit()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        Ok(settings)
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        validate_preferences(settings)?;
        let connection = self.connection("saveFailed")?;
        connection.execute(
            "INSERT INTO app_settings (id, locale, theme, notify_on_completion, notify_on_failure, close_action, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET locale=excluded.locale, theme=excluded.theme,
             notify_on_completion=excluded.notify_on_completion, notify_on_failure=excluded.notify_on_failure,
             close_action=excluded.close_action, updated_at=excluded.updated_at",
            params![settings.locale, settings.theme, settings.notify_on_completion, settings.notify_on_failure, settings.close_action, now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }

    pub fn tools(&self) -> Result<RequiredToolSettings, StorageError> {
        let connection = self.connection("loadFailed")?;
        let mut settings = RequiredToolSettings::default();
        let mut statement = connection
            .prepare("SELECT tool_id, source, manual_path, checked_at FROM required_tools")
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(|e| StorageError::new("loadFailed", e))?;
        for row in rows {
            let (id, source, manual_path, checked_at) =
                row.map_err(|e| StorageError::new("loadFailed", e))?;
            let tool_id = match id.as_str() {
                "ytdlp" => RequiredToolId::Ytdlp,
                "ffmpeg" => RequiredToolId::Ffmpeg,
                "deno" => RequiredToolId::Deno,
                _ => return Err(StorageError::new("loadFailed", "invalid tool id")),
            };
            let source = match source.as_str() {
                "path" => RequiredToolSource::Path,
                "manual" => RequiredToolSource::Manual,
                _ => return Err(StorageError::new("loadFailed", "invalid tool source")),
            };
            let mut programs = connection.prepare(
                "SELECT program_name, executable_path, version FROM required_tool_programs WHERE tool_id = ?1 ORDER BY program_name"
            ).map_err(|e| StorageError::new("loadFailed", e))?;
            let programs = programs
                .query_map([id], |row| {
                    Ok(Program {
                        name: row.get(0)?,
                        path: row.get::<_, String>(1)?.into(),
                        version: row.get(2)?,
                    })
                })
                .map_err(|e| StorageError::new("loadFailed", e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| StorageError::new("loadFailed", e))?;
            settings.tools.insert(
                tool_id,
                RequiredToolConfig {
                    source,
                    manual_path: manual_path.unwrap_or_default(),
                    programs,
                    checked_at,
                },
            );
        }
        required_tools::validate_settings(&settings)
            .map_err(|e| StorageError::new("loadFailed", e.detail))?;
        Ok(settings)
    }

    pub fn save_tool(
        &self,
        id: RequiredToolId,
        config: &RequiredToolConfig,
    ) -> Result<(), StorageError> {
        let candidate = RequiredToolSettings {
            tools: [(id, config.clone())].into(),
        };
        required_tools::validate_settings(&candidate)
            .map_err(|e| StorageError::new("saveFailed", e.detail))?;
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        write_tool(&transaction, id, config).map_err(|e| StorageError::new("saveFailed", e))?;
        transaction
            .commit()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn validate_preferences(settings: &AppSettings) -> Result<(), StorageError> {
    if matches!(settings.locale.as_str(), "zh-CN" | "en")
        && matches!(settings.theme.as_str(), "system" | "light" | "dark")
        && matches!(settings.close_action.as_str(), "ask" | "tray" | "exit")
    {
        Ok(())
    } else {
        Err(StorageError::new(
            "invalidSettings",
            "invalid preference value",
        ))
    }
}

fn read_app_settings(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppSettings> {
    Ok(AppSettings {
        locale: row.get(0)?,
        theme: row.get(1)?,
        notify_on_completion: row.get(2)?,
        notify_on_failure: row.get(3)?,
        close_action: row.get(4)?,
    })
}

fn write_tool(
    transaction: &Transaction<'_>,
    id: RequiredToolId,
    config: &RequiredToolConfig,
) -> rusqlite::Result<()> {
    let id = match id {
        RequiredToolId::Ytdlp => "ytdlp",
        RequiredToolId::Ffmpeg => "ffmpeg",
        RequiredToolId::Deno => "deno",
    };
    let source = match config.source {
        RequiredToolSource::Path => "path",
        RequiredToolSource::Manual => "manual",
    };
    let manual_path =
        (config.source == RequiredToolSource::Manual).then_some(config.manual_path.as_str());
    transaction.execute(
        "INSERT INTO required_tools (tool_id, source, manual_path, checked_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (tool_id) DO UPDATE SET source=excluded.source, manual_path=excluded.manual_path, checked_at=excluded.checked_at",
        params![id, source, manual_path, config.checked_at],
    )?;
    transaction.execute(
        "DELETE FROM required_tool_programs WHERE tool_id = ?1",
        [id],
    )?;
    for program in &config.programs {
        transaction.execute(
            "INSERT INTO required_tool_programs (tool_id, program_name, executable_path, version) VALUES (?1, ?2, ?3, ?4)",
            params![id, program.name, program.path.to_string_lossy(), program.version],
        )?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct Storage {
    database: Result<Arc<Database>, StorageError>,
}

impl Storage {
    pub fn new(path: &Path, legacy: &Path) -> Self {
        Self {
            database: Database::open(path, legacy).map(Arc::new),
        }
    }

    pub fn database(&self) -> Result<Arc<Database>, StorageError> {
        self.database.clone()
    }
}

#[tauri::command]
pub async fn get_app_settings(
    initial_locale: String,
    state: tauri::State<'_, Storage>,
) -> Result<AppSettings, StorageError> {
    let database = state.database()?;
    tauri::async_runtime::spawn_blocking(move || database.app_settings(&initial_locale))
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}

#[tauri::command]
pub async fn save_app_settings(
    settings: AppSettings,
    state: tauri::State<'_, Storage>,
) -> Result<AppSettings, StorageError> {
    let database = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        database.save_app_settings(&settings)?;
        Ok(settings)
    })
    .await
    .map_err(|e| StorageError::new("saveFailed", e))?
}

#[cfg(test)]
mod tests;

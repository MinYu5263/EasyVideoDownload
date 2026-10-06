use crate::datetime;
use crate::proxy::ProxySettings;
use crate::required_tools::{
    self, Program, RequiredToolConfig, RequiredToolId, RequiredToolSettings, RequiredToolSource,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tauri_plugin_opener::OpenerExt;

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
    pub close_action: String,
    pub max_concurrent_downloads: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsResponse {
    #[serde(flatten)]
    settings: AppSettings,
    close_background_mode: crate::app_preferences::CloseBackgroundMode,
}

impl From<AppSettings> for AppSettingsResponse {
    fn from(settings: AppSettings) -> Self {
        Self {
            settings,
            close_background_mode: crate::app_preferences::close_background_mode(),
        }
    }
}

pub(crate) const DEFAULT_DOWNLOAD_LIMIT: usize = 3;

// The first-release schema starts at 0. Increase only with a corresponding migration;
// application releases and packaging do not change the database version automatically.
const SCHEMA_VERSION: i64 = 0;
// Fixed file identity (ASCII "EVD1"), independent of schema and application versions.
const DATABASE_APPLICATION_ID: i32 = 0x45564431;

pub struct Database {
    connection: Mutex<Connection>,
}

pub(crate) mod download_records;
pub(crate) mod page_states;
pub(crate) mod platform_directories;
pub(crate) mod platform_settings;
pub(crate) mod tool_checks;
pub(crate) mod ui_preferences;

#[cfg(test)]
pub(crate) mod persistence_tests;

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
            // Inspect and initialize under one lock so simultaneous launches cannot both import JSON.
            let transaction = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|e| StorageError::new("loadFailed", e))?;
            let version: i64 = transaction
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|e| StorageError::new("loadFailed", e))?;
            let application_id: i32 = transaction
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .map_err(|e| StorageError::new("loadFailed", e))?;
            if application_id != 0 && application_id != DATABASE_APPLICATION_ID {
                return Err(StorageError::new("loadFailed", "unexpected database application id"));
            }
            // SQLite's default version is also 0, so only an empty schema is a new database.
            let has_schema: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| StorageError::new("loadFailed", e))?;
            match version {
                0 if !has_schema => {
                    let imported_tools = required_tools::load_settings(legacy)
                        .map_err(|e| StorageError::new("loadFailed", e.detail))?;
                    transaction
                        .execute_batch(include_str!("../migrations/initial.sql"))
                        .map_err(|e| StorageError::new("loadFailed", e))?;
                    for (id, config) in imported_tools.tools {
                        write_tool(&transaction, id, &config)
                            .map_err(|e| StorageError::new("loadFailed", e))?;
                    }
                    transaction
                        .pragma_update(None, "user_version", SCHEMA_VERSION)
                        .map_err(|e| StorageError::new("loadFailed", e))?;
                }
                SCHEMA_VERSION => validate_baseline_tables(&transaction)?,
                // The development baseline had no application identity. Future version 15
                // carries our fixed identity and must not be renumbered by an older app.
                15 if SCHEMA_VERSION == 0 && application_id == 0 => {
                    validate_baseline_tables(&transaction)?;
                    transaction
                        .pragma_update(None, "user_version", SCHEMA_VERSION)
                        .map_err(|e| StorageError::new("loadFailed", e))?;
                }
                _ => {
                    return Err(StorageError::new(
                        "loadFailed",
                        format!("unsupported database schema version: {version}"),
                    ))
                }
            }
            if application_id == 0 {
                transaction
                    .pragma_update(None, "application_id", DATABASE_APPLICATION_ID)
                    .map_err(|e| StorageError::new("loadFailed", e))?;
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
        let settings = read_app_settings(&transaction, initial_locale)?;
        let date = datetime::now();
        for (key, value) in setting_values(&settings) {
            transaction.execute(
                "INSERT INTO app_settings (setting_key, value_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (setting_key) DO NOTHING",
                params![key, value.to_string(), date],
            ).map_err(|e| StorageError::new("loadFailed", e))?;
        }
        transaction
            .commit()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        Ok(settings)
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        validate_preferences(settings)?;
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let date = datetime::now();
        for (key, value) in setting_values(settings) {
            transaction.execute(
                "INSERT INTO app_settings (setting_key, value_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (setting_key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at
                 WHERE app_settings.value_json <> excluded.value_json",
                params![key, value.to_string(), date],
            ).map_err(|e| StorageError::new("saveFailed", e))?;
        }
        transaction
            .commit()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }

    pub(crate) fn download_limit(&self) -> Result<usize, StorageError> {
        let connection = self.connection("loadFailed")?;
        let value: Option<String> = connection.query_row(
            "SELECT value_json FROM app_settings WHERE setting_key = 'max_concurrent_downloads'",
            [], |row| row.get(0),
        ).optional().map_err(|e| StorageError::new("loadFailed", e))?;
        let limit = match value {
            Some(value) => serde_json::from_str::<usize>(&value)
                .map_err(|e| StorageError::new("loadFailed", e))?,
            None => DEFAULT_DOWNLOAD_LIMIT,
        };
        if !(1..=6).contains(&limit) {
            return Err(StorageError::new(
                "loadFailed",
                "Invalid download concurrency",
            ));
        }
        Ok(limit)
    }

    pub fn proxy_settings(&self) -> Result<Option<ProxySettings>, StorageError> {
        let connection = self.connection("loadFailed")?;
        read_proxy_settings(&connection)
    }

    pub fn proxy_settings_for_editing(&self) -> Result<Option<ProxySettings>, StorageError> {
        let connection = self.connection("loadFailed")?;
        read_stored_proxy_settings(&connection)?
            .map(|settings| settings.normalized_for_editing()
                .map_err(|_| StorageError::new("loadFailed", "Invalid stored proxy settings")))
            .transpose()
    }

    pub fn save_proxy_settings(
        &self,
        settings: Option<&ProxySettings>,
    ) -> Result<(), StorageError> {
        let settings = settings.map(ProxySettings::normalized).transpose()?;
        let value =
            serde_json::to_string(&settings).map_err(|e| StorageError::new("saveFailed", e))?;
        self.connection("saveFailed")?.execute(
            "INSERT INTO app_settings (setting_key, value_json, updated_at) VALUES ('proxy', ?1, ?2)
             ON CONFLICT (setting_key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at
             WHERE app_settings.value_json <> excluded.value_json", params![value, datetime::now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
    pub fn tools(&self) -> Result<RequiredToolSettings, StorageError> {
        let connection = self.connection("loadFailed")?;
        read_tools(&connection)
    }

    #[cfg(test)]
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

fn validate_baseline_tables(connection: &Connection) -> Result<(), StorageError> {
    // Prepare without reading rows; reject missing runtime tables/columns before
    // treating version 0 as initialized or renumbering the development baseline.
    for query in [
        "SELECT setting_key,value_json,updated_at FROM app_settings LIMIT 0",
        "SELECT tool_id,program_name,source,manual_path,executable_path,version,checked_at FROM required_tools LIMIT 0",
        "SELECT platform,input_link,video_id,title,thumbnail_url,thumbnail_cache_path,duration_seconds,
                extension,formats_json,selected_format_id,selected_height,selected_fps,cookie_fallback,
                download_directory,directory_customized,parser_fingerprint,parsed_at,updated_at
         FROM download_page_states LIMIT 0",
        "SELECT id,request_id,platform,video_id,source_link,title,thumbnail_url,thumbnail_cache_path,
                duration_seconds,format_id,format_extension,height,fps,selected_size_bytes,
                size_approximate,cookie_fallback,download_directory,output_path,output_extension,
                file_size_bytes,status,error_code,error_detail,started_at,finished_at,updated_at,
                error_stage,failure_kind,deleted_at,file_deleted_at,file_availability,
                successful_format_id,successful_format_extension,successful_height,successful_fps,
                successful_directory,successful_finished_at,output_identity,format_snapshot_json,
                successful_format_snapshot_json,pause_requested
         FROM download_records LIMIT 0",
        "SELECT record_id,directory,path,identity FROM download_temporary_directories LIMIT 0",
    ] {
        connection
            .prepare(query)
            .map_err(|e| StorageError::new("loadFailed", e))?;
    }
    Ok(())
}

fn read_proxy_settings(connection: &Connection) -> Result<Option<ProxySettings>, StorageError> {
    read_stored_proxy_settings(connection)?
        .map(|settings| {
            settings
                .normalized()
                .map_err(|_| StorageError::new("loadFailed", "Invalid stored proxy settings"))
        })
        .transpose()
}

fn read_stored_proxy_settings(connection: &Connection) -> Result<Option<ProxySettings>, StorageError> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM app_settings WHERE setting_key = 'proxy'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| StorageError::new("loadFailed", e))?;
    let settings: Option<ProxySettings> = value
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|_| StorageError::new("loadFailed", "Invalid stored proxy settings"))?
        .flatten();
    Ok(settings)
}

fn validate_preferences(settings: &AppSettings) -> Result<(), StorageError> {
    if matches!(settings.locale.as_str(), "zh-CN" | "en")
        && matches!(settings.theme.as_str(), "system" | "light" | "dark")
        && matches!(settings.close_action.as_str(), "ask" | "tray" | "exit")
        && (1..=6).contains(&settings.max_concurrent_downloads)
    {
        Ok(())
    } else {
        Err(StorageError::new(
            "invalidSettings",
            "invalid preference value",
        ))
    }
}

fn setting_values(settings: &AppSettings) -> [(&'static str, serde_json::Value); 5] {
    [
        ("locale", serde_json::json!(settings.locale)),
        ("theme", serde_json::json!(settings.theme)),
        (
            "notify_on_completion",
            serde_json::json!(settings.notify_on_completion),
        ),
        ("close_action", serde_json::json!(settings.close_action)),
        (
            "max_concurrent_downloads",
            serde_json::json!(settings.max_concurrent_downloads),
        ),
    ]
}

fn read_app_settings(
    connection: &Connection,
    initial_locale: &str,
) -> Result<AppSettings, StorageError> {
    let mut settings = AppSettings {
        locale: initial_locale.into(),
        theme: "system".into(),
        notify_on_completion: true,
        close_action: "ask".into(),
        max_concurrent_downloads: DEFAULT_DOWNLOAD_LIMIT,
    };
    let mut statement = connection
        .prepare("SELECT setting_key, value_json FROM app_settings")
        .map_err(|e| StorageError::new("loadFailed", e))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| StorageError::new("loadFailed", e))?;
    for row in rows {
        let (key, value) = row.map_err(|e| StorageError::new("loadFailed", e))?;
        let invalid =
            |error| StorageError::new("loadFailed", format!("invalid setting {key}: {error}"));
        match key.as_str() {
            "locale" => settings.locale = serde_json::from_str(&value).map_err(invalid)?,
            "theme" => settings.theme = serde_json::from_str(&value).map_err(invalid)?,
            "notify_on_completion" => {
                settings.notify_on_completion = serde_json::from_str(&value).map_err(invalid)?
            }
            "close_action" => {
                settings.close_action = serde_json::from_str(&value).map_err(invalid)?
            }
            "max_concurrent_downloads" => {
                settings.max_concurrent_downloads = serde_json::from_str(&value).map_err(invalid)?
            }
            _ => {} // Future settings survive reads and saves by this application version.
        }
    }
    validate_preferences(&settings)?;
    Ok(settings)
}

fn read_tools(connection: &Connection) -> Result<RequiredToolSettings, StorageError> {
    let query = "SELECT tool_id, source, manual_path, checked_at, program_name, executable_path, version
                 FROM required_tools ORDER BY tool_id, program_name";
    let mut settings = RequiredToolSettings::default();
    let mut statement = connection
        .prepare(query)
        .map_err(|e| StorageError::new("loadFailed", e))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                Program {
                    name: row.get(4)?,
                    path: row.get::<_, String>(5)?.into(),
                    version: row.get(6)?,
                },
            ))
        })
        .map_err(|e| StorageError::new("loadFailed", e))?;
    for row in rows {
        let (id, source, manual_path, checked_at, program) =
            row.map_err(|e| StorageError::new("loadFailed", e))?;
        let id = match id.as_str() {
            "ytdlp" => RequiredToolId::Ytdlp,
            "ffmpeg" => RequiredToolId::Ffmpeg,
            "deno" => RequiredToolId::Deno,
            _ => return Err(StorageError::new("loadFailed", "invalid tool id")),
        };
        let source = match source.as_str() {
            "path" => RequiredToolSource::Path,
            "manual" => RequiredToolSource::Manual,
            "automatic" => RequiredToolSource::Automatic,
            _ => return Err(StorageError::new("loadFailed", "invalid tool source")),
        };
        let manual_path = manual_path.unwrap_or_default();
        let config = settings
            .tools
            .entry(id)
            .or_insert_with(|| RequiredToolConfig {
                source,
                manual_path: manual_path.clone(),
                checked_at: checked_at.clone(),
                programs: Vec::new(),
            });
        if config.source != source
            || config.manual_path != manual_path
            || config.checked_at != checked_at
        {
            return Err(StorageError::new(
                "loadFailed",
                format!("inconsistent metadata for {id:?}"),
            ));
        }
        config.programs.push(program);
    }
    required_tools::validate_settings(&settings)
        .map_err(|e| StorageError::new("loadFailed", e.detail))?;
    Ok(settings)
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
        RequiredToolSource::Automatic => "automatic",
    };
    let manual_path =
        (config.source == RequiredToolSource::Manual).then_some(config.manual_path.as_str());
    transaction.execute("DELETE FROM required_tools WHERE tool_id = ?1", [id])?;
    for program in &config.programs {
        transaction.execute(
            "INSERT INTO required_tools (tool_id, program_name, source, manual_path, executable_path, version, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, program.name, source, manual_path, program.path.to_string_lossy(), program.version, config.checked_at],
        )?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct Storage {
    database: Result<Arc<Database>, StorageError>,
    database_path: PathBuf,
}

impl Storage {
    pub fn new(path: &Path, legacy: &Path) -> Self {
        Self {
            database: Database::open(path, legacy).map(Arc::new),
            database_path: path.to_path_buf(),
        }
    }

    pub fn database(&self) -> Result<Arc<Database>, StorageError> {
        self.database.clone()
    }

    pub fn new_with_previous(path: &Path, legacy: &Path, previous: &Path) -> Self {
        match import_previous_database(path, previous) {
            Ok(()) => Self::new(path, legacy),
            Err(error) => Self {
                database: Err(error),
                database_path: path.to_path_buf(),
            },
        }
    }

    pub(crate) fn prepare_data_directory(&self) -> Result<PathBuf, StorageError> {
        let directory = self
            .database_path
            .parent()
            .ok_or_else(|| StorageError::new("openFailed", "missing application data directory"))?;
        std::fs::create_dir_all(directory).map_err(|e| StorageError::new("openFailed", e))?;
        Ok(directory.to_path_buf())
    }
}

fn import_previous_database(path: &Path, previous: &Path) -> Result<(), StorageError> {
    let failed = |error| StorageError::new("loadFailed", error);
    if path.try_exists().map_err(failed)? || !previous.try_exists().map_err(failed)? {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| StorageError::new("loadFailed", "missing database directory"))?;
    std::fs::create_dir_all(parent).map_err(failed)?;
    let snapshot = tempfile::Builder::new()
        .prefix(".database-migration-")
        .tempfile_in(parent)
        .map_err(failed)?
        .into_temp_path();
    let source = Connection::open_with_flags(previous, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| StorageError::new("loadFailed", e))?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| StorageError::new("loadFailed", e))?;
    // SQLite backup includes committed WAL pages; copying only app.db could lose them.
    source
        .backup(rusqlite::MAIN_DB, &snapshot, None)
        .map_err(|e| StorageError::new("loadFailed", e))?;
    match snapshot.persist_noclobber(path) {
        Ok(()) => Ok(()),
        // Another launch may have initialized the destination while the snapshot was made.
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(StorageError::new("loadFailed", error.error)),
    }
}

#[tauri::command]
pub async fn open_app_data_directory(
    app: tauri::AppHandle,
    state: tauri::State<'_, Storage>,
) -> Result<(), StorageError> {
    let storage = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let directory = storage.prepare_data_directory()?;
        app.opener()
            .open_path(directory.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|e| StorageError::new("openFailed", e))
    })
    .await
    .map_err(|e| StorageError::new("openFailed", e))?
}

#[tauri::command]
pub async fn get_app_settings(
    app: tauri::AppHandle,
    initial_locale: String,
    state: tauri::State<'_, Storage>,
) -> Result<AppSettingsResponse, StorageError> {
    let database = state.database()?;
    let settings =
        tauri::async_runtime::spawn_blocking(move || database.app_settings(&initial_locale))
            .await
            .map_err(|e| StorageError::new("loadFailed", e))??;
    crate::app_preferences::update(&app, &settings);
    Ok(settings.into())
}

#[tauri::command]
pub async fn save_app_settings(
    app: tauri::AppHandle,
    settings: AppSettings,
    state: tauri::State<'_, Storage>,
    downloads: tauri::State<'_, crate::video::download::DownloadManager>,
) -> Result<AppSettingsResponse, StorageError> {
    let database = state.database()?;
    let downloads = downloads.inner().clone();
    let settings = tauri::async_runtime::spawn_blocking(move || {
        downloads.save_settings(&database, &settings)?;
        Ok::<_, StorageError>(settings)
    })
    .await
        .map_err(|e| StorageError::new("saveFailed", e))??;
    crate::app_preferences::update(&app, &settings);
    Ok(settings.into())
}

#[cfg(test)]
mod tests;

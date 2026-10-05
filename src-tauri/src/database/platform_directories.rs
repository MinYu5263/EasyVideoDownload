use super::*;
use crate::cookies::CookiePlatform;

fn directory_key(platform: CookiePlatform) -> String {
    format!("platform.{}.downloadDirectory", platform_name(platform))
}
fn platform_name(platform: CookiePlatform) -> &'static str {
    match platform {
        CookiePlatform::Douyin => "douyin",
        CookiePlatform::Bilibili => "bilibili",
        CookiePlatform::Youtube => "youtube",
    }
}

fn validate_directory(directory: Option<&str>, code: &str) -> Result<(), StorageError> {
    if directory.is_some_and(|value| {
        value.len() > 32768 || value.contains('\0') || !Path::new(value).is_absolute()
    }) {
        return Err(StorageError::new(
            code,
            "Invalid platform download directory",
        ));
    }
    Ok(())
}

impl Database {
    pub fn platform_download_directory(
        &self,
        platform: CookiePlatform,
    ) -> Result<Option<String>, StorageError> {
        let connection = self.connection("loadFailed")?;
        let stored: Option<String> = connection
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key=?1",
                [directory_key(platform)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let directory: Option<String> = if let Some(json) = stored {
            serde_json::from_str(&json)
                .map_err(|e| StorageError::new("invalidDownloadDirectory", e))?
        } else {
            // Read the old per-platform preference until an explicit setting exists.
            connection.query_row(
                "SELECT download_directory FROM download_page_states WHERE platform=?1 AND directory_customized=1",
                [platform_name(platform)], |row| row.get(0),
            ).optional().map_err(|e| StorageError::new("loadFailed", e))?
        };
        validate_directory(directory.as_deref(), "invalidDownloadDirectory")?;
        Ok(directory)
    }

    pub fn save_platform_download_directory(
        &self,
        platform: CookiePlatform,
        directory: Option<&str>,
    ) -> Result<(), StorageError> {
        validate_directory(directory, "invalidDownloadDirectory")?;
        let connection = self.connection("saveFailed")?;
        connection.execute(
            "INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at
             WHERE app_settings.value_json<>excluded.value_json",
            params![directory_key(platform), serde_json::to_string(&directory).map_err(|e| StorageError::new("saveFailed", e))?, datetime::now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
}

#[tauri::command]
pub async fn get_platform_download_directory(
    platform: CookiePlatform,
    state: tauri::State<'_, Storage>,
) -> Result<Option<String>, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || db.platform_download_directory(platform))
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}

#[tauri::command]
pub async fn save_platform_download_directory(
    platform: CookiePlatform,
    directory: Option<String>,
    state: tauri::State<'_, Storage>,
) -> Result<Option<String>, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        db.save_platform_download_directory(platform, directory.as_deref())?;
        Ok(directory)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?
}

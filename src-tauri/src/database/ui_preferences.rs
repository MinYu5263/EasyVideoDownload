use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiPreferences {
    pub download_platform: String,
    pub settings_section: String,
    pub active_page: String,
}
impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            download_platform: "douyin".into(),
            settings_section: "application".into(),
            active_page: "download".into(),
        }
    }
}
impl UiPreferences {
    fn values(&self) -> [(&str, &str); 3] {
        [
            ("download_platform", &self.download_platform),
            ("settings_section", &self.settings_section),
            ("active_page", &self.active_page),
        ]
    }
    fn validate(&self, code: &str) -> Result<(), StorageError> {
        if !matches!(
            self.download_platform.as_str(),
            "douyin" | "bilibili" | "youtube"
        ) || !matches!(
            self.settings_section.as_str(),
            "application" | "tools" | "proxy" | "about"
        ) || !matches!(
            self.active_page.as_str(),
            "download" | "history" | "settings"
        ) {
            return Err(StorageError::new(code, "Invalid interface preferences"));
        }
        Ok(())
    }
}
impl Database {
    pub fn ui_preferences(&self) -> Result<UiPreferences, StorageError> {
        let mut c = self.connection("loadFailed")?;
        let tx = c
            .transaction()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let mut settings = UiPreferences::default();
        for (key, value) in [
            ("download_platform", &mut settings.download_platform),
            ("settings_section", &mut settings.settings_section),
            ("active_page", &mut settings.active_page),
        ] {
            let stored: Option<String> = tx
                .query_row(
                    "SELECT value_json FROM app_settings WHERE setting_key=?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| StorageError::new("loadFailed", e))?;
            if let Some(stored) = stored {
                *value = serde_json::from_str(&stored).map_err(|_| {
                    StorageError::new("loadFailed", "Invalid interface preference type")
                })?;
            }
        }
        settings.validate("loadFailed")?;
        for (key, value) in settings.values() {
            tx.execute("INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES (?1,?2,?3) ON CONFLICT(setting_key) DO NOTHING", params![key, serde_json::to_string(value).unwrap(), datetime::now()]).map_err(|e| StorageError::new("loadFailed", e))?;
        }
        tx.commit()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        Ok(settings)
    }
    pub fn save_ui_preferences(&self, settings: &UiPreferences) -> Result<(), StorageError> {
        settings.validate("invalidSettings")?;
        let mut c = self.connection("saveFailed")?;
        let tx = c
            .transaction()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        for (key, value) in settings.values() {
            tx.execute("INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES (?1,?2,?3) ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at WHERE value_json<>excluded.value_json", params![key, serde_json::to_string(value).unwrap(), datetime::now()]).map_err(|e| StorageError::new("saveFailed", e))?;
        }
        tx.commit().map_err(|e| StorageError::new("saveFailed", e))
    }
}
#[tauri::command]
pub async fn get_ui_preferences(
    state: tauri::State<'_, Storage>,
) -> Result<UiPreferences, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || db.ui_preferences())
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}
#[tauri::command]
pub async fn save_ui_preferences(
    settings: UiPreferences,
    state: tauri::State<'_, Storage>,
) -> Result<UiPreferences, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        db.save_ui_preferences(&settings)?;
        Ok(settings)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?
}

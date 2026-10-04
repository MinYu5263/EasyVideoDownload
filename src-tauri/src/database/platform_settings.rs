use super::*;
use crate::cookies::CookiePlatform;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSettings {
    #[serde(default)]
    pub proxy_enabled: bool,
}

fn setting_key(platform: CookiePlatform) -> &'static str {
    match platform {
        CookiePlatform::Douyin => "platform.douyin",
        CookiePlatform::Bilibili => "platform.bilibili",
        CookiePlatform::Youtube => "platform.youtube",
    }
}

fn read_record(
    connection: &Connection,
    platform: CookiePlatform,
) -> Result<serde_json::Value, StorageError> {
    let json: Option<String> = connection
        .query_row(
            "SELECT value_json FROM app_settings WHERE setting_key=?1",
            [setting_key(platform)],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| StorageError::new("loadFailed", e))?;
    let record = match json {
        Some(json) => {
            serde_json::from_str(&json).map_err(|e| StorageError::new("loadFailed", e))?
        }
        None => serde_json::json!({"proxyEnabled": false}),
    };
    serde_json::from_value::<PlatformSettings>(record.clone())
        .map_err(|e| StorageError::new("loadFailed", e))?;
    Ok(record)
}

impl Database {
    pub fn platform_settings(
        &self,
        platform: CookiePlatform,
    ) -> Result<PlatformSettings, StorageError> {
        let connection = self.connection("loadFailed")?;
        serde_json::from_value(read_record(&connection, platform)?)
            .map_err(|e| StorageError::new("loadFailed", e))
    }

    pub fn save_platform_settings(
        &self,
        platform: CookiePlatform,
        settings: &PlatformSettings,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let mut record = read_record(&transaction, platform)?;
        if settings.proxy_enabled && read_proxy_settings(&transaction)?.is_none() {
            return Err(StorageError::new(
                "proxyNotConfigured",
                "Configure a proxy server before enabling it",
            ));
        }
        record["proxyEnabled"] = settings.proxy_enabled.into();
        transaction.execute(
            "INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES (?1,?2,?3)
             ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at
             WHERE app_settings.value_json<>excluded.value_json",
            params![setting_key(platform), record.to_string(), datetime::now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        transaction
            .commit()
            .map_err(|e| StorageError::new("saveFailed", e))
    }

    pub fn proxy_for_platform(
        &self,
        platform: CookiePlatform,
    ) -> Result<Option<ProxySettings>, StorageError> {
        let mut connection = self.connection("loadFailed")?;
        let transaction = connection
            .transaction()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let settings: PlatformSettings =
            serde_json::from_value(read_record(&transaction, platform)?)
                .map_err(|e| StorageError::new("loadFailed", e))?;
        if !settings.proxy_enabled {
            return Ok(None);
        }
        read_proxy_settings(&transaction)?.map(Some).ok_or_else(|| {
            StorageError::new(
                "proxyNotConfigured",
                "The enabled platform proxy has no server configuration",
            )
        })
    }
}

#[tauri::command]
pub async fn get_platform_settings(
    platform: CookiePlatform,
    state: tauri::State<'_, Storage>,
) -> Result<PlatformSettings, StorageError> {
    let database = state.database()?;
    tauri::async_runtime::spawn_blocking(move || database.platform_settings(platform))
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}

#[tauri::command]
pub async fn save_platform_settings(
    platform: CookiePlatform,
    settings: PlatformSettings,
    state: tauri::State<'_, Storage>,
) -> Result<PlatformSettings, StorageError> {
    let database = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        database.save_platform_settings(platform, &settings)?;
        Ok(settings)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        eprintln!("platform settings test directory: {}", dir.path().display());
        let db =
            Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
        (dir, db)
    }
    fn proxy() -> ProxySettings {
        ProxySettings {
            protocol: "http".into(),
            address: "127.0.0.1".into(),
            port: 7897,
        }
    }
    #[test]
    fn platform_proxy_is_independent_and_restores_after_reopening() {
        let (dir, db) = fixture();
        db.save_proxy_settings(Some(&proxy())).unwrap();
        assert!(db
            .proxy_for_platform(CookiePlatform::Youtube)
            .unwrap()
            .is_none());
        db.save_platform_settings(
            CookiePlatform::Youtube,
            &PlatformSettings {
                proxy_enabled: true,
            },
        )
            .unwrap();
        assert_eq!(
            db.proxy_for_platform(CookiePlatform::Youtube).unwrap(),
            Some(proxy())
        );
        for platform in [CookiePlatform::Douyin, CookiePlatform::Bilibili] {
            assert!(!db.platform_settings(platform).unwrap().proxy_enabled);
            assert!(db.proxy_for_platform(platform).unwrap().is_none());
        }
        drop(db);
        let db =
            Database::open(&dir.path().join("app.db"), &dir.path().join("legacy.json")).unwrap();
        assert!(
            db.platform_settings(CookiePlatform::Youtube)
                .unwrap()
                .proxy_enabled
        );
        let snapshot = db.proxy_for_platform(CookiePlatform::Youtube).unwrap();
        db.save_platform_settings(CookiePlatform::Youtube, &PlatformSettings::default())
            .unwrap();
        assert!(db
            .proxy_for_platform(CookiePlatform::Youtube)
            .unwrap()
            .is_none());
        assert_eq!(snapshot, Some(proxy()));
    }
    #[test]
    fn missing_proxy_rejects_enabling_and_never_silently_falls_back() {
        let (_dir, db) = fixture();
        assert_eq!(
            db.save_platform_settings(
                CookiePlatform::Youtube,
                &PlatformSettings {
                    proxy_enabled: true
                }
            )
                .unwrap_err()
                .code,
            "proxyNotConfigured"
        );
        assert!(
            !db.platform_settings(CookiePlatform::Youtube)
                .unwrap()
                .proxy_enabled
        );
        db.save_proxy_settings(Some(&proxy())).unwrap();
        db.save_platform_settings(
            CookiePlatform::Youtube,
            &PlatformSettings {
                proxy_enabled: true,
            },
        )
            .unwrap();
        db.save_proxy_settings(None).unwrap();
        assert_eq!(
            db.proxy_for_platform(CookiePlatform::Youtube)
                .unwrap_err()
                .code,
            "proxyNotConfigured"
        );
        db.save_platform_settings(CookiePlatform::Youtube, &PlatformSettings::default())
            .unwrap();
        assert!(db
            .proxy_for_platform(CookiePlatform::Youtube)
            .unwrap()
            .is_none());
    }
    #[test]
    fn malformed_platform_settings_are_not_overwritten_and_direct_platforms_ignore_bad_global_proxy() {
        let (_dir, db) = fixture();
        db.connection("test").unwrap().execute("INSERT INTO app_settings(setting_key,value_json) VALUES ('proxy','{}'),('platform.youtube','{\"proxyEnabled\":\"true\"}')", []).unwrap();
        assert!(db.platform_settings(CookiePlatform::Youtube).is_err());
        assert!(db
            .save_platform_settings(CookiePlatform::Youtube, &PlatformSettings::default())
            .is_err());
        assert!(db
            .proxy_for_platform(CookiePlatform::Douyin)
            .unwrap()
            .is_none());
        assert!(db.proxy_settings().is_err());
    }
    #[test]
    fn switch_saves_preserve_future_fields_and_identical_values_do_not_write() {
        let (_dir, db) = fixture();
        db.save_proxy_settings(Some(&proxy())).unwrap();
        db.connection("test").unwrap().execute("INSERT INTO app_settings(setting_key,value_json,updated_at) VALUES ('platform.youtube','{\"future\":3,\"proxyEnabled\":false}','2026-01-01 00:00:00')", []).unwrap();
        db.save_platform_settings(CookiePlatform::Youtube, &PlatformSettings::default())
            .unwrap();
        let timestamp: String = db
            .connection("test")
            .unwrap()
            .query_row(
                "SELECT updated_at FROM app_settings WHERE setting_key='platform.youtube'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(timestamp, "2026-01-01 00:00:00");
        db.save_platform_settings(
            CookiePlatform::Youtube,
            &PlatformSettings {
                proxy_enabled: true,
            },
        )
            .unwrap();
        let json: String = db
            .connection("test")
            .unwrap()
            .query_row(
                "SELECT value_json FROM app_settings WHERE setting_key='platform.youtube'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({"future":3,"proxyEnabled":true})
        );
    }
}

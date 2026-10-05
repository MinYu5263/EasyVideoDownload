use super::*;
use crate::video::VideoFormat;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadPageState {
    pub platform: String,
    pub input_link: String,
    pub video_id: Option<String>,
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_cache_path: Option<String>,
    pub duration_seconds: Option<f64>,
    pub extension: Option<String>,
    pub formats: Vec<VideoFormat>,
    pub selected_format_id: Option<String>,
    pub selected_height: Option<u32>,
    pub selected_fps: Option<f64>,
    pub cookie_fallback: bool,
    pub download_directory: String,
    pub directory_customized: bool,
    pub parser_fingerprint: Option<String>,
    pub parsed_at: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}
impl DownloadPageState {
    pub fn clear_result(&mut self) {
        self.video_id = None;
        self.title = None;
        self.thumbnail_url = None;
        self.thumbnail_cache_path = None;
        self.duration_seconds = None;
        self.extension = None;
        self.formats.clear();
        self.selected_format_id = None;
        self.selected_height = None;
        self.selected_fps = None;
        self.cookie_fallback = false;
        self.parser_fingerprint = None;
        self.parsed_at = None;
    }
    pub fn validate(&self, code: &str) -> Result<(), StorageError> {
        let bad = || StorageError::new(code, "Invalid persisted video snapshot");
        if !matches!(self.platform.as_str(), "douyin" | "bilibili" | "youtube")
            || self.input_link.len() > 65536
            || self.download_directory.len() > 32768
        {
            return Err(bad());
        }
        if self
            .thumbnail_cache_path
            .as_ref()
            .is_some_and(|p| !valid_thumbnail_path(p))
        {
            return Err(bad());
        }
        if !self.updated_at.is_empty() && !datetime::is_valid(&self.updated_at) {
            return Err(bad());
        }
        if self.video_id.is_some() {
            let platform =
                serde_json::from_value(serde_json::json!(self.platform)).map_err(|_| bad())?;
            crate::video::normalize_link(&self.input_link, platform).map_err(|_| bad())?;
            if self.video_id.as_ref().is_none_or(|s| s.trim().is_empty())
                || self.title.as_ref().is_none_or(|s| s.trim().is_empty())
                || self
                .parser_fingerprint
                .as_ref()
                .is_none_or(|s| s.is_empty())
                || self
                .parsed_at
                .as_ref()
                .is_none_or(|s| !datetime::is_valid(s))
                || self.formats.is_empty()
            {
                return Err(bad());
            }
            if self
                .duration_seconds
                .is_some_and(|v| !v.is_finite() || v <= 0.0)
            {
                return Err(bad());
            }
            let mut ids = std::collections::HashSet::new();
            for f in &self.formats {
                if !crate::video::formats::valid_id(&f.format_id)
                    || !ids.insert(&f.format_id)
                    || f.height == Some(0)
                    || f.width == Some(0)
                    || f.bitrate.is_some_and(|v| v == 0 || v > crate::video::formats::MAX_EXACT_INTEGER)
                    || f.native_result_id
                    .as_ref()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
                    || f.fps.is_some_and(|v| !v.is_finite() || v <= 0.0)
                    || f.size_bytes
                    .is_some_and(|v| v == 0 || v > 9_007_199_254_740_991)
                    || (f.size_approximate && f.size_bytes.is_none())
                {
                    return Err(bad());
                }
            }
            let format = self
                .formats
                .iter()
                .find(|f| Some(&f.format_id) == self.selected_format_id.as_ref())
                .ok_or_else(bad)?;
            if format.height != self.selected_height || format.fps != self.selected_fps {
                return Err(bad());
            }
        } else if self.title.is_some()
            || self.thumbnail_url.is_some()
            || self.thumbnail_cache_path.is_some()
            || self.duration_seconds.is_some()
            || self.extension.is_some()
            || !self.formats.is_empty()
            || self.selected_format_id.is_some()
            || self.selected_height.is_some()
            || self.selected_fps.is_some()
            || self.cookie_fallback
            || self.parsed_at.is_some()
            || self.parser_fingerprint.is_some()
        {
            return Err(bad());
        }
        Ok(())
    }
}
pub fn valid_thumbnail_path(value: &str) -> bool {
    value.strip_prefix("thumbnails/").is_some_and(|name| {
        name.len() == 68
            && name.ends_with(".img")
            && name[..64].bytes().all(|b| b.is_ascii_hexdigit())
    })
}
pub fn parser_fingerprint(settings: &RequiredToolSettings) -> Result<String, StorageError> {
    let value: Vec<_> = [RequiredToolId::Ytdlp, RequiredToolId::Deno]
        .into_iter()
        .map(|id| {
            (
                id,
                settings
                    .tools
                    .get(&id)
                    .map(|config| (&config.source, &config.manual_path, &config.programs)),
            )
        })
        .collect();
    serde_json::to_string(&value).map_err(|e| StorageError::new("loadFailed", e))
}
pub fn parser_fingerprint_for_platform(
    settings: &RequiredToolSettings,
    platform: &str,
) -> Result<String, StorageError> {
    if platform == "douyin" {
        Ok("douyin-rust-v1".into())
    } else {
        parser_fingerprint(settings)
    }
}
pub(super) fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadPageState> {
    let json: String = row.get("formats_json")?;
    let mut formats: Vec<VideoFormat> = serde_json::from_str(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
    })?;
    crate::video::formats::normalize_all(&mut formats);
    Ok(DownloadPageState {
        platform: row.get("platform")?,
        input_link: row.get("input_link")?,
        video_id: row.get("video_id")?,
        title: row.get("title")?,
        thumbnail_url: row.get("thumbnail_url")?,
        thumbnail_cache_path: row.get("thumbnail_cache_path")?,
        duration_seconds: row.get("duration_seconds")?,
        extension: row.get("extension")?,
        formats,
        selected_format_id: row.get("selected_format_id")?,
        selected_height: row.get("selected_height")?,
        selected_fps: row.get("selected_fps")?,
        cookie_fallback: row.get("cookie_fallback")?,
        download_directory: row.get("download_directory")?,
        directory_customized: row.get("directory_customized")?,
        parser_fingerprint: row.get("parser_fingerprint")?,
        parsed_at: row.get("parsed_at")?,
        updated_at: row.get("updated_at")?,
    })
}
impl Database {
    pub fn download_page_states(&self) -> Result<Vec<DownloadPageState>, StorageError> {
        let c = self.connection("loadFailed")?;
        let mut statement = c
            .prepare("SELECT * FROM download_page_states ORDER BY platform")
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let states = statement
            .query_map([], from_row)
            .map_err(|e| StorageError::new("loadFailed", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        for state in &states {
            state.validate("loadFailed")?;
        }
        Ok(states)
    }
    pub fn save_download_page_state(
        &self,
        state: &DownloadPageState,
    ) -> Result<DownloadPageState, StorageError> {
        state.validate("invalidSettings")?;
        let mut c = self.connection("saveFailed")?;
        let tx = c
            .transaction()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let old = tx
            .query_row(
                "SELECT * FROM download_page_states WHERE platform=?1",
                [&state.platform],
                from_row,
            )
            .optional()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let mut next = state.clone();
        crate::video::formats::normalize_all(&mut next.formats);
        if let Some(old) = old {
            next.updated_at = old.updated_at.clone();
            if next == old {
                return Ok(old);
            }
        }
        next.updated_at = datetime::now();
        tx.execute("INSERT INTO download_page_states (platform,input_link,video_id,title,thumbnail_url,thumbnail_cache_path,duration_seconds,extension,formats_json,selected_format_id,selected_height,selected_fps,cookie_fallback,download_directory,directory_customized,parser_fingerprint,parsed_at,updated_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
            ON CONFLICT(platform) DO UPDATE SET input_link=excluded.input_link,video_id=excluded.video_id,title=excluded.title,thumbnail_url=excluded.thumbnail_url,thumbnail_cache_path=excluded.thumbnail_cache_path,duration_seconds=excluded.duration_seconds,extension=excluded.extension,formats_json=excluded.formats_json,selected_format_id=excluded.selected_format_id,selected_height=excluded.selected_height,selected_fps=excluded.selected_fps,cookie_fallback=excluded.cookie_fallback,download_directory=excluded.download_directory,directory_customized=excluded.directory_customized,parser_fingerprint=excluded.parser_fingerprint,parsed_at=excluded.parsed_at,updated_at=excluded.updated_at",
                   params![next.platform,next.input_link,next.video_id,next.title,next.thumbnail_url,next.thumbnail_cache_path,next.duration_seconds,next.extension,serde_json::to_string(&next.formats).map_err(|e| StorageError::new("saveFailed", e))?,next.selected_format_id,next.selected_height,next.selected_fps,next.cookie_fallback,next.download_directory,next.directory_customized,next.parser_fingerprint,next.parsed_at,next.updated_at]).map_err(|e| StorageError::new("saveFailed", e))?;
        tx.commit()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(next)
    }
}
#[tauri::command]
pub async fn get_download_page_states(
    state: tauri::State<'_, Storage>,
) -> Result<Vec<DownloadPageState>, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        let settings = db.tools()?;
        let mut states = db.download_page_states()?;
        for state in &mut states {
            let fingerprint = parser_fingerprint_for_platform(&settings, &state.platform)?;
            if state
                .parser_fingerprint
                .as_ref()
                .is_some_and(|old| old != &fingerprint)
            {
                state.clear_result();
                *state = db.save_download_page_state(state)?;
            }
        }
        Ok(states)
    })
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}
#[tauri::command]
pub async fn save_download_page_state(
    snapshot: DownloadPageState,
    state: tauri::State<'_, Storage>,
) -> Result<DownloadPageState, StorageError> {
    let db = state.database()?;
    tauri::async_runtime::spawn_blocking(move || db.save_download_page_state(&snapshot))
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?
}

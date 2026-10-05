use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CookiePlatform {
    Douyin,
    Bilibili,
    Youtube,
}

impl CookiePlatform {
    fn file_name(self) -> &'static str {
        match self {
            Self::Douyin => "douyin_cookies.txt",
            Self::Bilibili => "bilibili_cookies.txt",
            Self::Youtube => "youtube_cookies.txt",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CookieError {
    pub code: String,
    pub detail: String,
}

impl CookieError {
    fn new(code: &str, error: impl ToString) -> Self {
        Self {
            code: code.into(),
            detail: error.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct CookieStore {
    directory: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl CookieStore {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            directory: data_directory.join("cookies"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self, platform: CookiePlatform) -> PathBuf {
        self.directory.join(platform.file_name())
    }

    pub fn load(&self, platform: CookiePlatform) -> Result<String, CookieError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|e| CookieError::new("loadFailed", e))?;
        match fs::read_to_string(self.path(platform)) {
            Ok(contents) => Ok(contents),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(CookieError::new("loadFailed", error)),
        }
    }

    pub fn save(&self, platform: CookiePlatform, contents: &str) -> Result<(), CookieError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|e| CookieError::new("saveFailed", e))?;
        let path = self.path(platform);
        if contents.is_empty() {
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(CookieError::new("saveFailed", error)),
            };
        }
        fs::create_dir_all(&self.directory).map_err(|e| CookieError::new("saveFailed", e))?;
        // Replace the complete file so a failed write cannot leave a truncated Cookie file.
        let mut temporary = tempfile::Builder::new()
            .prefix(".cookie-write-")
            .tempfile_in(&self.directory)
            .map_err(|e| CookieError::new("saveFailed", e))?;
        temporary
            .write_all(contents.as_bytes())
            .map_err(|e| CookieError::new("saveFailed", e))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|e| CookieError::new("saveFailed", e))?;
        temporary
            .persist(path)
            .map_err(|e| CookieError::new("saveFailed", e.error))?;
        Ok(())
    }
}

#[tauri::command]
pub async fn get_cookie_contents(
    platform: CookiePlatform,
    state: tauri::State<'_, CookieStore>,
) -> Result<String, CookieError> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.load(platform))
        .await
        .map_err(|e| CookieError::new("loadFailed", e))?
}

#[tauri::command]
pub async fn save_cookie_contents(
    platform: CookiePlatform,
    contents: String,
    state: tauri::State<'_, CookieStore>,
) -> Result<(), CookieError> {
    let store = state.inner().clone();
    let cleared = contents.is_empty();
    let result = tauri::async_runtime::spawn_blocking(move || store.save(platform, &contents))
        .await
        .map_err(|e| CookieError::new("saveFailed", e)).and_then(|result| result);
    let summary = serde_json::json!({"event":"cookieSaved", "platform":platform, "cleared":cleared,
        "code":result.as_ref().err().map(|error| &error.code), "success":result.is_ok()});
    if result.is_ok() { log::info!("{summary}"); } else { log::warn!("{summary}"); }
    result
}

#[cfg(test)]
mod tests;

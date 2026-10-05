use crate::database::{page_states::valid_thumbnail_path, Storage, StorageError};
use crate::proxy::{platform_proxy, ProxySettings};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_BYTES: usize = 5 * 1024 * 1024;
#[derive(Clone)]
pub struct ThumbnailStore {
    directory: PathBuf,
}
#[derive(Debug, Serialize)]
pub struct ThumbnailData {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}
fn failure(detail: &str) -> StorageError {
    StorageError::new("thumbnailFailed", detail)
}
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
impl ThumbnailStore {
    async fn cache_for_platform(
        &self,
        url: &str,
        platform: crate::cookies::CookiePlatform,
        storage: &Storage,
    ) -> Result<String, StorageError> {
        let proxy = platform_proxy(storage, platform).await?;
        self.cache(url, proxy.as_ref()).await
    }

    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
        }
    }
    fn root(&self) -> Result<PathBuf, StorageError> {
        std::fs::create_dir_all(self.directory.join("thumbnails"))
            .map_err(|_| failure("Unable to create thumbnail directory"))?;
        let base = std::fs::canonicalize(&self.directory)
            .map_err(|_| failure("Unable to locate application directory"))?;
        let root = std::fs::canonicalize(self.directory.join("thumbnails"))
            .map_err(|_| failure("Unable to locate thumbnail directory"))?;
        if !root.starts_with(base) {
            return Err(failure("Thumbnail directory is outside application data"));
        }
        Ok(root)
    }
    pub fn read(&self, path: &str) -> Result<ThumbnailData, StorageError> {
        if !valid_thumbnail_path(path) {
            return Err(failure("Invalid thumbnail path"));
        }
        let root = self.root()?;
        let file = std::fs::canonicalize(root.join(&path[11..]))
            .map_err(|_| failure("Cached thumbnail is unavailable"))?;
        if !file.starts_with(root) {
            return Err(failure("Thumbnail is outside its cache"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(file)
            .map_err(|_| failure("Unable to open thumbnail"))?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| failure("Unable to read thumbnail"))?;
        if bytes.len() > MAX_BYTES {
            return Err(failure("Thumbnail exceeds 5 MiB"));
        }
        let mime = image_mime(&bytes).ok_or_else(|| failure("Unsupported thumbnail image"))?;
        Ok(ThumbnailData { mime, bytes })
    }
    pub async fn cache(
        &self,
        url: &str,
        proxy: Option<&ProxySettings>,
    ) -> Result<String, StorageError> {
        let parsed = url::Url::parse(url).map_err(|_| failure("Invalid thumbnail URL"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(failure("Unsupported thumbnail URL"));
        }
        let key = format!("thumbnails/{:x}.img", Sha256::digest(url.as_bytes()));
        let store = self.clone();
        let cached = key.clone();
        if tauri::async_runtime::spawn_blocking(move || store.read(&cached).is_ok())
            .await
            .unwrap_or(false)
        {
            return Ok(key);
        }
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(12))
            .connect_timeout(Duration::from_secs(5));
        if let Some(proxy) = proxy {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy.url()).map_err(|_| failure("Invalid thumbnail proxy"))?,
            );
        }
        let mut response = builder
            .build()
            .map_err(|_| failure("Unable to initialize thumbnail download"))?
            .get(parsed)
            .send()
            .await
            .map_err(|_| failure("Unable to download thumbnail"))?
            .error_for_status()
            .map_err(|_| failure("Thumbnail server rejected request"))?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err(failure("Thumbnail exceeds 5 MiB"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("Unable to read thumbnail response"))?
        {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err(failure("Thumbnail exceeds 5 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        image_mime(&bytes).ok_or_else(|| failure("Unsupported thumbnail image"))?;
        let store = self.clone();
        let output = key.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let root = store.root()?;
            let mut temp = tempfile::NamedTempFile::new_in(&root)
                .map_err(|_| failure("Unable to create thumbnail cache"))?;
            temp.write_all(&bytes)
                .map_err(|_| failure("Unable to write thumbnail"))?;
            temp.as_file()
                .sync_all()
                .map_err(|_| failure("Unable to sync thumbnail"))?;
            temp.persist(root.join(&output[11..]))
                .map_err(|_| failure("Unable to save thumbnail"))?;
            Ok::<_, StorageError>(())
        })
            .await
            .map_err(|_| failure("Thumbnail cache task failed"))??;
        Ok(key)
    }
}
#[tauri::command]
pub async fn cache_video_thumbnail(
    url: String,
    platform: crate::cookies::CookiePlatform,
    store: tauri::State<'_, ThumbnailStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<String, StorageError> {
    let result = store.cache_for_platform(&url, platform, storage.inner()).await;
    if let Err(error) = &result {
        log::warn!("thumbnailCacheFailed platform={platform:?} code={} detail={}", error.code, crate::app_logs::safe_text(&error.detail));
    }
    result
}
#[tauri::command]
pub async fn get_cached_thumbnail(
    path: String,
    store: tauri::State<'_, ThumbnailStore>,
) -> Result<ThumbnailData, StorageError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.read(&path))
        .await
        .map_err(|_| failure("Thumbnail read task failed"))?
}
#[cfg(test)]
mod tests;

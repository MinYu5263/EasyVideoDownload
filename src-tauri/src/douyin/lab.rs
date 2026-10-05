use super::{cancel::Cancellation, download::ObservedMedia, LabError, LabVideo, ParsedResult};
use crate::{
    cookies::{CookiePlatform, CookieStore},
    database::Storage,
    required_tools::{RequiredToolId, RequiredToolManager, RequiredToolSettings},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabSnapshot {
    pub session_id: String,
    pub revision: u64,
    pub task_id: Option<String>,
    pub phase: String,
    pub parsed: Option<LabVideo>,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub output_path: Option<String>,
    pub observed: Option<ObservedMedia>,
    pub error: Option<LabError>,
    pub default_directory: String,
    pub cookie_configured: bool,
    pub ffprobe_available: bool,
}
#[derive(Clone)]
struct Cache {
    client: reqwest::Client,
    parsed: ParsedResult,
}
struct Inner {
    snapshot: LabSnapshot,
    active: bool,
    exiting: bool,
    cancel: Cancellation,
    cache: Option<Cache>,
}
#[derive(Clone)]
pub(crate) struct LabManager(Arc<Mutex<Inner>>);
impl Default for LabManager {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            snapshot: LabSnapshot {
                session_id: uuid::Uuid::new_v4().to_string(),
                revision: 0,
                task_id: None,
                phase: "idle".into(),
                parsed: None,
                received_bytes: 0,
                total_bytes: None,
                output_path: None,
                observed: None,
                error: None,
                default_directory: String::new(),
                cookie_configured: false,
                ffprobe_available: false,
            },
            active: false,
            exiting: false,
            cancel: Cancellation::default(),
            cache: None,
        })))
    }
}
impl LabManager {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, LabError> {
        self.0
            .lock()
            .map_err(|_| LabError::new("internalError", "Experiment state unavailable"))
    }
    fn begin(inner: &mut Inner, phase: &str) -> Result<(String, Cancellation), LabError> {
        if inner.exiting {
            return Err(LabError::new("exiting", "Application is exiting"));
        }
        if inner.active {
            return Err(LabError::new("busy", "An experiment is already running"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        inner.active = true;
        inner.cancel = Cancellation::default();
        let s = &mut inner.snapshot;
        s.task_id = Some(id.clone());
        s.phase = phase.into();
        s.revision += 1;
        s.received_bytes = 0;
        s.total_bytes = None;
        s.output_path = None;
        s.observed = None;
        s.error = None;
        Ok((id, inner.cancel.clone()))
    }
    fn start(&self, phase: &str) -> Result<(String, Cancellation), LabError> {
        let mut inner = self.lock()?;
        let started = Self::begin(&mut inner, phase)?;
        if phase == "parsing" {
            inner.cache = None;
            inner.snapshot.parsed = None;
        }
        Ok(started)
    }
    fn start_download(
        &self,
        result_id: &str,
        format_id: &str,
    ) -> Result<(String, Cancellation, Cache, super::Candidate), LabError> {
        let mut inner = self.lock()?;
        let cache = inner
            .cache
            .as_ref()
            .filter(|c| c.parsed.video.result_id == result_id)
            .cloned()
            .ok_or_else(|| LabError::new("resultExpired", "Parse the link again"))?;
        let candidate = cache
            .parsed
            .candidates
            .iter()
            .find(|c| c.format.id == format_id)
            .cloned()
            .ok_or_else(|| LabError::new("formatExpired", "Select an available format"))?;
        let (id, cancel) = Self::begin(&mut inner, "downloading")?;
        Ok((id, cancel, cache, candidate))
    }
    fn snapshot(&self) -> Result<LabSnapshot, LabError> {
        Ok(self.lock()?.snapshot.clone())
    }
    fn change(&self, id: &str, work: impl FnOnce(&mut LabSnapshot)) {
        if let Ok(mut inner) = self.lock() {
            if inner.active
                && inner.snapshot.task_id.as_deref() == Some(id)
                && inner.snapshot.phase != "cancelling"
            {
                work(&mut inner.snapshot);
                inner.snapshot.revision += 1;
            }
        }
    }
    fn finish(&self, id: &str, result: Result<Outcome, LabError>) {
        if let Ok(mut inner) = self.lock() {
            if !inner.active || inner.snapshot.task_id.as_deref() != Some(id) {
                return;
            }
            match result {
                Ok(Outcome::Parsed(cache)) if inner.cancel.check().is_ok() => {
                    inner.snapshot.parsed = Some(cache.parsed.video.clone());
                    inner.cache = Some(cache);
                    inner.snapshot.phase = "ready".into();
                }
                // Publication and cancellation are serialized by Cancellation::commit.
                Ok(Outcome::Downloaded(path, media)) => {
                    inner.snapshot.output_path = Some(path);
                    inner.snapshot.observed = Some(media);
                    inner.snapshot.phase = "completed".into();
                }
                Ok(_) => {
                    inner.snapshot.phase = "cancelled".into();
                }
                Err(error) => {
                    inner.snapshot.phase = if error.code == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                        .into();
                    inner.snapshot.error = if error.code == "cancelled" {
                        None
                    } else {
                        Some(error)
                    };
                }
            }
            inner.active = false;
            inner.snapshot.revision += 1;
        }
    }
    fn cancel_task(&self, id: &str) -> Result<LabSnapshot, LabError> {
        let mut inner = self.lock()?;
        if inner.active && inner.snapshot.task_id.as_deref() == Some(id) {
            inner.cancel.cancel();
            inner.snapshot.phase = "cancelling".into();
            inner.snapshot.revision += 1;
        }
        Ok(inner.snapshot.clone())
    }
    pub(crate) fn begin_exit(&self) -> Result<(), String> {
        let mut inner = self.lock().map_err(|e| e.detail)?;
        inner.exiting = true;
        if inner.active {
            inner.cancel.cancel();
            inner.snapshot.phase = "cancelling".into();
            inner.snapshot.revision += 1;
        }
        Ok(())
    }
    pub(crate) fn is_active(&self) -> Result<bool, String> {
        Ok(self.lock().map_err(|e| e.detail)?.active)
    }
    pub(crate) fn abort_exit(&self) {
        if let Ok(mut inner) = self.lock() {
            inner.exiting = false;
        }
    }
}
enum Outcome {
    Parsed(Cache),
    Downloaded(String, ObservedMedia),
}
fn emit(app: &tauri::AppHandle, manager: &LabManager) {
    if let Ok(snapshot) = manager.snapshot() {
        let _ = app.emit_to("main", "douyin-lab-changed", snapshot);
    }
}
fn probe_path(settings: &RequiredToolSettings) -> Option<std::path::PathBuf> {
    settings
        .tools
        .get(&RequiredToolId::Ffmpeg)?
        .programs
        .iter()
        .find(|p| p.name == "ffprobe" && p.path.is_file())
        .map(|p| p.path.clone())
}

#[tauri::command]
pub async fn douyin_lab_get_state(
    app: tauri::AppHandle,
    state: tauri::State<'_, LabManager>,
    cookies: tauri::State<'_, CookieStore>,
    tools: tauri::State<'_, RequiredToolManager>,
) -> Result<LabSnapshot, LabError> {
    let directory = app
        .path()
        .video_dir()
        .or_else(|_| app.path().download_dir())
        .map_err(|_| LabError::new("fileFailed", "Unable to locate default directory"))?
        .join("EasyVideoDownload")
        .join("douyin-lab");
    let store = cookies.inner().clone();
    let configured = tauri::async_runtime::spawn_blocking(move || {
        store
            .load(CookiePlatform::Douyin)
            .ok()
            .is_some_and(|c| super::cookies::cookie_jar(&c).is_ok())
    })
        .await
        .unwrap_or(false);
    let available = tools
        .settings_snapshot()
        .ok()
        .and_then(|s| probe_path(&s))
        .is_some();
    let mut inner = state.lock()?;
    inner.snapshot.default_directory = directory.to_string_lossy().into_owned();
    inner.snapshot.cookie_configured = configured;
    inner.snapshot.ffprobe_available = available;
    inner.snapshot.revision += 1;
    Ok(inner.snapshot.clone())
}

#[tauri::command]
pub async fn douyin_lab_parse(
    url: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, LabManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<LabSnapshot, LabError> {
    super::normalize_link(&url)?;
    let manager = state.inner().clone();
    let (id, cancel) = manager.start("parsing")?;
    let store = cookies.inner().clone();
    let storage = storage.inner().clone();
    emit(&app, &manager);
    tauri::async_runtime::spawn(async move {
        let result = cancel
            .run(async {
                let contents = tauri::async_runtime::spawn_blocking(move || {
                    store.load(CookiePlatform::Douyin)
                })
                    .await
                    .map_err(|_| LabError::new("cookieFailed", "Unable to read Cookie configuration"))?
                    .map_err(|_| {
                        LabError::new("cookieFailed", "Unable to read Cookie configuration")
                    })?;
                let proxy = crate::proxy::platform_proxy(&storage, CookiePlatform::Douyin)
                    .await
                    .map_err(|_| {
                        LabError::new("proxyFailed", "Unable to read proxy configuration")
                    })?;
                let client = super::http::client(&contents, proxy.as_ref())?;
                let parsed = super::http::parse_video(&client, &url).await?;
                Ok(Outcome::Parsed(Cache { client, parsed }))
            })
            .await;
        manager.finish(&id, result);
        emit(&app, &manager);
    });
    state.snapshot()
}

#[tauri::command]
pub async fn douyin_lab_download(
    result_id: String,
    format_id: String,
    directory: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, LabManager>,
    tools: tauri::State<'_, RequiredToolManager>,
) -> Result<LabSnapshot, LabError> {
    let (settings, lease) = tools
        .settings_and_usage()
        .map_err(|_| LabError::new("toolFailed", "Unable to reserve configured tools"))?;
    let ffprobe = probe_path(&settings)
        .ok_or_else(|| LabError::new("ffprobeMissing", "Configure ffprobe in Settings first"))?;
    let manager = state.inner().clone();
    let (id, cancel, cache, candidate) = manager.start_download(&result_id, &format_id)?;
    emit(&app, &manager);
    tauri::async_runtime::spawn(async move {
        let _lease = lease;
        let result = super::download::download(
            &cache.client,
            &cache.parsed,
            &candidate,
            std::path::Path::new(&directory),
            &ffprobe,
            &cancel,
            |received, total| {
                manager.change(&id, |s| {
                    s.received_bytes = received;
                    s.total_bytes = total;
                });
                emit(&app, &manager);
            },
            || {
                manager.change(&id, |s| s.phase = "verifying".into());
                emit(&app, &manager);
            },
        )
            .await
            .map(|(path, media)| Outcome::Downloaded(path, media));
        manager.finish(&id, result);
        emit(&app, &manager);
    });
    state.snapshot()
}

#[tauri::command]
pub async fn douyin_lab_cancel(
    task_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, LabManager>,
) -> Result<LabSnapshot, LabError> {
    let snapshot = state.cancel_task(&task_id)?;
    emit(&app, &state);
    Ok(snapshot)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_task_at_a_time_and_exit_blocks_new_work() {
        let manager = LabManager::default();
        manager.start("parsing").unwrap();
        assert!(manager.start("parsing").is_err());
        manager.begin_exit().unwrap();
        assert!(manager.start("parsing").is_err());
        assert!(manager.is_active().unwrap());
    }
    #[test]
    fn cancelled_parse_and_stale_task_cannot_replace_current_result() {
        let manager = LabManager::default();
        let (old, token) = manager.start("parsing").unwrap();
        manager.cancel_task("other-task").unwrap();
        token.check().unwrap();
        manager.cancel_task(&old).unwrap();
        assert!(token.check().is_err());
        let cache = Cache { client: reqwest::Client::new(), parsed: super::super::parse_detail(&serde_json::json!({"aweme_detail":{"aweme_id":"1","video":{"play_addr":{"url_list":["https://media.test/a"]}}}})).unwrap() };
        manager.finish(&old, Ok(Outcome::Parsed(cache)));
        assert_eq!(manager.snapshot().unwrap().phase, "cancelled");
        assert!(manager.snapshot().unwrap().parsed.is_none());
        let (new, _) = manager.start("parsing").unwrap();
        manager.finish(
            &old,
            Err(LabError::new("networkFailed", "Late old response")),
        );
        assert_eq!(
            manager.snapshot().unwrap().task_id.as_deref(),
            Some(new.as_str())
        );
        assert_eq!(manager.snapshot().unwrap().phase, "parsing");
        manager.begin_exit().unwrap();
        manager.finish(&new, Err(LabError::cancelled()));
        assert!(!manager.is_active().unwrap());
        assert!(manager.start("parsing").is_err());
        manager.abort_exit();
        assert!(manager.start("parsing").is_ok());
    }
}

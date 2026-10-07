mod media;
#[cfg(test)]
mod native_tests;
mod process;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::watch;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSnapshot {
    pub revision: u64,
    pub phase: String,
    pub progress: Option<f64>,
    pub info: Option<media::AudioInfo>,
}

#[derive(Clone)]
struct ParsedSource {
    path: std::path::PathBuf,
    identity: String,
    info: media::AudioInfo,
}

struct Runtime {
    snapshot: AudioSnapshot,
    source: Option<ParsedSource>,
    active: bool,
    exiting: bool,
    cancel: Option<watch::Sender<bool>>,
}

#[derive(Clone)]
pub struct AudioExtractionManager(Arc<Mutex<Runtime>>);

impl Default for AudioExtractionManager {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Runtime {
            snapshot: AudioSnapshot {
                revision: 0,
                phase: "idle".into(),
                progress: None,
                info: None,
            },
            source: None,
            active: false,
            exiting: false,
            cancel: None,
        })))
    }
}

struct Operation {
    manager: AudioExtractionManager,
    app: Option<tauri::AppHandle>,
    cancel: watch::Receiver<bool>,
    finished: bool,
}

impl AudioExtractionManager {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Runtime>, AudioError> {
        self.0.lock().map_err(|e| AudioError::new("stateFailed", e))
    }
    fn snapshot(&self) -> Result<AudioSnapshot, AudioError> {
        Ok(self.lock()?.snapshot.clone())
    }
    fn emit(&self, app: Option<&tauri::AppHandle>) {
        if let Some(app) = app {
            if let Ok(snapshot) = self.snapshot() {
                if let Err(error) = app.emit("audio-extraction-state", &snapshot) {
                    log::error!(
                        "audioStateEmitFailed: {}",
                        crate::app_logs::safe_text(&error.to_string())
                    );
                }
            }
        }
    }
    fn start(
        &self,
        app: Option<tauri::AppHandle>,
        phase: &str,
        reset: bool,
    ) -> Result<Operation, AudioError> {
        let mut runtime = self.lock()?;
        if runtime.exiting {
            return Err(AudioError::new("exiting", "The application is exiting"));
        }
        if runtime.active {
            return Err(AudioError::new(
                "busy",
                "An audio operation is already running",
            ));
        }
        let (sender, cancel) = watch::channel(false);
        runtime.cancel = Some(sender);
        runtime.active = true;
        runtime.snapshot.revision += 1;
        runtime.snapshot.phase = phase.into();
        runtime.snapshot.progress = None;
        if reset {
            runtime.source = None;
            runtime.snapshot.info = None;
        }
        drop(runtime);
        self.emit(app.as_ref());
        Ok(Operation {
            manager: self.clone(),
            app,
            cancel,
            finished: false,
        })
    }
    pub(crate) fn is_active(&self) -> Result<bool, String> {
        self.lock().map(|r| r.active).map_err(|e| e.detail)
    }
    pub(crate) fn begin_exit(&self) -> Result<(), String> {
        let mut runtime = self.lock().map_err(|e| e.detail)?;
        runtime.exiting = true;
        if let Some(cancel) = &runtime.cancel {
            let _ = cancel.send(true);
        }
        Ok(())
    }
    pub(crate) fn abort_exit(&self) {
        if let Ok(mut r) = self.lock() {
            r.exiting = false;
        }
    }
}

impl Operation {
    fn finish(&mut self, phase: &str) -> Result<AudioSnapshot, AudioError> {
        let mut runtime = self.manager.lock()?;
        runtime.active = false;
        runtime.cancel = None;
        runtime.snapshot.revision += 1;
        runtime.snapshot.phase = phase.into();
        runtime.snapshot.progress = if phase == "completed" {
            Some(100.0)
        } else {
            None
        };
        let snapshot = runtime.snapshot.clone();
        self.finished = true;
        drop(runtime);
        self.manager.emit(self.app.as_ref());
        Ok(snapshot)
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish("failed");
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioError {
    pub code: String,
    pub detail: String,
}

impl AudioError {
    fn new(code: &str, detail: impl ToString) -> Self {
        Self {
            code: code.into(),
            detail: crate::app_logs::safe_text(&detail.to_string()),
        }
    }
}

fn programs(
    app: &tauri::AppHandle,
) -> Result<
    (
        std::path::PathBuf,
        std::path::PathBuf,
        crate::required_tools::usage::ToolUsageLease,
    ),
    AudioError,
> {
    use crate::required_tools::{validate_program_files, RequiredToolId, RequiredToolManager};
    let manager = app.state::<RequiredToolManager>();
    let (settings, lease) = manager
        .settings_and_usage()
        .map_err(|e| AudioError::new(&e.code, e.detail))?;
    let config = settings.tools.get(&RequiredToolId::Ffmpeg).ok_or_else(|| {
        AudioError::new(
            "toolsMissing",
            "FFmpeg is not configured; configure FFmpeg in Settings > Required tools",
        )
    })?;
    validate_program_files(RequiredToolId::Ffmpeg, config).map_err(|e| {
        AudioError::new(
            &e.code,
            if e.detail.is_empty() {
                format!("{}: {}", e.program, e.code)
            } else {
                e.detail
            },
        )
    })?;
    let path = |name: &str| {
        config
            .programs
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .path
            .clone()
    };
    Ok((path("ffmpeg"), path("ffprobe"), lease))
}

fn record(
    app: &tauri::AppHandle,
    id: &str,
    event: &str,
    result: &Result<AudioSnapshot, AudioError>,
) {
    let (level, code, message) = match result {
        Ok(snapshot) => (
            "INFO",
            None,
            format!(
                "phase={} tracks={}",
                snapshot.phase,
                snapshot.info.as_ref().map_or(0, |i| i.tracks.len())
            ),
        ),
        Err(error) => ("ERROR", Some(error.code.as_str()), error.detail.clone()),
    };
    crate::app_logs::record(
        app,
        crate::app_logs::entry(level, "local", None, Some(id), event, code, message),
    );
}

#[tauri::command]
pub fn get_audio_extraction_state(
    state: tauri::State<'_, AudioExtractionManager>,
) -> Result<AudioSnapshot, AudioError> {
    state.snapshot()
}

#[tauri::command]
pub fn clear_audio_source(
    app: tauri::AppHandle,
    state: tauri::State<'_, AudioExtractionManager>,
) -> Result<AudioSnapshot, AudioError> {
    let mut runtime = state.lock()?;
    if runtime.active || runtime.exiting {
        return Err(AudioError::new(
            "busy",
            "An audio operation is already running or the application is exiting",
        ));
    }
    runtime.source = None;
    runtime.snapshot.info = None;
    runtime.snapshot.phase = "idle".into();
    runtime.snapshot.progress = None;
    runtime.snapshot.revision += 1;
    let snapshot = runtime.snapshot.clone();
    drop(runtime);
    state.emit(Some(&app));
    Ok(snapshot)
}

#[tauri::command]
pub async fn parse_audio_source(
    app: tauri::AppHandle,
    path: String,
) -> Result<AudioSnapshot, AudioError> {
    let manager = app.state::<AudioExtractionManager>();
    let mut operation = manager.start(Some(app.clone()), "parsing", true)?;
    let id = uuid::Uuid::new_v4().to_string();
    let result = async {
        let (_ffmpeg, ffprobe, _lease) = programs(&app)?;
        let input = std::path::PathBuf::from(path);
        if !input.is_absolute() {
            return Err(AudioError::new(
                "invalidPath",
                "Select a video using its absolute native file path",
            ));
        }
        let path = std::fs::canonicalize(input).map_err(|e| AudioError::new("invalidPath", e))?;
        let identity = crate::database::download_records::identity::capture_checked(&path)
            .map_err(|e| AudioError::new("invalidPath", e))?;
        let value = process::probe(&ffprobe, &path, &mut operation.cancel).await?;
        let video = value["streams"].as_array().is_some_and(|streams| {
            streams.iter().any(|s| {
                s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_u64() != Some(1)
            })
        });
        if !video {
            return Err(AudioError::new(
                "noVideo",
                "The selected file contains no video stream",
            ));
        }
        let tracks = media::parse_tracks(&value)?;
        let default_track = tracks
            .iter()
            .find(|track| track.is_default)
            .unwrap_or(&tracks[0])
            .index;
        if crate::database::download_records::identity::capture_checked(&path)
            .map_err(|e| AudioError::new("sourceChanged", e))?
            != identity
        {
            return Err(AudioError::new(
                "sourceChanged",
                "The selected video changed during parsing",
            ));
        }
        let info = media::AudioInfo {
            id: id.clone(),
            file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            tracks,
            default_track,
        };
        let mut runtime = manager.lock()?;
        runtime.snapshot.info = Some(info.clone());
        runtime.source = Some(ParsedSource {
            path,
            identity,
            info,
        });
        drop(runtime);
        operation.finish("ready")
    }
        .await;
    if result.is_err() {
        let _ = operation.finish("failed");
    }
    record(&app, &id, "audioParsed", &result);
    result
}

#[tauri::command]
pub async fn extract_audio(
    app: tauri::AppHandle,
    source_id: String,
    track_index: u32,
) -> Result<AudioSnapshot, AudioError> {
    let manager = app.state::<AudioExtractionManager>();
    let mut operation = manager.start(Some(app.clone()), "extracting", false)?;
    let result = async {
        let source = manager
            .lock()?
            .source
            .clone()
            .filter(|s| s.info.id == source_id)
            .ok_or_else(|| {
                AudioError::new(
                    "invalidSource",
                    "Select and parse a video before extracting its audio",
                )
            })?;
        let track = source
            .info
            .tracks
            .iter()
            .find(|t| t.index == track_index)
            .ok_or_else(|| {
                AudioError::new(
                    "invalidTrack",
                    "The selected audio track does not exist in the parsed video",
                )
            })?;
        let (ffmpeg, ffprobe, _lease) = programs(&app)?;
        let output = process::extract(
            &ffmpeg,
            &ffprobe,
            &source.path,
            &source.identity,
            track,
            &mut operation.cancel,
            |progress| {
                if let Ok(mut runtime) = manager.lock() {
                    if runtime.snapshot.progress == Some(progress) {
                        return;
                    }
                    runtime.snapshot.progress = Some(progress);
                    runtime.snapshot.revision += 1;
                    drop(runtime);
                    manager.emit(Some(&app));
                }
            },
        )
            .await?;
        log::info!(
            "audioExtracted: source_id={} extension={} bytes={}",
            source_id,
            output.extension().unwrap_or_default().to_string_lossy(),
            std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0)
        );
        operation.finish("completed")
    }
        .await;
    if result.is_err() {
        let _ = operation.finish("failed");
    }
    record(&app, &source_id, "audioExtractionFinished", &result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_admits_only_one_operation_and_releases_on_drop() {
        let manager = AudioExtractionManager::default();
        let operation = manager.start(None, "parsing", true).unwrap();
        assert!(manager.is_active().unwrap());
        assert!(manager.start(None, "parsing", true).is_err());
        drop(operation);
        assert!(!manager.is_active().unwrap());
        assert!(manager.start(None, "parsing", true).is_ok());
    }
    #[test]
    fn exit_cancels_work_and_closes_admission_atomically() {
        let manager = AudioExtractionManager::default();
        let operation = manager.start(None, "extracting", false).unwrap();
        manager.begin_exit().unwrap();
        assert!(*operation.cancel.borrow());
        drop(operation);
        assert!(manager.start(None, "parsing", true).is_err());
        manager.abort_exit();
        assert!(manager.start(None, "parsing", true).is_ok());
    }
}

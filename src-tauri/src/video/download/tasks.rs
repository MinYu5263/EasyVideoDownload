use super::*;
use crate::database::{
    download_records::{tasks::RecordAcceptance, DownloadRecord, DownloadSnapshot},
    page_states::DownloadPageState,
    StorageError,
};
use std::{collections::BTreeMap, sync::Arc};
use tauri::Emitter;
pub(crate) mod commands;
pub(crate) mod scheduler;
#[cfg(test)]
mod tests;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskSnapshot {
    pub record: DownloadRecord,
    pub phase: String,
    pub percent: Option<f64>,
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    pub submission_order: u64,
    pub revision: u64,
    pub storage_error: Option<StorageError>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitDownloadRequest {
    pub snapshot: DownloadPageState,
    #[serde(default)]
    pub restore_trashed: bool,
    #[serde(default)]
    pub redownload: bool,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitDownloadResult {
    pub kind: &'static str,
    pub record: DownloadRecord,
    pub task: Option<DownloadTaskSnapshot>,
}
struct Job {
    restart_output: Option<DownloadRecord>,
    execution: Result<super::task_snapshot::TaskExecutionSnapshot, VideoError>,
    history: DownloadRecordSession,
    database: Arc<crate::database::Database>,
}
struct Entry {
    snapshot: DownloadTaskSnapshot,
    database: Arc<crate::database::Database>,
    cancel: watch::Sender<bool>,
    control: Arc<super::control::DownloadControl>,
    resume_phase: &'static str,
    suspended: bool,
    resume_error: Option<VideoError>,
    preserve_pause: bool,
    parked: bool,
    job: Option<Job>,
}
impl Entry {
    fn set_process_paused(&self, id: &str, paused: bool) -> Result<(), VideoError> {
        let code = if paused {
            "pauseFailed"
        } else {
            "resumeFailed"
        };
        self.control
            .set_paused(paused)
            .map_err(|e| error(code, e))?;
        if let Err(storage_error) = self.database.set_download_paused(id, paused) {
            let rollback = self.control.set_paused(!paused);
            return Err(error(
                code,
                match rollback {
                    Ok(()) => storage_error.detail,
                    Err(rollback_error) => format!(
                        "{}; restoring process state failed: {rollback_error}",
                        storage_error.detail
                    ),
                },
            ));
        }
        Ok(())
    }
}
#[derive(Default)]
struct Registry {
    scheduler: scheduler::Scheduler,
    limit_loaded: bool,
    entries: BTreeMap<String, Entry>,
    revision: u64,
    order: u64,
    exiting: bool,
    app: Option<tauri::AppHandle>,
}
#[derive(Clone, Default)]
pub struct DownloadManager {
    inner: Arc<Mutex<Registry>>,
    pub(super) submission: Arc<tokio::sync::Mutex<()>>,
}
fn active(phase: &str) -> bool {
    matches!(
        phase,
        "queued" | "preparing" | "downloading" | "paused" | "processing" | "cancelling"
    )
}

fn log_task(app: &tauri::AppHandle, update: &DownloadTaskSnapshot) {
    let record = &update.record;
    let mut entry = crate::app_logs::entry(
        if update.phase == "failed" {
            "error"
        } else {
            "info"
        },
        &record.platform,
        Some(&record.video_id),
        Some(&record.request_id),
        "downloadStage",
        record.error_code.as_deref(),
        format!(
            "{} · format={}{}",
            update.phase,
            record.format_id,
            record
                .error_detail
                .as_ref()
                .map(|detail| format!("\n{detail}"))
                .unwrap_or_default()
        ),
    );
    entry.revision = Some(update.revision);
    crate::app_logs::record(app, entry);
    if let Some(error) = &update.storage_error {
        crate::app_logs::record(
            app,
            crate::app_logs::entry(
                "error",
                &record.platform,
                Some(&record.video_id),
                Some(&record.request_id),
                "historySaveFailed",
                Some(&error.code),
                error.detail.clone(),
            ),
        );
    }
}

impl DownloadManager {
    fn initialize_limit(&self, database: &crate::database::Database) -> Result<(), VideoError> {
        let mut state = self.inner.lock().map_err(|e| error("bridgeFailed", e))?;
        if !state.limit_loaded {
            let limit = database.download_limit().map_err(|e| error("loadFailed", e.detail))?;
            state.scheduler.set_limit(limit);
            state.limit_loaded = true;
        }
        Ok(())
    }

    pub(crate) fn save_settings(
        &self,
        database: &crate::database::Database,
        settings: &crate::database::AppSettings,
    ) -> Result<(), StorageError> {
        let ready = {
            let mut state = self.inner.lock().map_err(|e| StorageError::new("saveFailed", e))?;
            // Serialize persistence and dispatch with submissions and other setting writes.
            // A failed save must leave the live limit and queue unchanged.
            database.save_app_settings(settings)?;
            state.limit_loaded = true;
            state.scheduler.set_limit(settings.max_concurrent_downloads)
        };
        self.launch(ready);
        Ok(())
    }

    fn publish(&self, id: &str, change: impl FnOnce(&mut DownloadTaskSnapshot)) {
        self.publish_entry(id, |entry| change(&mut entry.snapshot));
    }
    fn publish_entry(&self, id: &str, change: impl FnOnce(&mut Entry)) {
        let (app, update, phase_changed) = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            state.revision += 1;
            let revision = state.revision;
            let app = state.app.clone();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            let old_phase = entry.snapshot.phase.clone();
            change(entry);
            entry.snapshot.revision = revision;
            (
                app,
                entry.snapshot.clone(),
                old_phase != entry.snapshot.phase,
            )
        };
        if let Some(app) = app {
            if phase_changed {
                log_task(&app, &update);
            }
            let _ = app.emit("download-task-changed", update);
        }
    }
    pub(super) fn snapshots(&self) -> Result<Vec<DownloadTaskSnapshot>, VideoError> {
        self.inner
            .lock()
            .map(|s| {
                s.entries
                    .values()
                    .filter(|e| !e.parked)
                    .map(|e| e.snapshot.clone())
                    .collect()
            })
            .map_err(|e| error("bridgeFailed", e))
    }
    fn task_for(&self, id: i64) -> Option<DownloadTaskSnapshot> {
        self.inner
            .lock()
            .ok()?
            .entries
            .values()
            .find(|e| e.snapshot.record.id == id)
            .map(|e| e.snapshot.clone())
    }
    fn accept_internal(
        &self,
        app: Option<tauri::AppHandle>,
        record: DownloadRecord,
        job: Job,
    ) -> Result<DownloadTaskSnapshot, VideoError> {
        self.initialize_limit(&job.database)?;
        let id = record.request_id.clone();
        let (snapshot, ready) = {
            let mut state = self.inner.lock().map_err(|e| error("bridgeFailed", e))?;
            state
                .entries
                .retain(|_, e| e.snapshot.record.id != record.id);
            state.order += 1;
            state.revision += 1;
            state.app = app.clone();
            let snapshot = DownloadTaskSnapshot {
                record,
                phase: "queued".into(),
                percent: None,
                speed: None,
                eta: None,
                submission_order: state.order,
                revision: state.revision,
                storage_error: None,
            };
            let (cancel, _) = watch::channel(false);
            state.entries.insert(
                id.clone(),
                Entry {
                    snapshot: snapshot.clone(),
                    database: job.database.clone(),
                    cancel,
                    control: Arc::default(),
                    resume_phase: "downloading",
                    suspended: false,
                    resume_error: None,
                    preserve_pause: false,
                    parked: false,
                    job: Some(job),
                },
            );
            let ready = state.scheduler.enqueue(id.clone());
            (snapshot, ready)
        };
        if let Some(app) = app {
            log_task(&app, &snapshot);
            let _ = app.emit("download-task-changed", snapshot.clone());
        }
        self.launch(ready);
        if self
            .inner
            .lock()
            .map_err(|e| error("bridgeFailed", e))?
            .exiting
        {
            self.cancel(&id)?;
        }
        Ok(snapshot)
    }
    fn launch(&self, ids: Vec<String>) -> BTreeMap<String, VideoError> {
        let mut failures = BTreeMap::new();
        for id in ids {
            let (job, update) = {
                let Ok(mut state) = self.inner.lock() else {
                    return failures;
                };
                // A delayed dispatch can outlive a pause/cancel and a later resume cycle.
                if !state.scheduler.is_running(&id) {
                    continue;
                }
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                // A resumed task keeps its original worker and process, but can only
                // unfreeze after the scheduler has reserved a new slot for it.
                let resuming = entry.suspended && entry.snapshot.phase == "queued";
                if resuming {
                    match entry.control.set_paused(false) {
                        Ok(()) => {
                            entry.suspended = false;
                            entry.snapshot.phase = entry.resume_phase.into();
                        }
                        Err(failure) => {
                            let failure = error("resumeFailed", failure);
                            failures.insert(id.clone(), failure.clone());
                            entry.resume_error = Some(failure);
                            entry.cancel.send_replace(true);
                            entry.control.native.cancel();
                            entry.snapshot.phase = "cancelling".into();
                        }
                    }
                }
                let job = entry
                    .job
                    .take()
                    .map(|job| (job, entry.cancel.subscribe(), entry.control.clone()));
                let update = if resuming {
                    state.revision += 1;
                    let revision = state.revision;
                    let app = state.app.clone();
                    let entry = state.entries.get_mut(&id).expect("resumed entry exists");
                    entry.snapshot.revision = revision;
                    Some((app, entry.snapshot.clone()))
                } else {
                    None
                };
                (job, update)
            };
            if let Some((Some(app), update)) = update {
                log_task(&app, &update);
                let _ = app.emit("download-task-changed", update);
            }
            if let Some((job, cancel, control)) = job {
                let manager = self.clone();
                tauri::async_runtime::spawn(async move {
                    manager.execute(id, job, cancel, control).await;
                });
            }
        }
        failures
    }
    async fn execute(
        &self,
        id: String,
        job: Job,
        cancel: watch::Receiver<bool>,
        control: Arc<super::control::DownloadControl>,
    ) {
        let result = async {
            if *cancel.borrow() {
                return Err(error("downloadCancelled", ""));
            }
            job.database
                .mark_download_running(&id)
                .map_err(|e| error("historyCreateFailed", e.detail))?;
            self.publish(&id, |s| {
                if s.phase != "cancelling" {
                    s.phase = "preparing".into();
                }
                s.record.status = "running".into();
            });
            let mut execution = job.execution?;
            if let Some(app) = self.inner.lock().ok().and_then(|state| state.app.clone()) {
                crate::app_logs::record(
                    &app,
                    crate::app_logs::entry(
                        "info",
                        &execution.page.platform,
                        execution.page.video_id.as_deref(),
                        Some(&id),
                        "downloadNetworkConfigured",
                        None,
                        execution.network_diagnostic.clone(),
                    ),
                );
            }
            execution.check_tools().await?;
            let directory = PathBuf::from(&execution.page.download_directory);
            prepare_download_directory(directory.clone()).await?;
            let temporary = job
                .database
                .prepare_download_temporary_directory(&id, &directory)
                .map_err(|e| error(&e.code, e.detail))?;
            let temporary_directory = PathBuf::from(&temporary.path);
            let temporary_guard =
                crate::database::download_records::temporary::validate(&temporary)
                    .map_err(|e| error(&e.code, e.detail))?;
            execution.set_temporary_directory(&temporary_directory)?;
            if *cancel.borrow() {
                return Err(error("downloadCancelled", ""));
            }
            if let Some(previous) = job.restart_output {
                job.history
                    .delete_previous_output(previous, cancel.clone())
                    .await?;
            }
            if *cancel.borrow() {
                return Err(error("downloadCancelled", ""));
            }
            let notify = |progress: DownloadProgress| {
                self.publish_progress(&id, progress);
                Ok(())
            };
            let super::task_snapshot::TaskExecutionSnapshot {
                runner,
                _cookie,
                _tools,
                ..
            } = execution;
            let result = match runner {
                super::task_snapshot::DownloadExecution::Douyin(native) => {
                    native
                        .run(
                            cancel,
                            &directory,
                            &temporary_directory,
                            notify,
                            &job.history,
                            control.native.clone(),
                        )
                        .await
                }
                super::task_snapshot::DownloadExecution::Ytdlp(command) => {
                    run_controlled_download(
                        command,
                        &directory,
                        cancel,
                        Duration::from_secs(6 * 60 * 60),
                        notify,
                        Some(&job.history),
                        Some(&control),
                    )
                        .await
                }
            };
            // execution owns Cookie and tool leases until the process tree is stopped.
            drop(temporary_guard);
            if let Err(failure) =
                crate::database::download_records::temporary::remove_empty(&temporary)
            {
                log::warn!(
                    "Empty download temporary directory cleanup failed: {}: {}",
                    failure.code,
                    failure.detail
                );
            }
            drop((_cookie, _tools));
            result
        }
            .await;
        self.settle(id, job.history, job.database, result).await;
    }
    async fn settle(
        &self,
        id: String,
        history: DownloadRecordSession,
        database: Arc<crate::database::Database>,
        mut result: Result<DownloadResult, VideoError>,
    ) {
        if result
            .as_ref()
            .err()
            .is_some_and(|e| e.code == "downloadCancelled")
        {
            if let Some(failure) = self.inner.lock().ok().and_then(|mut state| {
                state
                    .entries
                    .get_mut(&id)
                    .and_then(|entry| entry.resume_error.take())
            }) {
                result = Err(failure);
            }
        }
        let preserve_pause = result
            .as_ref()
            .err()
            .is_some_and(|e| e.code == "downloadCancelled")
            && self
            .inner
            .lock()
            .ok()
            .and_then(|s| s.entries.get(&id).map(|e| e.preserve_pause))
            .unwrap_or(false);
        let mut storage_error = history.finish_with_pause(&result, preserve_pause).await;
        let record = match database.find_request_record(&id) {
            Ok(record) => record,
            Err(e) => {
                if storage_error.is_none() {
                    storage_error = Some(e);
                }
                None
            }
        };
        self.publish_entry(&id, |entry| {
            entry.parked = preserve_pause;
            entry.suspended = false;
            let s = &mut entry.snapshot;
            if let Some(record) = record {
                s.record = record;
            }
            s.phase = match &result {
                Ok(_) => "completed",
                Err(_) if preserve_pause => "paused",
                Err(e) if e.code == "downloadCancelled" => "cancelled",
                Err(_) => "failed",
            }
                .into();
            s.percent = if result.is_ok() {
                Some(100.0)
            } else {
                s.percent
            };
            s.speed = None;
            s.eta = None;
            s.storage_error = storage_error;
        });
        use crate::app_preferences::{notify_download, DownloadOutcome};
        // Release the registry lock before reading the settled task or dispatching notices.
        let app = self.inner.lock().ok().and_then(|s| s.app.clone());
        if let Some(app) = app {
            match &result {
                Ok(r) => notify_download(
                    &app,
                    &id,
                    DownloadOutcome::Completed {
                        path: &r.path,
                        already_downloaded: r.already_downloaded,
                    },
                ),
                Err(e) => {
                    let task = self.inner.lock().ok().and_then(|state| {
                        state.entries.get(&id).map(|entry| entry.snapshot.clone())
                    });
                    if e.code != "downloadCancelled" {
                        if let Some(task) = &task {
                            crate::app_logs::record(
                                &app,
                                crate::app_logs::entry(
                                    "error",
                                    &task.record.platform,
                                    Some(&task.record.video_id),
                                    Some(&id),
                                    "downloadFailed",
                                    Some(&e.code),
                                    e.detail.clone(),
                                ),
                            );
                        }
                    }
                    if !preserve_pause {
                        notify_download(
                            &app,
                            &id,
                            DownloadOutcome::Failed {
                                code: &e.code,
                                title: task
                                    .as_ref()
                                    .map(|task| task.record.title.as_str())
                                    .unwrap_or("EasyVideoDownload"),
                                detail: &e.detail,
                            },
                        )
                    }
                }
            }
        }
        drop(history);
        let ready = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            state.scheduler.finish(&id)
        };
        self.launch(ready);
    }
    pub(crate) fn cancel(&self, id: &str) -> Result<(), VideoError> {
        self.cancel_at_boundary(id, || {})
    }
    pub(crate) async fn cancel_and_remove(
        &self,
        id: &str,
        database: Arc<crate::database::Database>,
    ) -> Result<i64, VideoError> {
        // Serialize with resume/restart so cleanup cannot race a new writer.
        let _submission = self.submission.lock().await;
        self.cancel(id)?;
        loop {
            let pending = self
                .inner
                .lock()
                .map_err(|e| error("cancelFailed", e))?
                .entries
                .get(id)
                .is_some_and(|entry| !entry.parked && active(&entry.snapshot.phase));
            if !pending {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let request_id = id.to_owned();
        let record_id = tauri::async_runtime::spawn_blocking(move || {
            database.remove_cancelled_download(&request_id)
        })
            .await
            .map_err(|e| error("cancelFailed", e))?
            .map_err(|e| error(&e.code, e.detail))?;
        let mut state = self.inner.lock().map_err(|e| error("cancelFailed", e))?;
        state.entries.remove(id);
        Ok(record_id)
    }
    fn publish_progress(&self, id: &str, progress: DownloadProgress) {
        self.publish_entry(id, |entry| {
            if !active(&entry.snapshot.phase) {
                return;
            }
            entry.resume_phase = progress.phase;
            let s = &mut entry.snapshot;
            if entry.suspended {
                return;
            }
            if s.phase != "cancelling" {
                s.phase = progress.phase.into();
            }
            let nonnegative = |v: Option<f64>| v.filter(|n| n.is_finite() && *n >= 0.0);
            s.percent = nonnegative(progress.percent).map(|n| n.min(100.0));
            s.speed = nonnegative(progress.speed);
            s.eta = nonnegative(progress.eta);
        });
    }
    fn set_paused(&self, id: &str, paused: bool) -> Result<DownloadTaskSnapshot, VideoError> {
        let (app, update, ready) = {
            let mut state = self.inner.lock().map_err(|e| error("bridgeFailed", e))?;
            let code = if paused {
                "pauseFailed"
            } else {
                "resumeFailed"
            };
            if state.exiting {
                return Err(error(code, "Application is exiting"));
            }
            let entry = state
                .entries
                .get(id)
                .ok_or_else(|| error(code, "Task is no longer active"))?;
            if entry.parked {
                return Err(error(code, "Task has no running process"));
            }
            let target = if paused { "paused" } else { "queued" };
            if (paused && entry.snapshot.phase == "paused")
                || (!paused
                && (matches!(entry.snapshot.phase.as_str(), "downloading" | "processing")
                || (entry.suspended && entry.snapshot.phase == "queued")))
            {
                return Ok(entry.snapshot.clone());
            }
            if entry.snapshot.phase != if paused { "downloading" } else { "paused" } {
                return Err(error(code, "Only a downloading task can be paused"));
            }
            if paused {
                entry.set_process_paused(id, paused)?;
            } else {
                // Persist the resume intent now; keep the process suspended until dispatch.
                entry
                    .database
                    .set_download_paused(id, false)
                    .map_err(|e| error(code, e.detail))?;
            }
            state.revision += 1;
            let revision = state.revision;
            let app = state.app.clone();
            let entry = state
                .entries
                .get_mut(id)
                .expect("task checked under registry lock");
            if paused {
                entry.suspended = true;
            }
            entry.snapshot.phase = target.into();
            entry.snapshot.speed = None;
            entry.snapshot.eta = None;
            entry.snapshot.revision = revision;
            let update = entry.snapshot.clone();
            let ready = if paused {
                state.scheduler.finish(id)
            } else {
                state.scheduler.enqueue(id.to_owned())
            };
            (app, update, ready)
        };
        if let Some(app) = app {
            log_task(&app, &update);
            let _ = app.emit("download-task-changed", update.clone());
            let _ = app.emit("download-records-changed", ());
        }
        if let Some(failure) = self.launch(ready).remove(id) {
            return Err(failure);
        }
        if paused {
            Ok(update)
        } else {
            self.inner
                .lock()
                .map_err(|e| error("bridgeFailed", e))?
                .entries
                .get(id)
                .map(|entry| entry.snapshot.clone())
                .ok_or_else(|| error("resumeFailed", "Task is no longer active"))
        }
    }
    fn cancel_at_boundary(&self, id: &str, boundary: impl FnOnce()) -> Result<(), VideoError> {
        let (app, update, queued) = {
            let mut state = self.inner.lock().map_err(|e| error("bridgeFailed", e))?;
            if !state
                .entries
                .get(id)
                .is_some_and(|entry| !entry.parked && active(&entry.snapshot.phase))
            {
                return Ok(());
            }
            let is_queued = state.scheduler.cancel_queued(id);
            state.revision += 1;
            let revision = state.revision;
            let app = state.app.clone();
            let exiting = state.exiting;
            let entry = state
                .entries
                .get_mut(id)
                .expect("entry checked under the registry lock");
            entry.preserve_pause |=
                exiting && entry.suspended && entry.snapshot.phase != "cancelling";
            entry.cancel.send_replace(true);
            entry.control.native.cancel();
            entry.snapshot.phase = "cancelling".into();
            entry.snapshot.revision = revision;
            let queued = if is_queued { entry.job.take() } else { None };
            (app, entry.snapshot.clone(), queued)
        };
        // Sending may be delayed; the captured revision cannot overwrite a newer terminal event.
        boundary();
        if let Some(app) = app {
            log_task(&app, &update);
            let _ = app.emit("download-task-changed", update);
        }
        if let Some(job) = queued {
            let manager = self.clone();
            let id = id.to_owned();
            tauri::async_runtime::spawn(async move {
                drop(job.execution);
                manager
                    .settle(
                        id,
                        job.history,
                        job.database,
                        Err(error("downloadCancelled", "")),
                    )
                    .await;
            });
        }
        Ok(())
    }
    pub(crate) fn begin_exit(&self) -> Result<(), String> {
        let ids = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.exiting = true;
            state.scheduler.stop();
            state
                .entries
                .iter()
                .filter(|(_, e)| !e.parked && active(&e.snapshot.phase))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.cancel(&id).map_err(|e| e.detail)?;
        }
        Ok(())
    }
    pub(crate) fn is_active(&self) -> Result<bool, String> {
        self.inner
            .lock()
            .map(|s| {
                self.submission.try_lock().is_err()
                    || s.entries
                    .values()
                    .any(|e| !e.parked && active(&e.snapshot.phase))
                    || s.scheduler.running_count() > 0
            })
            .map_err(|e| e.to_string())
    }
    pub(crate) fn abort_exit(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.exiting = false;
            state.scheduler.resume();
        }
    }
}

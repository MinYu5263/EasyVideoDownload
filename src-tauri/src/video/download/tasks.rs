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
    cancel: watch::Sender<bool>,
    job: Option<Job>,
}
#[derive(Default)]
struct Registry {
    scheduler: scheduler::Scheduler,
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
        "queued" | "preparing" | "downloading" | "processing" | "cancelling"
    )
}

impl DownloadManager {
    fn publish(&self, id: &str, change: impl FnOnce(&mut DownloadTaskSnapshot)) {
        let (app, update) = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            state.revision += 1;
            let revision = state.revision;
            let app = state.app.clone();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            change(&mut entry.snapshot);
            entry.snapshot.revision = revision;
            (app, entry.snapshot.clone())
        };
        if let Some(app) = app {
            let _ = app.emit("download-task-changed", update);
        }
    }
    pub(super) fn snapshots(&self) -> Result<Vec<DownloadTaskSnapshot>, VideoError> {
        self.inner
            .lock()
            .map(|s| s.entries.values().map(|e| e.snapshot.clone()).collect())
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
                    cancel,
                    job: Some(job),
                },
            );
            let ready = state.scheduler.enqueue(id.clone());
            (snapshot, ready)
        };
        if let Some(app) = app {
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
    fn launch(&self, ids: Vec<String>) {
        for id in ids {
            let job = {
                let Ok(mut state) = self.inner.lock() else {
                    return;
                };
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                entry.job.take().map(|job| (job, entry.cancel.subscribe()))
            };
            if let Some((job, cancel)) = job {
                let manager = self.clone();
                tauri::async_runtime::spawn(async move {
                    manager.execute(id, job, cancel).await;
                });
            }
        }
    }
    async fn execute(&self, id: String, job: Job, cancel: watch::Receiver<bool>) {
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
            let execution = job.execution?;
            check_download_tool_files(&execution.settings, execution.platform).await?;
            let directory = PathBuf::from(&execution.page.download_directory);
            prepare_download_directory(directory.clone()).await?;
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
                self.publish(&id, |s| {
                    if s.phase != "cancelling" {
                        s.phase = progress.phase.into();
                    }
                    s.percent = progress.percent;
                    s.speed = progress.speed;
                    s.eta = progress.eta;
                });
                Ok(())
            };
            let super::task_snapshot::TaskExecutionSnapshot {
                command,
                _cookie,
                _tools,
                ..
            } = execution;
            let result = run_download_with_history(
                command,
                &directory,
                cancel,
                Duration::from_secs(6 * 60 * 60),
                notify,
                Some(&job.history),
            )
                .await;
            // execution owns Cookie and tool leases until the process tree is stopped.
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
        result: Result<DownloadResult, VideoError>,
    ) {
        let mut storage_error = history.finish(&result).await;
        let record = match database.find_request_record(&id) {
            Ok(record) => record,
            Err(e) => {
                if storage_error.is_none() {
                    storage_error = Some(e);
                }
                None
            }
        };
        self.publish(&id, |s| {
            if let Some(record) = record {
                s.record = record;
            }
            s.phase = match &result {
                Ok(_) => "completed",
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
        if let Some(app) = self.inner.lock().ok().and_then(|s| s.app.clone()) {
            match &result {
                Ok(r) => notify_download(
                    &app,
                    DownloadOutcome::Completed {
                        path: &r.path,
                        already_downloaded: r.already_downloaded,
                    },
                ),
                Err(e) => notify_download(&app, DownloadOutcome::Failed { code: &e.code }),
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
    fn cancel_at_boundary(&self, id: &str, boundary: impl FnOnce()) -> Result<(), VideoError> {
        let (app, update, queued) = {
            let mut state = self.inner.lock().map_err(|e| error("bridgeFailed", e))?;
            if !state
                .entries
                .get(id)
                .is_some_and(|entry| active(&entry.snapshot.phase))
            {
                return Ok(());
            }
            let is_queued = state.scheduler.cancel_queued(id);
            state.revision += 1;
            let revision = state.revision;
            let app = state.app.clone();
            let entry = state
                .entries
                .get_mut(id)
                .expect("entry checked under the registry lock");
            entry.cancel.send_replace(true);
            entry.snapshot.phase = "cancelling".into();
            entry.snapshot.revision = revision;
            let queued = if is_queued { entry.job.take() } else { None };
            (app, entry.snapshot.clone(), queued)
        };
        // Sending may be delayed; the captured revision cannot overwrite a newer terminal event.
        boundary();
        if let Some(app) = app {
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
                .filter(|(_, e)| active(&e.snapshot.phase))
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
                    || s.entries.values().any(|e| active(&e.snapshot.phase))
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

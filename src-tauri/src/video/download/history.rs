use super::{DownloadResult, VideoError};
use crate::database::{
    download_records::{
        DownloadRecordOutcome, DownloadSnapshot, HistoryCursor, HistoryPageResult, HistoryQuery,
    },
    Database, Storage, StorageError,
};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};
type ChangeNotification = Arc<dyn Fn() + Send + Sync>;

struct RequestLock {
    file: Option<File>,
    path: PathBuf,
    remove: bool,
}
impl RequestLock {
    fn acquire(root: &Path, id: &str, remove: bool) -> Result<Option<Self>, StorageError> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid download request ID",
            ));
        }
        let directory = root.join("runtime/downloads");
        std::fs::create_dir_all(&directory).map_err(|e| StorageError::new("saveFailed", e))?;
        let directory = directory
            .canonicalize()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        if !directory.starts_with(
            root.canonicalize()
                .map_err(|e| StorageError::new("saveFailed", e))?,
        ) {
            return Err(StorageError::new("saveFailed", "Invalid runtime directory"));
        }
        let path = directory.join(format!("{id}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                file: Some(file),
                path,
                remove,
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(e) => Err(StorageError::new("saveFailed", e)),
        }
    }
}
impl Drop for RequestLock {
    fn drop(&mut self) {
        self.file.take();
        if self.remove {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
#[derive(Default)]
struct RecordState {
    stage: &'static str,
    storage_error: Option<StorageError>,
}
#[derive(Default)]
struct Observations {
    pending: usize,
    closed: bool,
}
struct SessionInner {
    database: Arc<Database>,
    request_id: String,
    state: Mutex<RecordState>,
    observations: Mutex<Observations>,
    drained: Condvar,
    notify: Option<ChangeNotification>,
    _lock: RequestLock,
}
struct PendingObservation(Arc<SessionInner>);
impl Drop for PendingObservation {
    fn drop(&mut self) {
        let mut observations = self
            .0
            .observations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        observations.pending -= 1;
        if observations.pending == 0 {
            self.0.drained.notify_all();
        }
    }
}
pub(crate) struct DownloadRecordSession(Arc<SessionInner>);
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RestartOutputPolicy {
    Deferred,
    RequireMissing,
    Replace,
}
pub(crate) struct RestartTarget<'a> {
    pub record: &'a crate::database::download_records::DownloadRecord,
    pub restore_trashed: bool,
    pub output_policy: RestartOutputPolicy,
}
impl DownloadRecordSession {
    pub(crate) fn native_stage(&self, stage: &'static str) {
        if let Ok(mut state) = self.0.state.lock() {
            state.stage = stage;
        }
    }
    pub fn accept(
        database: Arc<Database>,
        root: &Path,
        request_id: String,
        snapshot: DownloadSnapshot,
        notify: Option<ChangeNotification>,
        restore: bool,
        redownload: bool,
    ) -> Result<
        (
            crate::database::download_records::tasks::RecordAcceptance,
            Option<Self>,
        ),
        StorageError,
    > {
        let lock = RequestLock::acquire(root, &request_id, true)?.ok_or_else(|| {
            StorageError::new("downloadBusy", "Task identifier is already active")
        })?;
        let acceptance =
            database.accept_download_record(&request_id, &snapshot, restore, redownload)?;
        if matches!(
            &acceptance,
            crate::database::download_records::tasks::RecordAcceptance::Accepted(_)
        ) {
            let session = Self(Arc::new(SessionInner {
                database,
                request_id,
                state: Mutex::new(RecordState {
                    stage: "preparing",
                    ..Default::default()
                }),
                observations: Mutex::new(Observations::default()),
                drained: Condvar::new(),
                notify,
                _lock: lock,
            }));
            session.0.changed();
            Ok((acceptance, Some(session)))
        } else {
            Ok((acceptance, None))
        }
    }
    pub fn restart(
        database: Arc<Database>,
        root: &Path,
        request_id: String,
        target: RestartTarget<'_>,
        snapshot: DownloadSnapshot,
        notify: Option<ChangeNotification>,
    ) -> Result<
        (
            crate::database::download_records::tasks::RecordAcceptance,
            Option<Self>,
        ),
        StorageError,
    > {
        let lock = RequestLock::acquire(root, &request_id, true)?
            .ok_or_else(|| StorageError::new("downloadBusy", "Task identifier is active"))?;
        let acceptance = database.restart_download_record(
            &request_id,
            target.record.id,
            &target.record.request_id,
            &snapshot,
            target.restore_trashed,
            |record, protected| match target.output_policy {
                RestartOutputPolicy::Deferred => Ok(None),
                RestartOutputPolicy::Replace => {
                    permanent::delete_output_file(record, protected).map(Some)
                }
                RestartOutputPolicy::RequireMissing => {
                    if permanent::validated_record_output(record)?.is_some() {
                        Err(StorageError::new(
                            "historyOutputReturned",
                            "The final output reappeared before download",
                        ))
                    } else {
                        Ok(Some(false))
                    }
                }
            },
        )?;
        if matches!(
            &acceptance,
            crate::database::download_records::tasks::RecordAcceptance::Accepted(_)
        ) {
            let session = Self(Arc::new(SessionInner {
                database,
                request_id,
                state: Mutex::new(RecordState {
                    stage: "preparing",
                    ..Default::default()
                }),
                observations: Mutex::new(Observations::default()),
                drained: Condvar::new(),
                notify,
                _lock: lock,
            }));
            session.0.changed();
            Ok((acceptance, Some(session)))
        } else {
            Ok((acceptance, None))
        }
    }
    pub async fn delete_previous_output(
        &self,
        previous: crate::database::download_records::DownloadRecord,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), VideoError> {
        let inner = self.0.clone();
        tauri::async_runtime::spawn_blocking(move || {
            inner.database.prepare_restart_output(
                &inner.request_id,
                &previous,
                |record, protected| {
                    if *cancel.borrow() {
                        return Err(StorageError::new("downloadCancelled", ""));
                    }
                    permanent::delete_output_file(record, protected)
                },
            )
        })
            .await
            .map_err(|e| super::error("historyFileDeleteFailed", e))?
            .map_err(|e| super::error(&e.code, e.detail))
    }

    #[cfg(test)]
    pub fn new(
        database: Arc<Database>,
        root: &Path,
        request_id: String,
        snapshot: DownloadSnapshot,
        notify: Option<ChangeNotification>,
    ) -> Result<Self, StorageError> {
        let lock = RequestLock::acquire(root, &request_id, true)?.ok_or_else(|| {
            StorageError::new("invalidSettings", "Download request is already active")
        })?;
        if database.record_exists(&request_id)? {
            return Err(StorageError::new(
                "invalidSettings",
                "Download request ID has already been used",
            ));
        }
        database.begin_download_record(&request_id, &snapshot, &crate::datetime::now())?;
        if let Some(notify) = &notify {
            notify();
        }
        Ok(Self(Arc::new(SessionInner {
            database,
            request_id,
            state: Mutex::new(RecordState {
                stage: "preparing",
                ..Default::default()
            }),
            observations: Mutex::new(Observations::default()),
            drained: Condvar::new(),
            notify,
            _lock: lock,
        })))
    }
    pub async fn observe(&self, line: &str) {
        if !line.starts_with("__EVD_PROGRESS__")
            && !line.starts_with("__EVD_PROCESSING__")
            && !line.starts_with("__EVD_FILE__")
        {
            return;
        }
        let inner = self.0.clone();
        {
            let mut observations = inner.observations.lock().unwrap_or_else(|e| e.into_inner());
            if observations.closed {
                return;
            }
            observations.pending += 1;
        }
        // A dropped async reader does not cancel an already submitted blocking write.
        let pending = PendingObservation(inner.clone());
        let line = line.to_owned();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _pending = pending;
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if line.starts_with("__EVD_PROGRESS__") {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(
                    line.trim_start_matches("__EVD_PROGRESS__"),
                ) {
                    if value["progress"]["status"] == "downloading" {
                        state.stage = "downloading";
                    }
                }
            } else if line.starts_with("__EVD_PROCESSING__") {
                state.stage = "processing";
            } else if line.starts_with("__EVD_FILE__") {
                state.stage = "finalizing";
            }
        })
            .await;
    }
    pub async fn finish(
        &self,
        result: &Result<DownloadResult, VideoError>,
    ) -> Option<StorageError> {
        self.finish_with_pause(result, false).await
    }
    pub async fn finish_with_pause(
        &self,
        result: &Result<DownloadResult, VideoError>,
        preserve_pause: bool,
    ) -> Option<StorageError> {
        let result = result.clone();
        let inner = self.0.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut observations = inner.observations.lock().unwrap_or_else(|e| e.into_inner());
            observations.closed = true;
            while observations.pending != 0 {
                observations = inner
                    .drained
                    .wait(observations)
                    .unwrap_or_else(|e| e.into_inner());
            }
            drop(observations);
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let outcome = match &result {
                Ok(r) => match std::fs::metadata(&r.path) {
                    Ok(m) => Some(DownloadRecordOutcome::Completed {
                        path: r.path.clone(),
                        size: m.len(),
                        extension: Path::new(&r.path)
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(str::to_owned),
                    }),
                    Err(e) => return Some(StorageError::new("saveFailed", e)),
                },
                Err(e) if e.code == "downloadCancelled" => Some(if preserve_pause {
                    DownloadRecordOutcome::Paused
                } else {
                    DownloadRecordOutcome::Cancelled
                }),
                Err(e) => Some(DownloadRecordOutcome::Failed {
                    code: e.code.clone(),
                    detail: super::failure::sanitize_diagnostic(&e.detail),
                }),
            };
            let save = || -> Result<bool, StorageError> {
                let failure = result
                    .as_ref()
                    .err()
                    .filter(|e| e.code != "downloadCancelled");
                let kind = failure.map(|e| {
                    e.failure_kind.unwrap_or_else(|| {
                        super::failure::failure_kind(&e.code, &e.detail, state.stage)
                    })
                });
                inner.database.finish_download_record_with_failure(
                    &inner.request_id,
                    outcome.as_ref().unwrap(),
                    failure.map(|_| state.stage),
                    kind,
                )?;
                Ok(true)
            };
            match save() {
                Ok(changed) => {
                    state.storage_error = None;
                    if changed {
                        inner.changed();
                    }
                }
                Err(e) => state.storage_error = Some(e),
            }
            state.storage_error.clone()
        })
            .await
            .unwrap_or_else(|e| Some(StorageError::new("saveFailed", e)))
    }
}
impl SessionInner {
    fn changed(&self) {
        if let Some(notify) = &self.notify {
            notify();
        }
    }
}
pub(crate) fn recover(database: &Database, root: &Path) -> Result<(), StorageError> {
    for id in database.running_request_ids()? {
        if let Some(_lock) = RequestLock::acquire(root, &id, false)? {
            let outcome = if database.download_pause_requested(&id)? {
                DownloadRecordOutcome::Paused
            } else {
                DownloadRecordOutcome::Interrupted
            };
            database.finish_download_record(&id, &outcome)?;
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn list_download_records(
    cursor: Option<HistoryCursor>,
    limit: Option<u32>,
    query: Option<String>,
    status: Option<String>,
    trashed: Option<bool>,
    storage: tauri::State<'_, Storage>,
) -> Result<HistoryListResult, StorageError> {
    let database = storage.database()?;
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        recover(&database, &storage.prepare_data_directory()?)?;
        database
            .query_download_records_in_scope(
                &HistoryQuery {
                    cursor,
                    limit: limit.unwrap_or(50),
                    query: query.unwrap_or_default(),
                    status,
                },
                trashed.unwrap_or(false),
            )
            .map(|page| HistoryListResult {
                page,
                file_deletion_supported: permanent::supported(),
            })
    })
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryListResult {
    #[serde(flatten)]
    page: HistoryPageResult,
    file_deletion_supported: bool,
}

pub(crate) mod actions;
pub(crate) mod permanent;
pub(crate) mod recycle;
#[cfg(test)]
mod tests;

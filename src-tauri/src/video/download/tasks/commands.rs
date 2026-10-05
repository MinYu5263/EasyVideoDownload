use super::super::history::RestartOutputPolicy;
use super::*;

#[tauri::command]
pub async fn enqueue_video_download(
    app: tauri::AppHandle,
    request: SubmitDownloadRequest,
    downloads: tauri::State<'_, DownloadManager>,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<SubmitDownloadResult, VideoError> {
    let platform = request.snapshot.platform.clone();
    let video_id = request.snapshot.video_id.clone();
    let result = submit_from_page(
        Some(app.clone()),
        request,
        &downloads,
        &tools,
        &cookies,
        &storage,
    )
        .await;
    if let Err(error) = &result {
        crate::app_logs::record(
            &app,
            crate::app_logs::entry(
                "error",
                &platform,
                video_id.as_deref(),
                None,
                "downloadRejected",
                Some(&error.code),
                error.detail.clone(),
            ),
        );
    }
    result
}

pub(super) async fn submit_from_page(
    app: Option<tauri::AppHandle>,
    mut request: SubmitDownloadRequest,
    downloads: &DownloadManager,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
) -> Result<SubmitDownloadResult, VideoError> {
    let _submit = downloads.submission.lock().await;
    if downloads
        .inner
        .lock()
        .map_err(|e| error("bridgeFailed", e))?
        .exiting
    {
        return Err(error("applicationExiting", ""));
    }
    request
        .snapshot
        .validate("invalidDownloadOptions")
        .map_err(|e| error("invalidDownloadOptions", e.detail))?;
    crate::video::formats::normalize_all(&mut request.snapshot.formats);
    let video_id = request.snapshot.video_id.as_deref().ok_or_else(|| {
        error(
            "invalidDownloadOptions",
            "Parse a video before starting a download",
        )
    })?;
    let database = storage
        .database()
        .map_err(|e| error("pageSaveFailed", e.detail))?;
    downloads.initialize_limit(&database)?;
    let saved = database
        .download_page_states()
        .map_err(|e| error("pageSaveFailed", e.detail))?
        .into_iter()
        .find(|p| p.platform == request.snapshot.platform);
    if saved.is_none_or(|p| {
        p.input_link != request.snapshot.input_link
            || p.video_id != request.snapshot.video_id
            || p.parser_fingerprint != request.snapshot.parser_fingerprint
            || p.parsed_at != request.snapshot.parsed_at
            || p.cookie_fallback != request.snapshot.cookie_fallback
            || p.title != request.snapshot.title
            || p.thumbnail_url != request.snapshot.thumbnail_url
            || p.thumbnail_cache_path != request.snapshot.thumbnail_cache_path
            || p.duration_seconds != request.snapshot.duration_seconds
            || p.extension != request.snapshot.extension
            || p.formats != request.snapshot.formats
            || p.selected_format_id != request.snapshot.selected_format_id
            || p.download_directory != request.snapshot.download_directory
    }) {
        return Err(error(
            "pageSaveFailed",
            "The saved parse snapshot does not match the submitted task",
        ));
    }
    if let Some(mut record) = database
        .find_format_download_record(
            &request.snapshot.platform,
            video_id,
            request
                .snapshot
                .selected_format_id
                .as_deref()
                .unwrap_or_default(),
        )
        .map_err(|e| error(&e.code, e.detail))?
    {
        if record.deleted_at.is_some() && !request.restore_trashed {
            return Err(error("recordTrashed", ""));
        }
        if matches!(record.status.as_str(), "queued" | "running")
            || downloads
            .inner
            .lock()
            .map_err(|e| error("bridgeFailed", e))?
            .entries
            .get(&record.request_id)
            .is_some_and(|entry| !entry.parked && active(&entry.snapshot.phase))
        {
            return Ok(SubmitDownloadResult {
                kind: "existing",
                task: downloads.task_for(record.id),
                record,
            });
        }
        if !request.redownload {
            if record.deleted_at.is_some() {
                database
                    .restore_download_record(record.id)
                    .map_err(|e| error(&e.code, e.detail))?;
                if let Some(app) = &app {
                    let _ = app.emit("download-records-changed", ());
                    crate::app_logs::record(
                        app,
                        crate::app_logs::entry(
                            "info",
                            &record.platform,
                            Some(&record.video_id),
                            Some(&record.request_id),
                            "downloadStage",
                            None,
                            format!("Restored record from trash · format={}", record.format_id),
                        ),
                    );
                }
            }
            record =
                refresh_record_output(app.as_ref(), database.clone(), record, downloads).await?;
            if record.output_path.is_some() && record.file_availability == "present" {
                return Ok(SubmitDownloadResult {
                    kind: "alreadyDownloaded",
                    record,
                    task: None,
                });
            }
        }
        if record.output_path.is_some()
            && (request.redownload || record.file_availability == "missing")
        {
            let execution = super::super::task_snapshot::capture_task_snapshot(
                request.snapshot,
                tools,
                cookies,
                storage,
            )
                .await?;
            return restart_with_snapshot(
                app,
                record,
                execution,
                request.restore_trashed,
                RestartOutputPolicy::Deferred,
                downloads,
                storage,
            )
                .await;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let root = storage
        .prepare_data_directory()
        .map_err(|e| error("pageSaveFailed", e.detail))?;
    let page = request.snapshot;
    let notify = app.clone().map(|app| {
        Arc::new(move || {
            let _ = app.emit("download-records-changed", ());
        }) as Arc<dyn Fn() + Send + Sync>
    });
    let (acceptance, history) = DownloadRecordSession::accept(
        database.clone(),
        &root,
        id,
        DownloadSnapshot { page: page.clone() },
        notify,
        request.restore_trashed,
        request.redownload,
    )
        .map_err(|e| error(&e.code, e.detail))?;
    match acceptance {
        RecordAcceptance::Existing(record) => Ok(SubmitDownloadResult {
            kind: "existing",
            task: downloads.task_for(record.id),
            record,
        }),
        RecordAcceptance::AlreadyDownloaded(record) => Ok(SubmitDownloadResult {
            kind: "alreadyDownloaded",
            task: None,
            record,
        }),
        RecordAcceptance::Accepted(record) => {
            let execution =
                super::super::task_snapshot::capture_task_snapshot(page, tools, cookies, storage)
                    .await;
            let history = history.expect("accepted record owns its runtime lock");
            // An exit may begin during snapshot preparation: settle instead of orphaning this record.
            if downloads
                .inner
                .lock()
                .map_err(|e| error("bridgeFailed", e))?
                .exiting
            {
                history.finish(&Err(error("downloadCancelled", ""))).await;
                return Err(error("applicationExiting", ""));
            }
            let task = downloads.accept_internal(
                app,
                record.clone(),
                Job {
                    restart_output: None,
                    execution,
                    history,
                    database,
                },
            )?;
            Ok(SubmitDownloadResult {
                kind: "accepted",
                record,
                task: Some(task),
            })
        }
    }
}

#[tauri::command]
pub fn list_download_tasks(
    downloads: tauri::State<'_, DownloadManager>,
    storage: tauri::State<'_, Storage>,
) -> Result<Vec<DownloadTaskSnapshot>, VideoError> {
    let database = storage
        .database()
        .map_err(|e| error("loadFailed", e.detail))?;
    let root = storage
        .prepare_data_directory()
        .map_err(|e| error("loadFailed", e.detail))?;
    super::super::history::recover(&database, &root).map_err(|e| error("loadFailed", e.detail))?;
    downloads.snapshots()
}
#[tauri::command]
pub async fn cancel_video_download(
    app: tauri::AppHandle,
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
    storage: tauri::State<'_, Storage>,
) -> Result<i64, VideoError> {
    let database = storage
        .database()
        .map_err(|e| error("cancelFailed", e.detail))?;
    let id = downloads.cancel_and_remove(&request_id, database).await?;
    let _ = app.emit("download-records-changed", ());
    Ok(id)
}
#[tauri::command]
pub fn pause_video_download(
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
) -> Result<DownloadTaskSnapshot, VideoError> {
    downloads.set_paused(&request_id, true)
}
#[tauri::command]
pub async fn resume_video_download(
    app: tauri::AppHandle,
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<DownloadTaskSnapshot, VideoError> {
    resume_from_request(
        Some(app),
        &request_id,
        &downloads,
        &tools,
        &cookies,
        &storage,
    )
        .await
}

pub(super) async fn resume_from_request(
    app: Option<tauri::AppHandle>,
    request_id: &str,
    downloads: &DownloadManager,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
) -> Result<DownloadTaskSnapshot, VideoError> {
    let live = downloads
        .inner
        .lock()
        .map_err(|e| error("resumeFailed", e))?
        .entries
        .get(request_id)
        .is_some_and(|entry| !entry.parked);
    if live {
        return downloads.set_paused(request_id, false);
    }
    let database = storage
        .database()
        .map_err(|e| error("resumeFailed", e.detail))?;
    let record = database
        .find_request_record(request_id)
        .map_err(|e| error("resumeFailed", e.detail))?
        .filter(|record| record.status == "paused" && record.deleted_at.is_none())
        .ok_or_else(|| error("resumeFailed", "The paused task is unavailable"))?;
    restart_from_record(
        app,
        record.id,
        request_id,
        RecordRestartMode::Resume,
        downloads,
        tools,
        cookies,
        storage,
    )
        .await?
        .task
        .ok_or_else(|| error("resumeFailed", "Unable to create a resumed task"))
}
#[tauri::command]
pub fn find_download_record(
    platform: String,
    video_id: String,
    storage: tauri::State<'_, Storage>,
) -> Result<Option<DownloadRecord>, StorageError> {
    storage
        .database()?
        .find_download_record(&platform, &video_id)
}

#[tauri::command]
pub fn find_video_download_records(
    platform: String,
    video_id: String,
    storage: tauri::State<'_, Storage>,
) -> Result<Vec<DownloadRecord>, StorageError> {
    storage
        .database()?
        .video_download_records(&platform, &video_id)
}

#[tauri::command]
pub async fn redownload_record(
    app: tauri::AppHandle,
    id: i64,
    request_id: String,
    delete_existing: Option<bool>,
    downloads: tauri::State<'_, DownloadManager>,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<SubmitDownloadResult, VideoError> {
    let result = restart_from_record(
        Some(app.clone()),
        id,
        &request_id,
        if delete_existing.unwrap_or(false) {
            RecordRestartMode::ReplaceOutput
        } else {
            RecordRestartMode::CheckOutput
        },
        &downloads,
        &tools,
        &cookies,
        &storage,
    )
        .await;
    if let Err(error) = &result {
        let record = storage
            .database()
            .ok()
            .and_then(|db| db.get_download_record(id).ok());
        crate::app_logs::record(
            &app,
            crate::app_logs::entry(
                "error",
                record
                    .as_ref()
                    .map(|row| row.platform.as_str())
                    .unwrap_or("app"),
                record.as_ref().map(|row| row.video_id.as_str()),
                Some(&request_id),
                "downloadRejected",
                Some(&error.code),
                error.detail.clone(),
            ),
        );
    }
    result
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RecordRestartMode {
    CheckOutput,
    ReplaceOutput,
    Resume,
}

// Page submissions and record retries share one authoritative reconciliation path.
async fn refresh_record_output(
    app: Option<&tauri::AppHandle>,
    database: Arc<crate::database::Database>,
    record: DownloadRecord,
    downloads: &DownloadManager,
) -> Result<DownloadRecord, VideoError> {
    let record = tauri::async_runtime::spawn_blocking(move || {
        database.refresh_download_record_output(&record)
    })
        .await
        .map_err(|e| error("loadFailed", e))?
        .map_err(|e| error(&e.code, e.detail))?;
    if let Some(app) = app {
        let _ = app.emit("download-records-changed", ());
    }
    if record.output_path.is_some() && record.file_availability == "present" {
        {
            let mut registry = downloads
                .inner
                .lock()
                .map_err(|e| error("bridgeFailed", e))?;
            if registry
                .entries
                .get(&record.request_id)
                .is_some_and(|entry| entry.parked || !active(&entry.snapshot.phase))
            {
                registry.entries.remove(&record.request_id);
            }
        }
        if let Some(app) = app {
            crate::app_logs::record(
                app,
                crate::app_logs::entry(
                    "info",
                    &record.platform,
                    Some(&record.video_id),
                    Some(&record.request_id),
                    "downloadStage",
                    None,
                    format!(
                        "completed · verified existing output · format={}",
                        record.format_id
                    ),
                ),
            );
        }
    }
    Ok(record)
}

pub(super) async fn restart_from_record(
    app: Option<tauri::AppHandle>,
    id: i64,
    expected_request: &str,
    mode: RecordRestartMode,
    downloads: &DownloadManager,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
) -> Result<SubmitDownloadResult, VideoError> {
    let _submit = downloads.submission.lock().await;
    if downloads
        .inner
        .lock()
        .map_err(|e| error("bridgeFailed", e))?
        .exiting
    {
        return Err(error("applicationExiting", ""));
    }
    let database = storage
        .database()
        .map_err(|e| error("loadFailed", e.detail))?;
    downloads.initialize_limit(&database)?;
    let mut record = database
        .get_download_record(id)
        .map_err(|e| error(&e.code, e.detail))?;
    if record.deleted_at.is_some() {
        return Err(error("recordTrashed", ""));
    }
    if matches!(record.status.as_str(), "queued" | "running")
        || downloads
        .inner
        .lock()
        .map_err(|e| error("bridgeFailed", e))?
        .entries
        .get(&record.request_id)
        .is_some_and(|entry| !entry.parked && active(&entry.snapshot.phase))
    {
        return Ok(SubmitDownloadResult {
            kind: "existing",
            task: downloads.task_for(id),
            record,
        });
    }
    if record.request_id != expected_request {
        return Err(error("historyFileChanged", "Refresh the changed record"));
    }
    if mode != RecordRestartMode::Resume {
        record = refresh_record_output(app.as_ref(), database.clone(), record, downloads).await?;
        if record.output_path.is_some() && record.file_availability == "present" {
            if mode == RecordRestartMode::CheckOutput {
                return Ok(SubmitDownloadResult {
                    kind: "confirmationRequired",
                    record,
                    task: None,
                });
            }
        }
    }
    let execution =
        super::super::task_snapshot::capture_record_snapshot(&record, tools, cookies, storage)
            .await?;
    let output_policy = match mode {
        RecordRestartMode::CheckOutput => RestartOutputPolicy::RequireMissing,
        RecordRestartMode::ReplaceOutput => RestartOutputPolicy::Replace,
        RecordRestartMode::Resume => RestartOutputPolicy::Deferred,
    };
    let result = restart_with_snapshot(
        app.clone(),
        record.clone(),
        execution,
        false,
        output_policy,
        downloads,
        storage,
    )
        .await;
    if result
        .as_ref()
        .err()
        .is_some_and(|failure| failure.code == "historyOutputReturned")
    {
        let record = refresh_record_output(app.as_ref(), database, record, downloads).await?;
        if record.output_path.is_some() && record.file_availability == "present" {
            return Ok(SubmitDownloadResult {
                kind: "confirmationRequired",
                record,
                task: None,
            });
        }
        return Err(error(
            "historyFileChanged",
            "The output changed during redownload preparation; retry",
        ));
    }
    result
}

// Both callers hold the submission mutex until the immutable job is accepted.
async fn restart_with_snapshot(
    app: Option<tauri::AppHandle>,
    record: DownloadRecord,
    execution: super::super::task_snapshot::TaskExecutionSnapshot,
    restore_trashed: bool,
    output_policy: RestartOutputPolicy,
    downloads: &DownloadManager,
    storage: &Storage,
) -> Result<SubmitDownloadResult, VideoError> {
    let database = storage
        .database()
        .map_err(|e| error("loadFailed", e.detail))?;
    execution.check_tools().await?;
    prepare_download_directory(PathBuf::from(&execution.page.download_directory)).await?;
    let root = storage
        .prepare_data_directory()
        .map_err(|e| error("saveFailed", e.detail))?;
    if downloads
        .inner
        .lock()
        .map_err(|e| error("bridgeFailed", e))?
        .exiting
    {
        return Err(error("applicationExiting", ""));
    }
    let snapshot = DownloadSnapshot {
        page: execution.page.clone(),
    };
    let notify = app.clone().map(|app| {
        Arc::new(move || {
            let _ = app.emit("download-records-changed", ());
        }) as Arc<dyn Fn() + Send + Sync>
    });
    let restart_output = if output_policy == RestartOutputPolicy::Deferred {
        record.output_path.as_ref().map(|_| record.clone())
    } else {
        None
    };
    let db = database.clone();
    let (acceptance, history) = tauri::async_runtime::spawn_blocking(move || {
        DownloadRecordSession::restart(
            db,
            &root,
            uuid::Uuid::new_v4().to_string(),
            super::super::history::RestartTarget {
                record: &record,
                restore_trashed,
                output_policy,
            },
            snapshot,
            notify,
        )
    })
        .await
        .map_err(|e| error("saveFailed", e))?
        .map_err(|e| error(&e.code, e.detail))?;
    match acceptance {
        RecordAcceptance::Existing(record) => Ok(SubmitDownloadResult {
            kind: "existing",
            task: downloads.task_for(record.id),
            record,
        }),
        RecordAcceptance::Accepted(record) => {
            let history = history.expect("accepted restart owns its runtime lock");
            if downloads
                .inner
                .lock()
                .map_err(|e| error("bridgeFailed", e))?
                .exiting
            {
                history.finish(&Err(error("downloadCancelled", ""))).await;
                return Err(error("applicationExiting", ""));
            }
            let task = downloads.accept_internal(
                app,
                record.clone(),
                Job {
                    restart_output,
                    execution: Ok(execution),
                    history,
                    database,
                },
            )?;
            Ok(SubmitDownloadResult {
                kind: "accepted",
                record,
                task: Some(task),
            })
        }
        RecordAcceptance::AlreadyDownloaded(_) => {
            unreachable!("explicit restart never skips an existing output")
        }
    }
}

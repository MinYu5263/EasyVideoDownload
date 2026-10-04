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
    submit_from_page(Some(app), request, &downloads, &tools, &cookies, &storage).await
}

pub(super) async fn submit_from_page(
    app: Option<tauri::AppHandle>,
    request: SubmitDownloadRequest,
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
    let video_id = request.snapshot.video_id.as_deref().ok_or_else(|| {
        error(
            "invalidDownloadOptions",
            "Parse a video before starting a download",
        )
    })?;
    let database = storage
        .database()
        .map_err(|e| error("pageSaveFailed", e.detail))?;
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
    if let Some(record) = database
        .find_download_record(&request.snapshot.platform, video_id)
        .map_err(|e| error(&e.code, e.detail))?
    {
        if record.deleted_at.is_some() && !request.restore_trashed {
            return Err(error("recordTrashed", ""));
        }
        if matches!(record.status.as_str(), "queued" | "running") {
            return Ok(SubmitDownloadResult {
                kind: "existing",
                task: downloads.task_for(record.id),
                record,
            });
        }
        if record.output_path.is_some()
            && (request.redownload
            || record.file_availability == "missing"
            || request.restore_trashed)
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
) -> Result<Vec<DownloadTaskSnapshot>, VideoError> {
    downloads.snapshots()
}
#[tauri::command]
pub fn cancel_video_download(
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
) -> Result<(), VideoError> {
    downloads.cancel(&request_id)
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
pub async fn redownload_record(
    app: tauri::AppHandle,
    id: i64,
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<SubmitDownloadResult, VideoError> {
    restart_from_record(
        Some(app),
        id,
        &request_id,
        &downloads,
        &tools,
        &cookies,
        &storage,
    )
        .await
}

pub(super) async fn restart_from_record(
    app: Option<tauri::AppHandle>,
    id: i64,
    expected_request: &str,
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
    let record = database
        .get_download_record(id)
        .map_err(|e| error(&e.code, e.detail))?;
    if record.deleted_at.is_some() {
        return Err(error("recordTrashed", ""));
    }
    if matches!(record.status.as_str(), "queued" | "running") {
        return Ok(SubmitDownloadResult {
            kind: "existing",
            task: downloads.task_for(id),
            record,
        });
    }
    if record.request_id != expected_request {
        return Err(error("historyFileChanged", "Refresh the changed record"));
    }
    let execution =
        super::super::task_snapshot::capture_record_snapshot(&record, tools, cookies, storage)
            .await?;
    restart_with_snapshot(app, record, execution, false, downloads, storage).await
}

// Both callers hold the submission mutex until the immutable job is accepted.
async fn restart_with_snapshot(
    app: Option<tauri::AppHandle>,
    record: DownloadRecord,
    execution: super::super::task_snapshot::TaskExecutionSnapshot,
    restore_trashed: bool,
    downloads: &DownloadManager,
    storage: &Storage,
) -> Result<SubmitDownloadResult, VideoError> {
    let database = storage
        .database()
        .map_err(|e| error("loadFailed", e.detail))?;
    check_download_tool_files(&execution.settings, execution.platform).await?;
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
    let restart_output = record.output_path.as_ref().map(|_| record.clone());
    let db = database.clone();
    let (acceptance, history) = tauri::async_runtime::spawn_blocking(move || {
        DownloadRecordSession::restart(
            db,
            &root,
            uuid::Uuid::new_v4().to_string(),
            super::super::history::RestartTarget {
                record: &record,
                restore_trashed,
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

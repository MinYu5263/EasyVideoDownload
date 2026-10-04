use super::page_states::DownloadPageState;
use super::*;
use std::collections::BTreeMap;
pub(crate) mod identity;
pub(crate) mod tasks;

pub(crate) fn sanitize_source_link(source: &str) -> Result<String, StorageError> {
    let mut url = url::Url::parse(source)
        .map_err(|_| StorageError::new("invalidSettings", "Invalid source link"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(StorageError::new("invalidSettings", "Invalid source link"));
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            let key: String = key
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            !matches!(
                key.as_str(),
                "accesstoken"
                    | "refreshtoken"
                    | "token"
                    | "auth"
                    | "authorization"
                    | "cookie"
                    | "password"
                    | "passwd"
                    | "secret"
                    | "apikey"
                    | "signature"
                    | "sig"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

#[derive(Debug, Clone)]
pub struct DownloadSnapshot {
    pub page: DownloadPageState,
}

#[cfg(test)]
mod source_link_tests {
    #[test]
    fn source_link_credentials_are_removed_at_persistence_boundary() {
        use crate::database::{persistence_tests::page, Database};
        let dir = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary source credential directory: {}",
            dir.path().display()
        );
        let db = Database::open(&dir.path().join("app.db"), &dir.path().join("legacy")).unwrap();
        let mut page = page();
        page.download_directory = dir.path().to_string_lossy().into_owned();
        page.input_link = "https://www.youtube.com/watch?v=abc&access_token=private-token&password=private-password#secret".into();
        let snapshot = super::DownloadSnapshot { page };
        let id = db
            .begin_download_record("credential-test", &snapshot, &crate::datetime::now())
            .unwrap();
        db.begin_download_record("credential-test", &snapshot, &crate::datetime::now())
            .unwrap();
        let source = db.get_download_record(id).unwrap().source_link;
        assert_eq!(source, "https://www.youtube.com/watch?v=abc");
        let connection = db.connection("loadFailed").unwrap();
        let stored: String = connection
            .query_row(
                "SELECT source_link FROM download_records WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, source);
    }
}
pub enum DownloadRecordOutcome {
    Completed {
        path: String,
        size: u64,
        extension: Option<String>,
    },
    Failed {
        code: String,
        detail: String,
    },
    Cancelled,
    Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCursor {
    pub started_at: String,
    pub id: i64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub cursor: Option<HistoryCursor>,
    pub limit: u32,
    pub query: String,
    pub status: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: i64,
    pub request_id: String,
    pub platform: String,
    pub video_id: String,
    pub source_link: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_cache_path: Option<String>,
    pub duration_seconds: Option<f64>,
    pub format_id: String,
    pub format_extension: Option<String>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub selected_size_bytes: Option<u64>,
    pub size_approximate: bool,
    pub cookie_fallback: bool,
    pub download_directory: String,
    pub output_path: Option<String>,
    pub output_extension: Option<String>,
    pub file_size_bytes: Option<u64>,
    #[serde(skip)]
    pub output_identity: Option<String>,
    pub file_availability: String,
    pub successful_output: Option<SuccessfulOutput>,
    pub status: String,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub error_stage: Option<String>,
    pub failure_kind: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub file_deleted_at: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessfulOutput {
    pub format_id: String,
    pub format_extension: Option<String>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub directory: String,
    pub finished_at: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPageResult {
    pub records: Vec<DownloadRecord>,
    pub next_cursor: Option<HistoryCursor>,
    pub total_count: u64,
    pub matched_count: u64,
    pub status_counts: BTreeMap<String, u64>,
    pub trash_count: u64,
}
fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadRecord> {
    Ok(DownloadRecord {
        id: r.get("id")?,
        request_id: r.get("request_id")?,
        platform: r.get("platform")?,
        video_id: r.get("video_id")?,
        source_link: r.get("source_link")?,
        title: r.get("title")?,
        thumbnail_url: r.get("thumbnail_url")?,
        thumbnail_cache_path: r.get("thumbnail_cache_path")?,
        duration_seconds: r.get("duration_seconds")?,
        format_id: r.get("format_id")?,
        format_extension: r.get("format_extension")?,
        height: r.get("height")?,
        fps: r.get("fps")?,
        selected_size_bytes: r.get("selected_size_bytes")?,
        size_approximate: r.get("size_approximate")?,
        cookie_fallback: r.get("cookie_fallback")?,
        download_directory: r.get("download_directory")?,
        output_path: r.get("output_path")?,
        output_extension: r.get("output_extension")?,
        file_size_bytes: r.get("file_size_bytes")?,
        output_identity: r.get("output_identity")?,
        file_availability: r.get("file_availability")?,
        successful_output: r
            .get::<_, Option<String>>("successful_format_id")?
            .map(|format_id| {
                Ok::<_, rusqlite::Error>(SuccessfulOutput {
                    format_id,
                    format_extension: r.get("successful_format_extension")?,
                    height: r.get("successful_height")?,
                    fps: r.get("successful_fps")?,
                    directory: r
                        .get::<_, Option<String>>("successful_directory")?
                        .unwrap_or_default(),
                    finished_at: r.get("successful_finished_at")?,
                })
            })
            .transpose()?,
        status: r.get("status")?,
        error_code: r.get("error_code")?,
        error_detail: r.get("error_detail")?,
        error_stage: r.get("error_stage")?,
        failure_kind: r.get("failure_kind")?,
        started_at: r.get("started_at")?,
        finished_at: r.get("finished_at")?,
        updated_at: r.get("updated_at")?,
        deleted_at: r.get("deleted_at")?,
        file_deleted_at: r.get("file_deleted_at")?,
    })
}
impl Database {
    #[cfg(test)]
    pub fn begin_download_record(
        &self,
        request_id: &str,
        snapshot: &DownloadSnapshot,
        started_at: &str,
    ) -> Result<i64, StorageError> {
        let acceptance = self
            .accept_download_record_at(request_id, snapshot, false, true, "running", started_at)?;
        match acceptance {
            tasks::RecordAcceptance::Accepted(row)
            | tasks::RecordAcceptance::AlreadyDownloaded(row) => Ok(row.id),
            tasks::RecordAcceptance::Existing(row) if row.request_id == request_id => Ok(row.id),
            tasks::RecordAcceptance::Existing(_) => Err(StorageError::new(
                "downloadBusy",
                "The video already has an active task",
            )),
        }
    }
    pub fn finish_download_record(
        &self,
        request_id: &str,
        outcome: &DownloadRecordOutcome,
    ) -> Result<(), StorageError> {
        self.finish_download_record_with_failure(request_id, outcome, None, None)
    }
    pub fn finish_download_record_with_failure(
        &self,
        request_id: &str,
        outcome: &DownloadRecordOutcome,
        stage: Option<&str>,
        kind: Option<&str>,
    ) -> Result<(), StorageError> {
        if stage.is_some_and(|value| {
            !matches!(
                value,
                "preparing" | "downloading" | "processing" | "finalizing"
            )
        }) || kind.is_some_and(|value| {
            !matches!(
                value,
                "tools"
                    | "cookie"
                    | "network"
                    | "content"
                    | "format"
                    | "filesystem"
                    | "processing"
                    | "output"
                    | "execution"
                    | "unknown"
            )
        }) {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid download failure classification",
            ));
        }
        let (stage, kind) = if matches!(outcome, DownloadRecordOutcome::Failed { .. }) {
            (stage, kind)
        } else {
            (None, None)
        };
        let (status, path, size, extension, code, detail) = match outcome {
            DownloadRecordOutcome::Completed {
                path,
                size,
                extension,
            } => {
                if !Path::new(path).is_absolute() || *size == 0 || *size > 9_007_199_254_740_991 {
                    return Err(StorageError::new("invalidSettings", "Invalid output video"));
                }
                (
                    "completed",
                    Some(path.as_str()),
                    Some(*size),
                    extension.as_deref(),
                    None,
                    None,
                )
            }
            DownloadRecordOutcome::Failed { code, detail } => (
                "failed",
                None,
                None,
                None,
                Some(code.as_str()),
                Some(detail.as_str()),
            ),
            DownloadRecordOutcome::Cancelled => ("cancelled", None, None, None, None, None),
            DownloadRecordOutcome::Interrupted => ("interrupted", None, None, None, None, None),
        };
        let output_identity = path.and_then(|path| identity::capture(Path::new(path)));
        let mut c = self.connection("saveFailed")?;
        let tx = c
            .transaction()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let changed = tx.execute("UPDATE download_records SET status=?2,output_identity=CASE WHEN ?2='completed' THEN ?11 ELSE output_identity END,output_path=CASE WHEN ?2='completed' THEN ?3 ELSE output_path END,file_size_bytes=CASE WHEN ?2='completed' THEN ?4 ELSE file_size_bytes END,output_extension=CASE WHEN ?2='completed' THEN ?5 ELSE output_extension END,successful_format_id=CASE WHEN ?2='completed' THEN format_id ELSE successful_format_id END,successful_format_extension=CASE WHEN ?2='completed' THEN format_extension ELSE successful_format_extension END,successful_height=CASE WHEN ?2='completed' THEN height ELSE successful_height END,successful_fps=CASE WHEN ?2='completed' THEN fps ELSE successful_fps END,successful_directory=CASE WHEN ?2='completed' THEN download_directory ELSE successful_directory END,successful_finished_at=CASE WHEN ?2='completed' THEN ?8 ELSE successful_finished_at END,file_availability=CASE WHEN ?2='completed' THEN 'present' ELSE file_availability END,file_deleted_at=CASE WHEN ?2='completed' THEN NULL ELSE file_deleted_at END,error_code=?6,error_detail=?7,finished_at=?8,updated_at=?8,error_stage=?9,failure_kind=?10 WHERE request_id=?1 AND status IN ('queued','running')", params![request_id,status,path,size,extension,code,detail,datetime::now(),stage,kind,output_identity]).map_err(|e| StorageError::new("saveFailed", e))?;
        // Recovery scans can become stale when another instance settles a task.
        if changed == 0 && !matches!(outcome, DownloadRecordOutcome::Interrupted) {
            let previous: Option<String> = tx
                .query_row(
                    "SELECT status FROM download_records WHERE request_id=?1",
                    [request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| StorageError::new("saveFailed", e))?;
            if previous.is_some() && previous.as_deref() != Some(status) {
                return Err(StorageError::new(
                    "saveFailed",
                    "Download record is not running",
                ));
            }
        }
        tx.commit().map_err(|e| StorageError::new("saveFailed", e))
    }
    #[cfg(test)]
    pub fn discard_running_record(&self, request_id: &str) -> Result<bool, StorageError> {
        self.connection("saveFailed")?
            .execute(
                "DELETE FROM download_records WHERE request_id=?1 AND status IN ('queued','running')",
                [request_id],
            )
            .map(|n| n > 0)
            .map_err(|e| StorageError::new("saveFailed", e))
    }
    #[cfg(test)]
    pub fn record_exists(&self, request_id: &str) -> Result<bool, StorageError> {
        self.connection("loadFailed")?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM download_records WHERE request_id=?1)",
                [request_id],
                |r| r.get(0),
            )
            .map_err(|e| StorageError::new("loadFailed", e))
    }
    pub fn running_request_ids(&self) -> Result<Vec<String>, StorageError> {
        let c = self.connection("loadFailed")?;
        let mut statement = c
            .prepare("SELECT request_id FROM download_records WHERE status IN ('queued','running')")
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let result = statement
            .query_map([], |r| r.get(0))
            .map_err(|e| StorageError::new("loadFailed", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::new("loadFailed", e));
        result
    }
    #[cfg(test)]
    pub fn list_download_records(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u32,
    ) -> Result<HistoryPageResult, StorageError> {
        self.query_download_records(&HistoryQuery {
            cursor,
            limit,
            query: String::new(),
            status: None,
        })
    }

    pub fn get_download_record(&self, id: i64) -> Result<DownloadRecord, StorageError> {
        let record = self
            .connection("loadFailed")?
            .query_row("SELECT * FROM download_records WHERE id=?1", [id], from_row)
            .optional()
            .map_err(|error| StorageError::new("loadFailed", error))?
            .ok_or_else(|| StorageError::new("recordNotFound", "Download record not found"))?;
        validate_record_time(&record)?;
        Ok(record)
    }

    pub fn delete_download_record(&self, id: i64) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        // Lock the writer before checking status so another instance cannot race the removal.
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| StorageError::new("saveFailed", error))?;
        let record = read_record_for_write(&transaction, id)?;
        require_nonrunning(&record)?;
        transaction
            .execute(
                "UPDATE download_records SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL AND status NOT IN ('queued','running')",
                params![id, datetime::now()],
            )
            .map_err(|error| StorageError::new("saveFailed", error))?;
        transaction
            .commit()
            .map_err(|error| StorageError::new("saveFailed", error))
    }

    pub fn restore_download_record(&self, id: i64) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| StorageError::new("saveFailed", error))?;
        read_record_for_write(&transaction, id)?;
        transaction.execute("UPDATE download_records SET deleted_at=NULL WHERE id=?1 AND deleted_at IS NOT NULL", [id])
            .map_err(|error| StorageError::new("saveFailed", error))?;
        transaction
            .commit()
            .map_err(|error| StorageError::new("saveFailed", error))
    }

    pub fn purge_download_record(
        &self,
        id: i64,
        delete_file: impl FnOnce(&DownloadRecord, &[String]) -> Result<bool, StorageError>,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| StorageError::new("saveFailed", error))?;
        let record = read_record_for_write(&transaction, id)?;
        require_nonrunning(&record)?;
        if record.deleted_at.is_none() {
            return Err(StorageError::new(
                "recordNotTrashed",
                "Download record is not in trash",
            ));
        }
        require_no_active_downloads(&transaction, &record)?;
        let protected = protected_output_paths(&transaction, &[id])?;
        let deleted = delete_file(&record, &protected)?;
        let save_code = if deleted {
            "historyFileDeletedSaveFailed"
        } else {
            "saveFailed"
        };
        let removed = transaction.execute("DELETE FROM download_records WHERE id=?1 AND deleted_at IS NOT NULL AND status NOT IN ('queued','running')", [id])
            .map_err(|error| StorageError::new(save_code, error))?;
        if removed != 1 {
            return Err(StorageError::new(
                save_code,
                "The trashed record was not deleted",
            ));
        }
        transaction
            .commit()
            .map_err(|error| StorageError::new(save_code, error))
    }

    pub fn empty_download_record_trash(
        &self,
        delete_file: impl Fn(&DownloadRecord, &[String]) -> Result<bool, StorageError>,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| StorageError::new("saveFailed", error))?;
        let records = transaction.prepare("SELECT * FROM download_records WHERE deleted_at IS NOT NULL AND status NOT IN ('queued','running') ORDER BY id")
            .map_err(|error| StorageError::new("loadFailed", error))?
            .query_map([], from_row).map_err(|error| StorageError::new("loadFailed", error))?
            .collect::<Result<Vec<_>, _>>().map_err(|error| StorageError::new("loadFailed", error))?;
        let excluded = records.iter().map(|record| record.id).collect::<Vec<_>>();
        let protected = protected_output_paths(&transaction, &excluded)?;
        let mut removed = 0;
        let mut failed = 0;
        let mut any_file_deleted = false;
        let mut first_error = None;
        // Filesystem changes cannot be rolled back. Settle each successful record,
        // retain failures, and commit those outcomes before reporting a partial clear.
        for record in records {
            let result = (|| {
                validate_record_time(&record)?;
                require_no_active_downloads(&transaction, &record)?;
                let deleted = delete_file(&record, &protected)?;
                any_file_deleted |= deleted;
                let save_code = if deleted {
                    "historyFileDeletedSaveFailed"
                } else {
                    "saveFailed"
                };
                let changed = transaction.execute("DELETE FROM download_records WHERE id=?1 AND deleted_at IS NOT NULL AND status NOT IN ('queued','running')", [record.id])
                    .map_err(|error| StorageError::new(save_code, error))?;
                if changed != 1 {
                    return Err(StorageError::new(
                        save_code,
                        "The trashed record was not deleted",
                    ));
                }
                Ok(())
            })();
            match result {
                Ok(()) => removed += 1,
                Err(error) => {
                    failed += 1;
                    first_error.get_or_insert(error);
                }
            }
        }
        transaction.commit().map_err(|error| {
            StorageError::new(
                if any_file_deleted {
                    "historyFileDeletedSaveFailed"
                } else {
                    "saveFailed"
                },
                error,
            )
        })?;
        match first_error {
            Some(error) if removed > 0 || any_file_deleted => Err(StorageError::new(
                "historyTrashPartiallyDeleted",
                format!(
                    "Deleted {removed} records; {failed} retained. {}: {}",
                    error.code, error.detail
                ),
            )),
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn recycle_download_record_file(
        &self,
        id: i64,
        recycle: impl FnOnce(&DownloadRecord) -> Result<bool, StorageError>,
    ) -> Result<bool, StorageError> {
        let mut connection = self.connection("saveFailed")?;
        // Serialize validation and settlement against every other database writer.
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| StorageError::new("saveFailed", error))?;
        let record = read_record_for_write(&transaction, id)?;
        require_nonrunning(&record)?;
        if record.deleted_at.is_some() {
            return Err(StorageError::new(
                "recordTrashed",
                "Download record is already in trash",
            ));
        }
        if record.status != "completed" {
            return Err(StorageError::new(
                "invalidSettings",
                "Only completed downloads have a final file to recycle",
            ));
        }
        require_no_active_downloads(&transaction, &record)?;
        let protected = protected_output_paths(&transaction, &[id])?;
        let target = record
            .output_path
            .as_deref()
            .and_then(|p| Path::new(p).canonicalize().ok());
        if target.as_ref().is_some_and(|target| {
            protected.iter().any(|path| {
                Path::new(path)
                    .canonicalize()
                    .is_ok_and(|other| &other == target)
            })
        }) {
            return Err(StorageError::new(
                "historyFileInUse",
                "Another video record references this output",
            ));
        }
        let recycled = recycle(&record)?;
        // The OS recycle operation cannot be rolled back when SQLite settlement fails.
        let save_code = if recycled {
            "historyFileRecycledSaveFailed"
        } else {
            "saveFailed"
        };
        transaction.execute("UPDATE download_records SET deleted_at=?2, file_deleted_at=CASE WHEN ?3 THEN ?2 ELSE file_deleted_at END WHERE id=?1", params![id, datetime::now(), recycled])
            .map_err(|error| StorageError::new(save_code, error))?;
        transaction
            .commit()
            .map_err(|error| StorageError::new(save_code, error))?;
        Ok(recycled)
    }

    #[cfg(test)]
    pub fn query_download_records(
        &self,
        query: &HistoryQuery,
    ) -> Result<HistoryPageResult, StorageError> {
        self.query_download_records_in_scope(query, false)
    }

    pub fn query_download_records_in_scope(
        &self,
        query: &HistoryQuery,
        trashed: bool,
    ) -> Result<HistoryPageResult, StorageError> {
        let cursor = &query.cursor;
        if cursor
            .as_ref()
            .is_some_and(|v| v.id <= 0 || !datetime::is_valid(&v.started_at))
        {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid history cursor",
            ));
        }
        if query
            .status
            .as_deref()
            .is_some_and(|status| !HISTORY_STATUSES.contains(&status))
        {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid history status",
            ));
        }
        let limit = query.limit.clamp(1, 200);
        // Treat SQL LIKE metacharacters as literal user input.
        let pattern = format!(
            "%{}%",
            query
                .query
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut c = self.connection("loadFailed")?;
        let tx = c
            .transaction()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let scope = if trashed {
            "deleted_at IS NOT NULL"
        } else {
            "deleted_at IS NULL"
        };
        let total_count = tx
            .query_row(
                &format!("SELECT count(*) FROM download_records WHERE {scope}"),
                [],
                |r| r.get(0),
            )
            .map_err(|e| StorageError::new("loadFailed", e))?;
        let trash_count = tx
            .query_row(
                "SELECT count(*) FROM download_records WHERE deleted_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|error| StorageError::new("loadFailed", error))?;
        let mut status_counts: BTreeMap<String, u64> = HISTORY_STATUSES
            .iter()
            .map(|status| ((*status).into(), 0))
            .collect();
        {
            let mut statement = tx.prepare(&format!("SELECT status,count(*) FROM download_records WHERE {scope} AND {HISTORY_SEARCH} GROUP BY status"))
                .map_err(|error| StorageError::new("loadFailed", error))?;
            let counts = statement
                .query_map([&pattern], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                })
                .map_err(|error| StorageError::new("loadFailed", error))?;
            for count in counts {
                let (status, count) =
                    count.map_err(|error| StorageError::new("loadFailed", error))?;
                status_counts.insert(status, count);
            }
        }
        let matched_count = match query.status.as_ref() {
            Some(status) if status == "running" => {
                status_counts["running"] + status_counts["queued"]
            }
            Some(status) => status_counts[status],
            None => status_counts.values().sum(),
        };
        let mut records = {
            let mut statement = tx.prepare(&format!("SELECT * FROM download_records WHERE {scope} AND {HISTORY_SEARCH} AND (?2 IS NULL OR status=?2 OR (?2='running' AND status='queued')) AND (?3 IS NULL OR started_at<?3 OR (started_at=?3 AND id<?4)) ORDER BY started_at DESC,id DESC LIMIT ?5")).map_err(|e| StorageError::new("loadFailed", e))?;
            let rows = statement
                .query_map(
                    params![
                        pattern,
                        query.status,
                        cursor.as_ref().map(|v| &v.started_at),
                        cursor.as_ref().map(|v| v.id),
                        limit + 1
                    ],
                    from_row,
                )
                .map_err(|e| StorageError::new("loadFailed", e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| StorageError::new("loadFailed", e))?;
            rows
        };
        let more = records.len() > limit as usize;
        if more {
            records.pop();
        }
        for record in &records {
            validate_record_time(record)?;
        }
        let next_cursor = if more {
            records.last().map(|r| HistoryCursor {
                started_at: r.started_at.clone(),
                id: r.id,
            })
        } else {
            None
        };
        tx.commit()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        Ok(HistoryPageResult {
            records,
            next_cursor,
            total_count,
            matched_count,
            status_counts,
            trash_count,
        })
    }
}

fn require_no_active_downloads(
    transaction: &Transaction<'_>,
    record: &DownloadRecord,
) -> Result<(), StorageError> {
    if record.output_path.is_none() {
        return Ok(());
    }
    let directories = transaction.prepare("SELECT download_directory,output_path FROM download_records WHERE status IN ('queued','running')")
        .map_err(|e| StorageError::new("loadFailed", e))?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))).map_err(|e| StorageError::new("loadFailed", e))?.collect::<Result<Vec<_>, _>>().map_err(|e| StorageError::new("loadFailed", e))?;
    let normalize = |path: &str| {
        let value = std::path::Path::new(path)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(path))
            .to_string_lossy()
            .replace('/', "\\");
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value
        }
    };
    let output = record.output_path.as_deref().unwrap();
    // yt-dlp resolves sanitized names only when execution starts. Reserve its destination
    // directory while queued/running so an unknown future output cannot be removed.
    let parent = Path::new(output)
        .parent()
        .map(|p| normalize(&p.to_string_lossy()));
    if directories.iter().any(|(directory, path)| {
        parent.as_ref() == Some(&normalize(directory))
            || path
            .as_deref()
            .is_some_and(|p| normalize(p) == normalize(output))
    }) {
        return Err(StorageError::new(
            "historyBusy",
            "A download task reserves this output directory",
        ));
    }
    Ok(())
}

fn protected_output_paths(
    transaction: &Transaction<'_>,
    excluded: &[i64],
) -> Result<Vec<String>, StorageError> {
    let excluded = excluded
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let paths = transaction
        .prepare("SELECT id,output_path FROM download_records WHERE output_path IS NOT NULL")
        .map_err(|e| StorageError::new("loadFailed", e))?
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| StorageError::new("loadFailed", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StorageError::new("loadFailed", e))?;
    Ok(paths
        .into_iter()
        .filter(|(id, _)| !excluded.contains(id))
        .map(|(_, path)| path)
        .collect())
}

fn read_record_for_write(
    transaction: &Transaction<'_>,
    id: i64,
) -> Result<DownloadRecord, StorageError> {
    let record = transaction
        .query_row("SELECT * FROM download_records WHERE id=?1", [id], from_row)
        .optional()
        .map_err(|error| StorageError::new("saveFailed", error))?
        .ok_or_else(|| StorageError::new("recordNotFound", "Download record not found"))?;
    validate_record_time(&record)?;
    Ok(record)
}

fn require_nonrunning(record: &DownloadRecord) -> Result<(), StorageError> {
    if matches!(record.status.as_str(), "queued" | "running") {
        Err(StorageError::new(
            "recordRunning",
            "Cannot remove a running download record",
        ))
    } else {
        Ok(())
    }
}

const HISTORY_STATUSES: [&str; 6] = [
    "queued",
    "running",
    "completed",
    "failed",
    "cancelled",
    "interrupted",
];

// A single predicate keeps rows and per-status counts aligned, including localized aliases.
const HISTORY_SEARCH: &str = "(title LIKE ?1 ESCAPE '\\' OR
    CASE platform WHEN 'douyin' THEN 'douyin 抖音'
                  WHEN 'bilibili' THEN 'bilibili 哔哩哔哩 B站'
                  WHEN 'youtube' THEN 'youtube 油管'
                  ELSE platform END LIKE ?1 ESCAPE '\\')";

fn validate_record_time(record: &DownloadRecord) -> Result<(), StorageError> {
    if !datetime::is_valid(&record.started_at)
        || !datetime::is_valid(&record.updated_at)
        || record
        .finished_at
        .as_ref()
        .is_some_and(|value| !datetime::is_valid(value))
        || record
        .deleted_at
        .as_ref()
        .is_some_and(|value| !datetime::is_valid(value))
        || record
        .file_deleted_at
        .as_ref()
        .is_some_and(|value| !datetime::is_valid(value))
    {
        Err(StorageError::new(
            "loadFailed",
            "Invalid download record time",
        ))
    } else {
        Ok(())
    }
}

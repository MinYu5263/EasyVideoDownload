use super::*;

#[cfg(test)]
mod tests;

pub enum RecordAcceptance {
    Accepted(DownloadRecord),
    Existing(DownloadRecord),
    AlreadyDownloaded(DownloadRecord),
}

impl Database {
    pub(crate) fn remove_cancelled_download(&self, request_id: &str) -> Result<i64, StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let id = tx
            .query_row(
                "SELECT id FROM download_records WHERE request_id=?1",
                [request_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| StorageError::new("recordNotFound", e))?;
        let record = read_record_for_write(&tx, id)?;
        if record.deleted_at.is_some() || !matches!(record.status.as_str(), "paused" | "cancelled")
        {
            return Err(StorageError::new(
                "cancelFailed",
                "The task is no longer cancellable",
            ));
        }
        let protected = protected_output_paths(&tx, &[id])?;
        crate::video::download::history::permanent::delete_download_fragments(&record, &protected)?;
        tx.execute("DELETE FROM download_records WHERE id=?1", [id])
            .map_err(|e| StorageError::new("historyFileDeletedSaveFailed", e))?;
        tx.commit()
            .map_err(|e| StorageError::new("historyFileDeletedSaveFailed", e))?;
        Ok(id)
    }
    pub fn find_request_record(
        &self,
        request_id: &str,
    ) -> Result<Option<DownloadRecord>, StorageError> {
        self.connection("loadFailed")?
            .query_row(
                "SELECT * FROM download_records WHERE request_id=?1",
                [request_id],
                from_row,
            )
            .optional()
            .map_err(|e| StorageError::new("loadFailed", e))
    }
    pub fn find_download_record(
        &self,
        platform: &str,
        video_id: &str,
    ) -> Result<Option<DownloadRecord>, StorageError> {
        self.connection("loadFailed")?
            .query_row(
                "SELECT * FROM download_records WHERE platform=?1 AND video_id=?2 ORDER BY started_at DESC,id DESC LIMIT 1",
                params![platform, video_id],
                from_row,
            )
            .optional()
            .map_err(|e| StorageError::new("loadFailed", e))
    }

    pub fn find_format_download_record(
        &self,
        platform: &str,
        video_id: &str,
        format_id: &str,
    ) -> Result<Option<DownloadRecord>, StorageError> {
        self.connection("loadFailed")?
            .query_row(
                "SELECT * FROM download_records WHERE platform=?1 AND video_id=?2 AND format_id=?3",
                params![platform, video_id, format_id],
                from_row,
            )
            .optional()
            .map_err(|e| StorageError::new("loadFailed", e))
    }
    pub fn video_download_records(
        &self,
        platform: &str,
        video_id: &str,
    ) -> Result<Vec<DownloadRecord>, StorageError> {
        let connection = self.connection("loadFailed")?;
        let mut query = connection.prepare("SELECT * FROM download_records WHERE platform=?1 AND video_id=?2 AND deleted_at IS NULL ORDER BY id DESC").map_err(|e| StorageError::new("loadFailed", e))?;
        let rows = query
            .query_map(params![platform, video_id], from_row)
            .map_err(|e| StorageError::new("loadFailed", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::new("loadFailed", e))?;
        Ok(rows)
    }

    pub fn accept_download_record(
        &self,
        request_id: &str,
        snapshot: &DownloadSnapshot,
        restore_trashed: bool,
        redownload: bool,
    ) -> Result<RecordAcceptance, StorageError> {
        self.accept_download_record_at(
            request_id,
            snapshot,
            restore_trashed,
            redownload,
            "queued",
            &datetime::now(),
        )
    }

    pub(super) fn accept_download_record_at(
        &self,
        request_id: &str,
        snapshot: &DownloadSnapshot,
        restore_trashed: bool,
        redownload: bool,
        status: &str,
        started_at: &str,
    ) -> Result<RecordAcceptance, StorageError> {
        let s = &snapshot.page;
        s.validate("invalidSettings")?;
        if request_id.trim().is_empty()
            || !datetime::is_valid(started_at)
            || !Path::new(&s.download_directory).is_absolute()
        {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid download snapshot",
            ));
        }
        let format = s
            .formats
            .iter()
            .find(|f| Some(&f.format_id) == s.selected_format_id.as_ref())
            .ok_or_else(|| StorageError::new("invalidSettings", "Missing selected format"))?;
        let source_link = sanitize_source_link(&s.input_link)?;
        let mut connection = self.connection("saveFailed")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let previous = tx
            .query_row(
                "SELECT * FROM download_records WHERE platform=?1 AND video_id=?2 AND format_id=?3",
                params![s.platform, s.video_id, format.format_id],
                from_row,
            )
            .optional()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        if let Some(row) = previous {
            if row.deleted_at.is_some() && !restore_trashed {
                return Err(StorageError::new(
                    "recordTrashed",
                    "The video is in the application trash",
                ));
            }
            if row.request_id == request_id || matches!(row.status.as_str(), "queued" | "running") {
                return Ok(RecordAcceptance::Existing(row));
            }
            if row.output_path.is_some()
                && row.file_availability != "missing"
                && !redownload
                && row.deleted_at.is_none()
            {
                return Ok(RecordAcceptance::AlreadyDownloaded(row));
            }
        }
        tx.execute("INSERT INTO download_records(request_id,platform,video_id,source_link,title,thumbnail_url,thumbnail_cache_path,duration_seconds,format_id,format_extension,height,fps,selected_size_bytes,size_approximate,cookie_fallback,download_directory,status,started_at,updated_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18)
            ON CONFLICT(platform,video_id,format_id) DO UPDATE SET request_id=excluded.request_id,source_link=excluded.source_link,title=excluded.title,
            thumbnail_url=excluded.thumbnail_url,thumbnail_cache_path=excluded.thumbnail_cache_path,duration_seconds=excluded.duration_seconds,
            format_id=excluded.format_id,format_extension=excluded.format_extension,height=excluded.height,fps=excluded.fps,
            selected_size_bytes=excluded.selected_size_bytes,size_approximate=excluded.size_approximate,cookie_fallback=excluded.cookie_fallback,
            download_directory=excluded.download_directory,status=excluded.status,started_at=excluded.started_at,updated_at=excluded.updated_at,
            finished_at=NULL,pause_requested=0,error_code=NULL,error_detail=NULL,error_stage=NULL,failure_kind=NULL,deleted_at=NULL",
                   params![request_id,s.platform,s.video_id,source_link,s.title,s.thumbnail_url,s.thumbnail_cache_path,s.duration_seconds,format.format_id,format.extension,format.height,format.fps,format.size_bytes,format.size_approximate,s.cookie_fallback,s.download_directory,status,started_at])
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let json = serde_json::to_string(format).map_err(|e| StorageError::new("saveFailed", e))?;
        tx.execute(
            "UPDATE download_records SET format_snapshot_json=?2 WHERE request_id=?1",
            params![request_id, json],
        )
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let row = tx
            .query_row(
                "SELECT * FROM download_records WHERE request_id=?1",
                [request_id],
                from_row,
            )
            .map_err(|e| StorageError::new("saveFailed", e))?;
        tx.commit()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(RecordAcceptance::Accepted(row))
    }

    pub fn restart_download_record(
        &self,
        request_id: &str,
        id: i64,
        expected_request: &str,
        snapshot: &DownloadSnapshot,
        restore_trashed: bool,
        prepare_output: impl FnOnce(&DownloadRecord, &[String]) -> Result<Option<bool>, StorageError>,
    ) -> Result<RecordAcceptance, StorageError> {
        let s = &snapshot.page;
        s.validate("invalidSettings")?;
        if request_id.trim().is_empty() || !Path::new(&s.download_directory).is_absolute() {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid recorded download",
            ));
        }
        let format = s
            .formats
            .iter()
            .find(|f| Some(&f.format_id) == s.selected_format_id.as_ref())
            .ok_or_else(|| StorageError::new("invalidSettings", "Missing recorded format"))?;
        let source_link = sanitize_source_link(&s.input_link)?;
        let mut connection = self.connection("saveFailed")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let row = read_record_for_write(&tx, id)?;
        if row.deleted_at.is_some() && !restore_trashed {
            return Err(StorageError::new(
                "recordTrashed",
                "Restore the record before restarting",
            ));
        }
        if matches!(row.status.as_str(), "queued" | "running") {
            return Ok(RecordAcceptance::Existing(row));
        }
        if row.request_id != expected_request
            || row.platform != s.platform
            || Some(&row.video_id) != s.video_id.as_ref()
        {
            return Err(StorageError::new(
                "historyFileChanged",
                "The record changed before restart",
            ));
        }
        require_no_restart_conflicts(&tx, &row)?;
        let protected = protected_output_paths(&tx, &[id])?;
        // None leaves the output untouched until the accepted task begins preparation.
        let removal = prepare_output(&row, &protected)?;
        let deleted = removal == Some(true);
        let code = if deleted {
            "historyFileDeletedSaveFailed"
        } else {
            "saveFailed"
        };
        let now = datetime::now();
        // Keep the history key stable even when a legacy selector resolves to a native format ID.
        // The execution selector and all its media attributes are saved in format_snapshot_json.
        tx.execute("UPDATE download_records SET request_id=?2,format_id=?3,format_extension=?4,height=?5,fps=?6,
            selected_size_bytes=?7,size_approximate=?8,cookie_fallback=?9,download_directory=?10,status='queued',
            started_at=?11,updated_at=?11,finished_at=NULL,pause_requested=0,error_code=NULL,error_detail=NULL,error_stage=NULL,failure_kind=NULL,
            file_availability=CASE WHEN ?18 AND output_path IS NOT NULL THEN 'missing' ELSE file_availability END,
            file_deleted_at=CASE WHEN ?12 THEN ?11 ELSE file_deleted_at END,deleted_at=NULL,
            source_link=?13,title=?14,thumbnail_url=?15,thumbnail_cache_path=?16,duration_seconds=?17 WHERE id=?1",
                   params![id,request_id,row.format_id,format.extension,format.height,format.fps,format.size_bytes,
                format.size_approximate,s.cookie_fallback,s.download_directory,now,deleted,source_link,s.title,s.thumbnail_url,s.thumbnail_cache_path,s.duration_seconds,removal.is_some()]).map_err(|e| StorageError::new(code, e))?;
        let json = serde_json::to_string(format).map_err(|e| StorageError::new(code, e))?;
        tx.execute(
            "UPDATE download_records SET format_snapshot_json=?2 WHERE request_id=?1",
            params![request_id, json],
        )
            .map_err(|e| StorageError::new(code, e))?;
        let row = read_record_for_write(&tx, id).map_err(|e| StorageError::new(code, e.detail))?;
        tx.commit().map_err(|e| StorageError::new(code, e))?;
        Ok(RecordAcceptance::Accepted(row))
    }

    pub fn prepare_restart_output(
        &self,
        request_id: &str,
        previous: &DownloadRecord,
        delete_file: impl FnOnce(&DownloadRecord, &[String]) -> Result<bool, StorageError>,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let current = read_record_for_write(&tx, previous.id)?;
        if current.request_id != request_id
            || current.status != "running"
            || current.deleted_at.is_some()
            || current.output_path != previous.output_path
            || current.output_identity != previous.output_identity
            || current.file_size_bytes != previous.file_size_bytes
        {
            return Err(StorageError::new(
                "historyFileChanged",
                "The restart output changed",
            ));
        }
        // Another record may have referenced this file since the task entered the queue.
        require_no_restart_conflicts(&tx, previous)?;
        let protected = protected_output_paths(&tx, &[previous.id])?;
        let deleted = delete_file(previous, &protected)?;
        let code = if deleted {
            "historyFileDeletedSaveFailed"
        } else {
            "saveFailed"
        };
        tx.execute(
            "UPDATE download_records SET file_availability='missing',
             file_deleted_at=CASE WHEN ?3 THEN ?4 ELSE file_deleted_at END,updated_at=?4
             WHERE id=?1 AND request_id=?2",
            params![previous.id, request_id, deleted, datetime::now()],
        )
            .map_err(|e| StorageError::new(code, e))?;
        tx.commit().map_err(|e| StorageError::new(code, e))?;
        Ok(())
    }

    pub fn mark_download_running(&self, request_id: &str) -> Result<(), StorageError> {
        self.connection("saveFailed")?.execute("UPDATE download_records SET status='running',updated_at=?2 WHERE request_id=?1 AND status='queued'", params![request_id,datetime::now()])
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
    pub fn set_download_paused(&self, request_id: &str, paused: bool) -> Result<(), StorageError> {
        let changed = self.connection("saveFailed")?.execute(
            "UPDATE download_records SET pause_requested=?2,updated_at=?3 WHERE request_id=?1 AND status='running'",
            params![request_id, paused, datetime::now()],
        ).map_err(|e| StorageError::new("saveFailed", e))?;
        if changed != 1 {
            return Err(StorageError::new(
                "saveFailed",
                "Download record is no longer running",
            ));
        }
        Ok(())
    }
    pub fn download_pause_requested(&self, request_id: &str) -> Result<bool, StorageError> {
        self.connection("loadFailed")?
            .query_row(
                "SELECT pause_requested FROM download_records WHERE request_id=?1",
                [request_id],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
            .map_err(|e| StorageError::new("loadFailed", e))
    }

    pub(crate) fn refresh_download_record_output(
        &self,
        record: &DownloadRecord,
    ) -> Result<DownloadRecord, StorageError> {
        let mut connection = self.connection("saveFailed")?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::new("saveFailed", e))?;
        let current = read_record_for_write(&tx, record.id)?;
        if current.request_id != record.request_id || current.deleted_at.is_some() {
            return Err(StorageError::new(
                "historyFileChanged",
                "The record changed before checking its output",
            ));
        }
        require_nonrunning(&current)?;
        if current.output_path.is_none() {
            return Ok(current);
        }
        let present =
            crate::video::download::history::permanent::validated_record_output(&current)?
                .is_some();
        if present {
            // Legacy successful outputs are hydrated from their confirmed scalar metadata.
            let completed_format = current
                .successful_output
                .as_ref()
                .and_then(|output| output.format_snapshot.as_ref())
                .or(current.format_snapshot.as_ref());
            let format_json = completed_format
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| StorageError::new("saveFailed", e))?;
            tx.execute("UPDATE download_records SET status='completed',file_availability='present',file_deleted_at=NULL,
                pause_requested=0,error_code=NULL,error_detail=NULL,error_stage=NULL,failure_kind=NULL,
                finished_at=COALESCE(successful_finished_at,finished_at),
                format_snapshot_json=?3,
                format_extension=CASE WHEN successful_format_id IS NOT NULL THEN successful_format_extension ELSE format_extension END,
                height=CASE WHEN successful_format_id IS NOT NULL THEN successful_height ELSE height END,
                fps=CASE WHEN successful_format_id IS NOT NULL THEN successful_fps ELSE fps END,
                download_directory=COALESCE(successful_directory,download_directory),updated_at=?2 WHERE id=?1",
                       params![current.id, datetime::now(), format_json])
                .map_err(|e| StorageError::new("saveFailed", e))?;
        } else {
            tx.execute(
                "UPDATE download_records SET file_availability='missing',updated_at=?2 WHERE id=?1",
                params![current.id, datetime::now()],
            )
                .map_err(|e| StorageError::new("saveFailed", e))?;
        }
        let refreshed = read_record_for_write(&tx, current.id)?;
        tx.commit()
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(refreshed)
    }

    pub fn mark_download_output_availability(
        &self,
        record: &DownloadRecord,
        availability: &str,
    ) -> Result<(), StorageError> {
        self.connection("saveFailed")?.execute("UPDATE download_records SET file_availability=?4 WHERE id=?1 AND request_id=?2 AND output_path IS ?3", params![record.id,record.request_id,record.output_path,availability]).map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
    #[cfg(test)]
    pub fn mark_download_file_availability(
        &self,
        id: i64,
        availability: &str,
    ) -> Result<(), StorageError> {
        if !matches!(availability, "unknown" | "present" | "missing") {
            return Err(StorageError::new(
                "invalidSettings",
                "Invalid file availability",
            ));
        }
        self.connection("saveFailed")?
            .execute(
                "UPDATE download_records SET file_availability=?2 WHERE id=?1",
                params![id, availability],
            )
            .map_err(|e| StorageError::new("saveFailed", e))?;
        Ok(())
    }
}

// Normal task names always contain the video ID. Distinct IDs can share a directory;
// the permanent-delete guard separately protects every already-known output path.
fn require_no_restart_conflicts(
    tx: &Transaction<'_>,
    row: &DownloadRecord,
) -> Result<(), StorageError> {
    let Some(output) = row.output_path.as_deref() else {
        return Ok(());
    };
    let normalize = |path: &Path| {
        let value = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .replace('/', "\\");
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value
        }
    };
    let parent = Path::new(output).parent().map(normalize);
    let name = Path::new(output)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let output_format = if let Some(previous) = row.successful_output.as_ref() {
        previous
            .format_snapshot
            .as_ref()
            .map(|f| f.format_id.as_str())
            .unwrap_or(&previous.format_id)
    } else {
        row.format_snapshot
            .as_ref()
            .map(|f| f.format_id.as_str())
            .unwrap_or(&row.format_id)
    };
    let mut query = tx
        .prepare("SELECT * FROM download_records WHERE status IN ('queued','running') AND id<>?1")
        .map_err(|e| StorageError::new("loadFailed", e))?;
    let rows = query
        .query_map([row.id], from_row)
        .map_err(|e| StorageError::new("loadFailed", e))?;
    for other in rows {
        let other = other.map_err(|e| StorageError::new("loadFailed", e))?;
        let id = &other.video_id;
        let other_format = other
            .format_snapshot
            .as_ref()
            .map(|f| f.format_id.as_str())
            .unwrap_or(&other.format_id);
        let ambiguous = !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if other
            .output_path
            .as_deref()
            .is_some_and(|p| normalize(Path::new(p)) == normalize(Path::new(output)))
            || (parent.as_ref() == Some(&normalize(Path::new(&other.download_directory)))
            && (ambiguous
            || (id.eq_ignore_ascii_case(&row.video_id)
            && output_format.eq_ignore_ascii_case(other_format))
            || (!id.eq_ignore_ascii_case(&row.video_id)
            && name.contains(&format!(" [{}] [", id.to_lowercase())))))
        {
            return Err(StorageError::new(
                "historyBusy",
                "An active download may write the same output",
            ));
        }
    }
    Ok(())
}

-- Existing rows retain their snapshots and unknown failure information stays NULL.
ALTER TABLE download_records
    ADD COLUMN error_stage TEXT
        CHECK (error_stage IS NULL OR
               (status = 'failed' AND error_stage IN ('preparing', 'downloading', 'processing', 'finalizing')));

ALTER TABLE download_records
    ADD COLUMN failure_kind TEXT
        CHECK (failure_kind IS NULL OR
               (status = 'failed' AND failure_kind IN
                                      ('tools', 'cookie', 'network', 'content', 'format', 'filesystem', 'processing',
                                       'output', 'execution', 'unknown')));

CREATE INDEX idx_download_records_status_started
    ON download_records (status, started_at DESC, id DESC);

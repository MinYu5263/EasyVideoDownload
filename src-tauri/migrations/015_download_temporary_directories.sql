CREATE TABLE IF NOT EXISTS download_temporary_directories
(
    record_id
    INTEGER
    NOT
    NULL
    REFERENCES
    download_records
(
    id
) ON DELETE CASCADE,
    directory TEXT NOT NULL DEFAULT '',
    path TEXT PRIMARY KEY NOT NULL,
    identity TEXT NOT NULL
    );
CREATE INDEX IF NOT EXISTS idx_download_temporary_record ON download_temporary_directories(record_id);

-- Never reuse an ID referenced by a stale detail view, confirmation, or undo action.
-- Rebuild under Database::open's transaction while retaining every version-8 field.
CREATE TABLE download_records_recycle
(
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id           TEXT    NOT NULL UNIQUE CHECK (length(trim(request_id)) > 0),
    platform             TEXT    NOT NULL CHECK (platform IN ('douyin', 'bilibili', 'youtube')),
    video_id             TEXT    NOT NULL CHECK (length(trim(video_id)) > 0),
    source_link          TEXT    NOT NULL CHECK (length(trim(source_link)) > 0),
    title                TEXT    NOT NULL CHECK (length(trim(title)) > 0),
    thumbnail_url        TEXT,
    thumbnail_cache_path TEXT,
    duration_seconds     REAL CHECK (duration_seconds IS NULL OR duration_seconds > 0),
    format_id            TEXT    NOT NULL CHECK (length(trim(format_id)) > 0),
    format_extension     TEXT,
    height               INTEGER CHECK (height IS NULL OR height > 0),
    fps                  REAL CHECK (fps IS NULL OR fps > 0),
    selected_size_bytes  INTEGER CHECK (selected_size_bytes IS NULL OR selected_size_bytes > 0),
    size_approximate     INTEGER NOT NULL DEFAULT 0 CHECK (size_approximate IN (0, 1)),
    cookie_fallback      INTEGER NOT NULL DEFAULT 0 CHECK (cookie_fallback IN (0, 1)),
    download_directory   TEXT    NOT NULL CHECK (length(trim(download_directory)) > 0),
    output_path          TEXT CHECK (output_path IS NULL OR length(trim(output_path)) > 0),
    output_extension     TEXT,
    file_size_bytes      INTEGER CHECK (file_size_bytes IS NULL OR file_size_bytes > 0),
    status               TEXT    NOT NULL
        CHECK (status IN ('running', 'completed', 'failed', 'cancelled', 'interrupted')),
    error_code           TEXT CHECK (error_code IS NULL OR length(trim(error_code)) > 0),
    error_detail         TEXT,
    started_at           TEXT    NOT NULL,
    finished_at          TEXT,
    updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours')),
    error_stage          TEXT CHECK (error_stage IS NULL OR
                                     (status = 'failed' AND
                                      error_stage IN ('preparing', 'downloading', 'processing', 'finalizing'))),
    failure_kind         TEXT CHECK (failure_kind IS NULL OR
                                     (status = 'failed' AND failure_kind IN
                                                            ('tools', 'cookie', 'network', 'content', 'format',
                                                             'filesystem', 'processing', 'output', 'execution',
                                                             'unknown'))),
    deleted_at           TEXT CHECK (deleted_at IS NULL OR status <> 'running'),
    file_deleted_at      TEXT CHECK (file_deleted_at IS NULL OR status = 'completed'),
    CHECK ((status = 'running' AND finished_at IS NULL)
        OR (status <> 'running' AND finished_at IS NOT NULL)),
    CHECK (status <> 'completed' OR (output_path IS NOT NULL AND file_size_bytes IS NOT NULL)),
    CHECK ((status = 'failed' AND error_code IS NOT NULL)
        OR (status <> 'failed' AND error_code IS NULL AND error_detail IS NULL)),
    CHECK (status = 'completed' OR
           (output_path IS NULL AND output_extension IS NULL AND file_size_bytes IS NULL)),
    CHECK (selected_size_bytes IS NOT NULL OR size_approximate = 0)
) STRICT;

INSERT INTO download_records_recycle (id, request_id, platform, video_id, source_link, title, thumbnail_url,
                                      thumbnail_cache_path, duration_seconds, format_id, format_extension,
                                      height, fps, selected_size_bytes, size_approximate, cookie_fallback,
                                      download_directory, output_path, output_extension, file_size_bytes,
                                      status, error_code, error_detail, started_at, finished_at, updated_at,
                                      error_stage, failure_kind)
SELECT id,
       request_id,
       platform,
       video_id,
       source_link,
       title,
       thumbnail_url,
       thumbnail_cache_path,
       duration_seconds,
       format_id,
       format_extension,
       height,
       fps,
       selected_size_bytes,
       size_approximate,
       cookie_fallback,
       download_directory,
       output_path,
       output_extension,
       file_size_bytes,
       status,
       error_code,
       error_detail,
       started_at,
       finished_at,
       updated_at,
       error_stage,
       failure_kind
FROM download_records;

DROP TABLE download_records;
ALTER TABLE download_records_recycle RENAME TO download_records;

CREATE INDEX idx_download_records_started
    ON download_records (started_at DESC, id DESC);
CREATE INDEX idx_download_records_platform_started
    ON download_records (platform, started_at DESC, id DESC);
CREATE INDEX idx_download_records_status_started
    ON download_records (status, started_at DESC, id DESC);

CREATE INDEX idx_download_records_normal_started
    ON download_records (started_at DESC, id DESC) WHERE deleted_at IS NULL;

CREATE INDEX idx_download_records_trash_started
    ON download_records (started_at DESC, id DESC) WHERE deleted_at IS NOT NULL;

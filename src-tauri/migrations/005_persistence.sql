-- Preserve existing settings and tool configuration; persist page snapshots and real downloads.
CREATE TABLE download_page_states
(
    platform             TEXT PRIMARY KEY NOT NULL
        CHECK (platform IN ('douyin', 'bilibili', 'youtube')),
    input_link           TEXT             NOT NULL DEFAULT '',
    parsed_link          TEXT,
    video_id             TEXT,
    title                TEXT,
    thumbnail_url        TEXT,
    thumbnail_cache_path TEXT,
    duration_seconds     REAL CHECK (duration_seconds IS NULL OR duration_seconds > 0),
    extension            TEXT,
    formats_json         TEXT             NOT NULL DEFAULT '[]'
        CHECK (CASE
                   WHEN json_valid(formats_json)
                       THEN json_type(formats_json) = 'array'
                   ELSE 0 END),
    selected_format_id   TEXT,
    selected_height      INTEGER CHECK (selected_height IS NULL OR selected_height > 0),
    selected_fps         REAL CHECK (selected_fps IS NULL OR selected_fps > 0),
    cookie_fallback      INTEGER          NOT NULL DEFAULT 0 CHECK (cookie_fallback IN (0, 1)),
    download_directory   TEXT             NOT NULL DEFAULT '',
    directory_customized INTEGER          NOT NULL DEFAULT 0 CHECK (directory_customized IN (0, 1)),
    parser_fingerprint   TEXT,
    parsed_at            TEXT,
    updated_at           TEXT             NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours'))
) STRICT;

CREATE TABLE download_records
(
    id                   INTEGER PRIMARY KEY,
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
    CHECK ((status = 'running' AND finished_at IS NULL)
        OR (status <> 'running' AND finished_at IS NOT NULL)),
    CHECK (status <> 'completed' OR (output_path IS NOT NULL AND file_size_bytes IS NOT NULL)),
    CHECK ((status = 'failed' AND error_code IS NOT NULL)
        OR (status <> 'failed' AND error_code IS NULL AND error_detail IS NULL)),
    CHECK (status = 'completed' OR
           (output_path IS NULL AND output_extension IS NULL AND file_size_bytes IS NULL)),
    CHECK (selected_size_bytes IS NOT NULL OR size_approximate = 0)
) STRICT;

CREATE INDEX idx_download_records_started
    ON download_records (started_at DESC, id DESC);

CREATE INDEX idx_download_records_platform_started
    ON download_records (platform, started_at DESC, id DESC);

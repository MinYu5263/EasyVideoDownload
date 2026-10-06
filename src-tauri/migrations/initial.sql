-- First-release baseline: create the final runtime schema directly.
-- Database version 0 is the baseline; initialization is detected from the schema.
-- Structural changes add migrations from version 1; application builds do not increment it.

CREATE TABLE app_settings
(
    setting_key TEXT PRIMARY KEY NOT NULL CHECK (length(trim(setting_key)) > 0),
    value_json  TEXT             NOT NULL CHECK (json_valid(value_json)),
    updated_at  TEXT             NOT NULL
        DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours'))
) STRICT;

CREATE TABLE required_tools
(
    tool_id         TEXT NOT NULL,
    program_name    TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('path', 'manual', 'automatic')),
    manual_path     TEXT,
    executable_path TEXT NOT NULL CHECK (length(trim(executable_path)) > 0),
    version         TEXT NOT NULL CHECK (length(trim(version)) > 0),
    checked_at      TEXT NOT NULL,
    PRIMARY KEY (tool_id, program_name),
    CHECK ((tool_id = 'ytdlp' AND program_name = 'yt-dlp')
        OR (tool_id = 'ffmpeg' AND program_name IN ('ffmpeg', 'ffprobe'))
        OR (tool_id = 'deno' AND program_name = 'deno')),
    CHECK ((source IN ('path', 'automatic') AND manual_path IS NULL)
        OR (source = 'manual' AND manual_path IS NOT NULL AND length(trim(manual_path)) > 0))
) STRICT;

CREATE TABLE download_page_states
(
    platform             TEXT PRIMARY KEY NOT NULL
        CHECK (platform IN ('douyin', 'bilibili', 'youtube')),
    input_link           TEXT             NOT NULL DEFAULT '',
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
    id                              INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id                      TEXT    NOT NULL UNIQUE CHECK (length(trim(request_id)) > 0),
    platform                        TEXT    NOT NULL CHECK (platform IN ('douyin', 'bilibili', 'youtube')),
    video_id                        TEXT    NOT NULL CHECK (length(trim(video_id)) > 0),
    source_link                     TEXT    NOT NULL CHECK (length(trim(source_link)) > 0),
    title                           TEXT    NOT NULL CHECK (length(trim(title)) > 0),
    thumbnail_url                   TEXT,
    thumbnail_cache_path            TEXT,
    duration_seconds                REAL CHECK (duration_seconds IS NULL OR duration_seconds > 0),
    format_id                       TEXT    NOT NULL CHECK (length(trim(format_id)) > 0),
    format_extension                TEXT,
    height                          INTEGER CHECK (height IS NULL OR height > 0),
    fps                             REAL CHECK (fps IS NULL OR fps > 0),
    selected_size_bytes             INTEGER CHECK (selected_size_bytes IS NULL OR selected_size_bytes > 0),
    size_approximate                INTEGER NOT NULL DEFAULT 0 CHECK (size_approximate IN (0, 1)),
    cookie_fallback                 INTEGER NOT NULL DEFAULT 0 CHECK (cookie_fallback IN (0, 1)),
    download_directory              TEXT    NOT NULL CHECK (length(trim(download_directory)) > 0),
    output_path                     TEXT CHECK (output_path IS NULL OR length(trim(output_path)) > 0),
    output_extension                TEXT,
    file_size_bytes                 INTEGER CHECK (file_size_bytes IS NULL OR file_size_bytes > 0),
    status                          TEXT    NOT NULL
        CHECK (status IN ('queued', 'running', 'paused', 'completed', 'failed', 'cancelled', 'interrupted')),
    error_code                      TEXT CHECK (error_code IS NULL OR length(trim(error_code)) > 0),
    error_detail                    TEXT,
    started_at                      TEXT    NOT NULL,
    finished_at                     TEXT,
    updated_at                      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours')),
    error_stage                     TEXT CHECK (error_stage IS NULL OR
                                                (status = 'failed' AND error_stage IN
                                                                       ('preparing', 'downloading', 'processing',
                                                                        'finalizing'))),
    failure_kind                    TEXT CHECK (failure_kind IS NULL OR
                                                (status = 'failed' AND failure_kind IN
                                                                       ('tools', 'cookie', 'network', 'content',
                                                                        'format', 'filesystem', 'processing',
                                                                        'output', 'execution', 'unknown'))),
    deleted_at                      TEXT CHECK (deleted_at IS NULL OR status NOT IN ('queued', 'running')),
    file_deleted_at                 TEXT CHECK (file_deleted_at IS NULL OR output_path IS NOT NULL),
    file_availability               TEXT    NOT NULL DEFAULT 'unknown' CHECK (file_availability IN ('unknown', 'present', 'missing')),
    successful_format_id            TEXT,
    successful_format_extension     TEXT,
    successful_height               INTEGER,
    successful_fps                  REAL,
    successful_directory            TEXT,
    successful_finished_at          TEXT,
    output_identity                 TEXT,
    format_snapshot_json            TEXT,
    successful_format_snapshot_json TEXT,
    pause_requested                 INTEGER NOT NULL DEFAULT 0 CHECK (pause_requested IN (0, 1)),
    UNIQUE (platform, video_id, format_id),
    CHECK ((status IN ('queued', 'running', 'paused') AND finished_at IS NULL)
        OR (status NOT IN ('queued', 'running', 'paused') AND finished_at IS NOT NULL)),
    CHECK (status <> 'completed' OR (output_path IS NOT NULL AND file_size_bytes IS NOT NULL)),
    CHECK ((status = 'failed' AND error_code IS NOT NULL)
        OR (status <> 'failed' AND error_code IS NULL AND error_detail IS NULL)),
    CHECK (selected_size_bytes IS NOT NULL OR size_approximate = 0)
) STRICT;

CREATE TABLE download_temporary_directories
(
    record_id INTEGER          NOT NULL REFERENCES download_records (id) ON DELETE CASCADE,
    directory TEXT             NOT NULL DEFAULT '',
    path      TEXT PRIMARY KEY NOT NULL,
    identity  TEXT             NOT NULL
);

CREATE INDEX idx_download_records_normal_started
    ON download_records (started_at DESC, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_download_records_platform_started
    ON download_records (platform, started_at DESC, id DESC);
CREATE INDEX idx_download_records_started
    ON download_records (started_at DESC, id DESC);
CREATE INDEX idx_download_records_status_started
    ON download_records (status, started_at DESC, id DESC);
CREATE INDEX idx_download_records_trash_started
    ON download_records (started_at DESC, id DESC) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_download_temporary_record
    ON download_temporary_directories (record_id);

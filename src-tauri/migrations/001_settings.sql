CREATE TABLE app_settings
(
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    locale               TEXT    NOT NULL CHECK (locale IN ('zh-CN', 'en')),
    theme                TEXT    NOT NULL DEFAULT 'system'
        CHECK (theme IN ('system', 'light', 'dark')),
    notify_on_completion INTEGER NOT NULL DEFAULT 1 CHECK (notify_on_completion IN (0, 1)),
    notify_on_failure    INTEGER NOT NULL DEFAULT 1 CHECK (notify_on_failure IN (0, 1)),
    close_action         TEXT    NOT NULL DEFAULT 'ask' CHECK (close_action IN ('ask', 'tray', 'exit')),
    updated_at           INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;

CREATE TABLE required_tools
(
    tool_id     TEXT PRIMARY KEY CHECK (tool_id IN ('ytdlp', 'ffmpeg', 'deno')),
    source      TEXT    NOT NULL CHECK (source IN ('path', 'manual')),
    manual_path TEXT,
    checked_at  INTEGER NOT NULL CHECK (checked_at >= 0),
    CHECK (
        (source = 'path' AND manual_path IS NULL)
            OR (source = 'manual' AND manual_path IS NOT NULL AND length(trim(manual_path)) > 0)
        )
) STRICT;

CREATE TABLE required_tool_programs
(
    tool_id         TEXT NOT NULL,
    program_name    TEXT NOT NULL,
    executable_path TEXT NOT NULL CHECK (length(trim(executable_path)) > 0),
    version         TEXT NOT NULL CHECK (length(trim(version)) > 0),
    PRIMARY KEY (tool_id, program_name),
    FOREIGN KEY (tool_id) REFERENCES required_tools (tool_id) ON DELETE CASCADE,
    CHECK (
        (tool_id = 'ytdlp' AND program_name = 'yt-dlp')
            OR (tool_id = 'ffmpeg' AND program_name IN ('ffmpeg', 'ffprobe'))
            OR (tool_id = 'deno' AND program_name = 'deno')
        )
) STRICT;

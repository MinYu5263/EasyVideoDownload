-- Historical timestamps are integer seconds. Preserve their UTC instant as text.
ALTER TABLE app_settings RENAME TO app_settings_v1;

CREATE TABLE app_settings
(
    setting_key TEXT PRIMARY KEY NOT NULL CHECK (length(trim(setting_key)) > 0),
    value_json  TEXT             NOT NULL CHECK (json_valid(value_json)),
    updated_at  TEXT             NOT NULL
        DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now') || '+00:00')
) STRICT;

INSERT INTO app_settings (setting_key, value_json, updated_at)
SELECT 'locale', json_quote(locale), strftime('%Y-%m-%d %H:%M:%S', updated_at, 'unixepoch') || '+00:00'
FROM app_settings_v1
UNION ALL
SELECT 'theme', json_quote(theme), strftime('%Y-%m-%d %H:%M:%S', updated_at, 'unixepoch') || '+00:00'
FROM app_settings_v1
UNION ALL
SELECT 'notify_on_completion',
       CASE notify_on_completion WHEN 1 THEN 'true' ELSE 'false' END,
       strftime('%Y-%m-%d %H:%M:%S', updated_at, 'unixepoch') || '+00:00'
FROM app_settings_v1
UNION ALL
SELECT 'notify_on_failure',
       CASE notify_on_failure WHEN 1 THEN 'true' ELSE 'false' END,
       strftime('%Y-%m-%d %H:%M:%S', updated_at, 'unixepoch') || '+00:00'
FROM app_settings_v1
UNION ALL
SELECT 'close_action', json_quote(close_action), strftime('%Y-%m-%d %H:%M:%S', updated_at, 'unixepoch') || '+00:00'
FROM app_settings_v1;

DROP TABLE app_settings_v1;

CREATE TABLE required_tools_v2
(
    tool_id         TEXT NOT NULL,
    program_name    TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('path', 'manual')),
    manual_path     TEXT,
    executable_path TEXT NOT NULL CHECK (length(trim(executable_path)) > 0),
    version         TEXT NOT NULL CHECK (length(trim(version)) > 0),
    checked_at      TEXT NOT NULL,
    PRIMARY KEY (tool_id, program_name),
    CHECK (
        (tool_id = 'ytdlp' AND program_name = 'yt-dlp')
            OR (tool_id = 'ffmpeg' AND program_name IN ('ffmpeg', 'ffprobe'))
            OR (tool_id = 'deno' AND program_name = 'deno')
        ),
    CHECK (
        (source = 'path' AND manual_path IS NULL)
            OR (source = 'manual' AND manual_path IS NOT NULL AND length(trim(manual_path)) > 0)
        )
) STRICT;

INSERT INTO required_tools_v2
(tool_id, program_name, source, manual_path, executable_path, version, checked_at)
SELECT t.tool_id,
       p.program_name,
       t.source,
       t.manual_path,
       p.executable_path,
       p.version,
       strftime('%Y-%m-%d %H:%M:%S', t.checked_at, 'unixepoch') || '+00:00'
FROM required_tools AS t
         JOIN required_tool_programs AS p ON p.tool_id = t.tool_id;

DROP TABLE required_tool_programs;
DROP TABLE required_tools;
ALTER TABLE required_tools_v2 RENAME TO required_tools;

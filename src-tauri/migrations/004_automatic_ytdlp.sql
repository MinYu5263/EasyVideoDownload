CREATE TABLE required_tools_v4
(
    tool_id         TEXT NOT NULL,
    program_name    TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('path', 'manual', 'automatic')),
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
    CHECK (source <> 'automatic' OR tool_id = 'ytdlp'),
    CHECK (
        (source IN ('path', 'automatic') AND manual_path IS NULL)
            OR (source = 'manual' AND manual_path IS NOT NULL AND length(trim(manual_path)) > 0)
        )
) STRICT;

INSERT INTO required_tools_v4
SELECT *
FROM required_tools;
DROP TABLE required_tools;
ALTER TABLE required_tools_v4 RENAME TO required_tools;

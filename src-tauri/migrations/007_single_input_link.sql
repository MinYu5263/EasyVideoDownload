-- A parsed result now belongs to the current input; discard mismatched legacy results.
UPDATE download_page_states
SET video_id             = NULL,
    title                = NULL,
    thumbnail_url        = NULL,
    thumbnail_cache_path = NULL,
    duration_seconds     = NULL,
    extension            = NULL,
    formats_json         = '[]',
    selected_format_id   = NULL,
    selected_height      = NULL,
    selected_fps         = NULL,
    cookie_fallback      = 0,
    parser_fingerprint   = NULL,
    parsed_at            = NULL,
    updated_at           = strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours')
WHERE parsed_link IS NOT NULL
  AND input_link <> parsed_link;

ALTER TABLE download_page_states DROP COLUMN parsed_link;

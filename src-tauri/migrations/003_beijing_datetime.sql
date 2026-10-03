-- Version 2 stored canonical UTC text. Preserve each instant as fixed UTC+8
-- Beijing time, without an offset suffix. Rust validates dates before this runs.
ALTER TABLE app_settings RENAME TO app_settings_v2;

CREATE TABLE app_settings
(
    setting_key TEXT PRIMARY KEY NOT NULL CHECK (length(trim(setting_key)) > 0),
    value_json  TEXT             NOT NULL CHECK (json_valid(value_json)),
    updated_at  TEXT             NOT NULL
        DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now', '+8 hours'))
) STRICT;

INSERT INTO app_settings (setting_key, value_json, updated_at)
SELECT setting_key, value_json, strftime('%Y-%m-%d %H:%M:%S', updated_at, '+8 hours')
FROM app_settings_v2;

DROP TABLE app_settings_v2;

UPDATE required_tools
SET checked_at = strftime('%Y-%m-%d %H:%M:%S', checked_at, '+8 hours');

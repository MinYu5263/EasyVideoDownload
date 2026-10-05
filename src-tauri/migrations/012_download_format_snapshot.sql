ALTER TABLE download_records
    ADD COLUMN format_snapshot_json TEXT;
ALTER TABLE download_records
    ADD COLUMN successful_format_snapshot_json TEXT;

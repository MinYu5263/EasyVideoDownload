-- Track the completed file independently of its name and byte count.
-- Legacy outputs remain NULL: their original identity cannot be reconstructed.
ALTER TABLE download_records
    ADD COLUMN output_identity TEXT;

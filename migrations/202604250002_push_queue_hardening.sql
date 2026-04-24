ALTER TABLE push_queue ADD COLUMN last_error TEXT;
ALTER TABLE push_queue ADD COLUMN claimed_at DATETIME;
ALTER TABLE push_queue ADD COLUMN processed_at DATETIME;

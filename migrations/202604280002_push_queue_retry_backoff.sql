ALTER TABLE push_queue ADD COLUMN next_retry_at DATETIME;

CREATE INDEX IF NOT EXISTS idx_push_queue_status_next_retry_at
ON push_queue(status, next_retry_at, id);

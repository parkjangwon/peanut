ALTER TABLE push_queue ADD COLUMN partial_failure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE push_queue ADD COLUMN failed_destinations_json TEXT;

UPDATE push_queue
SET partial_failure_count = CASE
    WHEN last_error LIKE 'partial delivery failures:%' THEN 1 + ((LENGTH(last_error) - LENGTH(REPLACE(last_error, ' | ', ''))) / 3)
    ELSE partial_failure_count
END
WHERE status = 'sent'
  AND partial_failure_count = 0
  AND last_error LIKE 'partial delivery failures:%';

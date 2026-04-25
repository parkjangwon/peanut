ALTER TABLE functions ADD COLUMN api_key_hash TEXT;
ALTER TABLE functions ADD COLUMN allowed_origins_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE functions ADD COLUMN rate_limit_per_minute INTEGER NOT NULL DEFAULT 60;

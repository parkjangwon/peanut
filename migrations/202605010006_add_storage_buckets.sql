CREATE TABLE IF NOT EXISTS storage_buckets (
    app_id TEXT NOT NULL,
    name TEXT NOT NULL,
    public_read BOOLEAN NOT NULL DEFAULT FALSE,
    allow_client_uploads BOOLEAN NOT NULL DEFAULT FALSE,
    max_object_bytes INTEGER NULL,
    allowed_mime_types_json TEXT NOT NULL DEFAULT '[]',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME NULL,
    PRIMARY KEY(app_id, name),
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_storage_buckets_app_id
    ON storage_buckets(app_id, deleted_at);

CREATE TABLE IF NOT EXISTS data_query_presets (
    id TEXT PRIMARY KEY,
    table_id TEXT NOT NULL,
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    params_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(table_id) REFERENCES data_tables(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE CASCADE,
    UNIQUE(table_id, name)
);

CREATE INDEX IF NOT EXISTS idx_data_query_presets_table_created_at
    ON data_query_presets(table_id, created_at DESC);

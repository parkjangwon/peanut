CREATE TABLE IF NOT EXISTS data_tables (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    access_policy_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, name),
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS data_rows (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    table_id TEXT NOT NULL,
    owner_user_id TEXT,
    data_json TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(table_id) REFERENCES data_tables(id) ON DELETE CASCADE,
    FOREIGN KEY(owner_user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_data_rows_table_created_at ON data_rows(table_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_data_rows_table_owner ON data_rows(table_id, owner_user_id);
CREATE INDEX IF NOT EXISTS idx_data_tables_app_id ON data_tables(app_id, name);
CREATE INDEX IF NOT EXISTS idx_data_rows_app_table ON data_rows(app_id, table_id, created_at DESC);

CREATE TABLE IF NOT EXISTS data_row_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id TEXT NOT NULL DEFAULT 'default',
    table_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    actor_user_id TEXT NOT NULL,
    action TEXT NOT NULL,
    diff_json TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(table_id) REFERENCES data_tables(id) ON DELETE CASCADE,
    FOREIGN KEY(actor_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_data_row_events_app_table ON data_row_events(app_id, table_id, created_at DESC);

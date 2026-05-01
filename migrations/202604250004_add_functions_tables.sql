CREATE TABLE IF NOT EXISTS functions (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    endpoint_slug TEXT NOT NULL,
    runtime TEXT NOT NULL,
    source_code TEXT NOT NULL,
    invoke_policy TEXT NOT NULL DEFAULT 'authenticated',
    env_json TEXT NOT NULL DEFAULT '{}',
    timeout_ms INTEGER NOT NULL DEFAULT 3000,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, name),
    UNIQUE(app_id, endpoint_slug),
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT,
    FOREIGN KEY(updated_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS function_invocations (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    function_id TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT,
    response_json TEXT,
    error TEXT,
    duration_ms INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at DATETIME,
    FOREIGN KEY(function_id) REFERENCES functions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_functions_app_id ON functions(app_id, name);
CREATE INDEX IF NOT EXISTS idx_functions_app_endpoint ON functions(app_id, endpoint_slug);
CREATE INDEX IF NOT EXISTS idx_function_invocations_app_function ON function_invocations(app_id, function_id, created_at DESC);

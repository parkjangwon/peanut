CREATE TABLE IF NOT EXISTS functions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    endpoint_slug TEXT NOT NULL UNIQUE,
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
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT,
    FOREIGN KEY(updated_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS function_invocations (
    id TEXT PRIMARY KEY,
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

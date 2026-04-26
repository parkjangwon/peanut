ALTER TABLE functions ADD COLUMN active_version_number INTEGER NOT NULL DEFAULT 1;
ALTER TABLE functions ADD COLUMN active_version_id TEXT;
ALTER TABLE functions ADD COLUMN secret_key_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE function_invocations ADD COLUMN function_version_id TEXT;
ALTER TABLE function_invocations ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE function_invocations ADD COLUMN parent_invocation_id TEXT;

CREATE TABLE IF NOT EXISTS function_versions (
    id TEXT PRIMARY KEY,
    function_id TEXT NOT NULL,
    version_number INTEGER NOT NULL,
    runtime TEXT NOT NULL,
    source_code TEXT NOT NULL,
    invoke_policy TEXT NOT NULL,
    env_json TEXT NOT NULL,
    api_key_hash TEXT,
    allowed_origins_json TEXT NOT NULL,
    rate_limit_per_minute INTEGER NOT NULL,
    timeout_ms INTEGER NOT NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(function_id) REFERENCES functions(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT,
    UNIQUE(function_id, version_number)
);

CREATE INDEX IF NOT EXISTS idx_function_versions_function_id
ON function_versions(function_id, version_number DESC);

CREATE TABLE IF NOT EXISTS function_version_secrets (
    version_id TEXT NOT NULL,
    secret_key TEXT NOT NULL,
    secret_value TEXT NOT NULL,
    PRIMARY KEY(version_id, secret_key),
    FOREIGN KEY(version_id) REFERENCES function_versions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_function_version_secrets_version_id
ON function_version_secrets(version_id);

INSERT INTO function_versions (
    id,
    function_id,
    version_number,
    runtime,
    source_code,
    invoke_policy,
    env_json,
    api_key_hash,
    allowed_origins_json,
    rate_limit_per_minute,
    timeout_ms,
    created_by,
    created_at
)
SELECT
    lower(hex(randomblob(16))),
    id,
    1,
    runtime,
    source_code,
    invoke_policy,
    env_json,
    api_key_hash,
    allowed_origins_json,
    rate_limit_per_minute,
    timeout_ms,
    created_by,
    created_at
FROM functions
WHERE NOT EXISTS (
    SELECT 1 FROM function_versions WHERE function_versions.function_id = functions.id
);

UPDATE functions
SET active_version_number = 1,
    active_version_id = (
        SELECT id
        FROM function_versions
        WHERE function_versions.function_id = functions.id
        ORDER BY version_number DESC, created_at DESC
        LIMIT 1
    )
WHERE active_version_id IS NULL;

UPDATE function_invocations
SET function_version_id = (
    SELECT active_version_id
    FROM functions
    WHERE functions.id = function_invocations.function_id
)
WHERE function_version_id IS NULL;

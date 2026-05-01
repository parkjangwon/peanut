CREATE TABLE IF NOT EXISTS auth_provider_configs (
    app_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    client_id TEXT NULL,
    client_secret_ciphertext TEXT NULL,
    redirect_uri TEXT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(app_id, provider),
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_auth_provider_configs_enabled
    ON auth_provider_configs(app_id, enabled);

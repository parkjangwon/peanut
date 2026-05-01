ALTER TABLE data_tables ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE data_rows ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE data_row_events ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE data_query_presets ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';

ALTER TABLE functions ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE function_versions ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE function_invocations ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';

ALTER TABLE push_subscriptions ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE push_queue ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';

CREATE INDEX IF NOT EXISTS idx_data_tables_app_id ON data_tables(app_id, name);
CREATE INDEX IF NOT EXISTS idx_data_rows_app_table ON data_rows(app_id, table_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_data_row_events_app_table ON data_row_events(app_id, table_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_data_query_presets_app_table ON data_query_presets(app_id, table_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_functions_app_id ON functions(app_id, name);
CREATE INDEX IF NOT EXISTS idx_functions_app_endpoint ON functions(app_id, endpoint_slug);
CREATE INDEX IF NOT EXISTS idx_function_versions_app_function ON function_versions(app_id, function_id, version_number DESC);
CREATE INDEX IF NOT EXISTS idx_function_invocations_app_function ON function_invocations(app_id, function_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_push_subscriptions_app_user ON push_subscriptions(app_id, user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_push_queue_app_user ON push_queue(app_id, user_id, id DESC);

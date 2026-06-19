use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTablesResponse {
    pub tables: Vec<DataTableSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableSummary {
    pub name: String,
    pub display_name: String,
    pub policy_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableResponse {
    pub table: DataTableDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableDetail {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowsResponse {
    pub rows: Vec<DataRowResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventsResponse {
    pub events: Vec<DataRowEventResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventCheckpointResponse {
    pub table_name: String,
    pub latest_event_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventResponse {
    pub id: i64,
    pub row_id: String,
    pub actor_user_id: String,
    pub action: String,
    pub diff: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowResponse {
    pub id: String,
    pub owner_user_id: Option<String>,
    pub data: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowRealtimeEvent {
    pub id: i64,
    pub app_id: String,
    pub event: String,
    pub table_name: String,
    pub row_id: String,
    pub owner_user_id: Option<String>,
    pub actor_user_id: String,
    pub action: String,
    pub diff: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPresetsResponse {
    pub presets: Vec<QueryPresetResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPresetResponse {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub params: ListRowsParams,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableExportResponse {
    pub metadata: TableExportMetadata,
    pub table: DataTableDetail,
    pub rows: Vec<DataRowResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableExportMetadata {
    pub export_version: String,
    pub row_count: usize,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableImportResponse {
    pub imported_count: usize,
    pub dry_run: bool,
    pub would_insert: usize,
    pub would_replace: usize,
    pub schema_changes: SchemaDiffPreview,
    pub validation_errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaDiffPreview {
    pub added_fields: Vec<String>,
    pub removed_fields: Vec<String>,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRowRequest {
    pub id: Option<String>,
    pub owner_user_id: Option<String>,
    pub data: Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableRestoreSpec {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
    pub created_by: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableImportRequest {
    pub mode: Option<String>,
    pub dry_run: Option<bool>,
    pub restore_table: Option<bool>,
    pub metadata: Option<TableExportMetadata>,
    pub verify_checksum: Option<bool>,
    pub table: Option<DataTableRestoreSpec>,
    pub rows: Vec<ImportRowRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTableRequest {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTableRequest {
    pub display_name: Option<String>,
    pub schema: Option<DataTableSchema>,
    pub access_policy: Option<AccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertQueryPresetRequest {
    pub name: String,
    pub display_name: String,
    pub params: ListRowsParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRowRequest {
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableSchema {
    pub fields: BTreeMap<String, DataFieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DataFieldSpec {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub relation_table: Option<String>,
    #[serde(default)]
    pub file_bucket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessRules {
    #[serde(default)]
    pub create: Option<String>,
    #[serde(default)]
    pub read: Option<String>,
    #[serde(default)]
    pub update: Option<String>,
    #[serde(default)]
    pub delete: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessPolicy {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub rules: Option<AccessRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRowsParams {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Option<String>,
    pub order: Option<String>,
    pub search: Option<String>,
    pub title_contains: Option<String>,
    pub done: Option<bool>,
    pub filter_field: Option<String>,
    pub filter_op: Option<String>,
    pub filter_value: Option<String>,
    pub expand: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GetRowParams {
    pub expand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRowEventsParams {
    pub limit: Option<usize>,
    pub row_id: Option<String>,
    pub action: Option<String>,
    pub since_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct DataTableRecord {
    pub(crate) id: String,
    pub(crate) app_id: String,
    pub(crate) name: String,
    pub(crate) display_name: String,
    pub(crate) schema_json: String,
    pub(crate) access_policy_json: String,
    pub(crate) created_by: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct DataRowRecord {
    pub(crate) id: String,
    pub(crate) owner_user_id: Option<String>,
    pub(crate) data_json: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct DataRowEventRecord {
    pub(super) id: i64,
    pub(super) row_id: String,
    pub(super) actor_user_id: String,
    pub(super) action: String,
    pub(super) diff_json: Option<String>,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct QueryPresetRecord {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) display_name: String,
    pub(crate) params_json: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

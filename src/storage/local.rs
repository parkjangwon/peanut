use std::{
    io,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const DEFAULT_BUCKET: &str = "default";
const METADATA_ROOT_DIR: &str = ".peanut_meta";
const MULTIPART_ROOT_DIR: &str = ".peanut_multipart";

#[derive(Debug)]
pub struct LocalStorage {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageObjectMetadata {
    pub content_type: String,
    pub content_length: u64,
    pub etag: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub data: Vec<u8>,
    pub metadata: StorageObjectMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageListItem {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageListPage {
    pub objects: Vec<StorageListItem>,
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub content_type: String,
    pub initiated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultipartUploadPart {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedMultipartPart {
    pub part_number: u32,
    pub etag: String,
}

impl LocalStorage {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub async fn put(&self, key: &str, data: &[u8]) -> io::Result<()> {
        self.put_object(DEFAULT_BUCKET, key, data, None)
            .await
            .map(|_| ())
    }

    pub async fn get(&self, key: &str) -> io::Result<Vec<u8>> {
        self.get_object(DEFAULT_BUCKET, key)
            .await
            .map(|object| object.data)
    }

    pub async fn delete(&self, key: &str) -> io::Result<()> {
        self.delete_object(DEFAULT_BUCKET, key).await
    }

    pub async fn list(&self) -> io::Result<Vec<String>> {
        self.list_objects_v2(DEFAULT_BUCKET, None, None, None, None)
            .await
            .map(|page| page.objects.into_iter().map(|item| item.key).collect())
    }

    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
        content_type: Option<&str>,
    ) -> io::Result<StorageObjectMetadata> {
        let path = self.resolve_object_path(bucket, key)?;
        let metadata_path = self.resolve_metadata_path(bucket, key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = metadata_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let previous_metadata = self.read_metadata(bucket, key).await.ok();
        let now = chrono::Utc::now().to_rfc3339();
        let metadata = StorageObjectMetadata {
            content_type: content_type
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("application/octet-stream")
                .to_string(),
            content_length: data.len() as u64,
            etag: compute_etag(data),
            created_at: previous_metadata
                .as_ref()
                .map(|value| value.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };

        tokio::fs::write(&path, data).await?;
        let encoded = serde_json::to_vec(&metadata)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        tokio::fs::write(&metadata_path, encoded).await?;
        Ok(metadata)
    }

    pub async fn get_object(&self, bucket: &str, key: &str) -> io::Result<StoredObject> {
        let path = self.resolve_object_path(bucket, key)?;
        let data = tokio::fs::read(path).await?;
        let metadata = self.read_metadata(bucket, key).await?;
        Ok(StoredObject { data, metadata })
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> io::Result<StorageObjectMetadata> {
        let path = self.resolve_object_path(bucket, key)?;
        let _ = tokio::fs::metadata(path).await?;
        self.read_metadata(bucket, key).await
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> io::Result<()> {
        let path = self.resolve_object_path(bucket, key)?;
        let metadata_path = self.resolve_metadata_path(bucket, key)?;
        tokio::fs::remove_file(path).await?;
        match tokio::fs::remove_file(metadata_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    pub async fn list_objects_v2(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        delimiter: Option<&str>,
        max_keys: Option<usize>,
        continuation_token: Option<&str>,
    ) -> io::Result<StorageListPage> {
        let prefix = normalize_optional_prefix(prefix)?.unwrap_or_default();
        let delimiter = normalize_optional_delimiter(delimiter)?;
        let continuation_token = normalize_optional_key(continuation_token)?;
        let max_keys = max_keys.unwrap_or(1000).min(1000);
        let bucket_root = self.resolve_bucket_root(bucket)?;
        let metadata_root = self.metadata_bucket_root(bucket)?;
        let mut objects = tokio::task::spawn_blocking(move || {
            collect_bucket_objects(&bucket_root, &metadata_root)
        })
        .await
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))??;
        objects.sort_by(|left, right| left.key.cmp(&right.key));

        let entries = build_list_entries(objects, &prefix, delimiter.as_deref());
        let filtered: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                continuation_token
                    .as_ref()
                    .map(|value| entry.token() > value.as_str())
                    .unwrap_or(true)
            })
            .collect();

        let is_truncated = filtered.len() > max_keys;
        let page_entries = filtered.into_iter().take(max_keys).collect::<Vec<_>>();
        let next_continuation_token = if is_truncated {
            page_entries.last().map(|entry| entry.token().to_string())
        } else {
            None
        };
        let mut page_objects = Vec::new();
        let mut common_prefixes = Vec::new();
        for entry in page_entries {
            match entry {
                ListEntry::Object(item) => page_objects.push(item),
                ListEntry::CommonPrefix(prefix) => common_prefixes.push(prefix),
            }
        }

        Ok(StorageListPage {
            objects: page_objects,
            common_prefixes,
            is_truncated,
            next_continuation_token,
        })
    }

    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: Option<&str>,
    ) -> io::Result<MultipartUpload> {
        let upload_id = uuid::Uuid::new_v4().to_string();
        let upload = MultipartUpload {
            upload_id: upload_id.clone(),
            bucket: normalize_namespace(bucket, "storage bucket cannot be empty")?
                .to_string_lossy()
                .replace('\\', "/"),
            key: normalize_object_key(key)?.to_string_lossy().replace('\\', "/"),
            content_type: content_type
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("application/octet-stream")
                .to_string(),
            initiated_at: chrono::Utc::now().to_rfc3339(),
        };
        let manifest_path = self.resolve_multipart_manifest_path(bucket, &upload_id)?;
        if let Some(parent) = manifest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let encoded = serde_json::to_vec(&upload)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        tokio::fs::write(manifest_path, encoded).await?;
        Ok(upload)
    }

    pub async fn put_multipart_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: u32,
        data: &[u8],
    ) -> io::Result<MultipartUploadPart> {
        if part_number == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "multipart part number must be greater than zero",
            ));
        }
        let upload = self.read_multipart_upload(bucket, key, upload_id).await?;
        let part_path = self.resolve_multipart_part_path(bucket, &upload.upload_id, part_number)?;
        let metadata_path = self.resolve_multipart_part_metadata_path(bucket, &upload.upload_id, part_number)?;
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let metadata = MultipartUploadPart {
            part_number,
            etag: compute_etag(data),
            size: data.len() as u64,
        };
        tokio::fs::write(&part_path, data).await?;
        let encoded = serde_json::to_vec(&metadata)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        tokio::fs::write(metadata_path, encoded).await?;
        Ok(metadata)
    }

    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[CompletedMultipartPart],
    ) -> io::Result<StorageObjectMetadata> {
        if parts.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "multipart completion requires at least one part",
            ));
        }
        let upload = self.read_multipart_upload(bucket, key, upload_id).await?;
        let mut assembled = Vec::new();
        let mut previous_part_number = 0;
        for part in parts {
            if part.part_number == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "multipart part number must be greater than zero",
                ));
            }
            if part.part_number <= previous_part_number {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "multipart parts must be in ascending order",
                ));
            }
            previous_part_number = part.part_number;
            let stored_part = self.read_multipart_part(bucket, &upload.upload_id, part.part_number).await?;
            if stored_part.0.etag != part.etag {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("multipart part {} etag mismatch", part.part_number),
                ));
            }
            assembled.extend_from_slice(&stored_part.1);
        }
        let metadata = self
            .put_object(bucket, &upload.key, &assembled, Some(upload.content_type.as_str()))
            .await?;
        self.abort_multipart_upload(bucket, key, upload_id).await?;
        Ok(metadata)
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> io::Result<()> {
        let upload = self.read_multipart_upload(bucket, key, upload_id).await?;
        let upload_root = self.resolve_multipart_upload_root(bucket, &upload.upload_id)?;
        tokio::fs::remove_dir_all(upload_root).await
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(normalized))
    }

    fn metadata_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(METADATA_ROOT_DIR).join(normalized))
    }

    fn resolve_object_path(&self, bucket: &str, key: &str) -> io::Result<PathBuf> {
        let bucket_root = self.resolve_bucket_root(bucket)?;
        let relative = normalize_object_key(key)?;
        Ok(bucket_root.join(relative))
    }

    fn resolve_metadata_path(&self, bucket: &str, key: &str) -> io::Result<PathBuf> {
        let metadata_root = self.metadata_bucket_root(bucket)?;
        let relative = normalize_object_key(key)?;
        let mut metadata_path = metadata_root.join(&relative);
        let file_name = metadata_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "storage key contains invalid filename",
                )
            })?;
        metadata_path.set_file_name(format!("{file_name}.json"));
        Ok(metadata_path)
    }

    async fn read_metadata(&self, bucket: &str, key: &str) -> io::Result<StorageObjectMetadata> {
        let metadata_path = self.resolve_metadata_path(bucket, key)?;
        let raw = tokio::fs::read(metadata_path).await?;
        serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }

    fn multipart_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(MULTIPART_ROOT_DIR).join(normalized))
    }

    fn resolve_upload_id(upload_id: &str) -> io::Result<String> {
        let trimmed = upload_id.trim();
        if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid multipart upload id",
            ));
        }
        Ok(trimmed.to_string())
    }

    fn resolve_multipart_upload_root(&self, bucket: &str, upload_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .multipart_bucket_root(bucket)?
            .join(Self::resolve_upload_id(upload_id)?))
    }

    fn resolve_multipart_manifest_path(&self, bucket: &str, upload_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .resolve_multipart_upload_root(bucket, upload_id)?
            .join("upload.json"))
    }

    fn resolve_multipart_part_path(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> io::Result<PathBuf> {
        Ok(self
            .resolve_multipart_upload_root(bucket, upload_id)?
            .join("parts")
            .join(format!("{part_number:05}.part")))
    }

    fn resolve_multipart_part_metadata_path(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> io::Result<PathBuf> {
        Ok(self
            .resolve_multipart_upload_root(bucket, upload_id)?
            .join("parts")
            .join(format!("{part_number:05}.json")))
    }

    async fn read_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> io::Result<MultipartUpload> {
        let manifest_path = self.resolve_multipart_manifest_path(bucket, upload_id)?;
        let raw = tokio::fs::read(manifest_path).await?;
        let upload: MultipartUpload = serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        let normalized_bucket = normalize_namespace(bucket, "storage bucket cannot be empty")?
            .to_string_lossy()
            .replace('\\', "/");
        let normalized_key = normalize_object_key(key)?.to_string_lossy().replace('\\', "/");
        if upload.bucket != normalized_bucket || upload.key != normalized_key {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "multipart upload not found for object",
            ));
        }
        Ok(upload)
    }

    async fn read_multipart_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> io::Result<(MultipartUploadPart, Vec<u8>)> {
        let metadata_path = self.resolve_multipart_part_metadata_path(bucket, upload_id, part_number)?;
        let part_path = self.resolve_multipart_part_path(bucket, upload_id, part_number)?;
        let raw = tokio::fs::read(metadata_path).await?;
        let metadata = serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        let data = tokio::fs::read(part_path).await?;
        Ok((metadata, data))
    }
}

fn normalize_namespace(namespace: &str, empty_message: &str) -> io::Result<PathBuf> {
    let trimmed = namespace.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, empty_message));
    }

    let path = Path::new(trimmed);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "storage namespace contains invalid path segments",
                ))
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, empty_message));
    }
    Ok(normalized)
}

fn normalize_object_key(key: &str) -> io::Result<PathBuf> {
    let trimmed = key.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage key cannot be empty",
        ));
    }

    let relative = Path::new(trimmed);
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "storage key contains invalid path segments",
                ))
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage key cannot be empty",
        ));
    }
    Ok(normalized)
}

fn normalize_optional_key(value: Option<&str>) -> io::Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => Ok(Some(
            normalize_object_key(raw)?
                .to_string_lossy()
                .replace('\\', "/"),
        )),
        _ => Ok(None),
    }
}

fn normalize_optional_prefix(value: Option<&str>) -> io::Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => {
            let trimmed = raw.trim().trim_start_matches('/');
            let has_trailing_slash = trimmed.ends_with('/');
            let normalized = normalize_object_key(trimmed)?
                .to_string_lossy()
                .replace('\\', "/");
            if has_trailing_slash {
                Ok(Some(format!("{normalized}/")))
            } else {
                Ok(Some(normalized))
            }
        }
        _ => Ok(None),
    }
}

fn normalize_optional_delimiter(value: Option<&str>) -> io::Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => {
            let trimmed = raw.trim();
            if trimmed.contains('/') {
                Ok(Some(trimmed.to_string()))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ListEntry {
    Object(StorageListItem),
    CommonPrefix(String),
}

impl ListEntry {
    fn token(&self) -> &str {
        match self {
            ListEntry::Object(item) => item.key.as_str(),
            ListEntry::CommonPrefix(prefix) => prefix.as_str(),
        }
    }
}

fn build_list_entries(
    objects: Vec<StorageListItem>,
    prefix: &str,
    delimiter: Option<&str>,
) -> Vec<ListEntry> {
    let mut entries = Vec::new();
    let mut common_prefixes = std::collections::BTreeSet::new();

    for item in objects {
        if !item.key.starts_with(prefix) {
            continue;
        }

        if let Some(delimiter) = delimiter {
            let suffix = &item.key[prefix.len()..];
            if let Some(index) = suffix.find(delimiter) {
                let common_prefix = format!("{}{}", prefix, &suffix[..index + delimiter.len()]);
                common_prefixes.insert(common_prefix);
                continue;
            }
        }

        entries.push(ListEntry::Object(item));
    }

    entries.extend(common_prefixes.into_iter().map(ListEntry::CommonPrefix));
    entries.sort_by(|left, right| left.token().cmp(right.token()));
    entries
}

fn compute_etag(data: &[u8]) -> String {
    let digest = openssl::sha::sha256(data);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn collect_bucket_objects(
    bucket_root: &Path,
    metadata_root: &Path,
) -> io::Result<Vec<StorageListItem>> {
    let mut objects = Vec::new();
    if !bucket_root.exists() {
        return Ok(objects);
    }
    collect_files(bucket_root, bucket_root, metadata_root, &mut objects)?;
    Ok(objects)
}

fn collect_files(
    root: &Path,
    current: &Path,
    metadata_root: &Path,
    objects: &mut Vec<StorageListItem>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_files(root, &path, metadata_root, objects)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let key = relative.to_string_lossy().replace('\\', "/");
            let metadata = read_metadata_sync(metadata_root, relative)?;
            objects.push(StorageListItem {
                key,
                size: metadata.content_length,
                etag: metadata.etag,
                last_modified: metadata.updated_at,
                content_type: metadata.content_type,
            });
        }
    }

    Ok(())
}

fn read_metadata_sync(metadata_root: &Path, relative: &Path) -> io::Result<StorageObjectMetadata> {
    let mut metadata_path = metadata_root.join(relative);
    let file_name = metadata_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage key contains invalid filename",
            )
        })?;
    metadata_path.set_file_name(format!("{file_name}.json"));
    let raw = std::fs::read(metadata_path)?;
    serde_json::from_slice(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        storage
            .put("nested/file.txt", b"hello peanut")
            .await
            .unwrap();
        let content = storage.get("nested/file.txt").await.unwrap();
        assert_eq!(content, b"hello peanut");

        storage.delete("nested/file.txt").await.unwrap();
        let err = storage.get("nested/file.txt").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn test_storage_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        let err = storage.put("../secrets.txt", b"nope").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn test_storage_lists_saved_keys() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        storage.put("notes/one.txt", b"1").await.unwrap();
        storage.put("notes/two.txt", b"2").await.unwrap();

        let keys = storage.list().await.unwrap();
        assert_eq!(keys, vec!["notes/one.txt", "notes/two.txt"]);
    }

    #[tokio::test]
    async fn test_storage_preserves_metadata_for_bucket_objects() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        let metadata = storage
            .put_object(
                "user-1/assets",
                "avatars/me.txt",
                b"hello",
                Some("text/plain"),
            )
            .await
            .unwrap();
        assert_eq!(metadata.content_type, "text/plain");
        assert_eq!(metadata.content_length, 5);

        let stored = storage
            .get_object("user-1/assets", "avatars/me.txt")
            .await
            .unwrap();
        assert_eq!(stored.data, b"hello");
        assert_eq!(stored.metadata.content_type, "text/plain");
        assert_eq!(stored.metadata.etag, metadata.etag);

        let page = storage
            .list_objects_v2("user-1/assets", Some("avatars/"), None, Some(10), None)
            .await
            .unwrap();
        assert_eq!(page.objects.len(), 1);
        assert_eq!(page.objects[0].key, "avatars/me.txt");
    }

    #[tokio::test]
    async fn test_storage_list_objects_v2_supports_delimiter_common_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        storage
            .put_object("user-1/assets", "photos/2026/a.jpg", b"a", Some("image/jpeg"))
            .await
            .unwrap();
        storage
            .put_object("user-1/assets", "photos/2027/b.jpg", b"b", Some("image/jpeg"))
            .await
            .unwrap();
        storage
            .put_object("user-1/assets", "photos/cover.jpg", b"c", Some("image/jpeg"))
            .await
            .unwrap();

        let page = storage
            .list_objects_v2("user-1/assets", Some("photos/"), Some("/"), Some(10), None)
            .await
            .unwrap();

        assert_eq!(page.objects.len(), 1);
        assert_eq!(page.objects[0].key, "photos/cover.jpg");
        assert_eq!(page.common_prefixes, vec!["photos/2026/", "photos/2027/"]);
    }
}

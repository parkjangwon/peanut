use std::{
    io,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const DEFAULT_BUCKET: &str = "default";
const METADATA_ROOT_DIR: &str = ".peanut_meta";

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
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
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
        self.list_objects_v2(DEFAULT_BUCKET, None, None, None)
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
        max_keys: Option<usize>,
        continuation_token: Option<&str>,
    ) -> io::Result<StorageListPage> {
        let prefix = normalize_optional_key(prefix)?;
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

        let filtered: Vec<_> = objects
            .into_iter()
            .filter(|item| {
                prefix
                    .as_ref()
                    .map(|value| item.key.starts_with(value))
                    .unwrap_or(true)
            })
            .filter(|item| {
                continuation_token
                    .as_ref()
                    .map(|value| item.key > *value)
                    .unwrap_or(true)
            })
            .collect();

        let is_truncated = filtered.len() > max_keys;
        let mut page_objects = filtered.into_iter().take(max_keys).collect::<Vec<_>>();
        let next_continuation_token = if is_truncated {
            page_objects.last().map(|item| item.key.clone())
        } else {
            None
        };

        Ok(StorageListPage {
            objects: std::mem::take(&mut page_objects),
            is_truncated,
            next_continuation_token,
        })
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
            .list_objects_v2("user-1/assets", Some("avatars/"), Some(10), None)
            .await
            .unwrap();
        assert_eq!(page.objects.len(), 1);
        assert_eq!(page.objects[0].key, "avatars/me.txt");
    }
}

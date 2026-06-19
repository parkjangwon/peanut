use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

const DEFAULT_BUCKET: &str = "default";
const METADATA_ROOT_DIR: &str = ".peanut_meta";
const MULTIPART_ROOT_DIR: &str = ".peanut_multipart";
const MIN_MULTIPART_PART_SIZE_BYTES: u64 = 5 * 1024 * 1024;

mod listing;
mod multipart;
mod object;

#[derive(Debug)]
pub struct LocalStorage {
    root: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageObjectResponseHeaders {
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub expires: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageObjectMetadata {
    pub content_type: String,
    pub content_length: u64,
    pub etag: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub custom_metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub response_headers: StorageObjectResponseHeaders,
    #[serde(default)]
    pub checksum_sha256: Option<String>,
    #[serde(default)]
    pub checksum_sha1: Option<String>,
    #[serde(default)]
    pub tagging: Option<String>,
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
pub struct MultipartUploadListing {
    pub upload_id: String,
    pub key: String,
    pub initiated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedMultipartPart {
    pub part_number: u32,
    pub etag: String,
}

impl LocalStorage {
    pub(super) fn resolve_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(normalized))
    }

    pub(super) fn metadata_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(METADATA_ROOT_DIR).join(normalized))
    }

    pub(super) fn resolve_object_path(&self, bucket: &str, key: &str) -> io::Result<PathBuf> {
        let bucket_root = self.resolve_bucket_root(bucket)?;
        let relative = normalize_object_key(key)?;
        Ok(bucket_root.join(relative))
    }

    pub(super) fn resolve_metadata_path(&self, bucket: &str, key: &str) -> io::Result<PathBuf> {
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

    pub(super) async fn read_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> io::Result<StorageObjectMetadata> {
        let metadata_path = self.resolve_metadata_path(bucket, key)?;
        let raw = tokio::fs::read(metadata_path).await?;
        serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }

    pub(super) async fn write_metadata(
        &self,
        bucket: &str,
        key: &str,
        metadata: &StorageObjectMetadata,
    ) -> io::Result<()> {
        let metadata_path = self.resolve_metadata_path(bucket, key)?;
        if let Some(parent) = metadata_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let encoded = serde_json::to_vec(metadata)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        tokio::fs::write(metadata_path, encoded).await
    }

    pub(super) fn multipart_bucket_root(&self, bucket: &str) -> io::Result<PathBuf> {
        let normalized = normalize_namespace(bucket, "storage bucket cannot be empty")?;
        Ok(self.root.join(MULTIPART_ROOT_DIR).join(normalized))
    }

    pub(super) fn resolve_upload_id(upload_id: &str) -> io::Result<String> {
        let trimmed = upload_id.trim();
        if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid multipart upload id",
            ));
        }
        Ok(trimmed.to_string())
    }

    pub(super) fn resolve_multipart_upload_root(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> io::Result<PathBuf> {
        Ok(self
            .multipart_bucket_root(bucket)?
            .join(Self::resolve_upload_id(upload_id)?))
    }

    pub(super) fn resolve_multipart_manifest_path(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> io::Result<PathBuf> {
        Ok(self
            .resolve_multipart_upload_root(bucket, upload_id)?
            .join("upload.json"))
    }

    pub(super) fn resolve_multipart_part_path(
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

    pub(super) fn resolve_multipart_part_metadata_path(
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
}

pub(super) fn normalize_namespace(namespace: &str, empty_message: &str) -> io::Result<PathBuf> {
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

pub(super) fn normalize_object_key(key: &str) -> io::Result<PathBuf> {
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

pub(super) fn normalize_optional_key(value: Option<&str>) -> io::Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => Ok(Some(
            normalize_object_key(raw)?
                .to_string_lossy()
                .replace('\\', "/"),
        )),
        _ => Ok(None),
    }
}

pub(super) fn normalize_optional_prefix(value: Option<&str>) -> io::Result<Option<String>> {
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

pub(super) fn normalize_optional_delimiter(value: Option<&str>) -> io::Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => {
            let trimmed = raw.trim();
            Ok(Some(trimmed.to_string()))
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListEntry {
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

pub(super) fn build_list_entries(
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

pub(super) fn compute_sha256_etag(data: &[u8]) -> String {
    let digest = openssl::sha::sha256(data);
    hex_encode(&digest)
}

pub(super) fn compute_multipart_part_etag(data: &[u8]) -> String {
    let digest = openssl::hash::hash(openssl::hash::MessageDigest::md5(), data)
        .expect("md5 hashing should succeed");
    hex_encode(digest.as_ref())
}

pub(super) fn compute_multipart_composite_etag(
    parts: &[MultipartUploadPart],
) -> io::Result<String> {
    let mut concatenated = Vec::with_capacity(parts.len() * 16);
    for part in parts {
        concatenated.extend_from_slice(&decode_hex(&part.etag)?);
    }
    let digest = openssl::hash::hash(openssl::hash::MessageDigest::md5(), &concatenated)
        .expect("md5 hashing should succeed");
    Ok(format!("{}-{}", hex_encode(digest.as_ref()), parts.len()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn decode_hex(value: &str) -> io::Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "multipart etag must be an even-length hex string",
        ));
    }

    let mut decoded = Vec::with_capacity(value.len() / 2);
    let mut chars = value.as_bytes().chunks_exact(2);
    for chunk in &mut chars {
        let pair = std::str::from_utf8(chunk).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "multipart etag must be valid UTF-8",
            )
        })?;
        let byte = u8::from_str_radix(pair, 16).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "multipart etag must be a valid hex string",
            )
        })?;
        decoded.push(byte);
    }
    Ok(decoded)
}

pub(super) fn collect_bucket_objects(
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

pub(super) fn collect_multipart_uploads(
    bucket_root: &Path,
) -> io::Result<Vec<MultipartUploadListing>> {
    let mut uploads = Vec::new();
    if !bucket_root.exists() {
        return Ok(uploads);
    }
    for entry in std::fs::read_dir(bucket_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let upload_root = entry.path();
        let raw = std::fs::read(upload_root.join("upload.json"))?;
        let upload: MultipartUpload = serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        uploads.push(MultipartUploadListing {
            upload_id: upload.upload_id,
            key: upload.key,
            initiated_at: upload.initiated_at,
        });
    }
    Ok(uploads)
}

pub(super) fn collect_multipart_parts(parts_root: &Path) -> io::Result<Vec<MultipartUploadPart>> {
    let mut parts = Vec::new();
    if !parts_root.exists() {
        return Ok(parts);
    }
    for entry in std::fs::read_dir(parts_root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || path.extension().and_then(|v| v.to_str()) != Some("json")
        {
            continue;
        }
        let raw = std::fs::read(path)?;
        let part: MultipartUploadPart = serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        parts.push(part);
    }
    Ok(parts)
}

pub(super) fn collect_stale_multipart_upload_roots(
    multipart_root: &Path,
    stale_before: SystemTime,
) -> io::Result<Vec<PathBuf>> {
    let mut stale = Vec::new();
    if !multipart_root.exists() {
        return Ok(stale);
    }

    let mut stack = vec![multipart_root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let manifest_path = path.join("upload.json");
        if manifest_path.exists() {
            let modified = std::fs::metadata(&manifest_path)?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if modified < stale_before {
                stale.push(path);
            }
            continue;
        }

        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            }
        }
    }

    Ok(stale)
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
            .put_object(
                "user-1/assets",
                "photos/2026/a.jpg",
                b"a",
                Some("image/jpeg"),
            )
            .await
            .unwrap();
        storage
            .put_object(
                "user-1/assets",
                "photos/2027/b.jpg",
                b"b",
                Some("image/jpeg"),
            )
            .await
            .unwrap();
        storage
            .put_object(
                "user-1/assets",
                "photos/cover.jpg",
                b"c",
                Some("image/jpeg"),
            )
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

    #[tokio::test]
    async fn test_cleanup_stale_multipart_uploads_removes_only_old_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        let stale = storage
            .create_multipart_upload("assets", "old.bin", Some("application/octet-stream"))
            .await
            .unwrap();
        let fresh = storage
            .create_multipart_upload("assets", "new.bin", Some("application/octet-stream"))
            .await
            .unwrap();
        storage
            .put_object("assets", "objects/keep.txt", b"keep", Some("text/plain"))
            .await
            .unwrap();

        let stale_root = storage
            .resolve_multipart_upload_root("assets", &stale.upload_id)
            .unwrap();
        let fresh_root = storage
            .resolve_multipart_upload_root("assets", &fresh.upload_id)
            .unwrap();
        let cutoff = std::time::SystemTime::now() + std::time::Duration::from_secs(60);

        let removed = storage
            .cleanup_stale_multipart_uploads(cutoff)
            .await
            .unwrap();

        assert_eq!(removed, 2);
        assert!(!stale_root.exists());
        assert!(!fresh_root.exists());
        let object = storage
            .get_object("assets", "objects/keep.txt")
            .await
            .unwrap();
        assert_eq!(object.data, b"keep");
    }

    #[tokio::test]
    async fn test_cleanup_stale_multipart_uploads_preserves_newer_than_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        let upload = storage
            .create_multipart_upload("assets", "new.bin", Some("application/octet-stream"))
            .await
            .unwrap();
        let upload_root = storage
            .resolve_multipart_upload_root("assets", &upload.upload_id)
            .unwrap();

        let removed = storage
            .cleanup_stale_multipart_uploads(std::time::SystemTime::UNIX_EPOCH)
            .await
            .unwrap();

        assert_eq!(removed, 0);
        assert!(upload_root.exists());
    }
}

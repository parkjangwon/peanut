use super::*;

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
        self.put_object_with_metadata(
            bucket,
            key,
            data,
            content_type,
            BTreeMap::new(),
            StorageObjectResponseHeaders::default(),
            None,
            None,
            None,
        )
        .await
    }

    pub async fn put_object_with_metadata(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
        content_type: Option<&str>,
        custom_metadata: BTreeMap<String, String>,
        response_headers: StorageObjectResponseHeaders,
        checksum_sha256: Option<String>,
        checksum_sha1: Option<String>,
        tagging: Option<String>,
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
            etag: compute_sha256_etag(data),
            created_at: previous_metadata
                .as_ref()
                .map(|value| value.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
            custom_metadata,
            response_headers,
            checksum_sha256,
            checksum_sha1,
            tagging,
        };

        tokio::fs::write(&path, data).await?;
        self.write_metadata(bucket, key, &metadata).await?;
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

    pub async fn set_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tagging: Option<String>,
    ) -> io::Result<StorageObjectMetadata> {
        let path = self.resolve_object_path(bucket, key)?;
        let _ = tokio::fs::metadata(path).await?;
        let mut metadata = self.read_metadata(bucket, key).await?;
        metadata.tagging = tagging;
        self.write_metadata(bucket, key, &metadata).await?;
        Ok(metadata)
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
}

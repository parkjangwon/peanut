use super::*;

impl LocalStorage {
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
            key: normalize_object_key(key)?
                .to_string_lossy()
                .replace('\\', "/"),
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
        let metadata_path =
            self.resolve_multipart_part_metadata_path(bucket, &upload.upload_id, part_number)?;
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let metadata = MultipartUploadPart {
            part_number,
            etag: compute_multipart_part_etag(data),
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
        let mut stored_parts = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
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
            let stored_part = self
                .read_multipart_part(bucket, &upload.upload_id, part.part_number)
                .await?;
            if stored_part.0.etag != part.etag {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("multipart part {} etag mismatch", part.part_number),
                ));
            }
            let is_last = index + 1 == parts.len();
            if !is_last && stored_part.0.size < MIN_MULTIPART_PART_SIZE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "multipart part {} is smaller than the required 5 MiB minimum for non-final parts",
                        part.part_number
                    ),
                ));
            }
            assembled.extend_from_slice(&stored_part.1);
            stored_parts.push(stored_part.0);
        }
        let mut metadata = self
            .put_object(
                bucket,
                &upload.key,
                &assembled,
                Some(upload.content_type.as_str()),
            )
            .await?;
        metadata.etag = compute_multipart_composite_etag(&stored_parts)?;
        self.write_metadata(bucket, &upload.key, &metadata).await?;
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

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> io::Result<Vec<MultipartUploadListing>> {
        let bucket_root = self.multipart_bucket_root(bucket)?;
        let uploads = tokio::task::spawn_blocking(move || collect_multipart_uploads(&bucket_root))
            .await
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))??;
        let normalized_prefix = normalize_optional_prefix(prefix)?.unwrap_or_default();
        let mut uploads = uploads
            .into_iter()
            .filter(|upload| upload.key.starts_with(&normalized_prefix))
            .collect::<Vec<_>>();
        uploads.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(left.upload_id.cmp(&right.upload_id))
        });
        Ok(uploads)
    }

    pub async fn list_multipart_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> io::Result<Vec<MultipartUploadPart>> {
        let upload = self.read_multipart_upload(bucket, key, upload_id).await?;
        let parts_root = self
            .resolve_multipart_upload_root(bucket, &upload.upload_id)?
            .join("parts");
        let mut parts = tokio::task::spawn_blocking(move || collect_multipart_parts(&parts_root))
            .await
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))??;
        parts.sort_by_key(|part| part.part_number);
        Ok(parts)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn cleanup_stale_multipart_uploads(
        &self,
        stale_before: SystemTime,
    ) -> io::Result<usize> {
        let multipart_root = self.root.join(MULTIPART_ROOT_DIR);
        let stale_upload_roots = tokio::task::spawn_blocking(move || {
            collect_stale_multipart_upload_roots(&multipart_root, stale_before)
        })
        .await
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))??;

        let mut removed = 0;
        for upload_root in stale_upload_roots {
            match tokio::fs::remove_dir_all(&upload_root).await {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(removed)
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
        let normalized_key = normalize_object_key(key)?
            .to_string_lossy()
            .replace('\\', "/");
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
        let metadata_path =
            self.resolve_multipart_part_metadata_path(bucket, upload_id, part_number)?;
        let part_path = self.resolve_multipart_part_path(bucket, upload_id, part_number)?;
        let raw = tokio::fs::read(metadata_path).await?;
        let metadata = serde_json::from_slice(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
        let data = tokio::fs::read(part_path).await?;
        Ok((metadata, data))
    }
}

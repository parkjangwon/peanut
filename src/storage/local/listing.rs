use super::*;

impl LocalStorage {
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
}

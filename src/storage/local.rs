use std::{
    io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub async fn put(&self, key: &str, data: &[u8]) -> io::Result<()> {
        let path = self.resolve_path(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, data).await
    }

    pub async fn get(&self, key: &str) -> io::Result<Vec<u8>> {
        let path = self.resolve_path(key)?;
        tokio::fs::read(path).await
    }

    pub async fn delete(&self, key: &str) -> io::Result<()> {
        let path = self.resolve_path(key)?;
        tokio::fs::remove_file(path).await
    }

    pub async fn list(&self) -> io::Result<Vec<String>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut keys = Vec::new();

            if !root.exists() {
                return Ok(keys);
            }

            collect_files(&root, &root, &mut keys)?;
            keys.sort();
            Ok(keys)
        })
        .await
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve_path(&self, key: &str) -> io::Result<PathBuf> {
        let trimmed = key.trim().trim_start_matches('/');
        if trimmed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage key cannot be empty",
            ));
        }

        let relative = Path::new(trimmed);
        if relative.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage key contains invalid path segments",
            ));
        }

        Ok(self.root.join(relative))
    }
}

fn collect_files(root: &Path, current: &Path, keys: &mut Vec<String>) -> io::Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_files(root, &path, keys)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            keys.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());

        storage.put("nested/file.txt", b"hello peanut").await.unwrap();
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
}

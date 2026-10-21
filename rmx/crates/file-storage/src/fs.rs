//! Files in a directory: each key is hashed with SHA-256 and stored as
//! `ab/<hash>.dat` with its metadata in `ab/<hash>.meta.json`, so keys may contain any
//! characters. Use a directory dedicated to one storage.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::future::Future;

use crate::{paginate, File, FileMetadata, FileStorage, ListOptions, ListResult};

/// [`FileStorage`] in a directory (`createFsFileStorage`).
#[derive(Clone, Debug)]
pub struct FsFileStorage {
    root: PathBuf,
}

impl FsFileStorage {
    /// Use `dir`, creating it if needed. Fails if it exists and is not a directory.
    pub fn new(dir: impl AsRef<Path>) -> std::io::Result<FsFileStorage> {
        let root = std::path::absolute(dir.as_ref())?;
        match std::fs::metadata(&root) {
            Ok(meta) if !meta.is_dir() => {
                return Err(std::io::Error::other(format!("Path \"{}\" is not a directory", root.display())));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => std::fs::create_dir_all(&root)?,
            Err(e) => return Err(e),
        }
        Ok(FsFileStorage { root })
    }

    fn paths(&self, key: &str) -> (PathBuf, PathBuf, PathBuf) {
        let hash = rmx_crypto::hex(&rmx_crypto::sha256(key.as_bytes()));
        let dir = self.root.join(&hash[..2]);
        (dir.clone(), dir.join(format!("{hash}.dat")), dir.join(format!("{hash}.meta.json")))
    }
}

async fn read_meta(path: &Path) -> std::io::Result<FileMetadata> {
    let json = tokio::fs::read(path).await?;
    serde_json::from_slice(&json).map_err(std::io::Error::other)
}

fn not_found_as<T>(result: std::io::Result<T>, value: T) -> std::io::Result<T> {
    match result {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(value),
        other => other,
    }
}

impl FileStorage for FsFileStorage {
    fn get(&self, key: &str) -> impl Future<Output = std::io::Result<Option<File>>> + Send {
        let (_, data, meta) = self.paths(key);
        async move {
            let meta = match read_meta(&meta).await {
                Ok(meta) => meta,
                Err(e) => return not_found_as(Err(e), None),
            };
            let bytes = tokio::fs::read(&data).await?;
            Ok(Some(File { name: meta.name, content_type: meta.content_type, bytes: bytes.into(), last_modified: meta.last_modified }))
        }
    }

    fn has(&self, key: &str) -> impl Future<Output = std::io::Result<bool>> + Send {
        let (_, _, meta) = self.paths(key);
        async move { tokio::fs::try_exists(meta).await }
    }

    /// Files come in directory order; the cursor is the last file's hash. 32 per page by
    /// default.
    fn list(&self, options: ListOptions) -> impl Future<Output = std::io::Result<ListResult>> + Send {
        let root = self.root.clone();
        async move {
            let mut entries = Vec::new();
            let mut dirs = tokio::fs::read_dir(&root).await?;
            while let Some(dir) = dirs.next_entry().await? {
                if !dir.file_type().await?.is_dir() {
                    continue;
                }
                let mut files = tokio::fs::read_dir(dir.path()).await?;
                while let Some(file) = files.next_entry().await? {
                    let name = file.file_name().to_string_lossy().into_owned();
                    if let Some(hash) = name.strip_suffix(".meta.json") {
                        entries.push((hash.to_string(), read_meta(&file.path()).await?));
                    }
                }
            }
            Ok(paginate(entries, &options, 32, |hash, _| hash.to_string()))
        }
    }

    fn put(&self, key: &str, file: File) -> impl Future<Output = std::io::Result<File>> + Send {
        let (dir, data, meta_path) = self.paths(key);
        let meta = FileMetadata::of(key, &file);
        async move {
            tokio::fs::create_dir_all(&dir).await?;
            tokio::fs::write(&data, &file.bytes).await?;
            tokio::fs::write(&meta_path, serde_json::to_vec(&meta).map_err(std::io::Error::other)?).await?;
            Ok(file)
        }
    }

    fn remove(&self, key: &str) -> impl Future<Output = std::io::Result<()>> + Send {
        let (dir, data, meta) = self.paths(key);
        async move {
            not_found_as(tokio::fs::remove_file(&data).await, ())?;
            not_found_as(tokio::fs::remove_file(&meta).await, ())?;
            // Drop the hash directory once it is empty.
            if let Ok(mut rest) = tokio::fs::read_dir(&dir).await {
                if rest.next_entry().await?.is_none() {
                    not_found_as(tokio::fs::remove_dir(&dir).await, ())?;
                }
            }
            Ok(())
        }
    }
}

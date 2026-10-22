//! Files in a directory: each key is hashed with SHA-256 and stored as
//! `ab/<hash>.dat` with its metadata in `ab/<hash>.meta.json`, so keys may contain any
//! characters. Use a directory dedicated to one storage.
//!
//! Every operation also has a blocking form (`get_blocking` and so on) for synchronous
//! callers; the async [`FileStorage`] methods run that same code on tokio's blocking pool.
//! Metadata that cannot be parsed is an [`ErrorKind::InvalidData`] error.

use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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

    /// [`FileStorage::get`], blocking.
    pub fn get_blocking(&self, key: &str) -> std::io::Result<Option<File>> {
        let (_, data, meta) = self.paths(key);
        let meta = match read_meta(&meta) {
            Ok(meta) => meta,
            Err(e) => return not_found_as(Err(e), None),
        };
        let bytes = std::fs::read(&data)?;
        Ok(Some(File { name: meta.name, content_type: meta.content_type, bytes: bytes.into(), last_modified: meta.last_modified }))
    }

    /// [`FileStorage::has`], blocking.
    pub fn has_blocking(&self, key: &str) -> std::io::Result<bool> {
        std::fs::exists(self.paths(key).2)
    }

    /// [`FileStorage::list`], blocking. Files come in directory order; the cursor is the
    /// last file's hash. 32 per page by default.
    pub fn list_blocking(&self, options: &ListOptions) -> std::io::Result<ListResult> {
        let mut entries = Vec::new();
        for dir in std::fs::read_dir(&self.root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(dir.path())? {
                let file = file?;
                let name = file.file_name().to_string_lossy().into_owned();
                if let Some(hash) = name.strip_suffix(".meta.json") {
                    entries.push((hash.to_string(), read_meta(&file.path())?));
                }
            }
        }
        Ok(paginate(entries, options, 32, |hash, _| hash.to_string()))
    }

    /// [`FileStorage::put`], blocking.
    pub fn put_blocking(&self, key: &str, file: File) -> std::io::Result<File> {
        let (dir, data, meta_path) = self.paths(key);
        let meta = FileMetadata::of(key, &file);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&data, &file.bytes)?;
        std::fs::write(&meta_path, serde_json::to_vec(&meta).map_err(std::io::Error::other)?)?;
        Ok(file)
    }

    /// [`FileStorage::set`], blocking.
    pub fn set_blocking(&self, key: &str, file: File) -> std::io::Result<()> {
        self.put_blocking(key, file).map(drop)
    }

    /// [`FileStorage::remove`], blocking.
    pub fn remove_blocking(&self, key: &str) -> std::io::Result<()> {
        let (dir, data, meta) = self.paths(key);
        not_found_as(std::fs::remove_file(&data), ())?;
        not_found_as(std::fs::remove_file(&meta), ())?;
        // Drop the hash directory once it is empty.
        if let Ok(mut rest) = std::fs::read_dir(&dir) {
            if rest.next().is_none() {
                not_found_as(std::fs::remove_dir(&dir), ())?;
            }
        }
        Ok(())
    }
}

fn read_meta(path: &Path) -> std::io::Result<FileMetadata> {
    let json = std::fs::read(path)?;
    serde_json::from_slice(&json).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))
}

fn not_found_as<T>(result: std::io::Result<T>, value: T) -> std::io::Result<T> {
    match result {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(value),
        other => other,
    }
}

/// Run a blocking operation on tokio's blocking pool. A panic there resumes here.
async fn blocking<T: Send + 'static>(run: impl FnOnce() -> std::io::Result<T> + Send + 'static) -> std::io::Result<T> {
    match tokio::task::spawn_blocking(run).await {
        Ok(result) => result,
        Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
        Err(e) => Err(std::io::Error::other(e)),
    }
}

impl FileStorage for FsFileStorage {
    fn get(&self, key: &str) -> impl Future<Output = std::io::Result<Option<File>>> + Send {
        let (storage, key) = (self.clone(), key.to_string());
        blocking(move || storage.get_blocking(&key))
    }

    fn has(&self, key: &str) -> impl Future<Output = std::io::Result<bool>> + Send {
        let (storage, key) = (self.clone(), key.to_string());
        blocking(move || storage.has_blocking(&key))
    }

    /// See [`FsFileStorage::list_blocking`].
    fn list(&self, options: ListOptions) -> impl Future<Output = std::io::Result<ListResult>> + Send {
        let storage = self.clone();
        blocking(move || storage.list_blocking(&options))
    }

    fn put(&self, key: &str, file: File) -> impl Future<Output = std::io::Result<File>> + Send {
        let (storage, key) = (self.clone(), key.to_string());
        blocking(move || storage.put_blocking(&key, file))
    }

    fn remove(&self, key: &str) -> impl Future<Output = std::io::Result<()>> + Send {
        let (storage, key) = (self.clone(), key.to_string());
        blocking(move || storage.remove_blocking(&key))
    }
}

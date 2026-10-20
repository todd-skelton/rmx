//! Key/value storage for [`File`]s (Remix's `file-storage` package).
//!
//! Keys are any string; they have no relation to file names on disk. Two backends:
//!
//! - [`MemoryFileStorage`]: a map in memory, for tests and demos.
//! - [`FsFileStorage`] (feature `fs`, on by default): a directory on disk.
//!
//! ```
//! use rmx_fetch::{block_on, File};
//! use rmx_file_storage::{FileStorage, ListOptions, MemoryFileStorage};
//!
//! let storage = MemoryFileStorage::new();
//! block_on(storage.set("user123/avatar.png", File::new("avatar.png", "image/png", vec![1, 2, 3])))?;
//! let file = block_on(storage.get("user123/avatar.png"))?.expect("stored");
//! assert_eq!(file.size(), 3);
//! let listed = block_on(storage.list(ListOptions { prefix: Some("user123/".into()), ..Default::default() }))?;
//! assert_eq!(listed.files[0].key, "user123/avatar.png");
//! # Ok::<(), std::io::Error>(())
//! ```

#[cfg(feature = "fs")]
mod fs;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

#[cfg(feature = "fs")]
pub use fs::FsFileStorage;
pub use rmx_fetch::File;

/// A key/value store for files.
pub trait FileStorage: Send + Sync + 'static {
    /// The file at `key`, if any.
    fn get(&self, key: &str) -> impl Future<Output = std::io::Result<Option<File>>> + Send;

    /// Whether a file is stored at `key`.
    fn has(&self, key: &str) -> impl Future<Output = std::io::Result<bool>> + Send;

    /// A page of stored files.
    fn list(&self, options: ListOptions) -> impl Future<Output = std::io::Result<ListResult>> + Send;

    /// Store `file` at `key` and return the stored copy.
    fn put(&self, key: &str, file: File) -> impl Future<Output = std::io::Result<File>> + Send;

    /// Remove the file at `key`, if any.
    fn remove(&self, key: &str) -> impl Future<Output = std::io::Result<()>> + Send;

    /// Store `file` at `key`, replacing any file already there.
    fn set(&self, key: &str, file: File) -> impl Future<Output = std::io::Result<()>> + Send {
        let put = self.put(key, file);
        async move { put.await.map(drop) }
    }
}

/// Options for [`FileStorage::list`].
#[derive(Clone, Debug, Default)]
pub struct ListOptions {
    /// The `cursor` from a previous result, to continue after it.
    pub cursor: Option<String>,
    /// Fill in [`FileEntry::metadata`].
    pub include_metadata: bool,
    /// The most files to return. `None` means the backend's default.
    pub limit: Option<usize>,
    /// Only keys that start with this.
    pub prefix: Option<String>,
}

/// One listed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub key: String,
    /// Present when [`ListOptions::include_metadata`] was set.
    pub metadata: Option<FileMetadata>,
}

/// What storage knows about a file besides its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "fs", derive(serde::Serialize, serde::Deserialize))]
pub struct FileMetadata {
    pub key: String,
    /// Milliseconds since the Unix epoch.
    #[cfg_attr(feature = "fs", serde(rename = "lastModified"))]
    pub last_modified: u64,
    pub name: String,
    pub size: u64,
    #[cfg_attr(feature = "fs", serde(rename = "type"))]
    pub content_type: String,
}

impl FileMetadata {
    pub fn of(key: &str, file: &File) -> FileMetadata {
        FileMetadata {
            key: key.to_string(),
            last_modified: file.last_modified,
            name: file.name.clone(),
            size: file.size() as u64,
            content_type: file.content_type.clone(),
        }
    }
}

/// A page from [`FileStorage::list`]. Pass `cursor` back to get the next page; `None`
/// means this was the last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListResult {
    pub cursor: Option<String>,
    pub files: Vec<FileEntry>,
}

/// Files kept in memory, in key order. Clones share the same files.
#[derive(Clone, Default)]
pub struct MemoryFileStorage {
    files: Arc<Mutex<BTreeMap<String, File>>>,
}

impl MemoryFileStorage {
    pub fn new() -> MemoryFileStorage {
        MemoryFileStorage::default()
    }

    fn files(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, File>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl FileStorage for MemoryFileStorage {
    fn get(&self, key: &str) -> impl Future<Output = std::io::Result<Option<File>>> + Send {
        std::future::ready(Ok(self.files().get(key).cloned()))
    }

    fn has(&self, key: &str) -> impl Future<Output = std::io::Result<bool>> + Send {
        std::future::ready(Ok(self.files().contains_key(key)))
    }

    fn list(&self, options: ListOptions) -> impl Future<Output = std::io::Result<ListResult>> + Send {
        let files = self.files();
        let entries = files.iter().map(|(key, file)| (key.clone(), FileMetadata::of(key, file)));
        std::future::ready(Ok(paginate(entries, &options, usize::MAX, |key, _| key.to_string())))
    }

    fn put(&self, key: &str, file: File) -> impl Future<Output = std::io::Result<File>> + Send {
        self.files().insert(key.to_string(), file.clone());
        std::future::ready(Ok(file))
    }

    fn remove(&self, key: &str) -> impl Future<Output = std::io::Result<()>> + Send {
        self.files().remove(key);
        std::future::ready(Ok(()))
    }
}

/// One page of `entries` (in storage order) after `options.cursor`, filtered by prefix.
/// `cursor_of` gives the cursor that resumes after an entry.
pub(crate) fn paginate(
    entries: impl IntoIterator<Item = (String, FileMetadata)>,
    options: &ListOptions,
    default_limit: usize,
    cursor_of: impl Fn(&str, &FileMetadata) -> String,
) -> ListResult {
    let limit = options.limit.unwrap_or(default_limit);
    let mut found_cursor = options.cursor.is_none();
    let mut result = ListResult::default();
    let mut last: Option<String> = None;
    for (position, meta) in entries {
        if !found_cursor {
            found_cursor = options.cursor.as_deref() == Some(position.as_str());
            continue;
        }
        if options.prefix.as_deref().is_some_and(|p| !meta.key.starts_with(p)) {
            continue;
        }
        if result.files.len() >= limit {
            result.cursor = last;
            break;
        }
        last = Some(cursor_of(&position, &meta));
        let key = meta.key.clone();
        result.files.push(FileEntry { key, metadata: options.include_metadata.then_some(meta) });
    }
    result
}

//! A persistent [`FileCache`] in a directory (Remix's `createFsFileCache`).
//!
//! Records live in `<directory>/files` through [`FsFileStorage`]. Each record is a 64-hex
//! SHA-256 checksum of the rest, a newline, a JSON header `[digest, name, type,
//! lastModified]`, a newline, then the file bytes, so torn or mixed writes read as misses.
//! A `pending` marker exists while the cache changes storage; finding it on startup means a
//! write was interrupted, and the records are discarded rather than trusted.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use rmx_fetch::File;
use rmx_file_storage::{FileMetadata, FileStorage, FsFileStorage, ListOptions, MemoryFileStorage};

use crate::{AssetError, AssetErrorCode, FileCache};

/// Where [`FsFileCacheOptions::default`] puts the cache, relative to the current directory.
pub const DEFAULT_CACHE_DIRECTORY: &str = "target/.cache/rmx/assets";

/// Directory and limits for an [`FsFileCache`].
#[derive(Clone, Debug)]
pub struct FsFileCacheOptions {
    /// Directory dedicated to this cache, created on first use. A relative path resolves
    /// from the current directory when [`FsFileCache::new`] is called. Defaults to
    /// [`DEFAULT_CACHE_DIRECTORY`].
    pub directory: PathBuf,
    /// Most cached files. Defaults to 1024.
    pub max_entries: usize,
    /// Most bytes per stored entry, including cache metadata. Defaults to 4 MiB.
    pub max_file_size: u64,
    /// Most bytes across stored entries, including cache metadata. Defaults to 256 MiB.
    pub max_total_size: u64,
}

impl Default for FsFileCacheOptions {
    fn default() -> Self {
        FsFileCacheOptions {
            directory: PathBuf::from(DEFAULT_CACHE_DIRECTORY),
            max_entries: 1024,
            max_file_size: 4 * 1024 * 1024,
            max_total_size: 256 * 1024 * 1024,
        }
    }
}

/// A filesystem [`FileCache`] that evicts the least recently used entries.
///
/// Reads and writes refresh an in-memory index; on first use the index is rebuilt from
/// the records' write times, so read recency does not survive a restart. Use one cache
/// per directory; for a cache shared between processes, implement [`FileCache`]. Files
/// over either byte limit are not cached. Filesystem overhead is on top of the budgets.
///
/// ```
/// use std::sync::Arc;
/// use rmx_assets::{FilesOptions, FsFileCache, FsFileCacheOptions};
///
/// let cache = FsFileCache::new(FsFileCacheOptions::default())?;
/// let files = FilesOptions { cache: Some(Arc::new(cache)), ..Default::default() };
/// # let _ = files;
/// # Ok::<(), rmx_assets::AssetError>(())
/// ```
pub struct FsFileCache {
    root: PathBuf,
    data: PathBuf,
    pending: PathBuf,
    max_entries: usize,
    max_file_size: u64,
    max_total_size: u64,
    state: Mutex<Option<State>>,
}

struct Entry {
    size: u64,
    /// Fixed when stored, so a read can tell its entry from a later replacement.
    id: u64,
    /// Position in the recency order.
    tick: u64,
}

struct State {
    storage: FsFileStorage,
    entries: HashMap<String, Entry>,
    /// Least recently used first.
    order: BTreeMap<u64, String>,
    size: u64,
    next: u64,
}

impl State {
    fn insert(&mut self, digest: String, size: u64) {
        let id = self.next;
        self.next += 1;
        self.order.insert(id, digest.clone());
        self.entries.insert(digest, Entry { size, id, tick: id });
        self.size += size;
    }

    /// Mark `digest` most recently used and return its id.
    fn touch(&mut self, digest: &str) -> Option<u64> {
        let entry = self.entries.get_mut(digest)?;
        self.order.remove(&entry.tick);
        entry.tick = self.next;
        self.next += 1;
        self.order.insert(entry.tick, digest.to_string());
        Some(entry.id)
    }

    fn remove(&mut self, digest: &str) -> io::Result<()> {
        self.storage.remove_blocking(digest)?;
        if let Some(entry) = self.entries.remove(digest) {
            self.order.remove(&entry.tick);
            self.size -= entry.size;
        }
        Ok(())
    }

    /// Evict until an entry of `incoming` bytes fits (or, for 0, until within limits).
    fn trim(&mut self, max_entries: usize, max_total_size: u64, incoming: u64) -> io::Result<()> {
        let entry_limit = max_entries - usize::from(incoming > 0);
        while self.entries.len() > entry_limit || self.size > max_total_size - incoming {
            let Some((_, digest)) = self.order.first_key_value() else { break };
            let digest = digest.clone();
            self.remove(&digest)?;
        }
        Ok(())
    }
}

impl FsFileCache {
    /// A cache in `options.directory`. Limits must be positive; nothing is created on disk
    /// until the cache is first used.
    pub fn new(options: FsFileCacheOptions) -> Result<FsFileCache, AssetError> {
        for (name, value) in [
            ("max_entries", options.max_entries as u64),
            ("max_file_size", options.max_file_size),
            ("max_total_size", options.max_total_size),
        ] {
            if value == 0 {
                return Err(AssetError::new(AssetErrorCode::Config, format!("{name} must be a positive integer")));
            }
        }
        let root = std::path::absolute(&options.directory)
            .map_err(|e| AssetError::new(AssetErrorCode::Config, format!("Invalid cache directory: {e}")))?;
        Ok(FsFileCache {
            data: root.join("files"),
            pending: root.join("pending"),
            root,
            max_entries: options.max_entries,
            max_file_size: options.max_file_size,
            max_total_size: options.max_total_size,
            state: Mutex::new(None),
        })
    }

    /// The absolute cache directory.
    pub fn directory(&self) -> &std::path::Path {
        &self.root
    }

    fn lock(&self) -> MutexGuard<'_, Option<State>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state<'a>(&self, slot: &'a mut Option<State>) -> io::Result<&'a mut State> {
        if slot.is_none() {
            *slot = Some(self.initialize()?);
        }
        Ok(slot.as_mut().expect("initialized"))
    }

    fn initialize(&self) -> io::Result<State> {
        std::fs::create_dir_all(&self.root)?;
        // A partial storage write may leave a body without metadata to account for it.
        if std::fs::exists(&self.pending)? {
            remove_dir_all(&self.data)?;
        }
        let storage = FsFileStorage::new(&self.data)?;
        let listed = storage.list_blocking(&ListOptions { include_metadata: true, limit: Some(usize::MAX), ..Default::default() });
        let records: Option<Vec<FileMetadata>> = match listed {
            Ok(list) => list.files.into_iter().map(|file| file.metadata.filter(is_record_metadata)).collect(),
            Err(e) if matches!(e.kind(), ErrorKind::InvalidData | ErrorKind::NotFound) => None,
            Err(e) => return Err(e),
        };
        let mut records = match records {
            Some(records) => records,
            None => {
                std::fs::write(&self.pending, "")?;
                remove_dir_all(&self.data)?;
                std::fs::create_dir_all(&self.data)?;
                Vec::new()
            }
        };
        records.sort_by(|a, b| a.last_modified.cmp(&b.last_modified).then_with(|| a.key.cmp(&b.key)));
        let mut state = State { storage, entries: HashMap::new(), order: BTreeMap::new(), size: 0, next: 0 };
        for record in &records {
            state.insert(record.key.clone(), record.size);
        }
        if records.iter().any(|r| r.size > self.max_file_size) || state.entries.len() > self.max_entries || state.size > self.max_total_size {
            std::fs::write(&self.pending, "")?;
            for record in records.iter().filter(|r| r.size > self.max_file_size) {
                state.remove(&record.key)?;
            }
            state.trim(self.max_entries, self.max_total_size, 0)?;
        }
        match std::fs::remove_file(&self.pending) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        Ok(state)
    }

    /// Change storage under the `pending` marker. On failure the marker stays, and the next
    /// use starts over from an empty cache.
    fn mutate(&self, action: impl FnOnce(&mut State) -> io::Result<()>) -> io::Result<()> {
        let mut guard = self.lock();
        let result = self.mutate_in(&mut guard, action);
        if result.is_err() {
            *guard = None;
        }
        result
    }

    fn mutate_in(&self, slot: &mut Option<State>, action: impl FnOnce(&mut State) -> io::Result<()>) -> io::Result<()> {
        let state = self.state(slot)?;
        std::fs::write(&self.pending, "")?;
        action(state)?;
        std::fs::remove_file(&self.pending)
    }

    /// [`FileCache::get`], running `after_read` once the record is read (for tests that
    /// overlap a read with a write).
    fn get_with(&self, key: &str, after_read: impl FnOnce()) -> io::Result<Option<File>> {
        let digest = hex_digest(key.as_bytes());
        let (storage, id) = {
            let mut guard = self.lock();
            let state = self.state(&mut guard)?;
            let Some(id) = state.touch(&digest) else { return Ok(None) };
            (state.storage.clone(), id)
        };
        let file = read_record(&storage, &digest, self.max_file_size)?;
        after_read();
        if file.is_none() {
            // The read may have overlapped a replacement or eviction: keep a newer entry.
            self.mutate(|state| match state.entries.get(&digest) {
                Some(entry) if entry.id == id => state.remove(&digest),
                _ => Ok(()),
            })?;
        }
        Ok(file)
    }
}

impl FileCache for FsFileCache {
    fn get(&self, key: &str) -> io::Result<Option<File>> {
        self.get_with(key, || {})
    }

    fn put(&self, key: &str, file: File) -> io::Result<()> {
        let limit = self.max_file_size.min(self.max_total_size);
        if file.size() as u64 > limit {
            return Ok(());
        }
        let digest = hex_digest(key.as_bytes());
        let header = serde_json::to_string(&(&digest, &file.name, &file.content_type, file.last_modified)).map_err(io::Error::other)?;
        let mut content = Vec::with_capacity(header.len() + 1 + file.size());
        content.extend_from_slice(header.as_bytes());
        content.push(b'\n');
        content.extend_from_slice(&file.bytes);
        if 65 + content.len() as u64 > limit {
            return Ok(());
        }
        let mut record = Vec::with_capacity(65 + content.len());
        record.extend_from_slice(hex_digest(&content).as_bytes());
        record.push(b'\n');
        record.extend_from_slice(&content);
        let size = record.len() as u64;
        let record = File::new("file-cache", "", record);
        self.mutate(|state| {
            if state.entries.contains_key(&digest) {
                state.remove(&digest)?;
            }
            state.trim(self.max_entries, self.max_total_size, size)?;
            state.storage.set_blocking(&digest, record)?;
            state.insert(digest, size);
            Ok(())
        })
    }
}

/// File storage works as a cache that keeps everything (Remix accepts `FileStorage`
/// backends as `FileCache` implementations).
impl FileCache for MemoryFileStorage {
    fn get(&self, key: &str) -> io::Result<Option<File>> {
        rmx_fetch::block_on(FileStorage::get(self, key))
    }

    fn put(&self, key: &str, file: File) -> io::Result<()> {
        rmx_fetch::block_on(FileStorage::set(self, key, file))
    }
}

impl FileCache for FsFileStorage {
    fn get(&self, key: &str) -> io::Result<Option<File>> {
        self.get_blocking(key)
    }

    fn put(&self, key: &str, file: File) -> io::Result<()> {
        self.set_blocking(key, file)
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    rmx_crypto::hex(&rmx_crypto::sha256(bytes))
}

fn remove_dir_all(path: &std::path::Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn is_record_metadata(meta: &FileMetadata) -> bool {
    meta.key.len() == 64 && meta.key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && meta.size > 0
}

/// The file in record `digest`, or `None` when it is missing, unreadable or does not
/// verify.
fn read_record(storage: &FsFileStorage, digest: &str, max_file_size: u64) -> io::Result<Option<File>> {
    let file = match storage.get_blocking(digest) {
        Ok(Some(file)) => file,
        Ok(None) => return Ok(None),
        // Invalid storage metadata cannot be used to recover a cached record.
        Err(e) if matches!(e.kind(), ErrorKind::InvalidData | ErrorKind::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let bytes = &file.bytes;
    if bytes.len() as u64 > max_file_size || bytes.len() < 65 || bytes[64] != b'\n' {
        return Ok(None);
    }
    if hex_digest(&bytes[65..]).as_bytes() != &bytes[..64] {
        return Ok(None);
    }
    let Some(header_len) = bytes[65..].iter().position(|&b| b == b'\n') else { return Ok(None) };
    let header_end = 65 + header_len;
    let Ok(serde_json::Value::Array(header)) = serde_json::from_slice(&bytes[65..header_end]) else { return Ok(None) };
    let [serde_json::Value::String(head_digest), serde_json::Value::String(name), serde_json::Value::String(content_type), last_modified] = header.as_slice() else {
        return Ok(None);
    };
    let Some(last_modified) = last_modified.as_u64() else { return Ok(None) };
    if head_digest != digest {
        return Ok(None);
    }
    Ok(Some(File { name: name.clone(), content_type: content_type.clone(), bytes: file.bytes.slice(header_end + 1..), last_modified }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rmx-file-cache-unit-{}", rmx_crypto::random_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn does_not_delete_a_replacement_when_an_overlapping_read_finds_a_corrupt_record() {
        let dir = temp_dir();
        let cache = FsFileCache::new(FsFileCacheOptions { directory: dir.clone(), ..Default::default() }).unwrap();
        cache.put("a", File::new("file.txt", "", "original")).unwrap();
        let storage = FsFileStorage::new(dir.join("files")).unwrap();
        let list = storage.list_blocking(&ListOptions::default()).unwrap();
        let key = &list.files[0].key;
        let mut bytes = storage.get_blocking(key).unwrap().unwrap().bytes.to_vec();
        *bytes.last_mut().unwrap() = b'!';
        storage.set_blocking(key, File::new("file-cache", "", bytes)).unwrap();

        let reading = cache.get_with("a", || cache.put("a", File::new("file.txt", "", "replacement")).unwrap());
        assert_eq!(reading.unwrap(), None);
        assert_eq!(&cache.get("a").unwrap().unwrap().bytes[..], b"replacement");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

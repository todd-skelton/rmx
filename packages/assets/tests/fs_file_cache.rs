//! Remix's cache.test.ts (`createFsFileCache`), one test per case. The case that overlaps
//! a read with a write is a unit test in `src/cache.rs`, and the default directory case is
//! in `fs_file_cache_default.rs` because it changes the current directory. Remix runs a
//! second process to show records outlive their writer; here the writing cache is dropped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use rmx_assets::{AssetErrorCode, FileCache, FsFileCache, FsFileCacheOptions};
use rmx_fetch::File;
use rmx_file_storage::{FsFileStorage, ListOptions};

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("rmx-file-cache-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    fn cache(&self, options: FsFileCacheOptions) -> FsFileCache {
        FsFileCache::new(FsFileCacheOptions { directory: self.0.clone(), ..options }).unwrap()
    }

    fn default_cache(&self) -> FsFileCache {
        self.cache(FsFileCacheOptions::default())
    }

    fn storage(&self) -> FsFileStorage {
        FsFileStorage::new(self.0.join("files")).unwrap()
    }

    fn stored(&self) -> usize {
        self.storage().list_blocking(&ListOptions { limit: Some(1000), ..Default::default() }).unwrap().files.len()
    }

    /// The first file under the directory whose name ends with `suffix`.
    fn find(&self, suffix: &str) -> PathBuf {
        walk(&self.0).into_iter().find(|p| p.to_string_lossy().ends_with(suffix)).unwrap_or_else(|| panic!("a {suffix} file"))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        }
        out.push(path);
    }
    out
}

fn limits(max_entries: usize) -> FsFileCacheOptions {
    FsFileCacheOptions { max_entries, ..Default::default() }
}

fn file(body: &str, name: &str) -> File {
    File::new(name, "", body.as_bytes().to_vec())
}

fn dated(body: impl Into<Vec<u8>>, name: &str, content_type: &str, last_modified: u64) -> File {
    File { name: name.into(), content_type: content_type.into(), bytes: body.into().into(), last_modified }
}

fn text(cache: &FsFileCache, key: &str) -> Option<String> {
    cache.get(key).unwrap().map(|f| String::from_utf8(f.bytes.to_vec()).unwrap())
}

fn miss(cache: &FsFileCache, key: &str) -> bool {
    cache.get(key).unwrap().is_none()
}

/// Remix's `assertCachedFile`: same bytes, name, type and lastModified.
fn assert_cached(actual: Option<File>, expected: &File) {
    let actual = actual.expect("a cached file");
    assert_eq!(actual.name, expected.name);
    assert_eq!(actual.content_type, expected.content_type);
    assert_eq!(actual.last_modified, expected.last_modified);
    assert_eq!(actual.bytes, expected.bytes);
}

/// The only stored record: its storage key and its bytes.
fn only_record(storage: &FsFileStorage) -> (String, Vec<u8>) {
    let list = storage.list_blocking(&ListOptions::default()).unwrap();
    let key = list.files.first().expect("a record").key.clone();
    let bytes = storage.get_blocking(&key).unwrap().unwrap().bytes.to_vec();
    (key, bytes)
}

#[test]
fn preserves_file_bytes_and_metadata_across_replacements_and_restarts() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    let original = dated(vec![0u8, 255, 10], "image\n\u{2603}.png", "image/png", 123456);
    cache.put("any key/\u{2603}", original.clone()).unwrap();
    assert_cached(cache.get("any key/\u{2603}").unwrap(), &original);
    let replacement = dated("new", "image.webp", "image/webp", 654321);
    cache.put("any key/\u{2603}", replacement.clone()).unwrap();
    let restarted = dir.default_cache();
    assert_cached(restarted.get("any key/\u{2603}").unwrap(), &replacement);
    assert_eq!(dir.stored(), 1);
}

#[test]
fn preserves_bytes_and_metadata_when_caching_a_file_from_filesystem_storage() {
    let dir = Dir::new();
    let storage = FsFileStorage::new(dir.0.join("source")).unwrap();
    let cache = FsFileCache::new(FsFileCacheOptions { directory: dir.0.join("cache"), ..Default::default() }).unwrap();
    let original = dated(vec![0u8, 255, 10], "image.png", "image/png", 123456);
    storage.set_blocking("source", original.clone()).unwrap();
    let stored = storage.get_blocking("source").unwrap().unwrap();

    cache.put("key", stored).unwrap();

    assert_cached(cache.get("key").unwrap(), &original);
}

#[test]
fn evicts_the_least_recently_used_entry_after_a_cache_hit() {
    let dir = Dir::new();
    let cache = dir.cache(limits(2));
    cache.put("a", file("a", "a.txt")).unwrap();
    cache.put("b", file("b", "b.txt")).unwrap();
    assert_eq!(text(&cache, "a").as_deref(), Some("a"));
    cache.put("c", file("c", "c.txt")).unwrap();
    assert!(miss(&cache, "b"));
    assert_eq!(text(&cache, "a").as_deref(), Some("a"));
    assert_eq!(text(&cache, "c").as_deref(), Some("c"));
}

#[test]
fn rebuilds_recency_from_record_timestamps_instead_of_previous_reads() {
    let dir = Dir::new();
    let cache = dir.cache(limits(2));
    cache.put("a", dated("a", "file.txt", "", 100)).unwrap();
    let storage = dir.storage();
    let (key, bytes) = only_record(&storage);
    storage.set_blocking(&key, dated(bytes, "file-cache", "", 1)).unwrap();
    cache.put("b", dated("b", "file.txt", "", 0)).unwrap();
    cache.get("a").unwrap();

    let restarted = dir.cache(limits(2));
    restarted.put("c", file("c", "file.txt")).unwrap();
    assert!(miss(&restarted, "a"));
    assert_eq!(text(&restarted, "b").as_deref(), Some("b"));
    assert_eq!(text(&restarted, "c").as_deref(), Some("c"));
}

/// Every path under the directory (and the directory itself) with its size and mtime.
fn snapshot(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut paths = walk(dir);
    paths.push(dir.to_path_buf());
    paths.sort();
    paths.into_iter().map(|p| {
        let meta = std::fs::metadata(&p).unwrap();
        (p, meta.len(), meta.modified().unwrap())
    }).collect()
}

#[test]
fn does_not_write_filesystem_metadata_for_cache_hits_or_misses_after_initialization() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("a", file("a", "file.txt")).unwrap();
    cache.put("b", file("b", "file.txt")).unwrap();
    // Remix resets every mtime to the epoch first so coarse clocks still show writes; the
    // mtimes compared here have sub-microsecond resolution instead.
    let before = snapshot(&dir.0);
    assert_eq!(text(&cache, "a").as_deref(), Some("a"));
    assert!(miss(&cache, "missing"));
    assert_eq!(text(&cache, "b").as_deref(), Some("b"));
    assert_eq!(snapshot(&dir.0), before);
}

#[test]
fn bounds_disk_entries_across_restarts_and_fills_every_available_entry() {
    let dir = Dir::new();
    for round in 0..2 {
        let cache = dir.cache(limits(4));
        for index in 0..5 {
            let key = format!("{round}:{index}");
            cache.put(&key, file(&key, &format!("{key}.txt"))).unwrap();
        }
        let mut misses = 0;
        for index in 0..5 {
            let key = format!("{round}:{index}");
            match cache.get(&key).unwrap() {
                None => misses += 1,
                Some(found) => {
                    assert_eq!(&found.bytes[..], key.as_bytes());
                    assert_eq!(found.name, format!("{key}.txt"));
                }
            }
        }
        assert_eq!(misses, 1);
        assert_eq!(dir.stored(), 4);
    }
}

#[test]
fn treats_incomplete_and_mixed_record_bytes_as_misses_independently_of_storage_metadata() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    let original = dated(vec![1u8, 2, 3], "image.png", "image/png", 123456);
    cache.put("key", original.clone()).unwrap();
    let storage = dir.storage();
    let (key, mut bytes) = only_record(&storage);
    storage.set_blocking(&key, File::new("other.webp", "image/webp", bytes.clone())).unwrap();
    assert_cached(cache.get("key").unwrap(), &original);
    *bytes.last_mut().unwrap() = 4;
    storage.set_blocking(&key, File::new("file-cache", "", bytes.clone())).unwrap();
    assert!(miss(&cache, "key"));
    cache.put("key", original).unwrap();
    storage.set_blocking(&key, File::new("file-cache", "", bytes[..80].to_vec())).unwrap();
    assert!(miss(&cache, "key"));
}

#[test]
fn treats_incomplete_storage_metadata_as_a_miss() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("key", file("hello", "file.txt")).unwrap();
    std::fs::write(dir.find(".meta.json"), "{").unwrap();
    assert!(miss(&cache, "key"));
    cache.put("key", file("replacement", "file.txt")).unwrap();
    assert_eq!(text(&cache, "key").as_deref(), Some("replacement"));
}

#[test]
fn bounds_concurrent_writes_within_one_cache_instance() {
    let dir = Dir::new();
    let cache = Arc::new(dir.cache(limits(8)));
    let workers: Vec<_> = (0..32).map(|index| {
        let cache = cache.clone();
        std::thread::spawn(move || {
            let key = format!("key-{index}");
            cache.put(&key, file(&key, "file.txt")).unwrap();
            if let Some(found) = cache.get(&key).unwrap() {
                assert_eq!(&found.bytes[..], key.as_bytes());
            }
            cache.put(&key, file(&key, "file.txt")).unwrap();
        })
    }).collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let stored = dir.stored();
    assert!(stored > 0);
    assert!(stored <= 8);
}

#[test]
fn includes_record_metadata_in_the_stored_size_limit() {
    let dir = Dir::new();
    let cache = dir.cache(FsFileCacheOptions { max_file_size: 512, ..Default::default() });
    let original = dated(Vec::new(), "image.png", "image/png", 123456);
    cache.put("key", original.clone()).unwrap();
    let storage = dir.storage();
    let (key, record) = only_record(&storage);
    let body = vec![0u8; 512 - record.len()];
    let at_limit = File { bytes: body.clone().into(), ..original.clone() };
    cache.put("key", at_limit.clone()).unwrap();
    assert_cached(cache.get("key").unwrap(), &at_limit);
    assert_eq!(storage.get_blocking(&key).unwrap().unwrap().size(), 512);
    storage.remove_blocking(&key).unwrap();
    let mut over = body;
    over.push(b'x');
    cache.put("key", File { bytes: over.into(), ..original }).unwrap();
    assert!(miss(&cache, "key"));
    assert_eq!(dir.stored(), 0);
}

#[test]
fn evicts_by_total_stored_bytes_and_counts_replacement_sizes_correctly() {
    let dir = Dir::new();
    let original = dated("x", "file.txt", "", 0);
    let cache = dir.default_cache();
    cache.put("a", original.clone()).unwrap();
    let storage = dir.storage();
    let listed = storage.list_blocking(&ListOptions { include_metadata: true, ..Default::default() }).unwrap();
    let size = listed.files[0].metadata.as_ref().unwrap().size;
    drop(cache);
    let cache = dir.cache(FsFileCacheOptions { max_total_size: 2 * size, max_entries: 10, ..Default::default() });
    cache.put("b", original.clone()).unwrap();
    cache.get("a").unwrap();
    cache.put("c", original.clone()).unwrap();
    assert!(miss(&cache, "b"));
    assert_cached(cache.get("a").unwrap(), &original);
    assert_cached(cache.get("c").unwrap(), &original);
    cache.put("a", File { bytes: b"xx".to_vec().into(), ..original.clone() }).unwrap();
    assert!(miss(&cache, "c"));
    assert_eq!(text(&cache, "a").as_deref(), Some("xx"));
    let entries = storage.list_blocking(&ListOptions { include_metadata: true, ..Default::default() }).unwrap().files;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].metadata.as_ref().unwrap().size, size + 1);
    cache.put("a", original.clone()).unwrap();
    cache.put("b", original).unwrap();
    assert_eq!(dir.stored(), 2);
}

#[test]
fn refreshes_recency_when_replacing_an_entry() {
    let dir = Dir::new();
    let cache = dir.cache(limits(2));
    cache.put("a", file("a", "file.txt")).unwrap();
    cache.put("b", file("b", "file.txt")).unwrap();
    cache.put("a", file("updated", "file.txt")).unwrap();
    cache.put("c", file("c", "file.txt")).unwrap();
    assert!(miss(&cache, "b"));
    assert_eq!(text(&cache, "a").as_deref(), Some("updated"));
}

#[test]
fn enforces_smaller_limits_when_reopening_a_directory() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    for key in ["a", "b", "c"] {
        cache.put(key, file(key, "file.txt")).unwrap();
    }
    let cache = dir.cache(limits(1));
    let found = ["a", "b", "c"].iter().filter(|key| !miss(&cache, key)).count();
    assert_eq!(found, 1);
    assert_eq!(dir.stored(), 1);
}

#[test]
fn allows_a_configured_file_limit_larger_than_the_default() {
    let dir = Dir::new();
    let cache = dir.cache(FsFileCacheOptions { max_file_size: 8 * 1024 * 1024, ..Default::default() });
    let large = File::new("large.bin", "", vec![0u8; 4 * 1024 * 1024 + 1]);
    cache.put("large", large.clone()).unwrap();
    assert_cached(cache.get("large").unwrap(), &large);
}

#[test]
fn declines_entries_larger_than_the_total_byte_budget() {
    let dir = Dir::new();
    let cache = dir.cache(FsFileCacheOptions { max_file_size: 4096, max_total_size: 256, ..Default::default() });
    cache.put("key", File::new("file.txt", "", vec![0u8; 257])).unwrap();
    assert!(miss(&cache, "key"));
    assert_eq!(dir.stored(), 0);
}

#[test]
fn recovers_an_interrupted_write_without_retaining_unaccounted_records() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("a", file("a", "file.txt")).unwrap();
    let storage = dir.storage();
    std::fs::write(dir.0.join("pending"), "").unwrap();
    storage.set_blocking("uncommitted", file("uncommitted", "file.txt")).unwrap();
    let restarted = dir.default_cache();
    assert!(miss(&restarted, "a"));
    assert_eq!(dir.stored(), 0);
    restarted.put("b", file("b", "file.txt")).unwrap();
    assert_eq!(text(&restarted, "b").as_deref(), Some("b"));
}

#[test]
fn recovers_from_corrupt_storage_metadata_when_rebuilding_the_index() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("a", file("a", "file.txt")).unwrap();
    std::fs::write(dir.find(".meta.json"), "{").unwrap();
    let restarted = dir.default_cache();
    assert!(miss(&restarted, "a"));
    assert_eq!(dir.stored(), 0);
}

#[test]
fn reuses_records_after_the_writing_cache_is_gone() {
    let dir = Dir::new();
    let writer = dir.default_cache();
    writer.put("a", dated("a", "file.txt", "", 123)).unwrap();
    drop(writer);
    let cache = dir.default_cache();
    assert_cached(cache.get("a").unwrap(), &dated("a", "file.txt", "", 123));
}

#[test]
fn recovers_from_a_failed_write_before_admitting_more_entries() {
    let dir = Dir::new();
    let cache = dir.cache(limits(1));
    cache.put("a", file("a", "file.txt")).unwrap();
    let body = dir.find(".dat");
    std::fs::remove_file(&body).unwrap();
    std::fs::create_dir(&body).unwrap();
    assert!(cache.put("a", file("replacement", "file.txt")).is_err());
    cache.put("b", file("b", "file.txt")).unwrap();
    assert!(miss(&cache, "a"));
    assert_eq!(text(&cache, "b").as_deref(), Some("b"));
    assert_eq!(dir.stored(), 1);
}

#[test]
fn discards_invalid_accounting_metadata_on_restart() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("a", file("a", "file.txt")).unwrap();
    std::fs::write(dir.find(".meta.json"), "{}").unwrap();
    let restarted = dir.default_cache();
    assert!(miss(&restarted, "a"));
    restarted.put("b", file("b", "file.txt")).unwrap();
    assert_eq!(dir.stored(), 1);
}

#[test]
fn treats_a_missing_backing_file_as_a_miss_and_allows_it_to_be_replaced() {
    let dir = Dir::new();
    let cache = dir.default_cache();
    cache.put("a", file("a", "file.txt")).unwrap();
    std::fs::remove_file(dir.find(".dat")).unwrap();
    assert!(miss(&cache, "a"));
    cache.put("a", file("replacement", "file.txt")).unwrap();
    assert_eq!(text(&cache, "a").as_deref(), Some("replacement"));
}

#[test]
fn rejects_invalid_limits_before_creating_the_cache_directory() {
    // Rust's unsigned limits rule out Remix's fractional, negative, NaN and unsafe values;
    // zero is the invalid value left.
    let directory = std::env::temp_dir().join(format!("rmx-file-cache-{}", rmx_crypto::random_uuid()));
    let invalid = [
        ("max_entries", FsFileCacheOptions { max_entries: 0, ..Default::default() }),
        ("max_file_size", FsFileCacheOptions { max_file_size: 0, ..Default::default() }),
        ("max_total_size", FsFileCacheOptions { max_total_size: 0, ..Default::default() }),
    ];
    for (name, options) in invalid {
        let error = FsFileCache::new(FsFileCacheOptions { directory: directory.clone(), ..options }).err().expect("invalid");
        assert_eq!(error.code, AssetErrorCode::Config);
        assert!(error.message.contains(name), "{}", error.message);
    }
    assert!(!directory.exists());
}

#[test]
fn creates_nothing_on_disk_until_first_use() {
    let dir = Dir::new();
    let directory = dir.0.join("cache");
    let cache = FsFileCache::new(FsFileCacheOptions { directory: directory.clone(), ..Default::default() }).unwrap();
    assert!(!directory.exists());
    assert!(miss(&cache, "a"));
    assert!(directory.join("files").is_dir());
    assert!(!directory.join("pending").exists());
}

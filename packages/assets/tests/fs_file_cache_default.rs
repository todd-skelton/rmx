//! Remix's "defaults to a cache directory under cwd and accepts limits without a
//! directory". It changes the current directory, so it has a test binary to itself.

use rsmix_assets::{FileCache, FsFileCache, FsFileCacheOptions, DEFAULT_CACHE_DIRECTORY};
use rsmix_fetch::File;
use rsmix_file_storage::{FsFileStorage, ListOptions};

#[test]
fn defaults_to_a_cache_directory_under_cwd_and_accepts_limits_without_a_directory() {
    let directory = std::env::temp_dir().join(format!("rsmix-file-cache-default-{}", rsmix_crypto::random_uuid()));
    std::fs::create_dir_all(&directory).unwrap();
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&directory).unwrap();
    let result = std::panic::catch_unwind(|| {
        let cache = FsFileCache::new(FsFileCacheOptions::default()).unwrap();
        assert_eq!(cache.directory(), directory.join(DEFAULT_CACHE_DIRECTORY));
        cache.put("a", File::new("file.txt", "", "a")).unwrap();
        let limited = FsFileCache::new(FsFileCacheOptions { max_entries: 1, ..Default::default() }).unwrap();
        limited.put("b", File::new("file.txt", "", "b")).unwrap();
        assert!(limited.get("a").unwrap().is_none(), "Entry limit was not applied");
        let restarted = FsFileCache::new(FsFileCacheOptions::default()).unwrap();
        let found = restarted.get("b").unwrap().expect("Default cache directory was not reused");
        assert_eq!(&found.bytes[..], b"b");
        let storage = FsFileStorage::new(directory.join(DEFAULT_CACHE_DIRECTORY).join("files")).unwrap();
        assert_eq!(storage.list_blocking(&ListOptions::default()).unwrap().files.len(), 1);
    });
    std::env::set_current_dir(previous).unwrap();
    let _ = std::fs::remove_dir_all(&directory);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

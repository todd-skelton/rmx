//! Remix's `file-storage/src/lib/backends/fs.test.ts`. Remix's backend returns lazy files
//! that read on demand; rsmix reads the stored bytes into a `File`.

#![cfg(feature = "fs")]

mod common;

use std::path::{Path, PathBuf};

use common::{parse_uploads, text, text_file, upload_request};
use rsmix_fetch::File;
use rsmix_file_storage::{FileEntry, FileStorage, FsFileStorage, ListOptions};

/// Remix's `beforeEach`/`afterEach` temporary directory.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("fs-file-storage-test-{}", rsmix_crypto::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn storage(&self) -> FsFileStorage {
        FsFileStorage::new(&self.0).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

fn dated(body: &str, name: &str, last_modified: u64) -> File {
    File { last_modified, ..text_file(body, name) }
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect()
}

fn find_metadata_path(dir: &Path) -> PathBuf {
    subdirs(dir)
        .into_iter()
        .flat_map(|sub| std::fs::read_dir(sub).unwrap().map(|e| e.unwrap().path()))
        .find(|p| p.to_string_lossy().ends_with(".meta.json"))
        .expect("Expected metadata file")
}

fn keys(files: &[FileEntry]) -> Vec<String> {
    let mut keys: Vec<String> = files.iter().map(|f| f.key.clone()).collect();
    keys.sort();
    keys
}

async fn set_all(storage: &FsFileStorage, keys: &[&str]) {
    for key in keys {
        storage.set(key, text_file(&format!("Hello {key}!"), "hello.txt")).await.unwrap();
    }
}

#[test]
fn stores_and_retrieves_files() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let last_modified = now_ms();
    let file = dated("Hello, world!", "hello.txt", last_modified);
    run(async {
        storage.set("hello", file.clone()).await.unwrap();
        assert!(storage.has("hello").await.unwrap());
        let retrieved = storage.get("hello").await.unwrap().unwrap();
        assert_eq!(retrieved.name, "hello.txt");
        assert_eq!(retrieved.content_type, file.content_type);
        assert_eq!(retrieved.last_modified, last_modified);
        assert_eq!(retrieved.size(), 13);
        assert_eq!(text(&retrieved), "Hello, world!");
        storage.remove("hello").await.unwrap();
        assert!(!storage.has("hello").await.unwrap());
        assert_eq!(storage.get("hello").await.unwrap(), None);
    });
}

#[test]
fn stores_file_size_in_metadata() {
    let tmp = TempDir::new();
    let file = text_file("Hello, world!", "hello.txt");
    run(tmp.storage().set("hello", file.clone())).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(find_metadata_path(&tmp.0)).unwrap()).unwrap();
    assert_eq!(metadata["size"].as_u64(), Some(file.size() as u64));
    // The same field names as Remix's `FileMetadata` JSON.
    let mut fields: Vec<&str> = metadata.as_object().unwrap().keys().map(String::as_str).collect();
    fields.sort();
    assert_eq!(fields, ["key", "lastModified", "name", "size", "type"]);
}

#[test]
fn removes_empty_hash_directories_after_removing_files() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    run(storage.set("test-key", text_file("Test content", "test.txt"))).unwrap();
    assert!(!subdirs(&tmp.0).is_empty());
    run(storage.remove("test-key")).unwrap();
    assert!(subdirs(&tmp.0).is_empty());
}

#[test]
fn lists_files_with_pagination() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let all_keys = ["a", "b", "c", "d", "e"];
    run(async {
        set_all(&storage, &all_keys).await;
        let all = storage.list(ListOptions::default()).await.unwrap();
        assert_eq!(all.cursor, None);
        assert_eq!(keys(&all.files), all_keys);

        let none = storage.list(ListOptions { limit: Some(0), ..Default::default() }).await.unwrap();
        assert_eq!((none.cursor, none.files.len()), (None, 0));

        let first = storage.list(ListOptions { limit: Some(2), ..Default::default() }).await.unwrap();
        assert!(first.cursor.is_some());
        assert_eq!(first.files.len(), 2);

        let rest = storage.list(ListOptions { cursor: first.cursor, ..Default::default() }).await.unwrap();
        assert_eq!(rest.cursor, None);
        assert_eq!(rest.files.len(), 3);

        assert_eq!(keys(&[first.files, rest.files].concat()), all_keys);
    });
}

#[test]
fn lists_files_by_key_prefix() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    run(async {
        set_all(&storage, &["a", "b", "b/c", "c", "d"]).await;
        let page = storage.list(ListOptions { prefix: Some("b".into()), ..Default::default() }).await.unwrap();
        assert_eq!(page.cursor, None);
        assert_eq!(keys(&page.files), ["b", "b/c"]);
    });
}

#[test]
fn lists_files_with_metadata() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let all_keys = ["a", "b", "c", "d", "e"];
    run(async {
        set_all(&storage, &all_keys).await;
        let page = storage.list(ListOptions { include_metadata: true, ..Default::default() }).await.unwrap();
        assert_eq!(page.cursor, None);
        assert_eq!(keys(&page.files), all_keys);
        for entry in &page.files {
            let meta = entry.metadata.as_ref().expect("metadata");
            assert_eq!((meta.name.as_str(), meta.size, meta.content_type.as_str()), ("hello.txt", 8, "text/plain"));
            assert!(meta.last_modified > 0);
        }
    });
}

#[test]
fn handles_race_conditions() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let last_modified = now_ms();
    run(async {
        let (one, two) = tokio::join!(
            storage.set("one", dated("Hello, world!", "hello1.txt", last_modified)),
            storage.set("two", dated("Hello, universe!", "hello2.txt", last_modified)),
        );
        one.unwrap();
        two.unwrap();
        assert_eq!(text(&storage.get("one").await.unwrap().unwrap()), "Hello, world!");
        assert_eq!(text(&storage.get("two").await.unwrap().unwrap()), "Hello, universe!");
    });
}

#[test]
fn throws_if_directory_is_a_file() {
    let tmp = TempDir::new();
    let file_path = tmp.0.join("not-a-directory");
    std::fs::write(&file_path, "I am a file").unwrap();
    let error = FsFileStorage::new(&file_path).unwrap_err();
    assert_eq!(error.to_string(), format!("Path \"{}\" is not a directory", file_path.display()));
}

#[test]
fn puts_files() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let last_modified = now_ms();
    let file = dated("Hello, world!", "hello.txt", last_modified);
    run(async {
        let retrieved = storage.put("hello", file.clone()).await.unwrap();
        assert!(storage.has("hello").await.unwrap());
        assert_eq!(retrieved.name, "hello.txt");
        assert_eq!(retrieved.content_type, file.content_type);
        assert_eq!(retrieved.last_modified, last_modified);
        assert_eq!(retrieved.size(), 13);
        assert_eq!(text(&retrieved), "Hello, world!");
    });
}

#[test]
fn integration_with_form_data_parser_stores_and_lists_file_uploads() {
    let tmp = TempDir::new();
    let storage = tmp.storage();
    let (content_type, body) = upload_request("----WebKitFormBoundary7MA4YWxkTrZu0gW", "hello");
    run(async {
        parse_uploads(&content_type, body, |file| {
            let storage = storage.clone();
            async move {
                storage.set("hello", file).await.unwrap();
            }
        })
        .await;
        assert!(storage.has("hello").await.unwrap());
        let page = storage.list(ListOptions { include_metadata: true, ..Default::default() }).await.unwrap();
        assert_eq!(page.files.len(), 1);
        let meta = page.files[0].metadata.as_ref().unwrap();
        assert_eq!((page.files[0].key.as_str(), meta.name.as_str(), meta.size, meta.content_type.as_str()), ("hello", "hello.txt", 13, "text/plain"));
        assert!(meta.last_modified > 0);
    });
}

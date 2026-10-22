//! Remix's `file-storage/src/lib/backends/memory.test.ts`.

mod common;

use common::{parse_uploads, text, text_file, upload_request};
use rmx_fetch::block_on;
use rmx_file_storage::{FileStorage, ListOptions, MemoryFileStorage};

fn keys(files: &[rmx_file_storage::FileEntry]) -> Vec<String> {
    let mut keys: Vec<String> = files.iter().map(|f| f.key.clone()).collect();
    keys.sort();
    keys
}

fn storage_with(keys: &[&str]) -> MemoryFileStorage {
    let storage = MemoryFileStorage::new();
    for key in keys {
        block_on(storage.set(key, text_file(&format!("Hello {key}!"), "hello.txt"))).unwrap();
    }
    storage
}

#[test]
fn stores_and_retrieves_files() {
    let storage = MemoryFileStorage::new();
    let file = text_file("Hello, world!", "hello.txt");
    block_on(async {
        storage.set("hello", file.clone()).await.unwrap();
        assert!(storage.has("hello").await.unwrap());
        let retrieved = storage.get("hello").await.unwrap().unwrap();
        assert_eq!(retrieved.name, "hello.txt");
        assert_eq!(retrieved.content_type, file.content_type);
        assert_eq!(retrieved.size(), 13);
        assert_eq!(text(&retrieved), "Hello, world!");
        storage.remove("hello").await.unwrap();
        assert!(!storage.has("hello").await.unwrap());
        assert_eq!(storage.get("hello").await.unwrap(), None);
    });
}

#[test]
fn lists_files_with_pagination() {
    let all_keys = ["a", "b", "c", "d", "e"];
    let storage = storage_with(&all_keys);
    block_on(async {
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
    let storage = storage_with(&["a", "b", "b/c", "c", "d"]);
    let page = block_on(storage.list(ListOptions { prefix: Some("b".into()), ..Default::default() })).unwrap();
    assert_eq!(page.cursor, None);
    assert_eq!(page.files.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["b", "b/c"]);
}

#[test]
fn lists_files_with_metadata() {
    let all_keys = ["a", "b", "c", "d", "e"];
    let storage = storage_with(&all_keys);
    let page = block_on(storage.list(ListOptions { include_metadata: true, ..Default::default() })).unwrap();
    assert_eq!(page.cursor, None);
    assert_eq!(keys(&page.files), all_keys);
    for entry in &page.files {
        let meta = entry.metadata.as_ref().expect("metadata");
        assert_eq!((meta.key.as_str(), meta.name.as_str(), meta.content_type.as_str()), (entry.key.as_str(), "hello.txt", "text/plain"));
        assert_eq!(meta.size, 8);
        assert!(meta.last_modified > 0);
    }
}

#[test]
fn integration_with_form_data_parser_stores_and_lists_file_uploads() {
    let storage = MemoryFileStorage::new();
    let (content_type, body) = upload_request("----WebKitFormBoundaryzv5f5B8XUeVl7e0A", "a");
    block_on(async {
        parse_uploads(&content_type, body, |file| {
            let storage = storage.clone();
            async move {
                storage.put("hello", file).await.unwrap();
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

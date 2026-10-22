use rmx_fetch::block_on;

use super::*;

fn file(name: &str, body: &str) -> File {
    File { name: name.into(), content_type: "text/plain".into(), bytes: body.as_bytes().to_vec().into(), last_modified: 1_737_955_705_270 }
}

fn exercise(storage: &impl FileStorage) {
    block_on(async {
        assert_eq!(storage.get("a").await.unwrap(), None);
        assert!(!storage.has("a").await.unwrap());
        let stored = storage.put("user123/hello.txt", file("hello.txt", "Hello, world!")).await.unwrap();
        assert_eq!(stored, file("hello.txt", "Hello, world!"));
        assert!(storage.has("user123/hello.txt").await.unwrap());
        assert_eq!(storage.get("user123/hello.txt").await.unwrap(), Some(file("hello.txt", "Hello, world!")));

        // Overwrite, then keys with characters that are not valid file names.
        storage.set("user123/hello.txt", file("hello.txt", "Bye")).await.unwrap();
        assert_eq!(storage.get("user123/hello.txt").await.unwrap().unwrap().bytes.as_ref(), b"Bye");
        storage.set("weird:key?*<>|", file("w.txt", "w")).await.unwrap();
        for i in 0..3 {
            storage.set(&format!("user456/{i}.txt"), file(&format!("{i}.txt"), "x")).await.unwrap();
        }

        let all = storage.list(ListOptions::default()).await.unwrap();
        assert_eq!(all.files.len(), 5);
        assert!(all.files.iter().all(|f| f.metadata.is_none()));

        let user = storage.list(ListOptions { prefix: Some("user123/".into()), include_metadata: true, ..Default::default() }).await.unwrap();
        assert_eq!(user.files.len(), 1);
        let meta = user.files[0].metadata.clone().unwrap();
        assert_eq!((meta.key.as_str(), meta.name.as_str(), meta.size, meta.content_type.as_str(), meta.last_modified), ("user123/hello.txt", "hello.txt", 3, "text/plain", 1_737_955_705_270));

        // Pages of two cover everything exactly once.
        let mut seen = Vec::new();
        let mut cursor = None;
        loop {
            let page = storage.list(ListOptions { cursor, limit: Some(2), ..Default::default() }).await.unwrap();
            assert!(page.files.len() <= 2);
            seen.extend(page.files.into_iter().map(|f| f.key));
            match page.cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        seen.sort();
        assert_eq!(seen, ["user123/hello.txt", "user456/0.txt", "user456/1.txt", "user456/2.txt", "weird:key?*<>|"]);

        storage.remove("user123/hello.txt").await.unwrap();
        storage.remove("missing").await.unwrap();
        assert!(!storage.has("user123/hello.txt").await.unwrap());
        assert_eq!(storage.list(ListOptions::default()).await.unwrap().files.len(), 4);
    });
}

#[test]
fn memory_storage() {
    exercise(&MemoryFileStorage::new());
}

#[cfg(feature = "fs")]
#[test]
fn fs_storage() {
    let dir = std::env::temp_dir().join(format!("rmx-file-storage-{}", std::process::id()));
    let storage = FsFileStorage::new(&dir).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async { tokio::task::spawn_blocking(move || exercise(&storage)).await.unwrap() });
    // A second storage over the same directory sees the same files.
    let again = FsFileStorage::new(&dir).unwrap();
    assert!(rt.block_on(again.has("user456/0.txt")).unwrap());
    assert!(FsFileStorage::new(dir.join("aa").join("missing")).is_ok());
}

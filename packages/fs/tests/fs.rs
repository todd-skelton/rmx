//! Port of Remix's `fs.test.ts`. Remix's `writeFile` takes a path, a file descriptor or a
//! `FileHandle`; the open-file forms are `write_to` with an owned or borrowed `std::fs::File`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rsmix_fetch::{block_on, Body};
use rsmix_fs::{open_lazy_file, write_file, write_to, OpenLazyFileOptions, Streamable};

/// A fresh directory under the system temp directory, removed when dropped (`mkdtempSync` in
/// `beforeEach`, `rmSync` in `afterEach`).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("rsmix-fs-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// `createTestFile(filename, content)`.
    fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn open(path: &Path) -> rsmix_lazy_file::LazyFile {
    open_lazy_file(path, OpenLazyFileOptions::default()).unwrap()
}

fn mtime_ms(path: &Path) -> u64 {
    fs::metadata(path).unwrap().modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

// openLazyFile -----------------------------------------------------------------------------

#[test]
fn opens_a_file_and_reads_content() {
    let tmp = TempDir::new();
    let path = tmp.file("test.txt", "hello world");
    let file = open(&path);

    assert_eq!(file.name(), path.to_string_lossy());
    assert_eq!(file.size(), 11);
    assert_eq!(block_on(file.text()).unwrap(), "hello world");
}

#[test]
fn sets_mime_type_based_on_file_extension() {
    let tmp = TempDir::new();

    assert_eq!(open(&tmp.file("test.html", "<html></html>")).content_type(), "text/html");
    assert_eq!(open(&tmp.file("test.json", "{}")).content_type(), "application/json");
    assert_eq!(open(&tmp.file("test.txt", "text")).content_type(), "text/plain");
}

#[test]
fn sets_last_modified_from_file_stats() {
    let tmp = TempDir::new();
    let path = tmp.file("test.txt", "content");

    assert_eq!(open(&path).last_modified(), mtime_ms(&path));
}

#[test]
fn overrides_file_name_with_options_name() {
    let tmp = TempDir::new();
    let path = tmp.file("test.txt", "content");
    let file = open_lazy_file(&path, OpenLazyFileOptions { name: Some("custom.txt".into()), ..Default::default() }).unwrap();

    assert_eq!(file.name(), "custom.txt");
}

#[test]
fn overrides_mime_type_with_options_type() {
    let tmp = TempDir::new();
    let path = tmp.file("test.txt", "content");
    let file = open_lazy_file(&path, OpenLazyFileOptions { content_type: Some("application/custom".into()), ..Default::default() }).unwrap();

    assert_eq!(file.content_type(), "application/custom");
}

#[test]
fn overrides_last_modified_with_options_last_modified() {
    let tmp = TempDir::new();
    let path = tmp.file("test.txt", "content");
    let custom = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 - 1_000_000;
    let file = open_lazy_file(&path, OpenLazyFileOptions { last_modified: Some(custom), ..Default::default() }).unwrap();

    assert_eq!(file.last_modified(), custom);
}

#[test]
fn reads_file_as_array_buffer() {
    let tmp = TempDir::new();
    let bytes = block_on(open(&tmp.file("test.txt", "hello")).bytes()).unwrap();

    assert_eq!(bytes.len(), 5);
    assert_eq!(&bytes[..], b"hello");
}

#[test]
fn streams_file_content() {
    let tmp = TempDir::new();
    let mut body = open(&tmp.file("test.txt", "streaming content")).stream();
    let mut combined = Vec::new();
    block_on(async {
        while let Some(chunk) = body.chunk().await {
            combined.extend_from_slice(&chunk.unwrap());
        }
    });

    assert_eq!(combined, b"streaming content");
}

#[test]
fn handles_empty_files() {
    let tmp = TempDir::new();
    let file = open(&tmp.file("empty.txt", ""));

    assert_eq!(file.size(), 0);
    assert_eq!(block_on(file.text()).unwrap(), "");
}

#[test]
fn handles_large_files() {
    let tmp = TempDir::new();
    let content = "x".repeat(10_000);
    let file = open(&tmp.file("large.txt", &content));

    assert_eq!(file.size(), 10_000);
    assert_eq!(block_on(file.text()).unwrap(), content);
}

#[test]
fn throws_error_for_non_existent_files() {
    let tmp = TempDir::new();
    let error = open_lazy_file(tmp.path("nonexistent.txt"), OpenLazyFileOptions::default()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn throws_error_when_opening_a_directory() {
    let tmp = TempDir::new();
    let dir = tmp.path("testdir");
    fs::create_dir(&dir).unwrap();
    let error = open_lazy_file(&dir, OpenLazyFileOptions::default()).unwrap_err();

    assert!(error.to_string().contains("is not a file"), "{error}");
}

/// Not in Remix's tests: a slice reads only its range from disk.
#[test]
fn streams_only_the_sliced_range_of_a_file() {
    let tmp = TempDir::new();
    let file = open(&tmp.file("digits.txt", "0123456789"));

    assert_eq!(block_on(file.slice(2, 8).slice(1, 4).text()).unwrap(), "345");
    assert_eq!(block_on(file.slice(-3, None).text()).unwrap(), "789");
}

// writeFile --------------------------------------------------------------------------------

#[test]
fn writes_file_to_a_path() {
    let tmp = TempDir::new();
    let source = tmp.file("source.txt", "test content");
    let dest = tmp.path("dest.txt");

    block_on(write_file(&dest, &open(&source))).unwrap();

    assert_eq!(fs::read_to_string(&dest).unwrap(), "test content");
}

#[test]
fn writes_file_using_a_file_descriptor() {
    let tmp = TempDir::new();
    let source = tmp.file("source.txt", "test content");
    let dest = tmp.path("dest.txt");
    let fd = fs::File::create(&dest).unwrap();

    block_on(write_to(fd, &open(&source))).unwrap();

    assert_eq!(fs::read_to_string(&dest).unwrap(), "test content");
}

#[test]
fn writes_file_using_a_file_handle() {
    let tmp = TempDir::new();
    let source = tmp.file("source.txt", "test content");
    let dest = tmp.path("dest.txt");
    let handle = fs::File::create(&dest).unwrap();

    block_on(write_to(&handle, &open(&source))).unwrap();
    drop(handle);

    assert_eq!(fs::read_to_string(&dest).unwrap(), "test content");
}

#[test]
fn writes_empty_files() {
    let tmp = TempDir::new();
    let source = tmp.file("empty.txt", "");
    let dest = tmp.path("dest.txt");

    block_on(write_file(&dest, &open(&source))).unwrap();

    assert_eq!(fs::read_to_string(&dest).unwrap(), "");
}

#[test]
fn writes_large_files() {
    let tmp = TempDir::new();
    let content = "x".repeat(100_000);
    let source = tmp.file("large.txt", &content);
    let dest = tmp.path("dest.txt");

    block_on(write_file(&dest, &open(&source))).unwrap();

    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

#[test]
fn creates_parent_directories_when_writing_to_path() {
    let tmp = TempDir::new();
    let source = tmp.file("source.txt", "content");
    let dest = tmp.path("nested/dir/dest.txt");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();

    block_on(write_file(&dest, &open(&source))).unwrap();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "content");
}

#[test]
fn preserves_file_content_exactly() {
    let tmp = TempDir::new();
    let binary = [0x00, 0x01, 0x02, 0xff, 0xfe, 0xfd];
    let source = tmp.path("binary.dat");
    fs::write(&source, binary).unwrap();
    let dest = tmp.path("dest.dat");

    block_on(write_file(&dest, &open(&source))).unwrap();

    assert_eq!(fs::read(&dest).unwrap(), binary);
}

/// An object whose stream fails at once.
struct FailingSource;

impl Streamable for FailingSource {
    fn stream(&self) -> Body {
        Body::from_task(|tx| async move { tx.abort("stream failed") })
    }
}

#[test]
fn rejects_when_the_source_stream_fails() {
    let tmp = TempDir::new();
    let error = block_on(write_file(tmp.path("dest.txt"), &FailingSource)).unwrap_err();

    assert!(error.to_string().contains("stream failed"), "{error}");
}

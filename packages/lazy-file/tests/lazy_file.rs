//! Port of Remix's `lazy-file.test.ts`.
//!
//! Remix checks that `LazyBlob` and `LazyFile` are not `Blob` or `File` instances, have their
//! own `Symbol.toStringTag`, and throw from `toString()`. Here they are distinct types from
//! the in-memory `File`, their `Debug` output names them, and they do not implement
//! `Display`, so they cannot be turned into a string; the `toString()` cases are compile-time
//! assertions that fail the build if either type gains `Display`.

use std::any::TypeId;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rsmix_fetch::{block_on, Body, File};
use rsmix_lazy_file::{BlobPart, LazyBlob, LazyBlobOptions, LazyContent, LazyFile, LazyFileOptions};

/// `createLazyContent(value)`, recording each `stream(start, end)` call (the Remix tests spy on
/// it with `t.mock.method`).
#[derive(Clone)]
struct TestContent {
    buffer: Vec<u8>,
    calls: Arc<Mutex<Vec<(u64, u64)>>>,
}

fn content(value: &str) -> TestContent {
    TestContent { buffer: value.as_bytes().to_vec(), calls: Arc::default() }
}

impl LazyContent for TestContent {
    fn byte_length(&self) -> u64 {
        self.buffer.len() as u64
    }

    fn stream(&self, start: u64, end: u64) -> Body {
        self.calls.lock().unwrap().push((start, end));
        Body::from(self.buffer[start as usize..end as usize].to_vec())
    }
}

fn text_blob(content: TestContent) -> LazyBlob {
    LazyBlob::from_content(content, LazyBlobOptions { content_type: "text/plain".into(), ..Default::default() })
}

fn text_file(content: TestContent, name: &str) -> LazyFile {
    LazyFile::from_content(content, name, LazyFileOptions { content_type: "text/plain".into(), ..Default::default() })
}

fn text_parts(parts: impl IntoIterator<Item = BlobPart>, name: &str) -> LazyFile {
    LazyFile::new(parts, name, LazyFileOptions { content_type: "text/plain".into(), ..Default::default() })
}

/// A native `Blob` of `parts`: an in-memory file with no name.
fn native_blob(parts: &[&str]) -> BlobPart {
    File::new("", "text/plain", parts.concat()).into()
}

/// Reads every chunk of `body` into a string, decoding as it goes.
fn collect(body: Body) -> String {
    block_on(async move {
        let mut body = body;
        let mut bytes = Vec::new();
        while let Some(chunk) = body.chunk().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        String::from_utf8(bytes).unwrap()
    })
}

/// True when `T` implements `Display` (an inherent constant shadows the trait's default only
/// when the bound holds).
struct Probe<T>(PhantomData<T>);

trait NotDisplay {
    const DISPLAY: bool = false;
}

impl<T> NotDisplay for Probe<T> {}

impl<T: Display> Probe<T> {
    #[allow(dead_code)]
    const DISPLAY: bool = true;
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

// LazyBlob ---------------------------------------------------------------------------------

#[test]
fn blob_has_the_correct_size_and_type() {
    let blob = text_blob(content(&"X".repeat(100)));

    assert_eq!(blob.size(), 100);
    assert_eq!(blob.content_type(), "text/plain");
}

#[test]
fn blob_is_not_an_instance_of_blob() {
    assert_ne!(TypeId::of::<LazyBlob>(), TypeId::of::<File>());
}

#[test]
fn blob_has_the_correct_to_string_tag() {
    let blob = text_blob(content("hello"));

    assert!(format!("{blob:?}").starts_with("LazyBlob "), "{blob:?}");
}

#[test]
fn blob_returns_the_contents_as_a_stream() {
    let blob = text_blob(content("hello world"));

    assert_eq!(collect(blob.stream()), "hello world");
}

#[test]
fn blob_returns_the_contents_as_a_string() {
    let blob = text_blob(content("hello world"));

    assert_eq!(block_on(blob.text()).unwrap(), "hello world");
}

#[test]
fn blob_returns_the_contents_as_bytes() {
    let blob = text_blob(content("hello"));
    let bytes = block_on(blob.bytes()).unwrap();

    assert_eq!(bytes.len(), 5);
    assert_eq!(&bytes[..], b"hello");
}

#[test]
fn blob_returns_the_contents_as_an_array_buffer() {
    let blob = text_blob(content("hello"));

    assert_eq!(block_on(blob.bytes()).unwrap().len(), 5);
}

#[test]
fn blob_to_blob_converts_to_a_native_blob() {
    let blob = block_on(text_blob(content("hello world")).to_blob()).unwrap();

    assert_eq!(blob.size(), 11);
    assert_eq!(blob.content_type, "text/plain");
    assert_eq!(&blob.bytes[..], b"hello world");
}

#[test]
fn blob_slice_returns_a_lazy_blob_with_the_correct_size() {
    let slice: LazyBlob = text_blob(content("hello world")).slice(0, 5);

    assert_eq!(slice.size(), 5);
}

#[test]
fn blob_slice_slices_relative_to_an_existing_slice() {
    let slice = text_blob(content("0123456789")).slice(2, 8).slice(1, 4);

    assert_eq!(slice.size(), 3);
    assert_eq!(block_on(slice.text()).unwrap(), "345");
}

// toString() throws a TypeError to prevent misuse with Response.
const _: () = assert!(!Probe::<LazyBlob>::DISPLAY, "LazyBlob must not implement Display");

// LazyFile ---------------------------------------------------------------------------------

#[test]
fn file_has_the_correct_name_size_type_and_last_modified_timestamp() {
    let now = now_ms();
    let file = LazyFile::from_content(content(&"X".repeat(100)), "example.txt", LazyFileOptions { content_type: "text/plain".into(), last_modified: Some(now), ..Default::default() });

    assert_eq!(file.name(), "example.txt");
    assert_eq!(file.size(), 100);
    assert_eq!(file.content_type(), "text/plain");
    assert_eq!(file.last_modified(), now);
}

#[test]
fn file_is_not_an_instance_of_file() {
    assert_ne!(TypeId::of::<LazyFile>(), TypeId::of::<File>());
}

#[test]
fn file_has_the_correct_to_string_tag() {
    let file = text_file(content("hello"), "hello.txt");

    assert!(format!("{file:?}").starts_with("LazyFile "), "{file:?}");
}

#[test]
fn file_can_be_initialized_with_a_blob_as_the_content() {
    let file = text_parts([native_blob(&["hello world"])], "hello.txt");

    assert_eq!(file.size(), 11);
    assert_eq!(block_on(file.text()).unwrap(), "hello world");
}

#[test]
fn file_can_be_initialized_with_another_lazy_file_as_the_content() {
    let file = text_parts([text_parts(["hello world".into()], "hello.txt").into()], "hello.txt");

    assert_eq!(file.size(), 11);
    assert_eq!(block_on(file.text()).unwrap(), "hello world");
}

#[test]
fn file_can_be_initialized_with_multiple_blobs_and_strings_and_slice_them_correctly() {
    let parts = [native_blob(&["  hello "]), "world".into(), native_blob(&["!", "  "]), "extra stuff".into()];
    let file = text_parts(parts, "hello.txt");

    assert_eq!(file.size(), 27);
    assert_eq!(block_on(file.slice(2, -13).text()).unwrap(), "hello world!");
}

#[test]
fn file_returns_the_contents_as_a_stream() {
    let file = text_file(content("hello world"), "hello.txt");

    assert_eq!(collect(file.stream()), "hello world");
}

#[test]
fn file_returns_the_contents_as_a_string() {
    let file = text_file(content("hello world"), "hello.txt");

    assert_eq!(block_on(file.text()).unwrap(), "hello world");
}

#[test]
fn file_to_file_converts_to_a_native_file() {
    let now = now_ms();
    let lazy = LazyFile::from_content(content("hello world"), "hello.txt", LazyFileOptions { content_type: "text/plain".into(), last_modified: Some(now), ..Default::default() });

    let file = block_on(lazy.to_file()).unwrap();

    assert_eq!(file.name, "hello.txt");
    assert_eq!(file.size(), 11);
    assert_eq!(file.content_type, "text/plain");
    assert_eq!(file.last_modified, now);
    assert_eq!(&file.bytes[..], b"hello world");
}

#[test]
fn file_to_blob_converts_to_a_native_blob() {
    let blob = block_on(text_file(content("hello world"), "hello.txt").to_blob()).unwrap();

    assert_eq!(blob.size(), 11);
    assert_eq!(blob.content_type, "text/plain");
    assert_eq!(&blob.bytes[..], b"hello world");
}

#[test]
fn file_slice_from_0_to_the_end_has_the_original_size() {
    let file = text_file(content("hello world"), "hello.txt");
    let slice: LazyBlob = file.slice(0, None);

    assert_eq!(slice.size(), file.size());
}

#[test]
fn file_slice_is_empty_when_start_is_past_the_content_length() {
    let file = text_parts(["hello world".into()], "hello.txt");

    assert_eq!(file.slice(100, None).size(), 0);
}

#[test]
fn file_slice_is_empty_when_start_is_greater_than_end() {
    let file = text_parts(["hello world".into()], "hello.txt");

    assert_eq!(file.slice(5, 0).size(), 0);
}

/// Slice a 100-byte file with `slice`, stream it, and return the ranges `content.stream()` saw.
fn streamed_ranges(slice: impl Fn(&LazyFile) -> LazyBlob) -> Vec<(u64, u64)> {
    let content = content(&"X".repeat(100));
    let calls = content.calls.clone();
    let file = text_file(content, "example.txt");
    let _stream = slice(&file).stream();
    let calls = calls.lock().unwrap().clone();
    calls
}

#[test]
fn file_slice_streams_the_correct_range() {
    assert_eq!(streamed_ranges(|f| f.slice(10, 20)), [(10, 20)]);
}

#[test]
fn file_slice_streams_the_correct_range_with_a_negative_start() {
    assert_eq!(streamed_ranges(|f| f.slice(-10, None)), [(90, 100)]);
}

#[test]
fn file_slice_streams_the_correct_range_with_a_negative_end() {
    assert_eq!(streamed_ranges(|f| f.slice(0, -10)), [(0, 90)]);
}

#[test]
fn file_slice_streams_the_correct_range_with_negative_start_and_end() {
    assert_eq!(streamed_ranges(|f| f.slice(-20, -10)), [(80, 90)]);
}

#[test]
fn file_slice_streams_the_correct_range_when_start_is_greater_than_end() {
    assert_eq!(streamed_ranges(|f| f.slice(20, 10)), [(20, 20)]);
}

#[test]
fn file_slice_streams_the_correct_range_when_slicing_an_existing_slice() {
    assert_eq!(streamed_ranges(|f| f.slice(20, 80).slice(-20, -10)), [(60, 70)]);
}

// toString() throws a TypeError to prevent misuse with Response.
const _: () = assert!(!Probe::<LazyFile>::DISPLAY, "LazyFile must not implement Display");

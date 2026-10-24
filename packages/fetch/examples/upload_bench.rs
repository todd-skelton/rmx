//! Stable vs streaming (`form_data::future`) multipart uploads, after the table in Remix
//! 3.1's `form-data-parser` changelog: total time, time until the upload handler is called,
//! and peak heap growth, as the median of 7 runs. The request streams in 64 KiB chunks that
//! share one buffer, so heap growth is what the parser and handler allocate. The handler
//! "stores" each file by reading it and dropping the bytes.
//!
//! `cargo run --release -p rmx-fetch --example upload_bench`

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use rmx_fetch::form_data::future;
use rmx_fetch::{block_on, Body, FileUpload, FormData, FormLimits, FormValue};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const MIB: usize = 1024 * 1024;
const CHUNK: usize = 64 * 1024;
const BOUNDARY: &str = "----rmxUploadBench";

/// A request body that yields 64 KiB views of one message.
struct Chunked(VecDeque<Bytes>);

impl http_body::Body for Chunked {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.0.pop_front().map(|chunk| Ok(http_body::Frame::data(chunk))))
    }
}

fn message(files: &[usize], fields: usize) -> Bytes {
    let mut out = Vec::new();
    for i in 0..fields {
        out.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"field{i}\"\r\n\r\nvalue {i}\r\n").as_bytes());
    }
    for (i, size) in files.iter().enumerate() {
        out.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file{i}\"; filename=\"f{i}.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        out.extend((0..*size).map(|j| (j % 251) as u8));
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out.into()
}

fn body(message: &Bytes) -> Body {
    Body::wrap(Chunked((0..message.len()).step_by(CHUNK).map(|i| message.slice(i..(i + CHUNK).min(message.len()))).collect()))
}

struct Run {
    total: Duration,
    first_handler: Duration,
    peak: usize,
}

fn limits() -> FormLimits {
    FormLimits { max_file_size: 128 * MIB, ..FormLimits::default() }
}

fn stable(message: &Bytes) -> Run {
    let body = body(message);
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let start = Instant::now();
    let mut first = None;
    let ct = format!("multipart/form-data; boundary={BOUNDARY}");
    let form = block_on(FormData::parse_with(Some(&ct), body, limits(), |upload: FileUpload| {
        first.get_or_insert_with(|| start.elapsed());
        let stored = upload.file.bytes.len();
        drop(upload);
        std::future::ready(Some(FormValue::Text(stored.to_string())))
    }))
    .unwrap();
    let total = start.elapsed();
    drop(form);
    Run { total, first_handler: first.unwrap_or(total), peak: PEAK.load(Ordering::Relaxed).saturating_sub(base) }
}

fn streaming(message: &Bytes) -> Run {
    let body = body(message);
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let start = Instant::now();
    let mut first = None;
    let ct = format!("multipart/form-data; boundary={BOUNDARY}");
    let form = block_on(future::parse_with(Some(&ct), body, limits(), |mut upload: future::FileUpload| {
        first.get_or_insert_with(|| start.elapsed());
        async move {
            let mut stored = 0;
            while let Some(chunk) = upload.chunk().await {
                stored += chunk?.len();
            }
            Ok::<_, rmx_fetch::FormDataError>(stored.to_string())
        }
    }))
    .unwrap();
    let total = start.elapsed();
    drop(form);
    Run { total, first_handler: first.unwrap_or(total), peak: PEAK.load(Ordering::Relaxed).saturating_sub(base) }
}

fn median<T: Copy + PartialOrd>(mut values: Vec<T>) -> T {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn mib(bytes: usize) -> String {
    format!("{:.1} MiB", bytes as f64 / MIB as f64)
}

fn main() {
    let runs: usize = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(7);
    let cases = [("1 x 100 MiB file", message(&[100 * MIB], 0)), ("5 x 20 MiB files", message(&[20 * MIB; 5], 0)), ("10 fields and an 8 KiB file", message(&[8 * 1024], 10))];
    println!("| Request | Total time (stable -> future) | Time until the upload handler is called | Peak heap growth |");
    println!("| --- | --- | --- | --- |");
    for (label, message) in &cases {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        for _ in 0..runs {
            s.push(stable(message));
            f.push(streaming(message));
        }
        let pick = |runs: &[Run]| (median(runs.iter().map(|r| r.total).collect()), median(runs.iter().map(|r| r.first_handler).collect()), median(runs.iter().map(|r| r.peak).collect()));
        let (st, sf, sp) = pick(&s);
        let (ft, ff, fp) = pick(&f);
        println!("| {label} | {} -> {} | {} -> {} | {} -> {} |", ms(st), ms(ft), ms(sf), ms(ff), mib(sp), mib(fp));
    }
}

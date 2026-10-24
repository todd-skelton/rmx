//! What the router costs per request, in process (no HTTP): allocations and bytes per request
//! (exact) and nanoseconds per request (the fastest of 15 runs of 200 ms), for four routers
//! that answer `GET /` with a short text body:
//!
//! 1. `handler`: the route alone.
//! 2. `4 pass-through`: four middleware written as `|ctx, next| next.run(ctx)`.
//! 3. `4 async`: four middleware written as `async move { next.run(ctx).await }`.
//! 4. `bench stack`: the bench server's middleware (static files, then form data).
//!
//! `cargo run --release -p rmx --example router_profile` prints the table. With
//! `-- <config> <n>` it only runs `n` requests through one router, for a profiler such as
//! `valgrind --tool=callgrind` (run it with two values of `n` and subtract to drop setup).

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rmx::fetch::{block_on, Body};
use rmx::middleware::form_data::form_data;
use rmx::middleware::static_files::{static_files, StaticOptions};
use rmx::response;
use rmx::router::{Context, Next, Router};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const CONFIGS: [&str; 4] = ["handler", "4 pass-through", "4 async", "bench stack"];

fn router(config: &str) -> Router {
    let mut router = Router::new();
    match config {
        "handler" => {}
        "4 pass-through" => {
            for _ in 0..4 {
                router.use_middleware(|ctx: Context, next: Next| next.run(ctx));
            }
        }
        "4 async" => {
            for _ in 0..4 {
                router.use_middleware(|ctx: Context, next: Next| async move { next.run(ctx).await });
            }
        }
        "bench stack" => {
            let public = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/rmx-app/public");
            router.use_middleware(static_files(public, StaticOptions { index: vec![], ..Default::default() }));
            router.use_middleware(form_data());
        }
        other => panic!("unknown config {other:?}; one of {CONFIGS:?}"),
    }
    router.get("/", |_: Context| async { response::text("Hello, world!") });
    router
}

fn request(router: &Router) -> usize {
    let req = http::Request::builder().uri("http://localhost/").body(Body::empty()).expect("request");
    let res = block_on(router.fetch(req));
    assert_eq!(res.status(), 200);
    black_box(res).headers().len()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [config, n] = args.as_slice() {
        let router = router(config);
        let n: usize = n.parse().expect("a request count");
        for _ in 0..n {
            request(&router);
        }
        return;
    }
    println!("{:<16} {:>12} {:>12} {:>10}", "router", "allocs/req", "bytes/req", "ns/req");
    for config in CONFIGS {
        let router = router(config);
        for _ in 0..1000 {
            request(&router);
        }
        let (a0, b0) = (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        let n = 10_000;
        for _ in 0..n {
            request(&router);
        }
        let allocs = (ALLOCS.load(Ordering::Relaxed) - a0) as f64 / n as f64;
        let bytes = (BYTES.load(Ordering::Relaxed) - b0) as f64 / n as f64;
        let mut best = f64::MAX;
        for _ in 0..15 {
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed().as_millis() < 200 {
                request(&router);
                count += 1;
            }
            best = best.min(start.elapsed().as_secs_f64() * 1e9 / count as f64);
        }
        println!("{config:<16} {allocs:>12.1} {bytes:>12.0} {best:>10.0}");
    }
}

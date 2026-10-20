//! Log each request: method, path, status and duration.

use std::time::Instant;

use rmx_router::{Context, Middleware, Next};

/// Prints `GET /path 200 3.1ms` to stdout for every request.
pub fn logger() -> impl Middleware {
    |ctx: Context, next: Next| async move {
        let start = Instant::now();
        let line = format!("{} {}", ctx.method(), ctx.url().path_and_query());
        let res = next.run(ctx).await;
        println!("{line} {} {:.1}ms", res.status().as_u16(), start.elapsed().as_secs_f64() * 1000.0);
        res
    }
}

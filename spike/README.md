# rmx-spike (phase 0)

axum + Leptos 0.8 server rendering with one WASM island (`Counter` in `src/app.rs`).
The task list is server-rendered only; the counter is the only code shipped to the browser.

## Toolchain on this machine

1. rustup, stable `x86_64-pc-windows-gnu` (rustc 1.99.1) plus the `wasm32-unknown-unknown` target.
   GNU was chosen because Visual Studio Build Tools are not installed.
2. WinLibs MinGW-w64 (winget `BrechtSanders.WinLibs.POSIX.UCRT`), needed for `dlltool.exe`
   when `windows-sys` builds. New shells pick it up from the user PATH; old shells need a restart.
3. `cargo-leptos` 0.3.11 prebuilt binary (sha256 verified) in `%USERPROFILE%\.cargo\bin`. It
   downloads `wasm-bindgen` 0.2.129 and binaryen `wasm-opt` into `%LOCALAPPDATA%\cargo-leptos`.

## Run

```powershell
cargo leptos serve            # http://127.0.0.1:3000
cargo leptos build --release
pwsh ./measure-wasm.ps1 -Label "islands"
# full-hydration comparison (whole app hydrates, no islands):
cargo leptos build --release --lib-features hydrate --bin-features ssr
```

## Measurements (2026-10-19, i9-10885H, 64 GB, Windows, GNU toolchain)

Bundle sizes (`wasm-release`: opt-level z, fat LTO, 1 codegen unit, panic=abort):

| Mode | WASM raw | WASM gzip | WASM brotli | JS glue gzip |
| --- | --- | --- | --- | --- |
| Islands (one `#[island]`) | 166.5 KB | 72.5 KB | 61.6 KB | 4.3 KB |
| Full hydration (same page) | 308.1 KB | 123.1 KB | 100.7 KB | 6.0 KB |

Build times (wall clock for `cargo leptos build`, server and WASM halves build in parallel):

| Build | Time |
| --- | --- |
| Clean debug, empty target dir, crates already downloaded | 264.7 s |
| Clean release (first release build) | 296.8 s |
| Incremental debug, one string changed in `app.rs` | 19.6 s, 21.3 s |
| Incremental release, same change | 22.8 s, 27.1 s |

In incremental builds the WASM half finishes in about 5-6 s; the server binary (about 18-26 s) is
the long pole, because it links axum, tokio and the whole SSR stack.

Browser check: the island hydrated, two clicks moved the counter from 2 to 4, one `<leptos-island>`
in the DOM, no console errors or warnings.

## Notes

1. `leptos_meta`/`leptos_router` version independently of `leptos` (0.8.7 / 0.8.17 vs 0.8.22).
2. `#[island]` needs `serde` as a direct dependency, and does not compile with the `islands` feature
   off, hence the `cfg_attr` on `Counter`.
3. Release WASM has no binaryen producer marker, so it is unconfirmed whether cargo-leptos ran
   `wasm-opt` on it; running it by hand is a cheap size check for later.

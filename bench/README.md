# rmx vs Remix 3 benchmarks

Compares rmx (`rmx-app/`: Rust, rmx's router, middleware and server, WASM islands) with
Remix 3.0.0 (`remix-app/`, Node). Both apps serve the same routes with the same markup, data
and islands:

1. `/`: a document shell, five tasks, a POST form, and a `Counter` island.
2. `/ssr/{n}`: `n` table rows, each rendered through a nested `RowView` component, with
   deterministic labels (same LCG on both sides).
3. `/client`: the `RowsBench` island (a js-framework-benchmark style table).

Parity check: after stripping hydration markers and self-closing slashes, the `<main>` HTML is
byte-identical for `/`, `/ssr/100` and `/client`. Remix responses are larger because `<head>`
carries modulepreload and import map tags.

Unless a section says otherwise, numbers were measured on one laptop: i9-10885H (8 cores, 16 threads),
64 GB RAM, Windows 11, Node 24.15.0, Rust 1.99.1, Chromium 148.0.7778.96, oha 1.16.0.

## How to Run

```powershell
pwsh ../scripts/build-bench.ps1                  # rmx release server + wasm client
# or: pwsh ../scripts/build-bench.ps1 -Nightly nightly   # client on nightly, immediate-abort panics
cd remix-app; npm install; cd ..
pwsh ./run.ps1 -Suffix -2026-10-22        # Windows: browser bench + server load tests
pwsh ./docker/run-docker.ps1 -Build       # Linux containers: server load tests only (-Out names the file)
```

1. `run.ps1` starts both servers on the host, runs `browser-bench.mjs` (Playwright driving
   Chromium; since 2026-10-23 every operation alternates between the two apps on each
   iteration, so background load hits both alike), then runs oha for 15 s at 50 connections after a 3 s warm-up per path.
   Configs: Remix in 1 Node process, Remix as a Node cluster (one worker per logical core),
   rmx on 1 thread, and rmx on all threads.
2. `docker/run-docker.ps1` runs each server alone in a container pinned to the same CPUs
   (`-OneCpu`, default `2`; `-ManyCpus`, default `0-7`) with an 8 GB cap. oha runs in its own
   container on CPUs `8-15` and talks to the server over a Docker network, so Windows port
   forwarding and bind mounts are not on the measured path.
3. `remix-app/diagnose.tsx` times Remix's router and renderer in process, without HTTP.

Results land in `results/`: `browser.json`, `server.json`, `docker-server*.json`, plus logs.

## Latest Run (2026-10-23, Afternoon): Frames and Navigation Compiled Out

The bench client now depends on rmx with `default-features = false`, which leaves the
browser half of frames, soft navigation and the server HTML differ out of the wasm
(decision record section 14). The bench page only hydrates two client entries, so it
behaves the same; the server keeps the defaults. Raw results:
`results/browser-2026-10-23-lean.json`, `results/browser-2026-10-23-lean-nightly.json` and
the matching `run-*.log` files.

### Client Size

| Build | `.wasm` raw | Total gzip | Total brotli | Gzip vs Remix |
| --- | --- | --- | --- | --- |
| Remix 3 (41 JS files) | | **42.5 KB** | **37.7 KB** | 1x |
| rmx stable, all features (morning run) | 317.3 KiB | 150.4 KB | 127.1 KB | 3.5x |
| rmx nightly, all features (morning run) | 273.7 KiB | 133.6 KB | 113.5 KB | 3.1x |
| rmx stable, no frames or navigation | 205.3 KiB | 99.7 KB | 85 KB | 2.3x |
| rmx nightly, no frames or navigation | 174.6 KiB | 87.2 KB | 75.3 KB | 2.1x |

Compiling the features out saves 50.7 KB gzip on stable and 46.4 KB on nightly. The
nightly client is still 44.7 KB above Remix. What is left, measured with twiggy on the
nightly build before `wasm-opt` (shallow bytes):

1. The runtime the page does use: the vdom (44 KB), component instances and handles
   (11 KB), startup, mixin hosting and the rest (about 28 KB).
2. What wasm brings that JavaScript gets from the browser: `core` (29 KB), `js-sys`
   (19 KB), serde_json for client entry props (17 KB), `alloc` (10 KB), `hashbrown`
   (9 KB), the allocator (5 KB) and string data (17 KB).
3. The JS glue: 38 KB raw, 6.6 KB gzip.

serde_json is the largest single item left; the rest are a few KB each. Remix cannot make
the same cut: its `run()` starts frames and the navigation listener on every page.

### Lists and Load

Median commit ms (click to DOM updated), operations interleaved between the apps. The two
runs had different background load, so compare within a pair of columns only.

| Operation | Remix 3 | rmx stable | Remix 3 | rmx nightly |
| --- | --- | --- | --- | --- |
| create 1k | 13.6 | **12** | 25.1 | **20.9** |
| replace 1k | 20.8 | **19.1** | 32.5 | **31.3** |
| update every 10th | 8.4 | **3.7** | 11.5 | **7.8** |
| select row | 5.4 | **2.9** | 9.5 | **5.4** |
| swap rows | 5.6 | **3.7** | 9.6 | **6.6** |
| remove row | 6.5 | **3.2** | 8.1 | **5.8** |
| create 10k | **501** | 502.9 | 843 | **791.8** |
| append 1k | 17.9 | **14.3** | 20.8 | **16.6** |
| clear 1k | 5.3 | 5.3 | 5.9 | 5.9 |
| Page hydrated (ms after navigation) | 146.4 | **67.85** | 162.2 | **83.25** |

Leaving the features out did not change list speed. An interleaved A/B of the all-features
and no-features clients (stable, one browser, 15 to 25 iterations, three rounds) kept every
operation within noise; create 10k ranged from 0.93x to 1.10x across the rounds. `clear
1k` still reaches the next painted frame later on rmx (13.6 vs 11.8 ms and 11.8 vs
7.3 ms).

## Earlier Run (2026-10-23, Morning): Faster Lists, Slightly Smaller Client

Browser only; the servers did not change, so the 2026-10-22 server numbers below still
stand. Results: `results/browser-2026-10-23.json` (stable Rust, the default build) and
`results/browser-2026-10-23-nightly.json` (`build-bench.ps1 -Nightly`). Same laptop and
versions as below, nightly rustc 1.101.0 (2026-10-22). Other programs kept the host busy
(roughly 30 to 90% CPU, sampled between runs), which is why both apps are slower in the
stable run than in the nightly one; compare columns within a run, not across runs.

What changed in rmx (commit `607bc43`):

1. The hot DOM calls go through one small JS object (`packages/component/src/dom/ops.rs`)
   instead of one web-sys call each: an element or text node is created and appended in
   one call, tags, attribute names and static text are indexes into a JS string table,
   short ASCII strings are decoded in JS straight from wasm memory instead of by
   `TextDecoder`, and binding a listener is one call.
2. Keyed lists patch their common start and end in place, so an append only creates the
   new rows; a list where nothing is reused skips the move pass.
3. Integers render without `core::fmt`, and wasm-bindgen's `enable-interning` is off (it
   looked up every string passed to JS in a hash map).

### Lists

Median commit ms (click to DOM updated), 15 iterations (5 for create 10k):

| Operation | Remix 3 | rmx stable | Remix 3 | rmx nightly | rmx on 2026-10-22 vs Remix |
| --- | --- | --- | --- | --- | --- |
| create 1k | 25.1 | **22.5** | 17.5 | **14.6** | 18.2 vs 20.7 |
| replace 1k | 42.3 | **39.3** | 20.6 | **20** | 26.3 vs 19.5 |
| create 10k | 1019 | **970.3** | 687.3 | **657.3** | 629.8 vs 566 |
| append 1k | 19.1 | **18** | 24.9 | **21.7** | 30.4 vs 24.6 |
| update every 10th | 12.1 | **7.5** | 8.7 | **5.7** | 3.7 vs 8.6 |
| select row | 11.4 | **6.5** | 11.3 | **6.7** | 3.2 vs 6.2 |
| swap rows | 12.5 | **8** | 11.2 | **6.5** | 4.6 vs 9.6 |
| remove row | 11.7 | **7.4** | 7.3 | **5.7** | 3.3 vs 8.5 |
| clear 1k | **6.6** | 6.8 | 8.7 | **7.7** | 5.3 vs 7.7 |

rmx now commits faster than Remix on replace, create 10k and append, the three it lost
before. The margins on create 1k, replace 1k, create 10k and append 1k are small (3 to 17%),
and in the nightly run the next painted frame for create 1k and replace 1k was still a few
ms later for rmx (78.4 vs 74.9 ms and 73.4 vs 70.3 ms), so call those a tie on screen.
`clear 1k` reaches the next frame later on rmx in both runs (16.9 vs 12 and 21.3 vs 12.1
ms) with the same commit time; that is not explained yet.

An interleaved A/B of the old and new runtime (stable builds, one browser, 20 iterations, run
before the commit) put the new one at 0.80x the old on create 10k, 0.64x on append 1k,
0.71x on replace 1k and 0.64x on create 1k.

### Client Size

| Build | `.wasm` raw | Total gzip | Total brotli |
| --- | --- | --- | --- |
| Remix 3 (41 JS files) | | 42.5 KB | 37.7 KB |
| rmx on 2026-10-22 | 320.8 KiB | 151.4 KB | 127.6 KB |
| rmx stable, this run | 317.3 KiB | 150.4 KB | 127.1 KB |
| rmx nightly, immediate-abort panics | 273.7 KiB | 133.6 KB | 113.5 KB |

The size gap is narrowed, not closed: about 3.1x Remix with the nightly build, 3.5x on
stable. Where the bytes go, measured with twiggy on the nightly build before `wasm-opt`
(shallow bytes):

1. The component runtime itself, about 145 KB: the vdom (41 KB), frames (35 KB), soft
   navigation (17 KB), the DOM differ for frame reloads (9 KB) and the rest. Remix ships
   the same features plus the app as about 112 KB of unminified JavaScript, and JavaScript compresses
   better than wasm (2.6x against 2.2x gzip here).
2. What JavaScript gets from the browser and wasm has to bring: `core` and `alloc`
   (52 KB), serde_json for client entry props (about 20 KB), `hashbrown` (9 KB), the
   allocator (5 KB) and the wasm-bindgen glue (66 KB of JS, 10 KB gzip).
3. String data, about 21 KB.

Things tried that did not pay: opt-level `s` instead of `z` (14 KB more gzip, no clear speed
gain), more `wasm-opt` passes (`--converge`, `--gufa`, `-O3`: within 0.3 KB gzip),
`-Zlocation-detail=none -Zfmt-debug=none` on top of immediate-abort (0.8 KB), and
minifying the JS glue (2 KB gzip, but it would add a Node tool to the Rust build). Closing
the rest would mean shipping less runtime per page (for example compiling frames and
navigation out of apps that do not use them, which Remix does not do either; done in the
afternoon run above) or rendering
from a JavaScript runtime, which would no longer be full stack Rust.

The nightly build's trade-off: with `panic=immediate-abort`, a panic traps without a
console message and `App::on_error` does not hear about it.

### Load

| Metric | Remix 3 | rmx stable | Remix 3 | rmx nightly |
| --- | --- | --- | --- | --- |
| Hydrated at | 241.8 ms | 105.85 ms | 138.15 ms | 64.4 ms |
| DOMContentLoaded | 228.95 ms | 73.45 ms | 129.55 ms | 45.8 ms |
| JS heap after run | 7.4 MB | 3.5 MB | 7.4 MB | 3.4 MB |

Both counters hydrated and incremented (5 to 6), with no page errors on either side, in
both runs.

## Previous Run (2026-10-22): rmx on Its Own Stack

Until this run, the rmx bench server was an axum app that used rmx only to render. It now
runs on rmx itself, mirroring `remix-app/app/router.tsx` line for line: the static
middleware first, then form data, then routes that call `ctx.render`, served by
`rmx::node_fetch_server`. The HTML is the same as the axum server's apart from the
hydration marker ids (checked for `/`, `/ssr/100`, `/client` and `/app.css`), and the
form POST still redirects with 303. An A/B on the Windows host put the two servers within
about 15% of each other (8 s at 50 connections, measured):

| rmx server | 1 thread, `/` | 1 thread, `/ssr/100` | 16 threads, `/` |
| --- | --- | --- | --- |
| axum (earlier runs) | 6,367 req/s | 1,257 req/s | 34,394 req/s |
| rmx stack, no static middleware | 6,047 req/s | 1,457 req/s | 34,879 req/s |
| rmx stack (this run) | 5,448 req/s | 1,394 req/s | 32,436 req/s |

Remix 3.0.0 on Node v24.22.1, Rust 1.99.1, Chromium 148.0.7778.96, same laptop as below. Results:
`results/docker-server-2026-10-22.json`, `results/server-2026-10-22.json` and
`results/browser-2026-10-22.json`.

### Server, Linux Containers (Pinned CPUs)

Requests per second, 15 s at 50 connections after a 3 s warm-up; oha runs in its own
container on other CPUs.

| Config | CPUs | `/` | `/ssr/100` | `/ssr/1000` | Idle / loaded memory | Startup |
| --- | --- | --- | --- | --- | --- | --- |
| Remix, 1 Node process | 1 | 233 | 195 | 31 | 83.9 / 340.4 MB | 5.3 s |
| rmx, 1 thread | 1 | 3,623 | 2,407 | 443 | 3 / 5.1 MB | 1.4 s |
| Remix, cluster of 8 workers | 8 | 999 | 903 | 109 | 528.7 / 2207.7 MB | 3.4 s |
| rmx, 8 threads | 8 | 42,881 | 15,601 | 2,165 | 1.1 / 9.1 MB | 1.7 s |

| rmx / Remix | `/` | `/ssr/100` | `/ssr/1000` |
| --- | --- | --- | --- |
| 1 CPU | 16x | 12x | 14x |
| 8 CPUs | 43x | 17x | 20x |

Startup includes starting the container; Remix's also includes `node-tsx` transpiling on
first load.

### Server, Windows Host

Same settings, servers unpinned. Other applications kept the CPU about 44% busy during this
run (measured just before it), so treat these as rough: rmx on 1 thread served `/` at
1,226 req/s here and at 5,448 req/s in the A/B above, minutes later. The Docker rows are
the ones to compare.

| Config | `/` | `/ssr/100` | `/ssr/1000` | Idle memory |
| --- | --- | --- | --- | --- |
| Remix, 1 Node process | 182 | 246 | 38 | 113.2 MB |
| Remix, cluster of 16 workers | 545 | 746 | 111 | 1710.8 MB |
| rmx, 1 thread | 1,226 | 646 | 140 | 17.6 MB |
| rmx, 16 threads | 4,034 | 11,214 | 2,006 | 18 MB |

### Browser (Chromium, Windows Host)

Medians over 10 fresh page loads and 15 iterations per operation; commit is click to DOM
updated, frame adds the next paint. One run.

| Metric | Remix 3 | rmx |
| --- | --- | --- |
| Client assets (gzip) | 42.5 KB in 41 files | 151.4 KB in 3 files (320.8 KiB raw `.wasm`) |
| Hydrated at | 136.4 ms | 63.45 ms |
| DOMContentLoaded | 127.9 ms | 43.95 ms |
| JS heap after run | 4.2 MB | 2.7 MB (excludes WASM memory) |

| Operation | Remix 3 commit / frame ms | rmx commit / frame ms |
| --- | --- | --- |
| create 1k | 20.7 / 87.7 | 18.2 / 66.4 |
| replace 1k | 19.5 / 68 | 26.3 / 71.6 |
| update every 10th | 8.6 / 45.4 | 3.7 / 39.3 |
| select row | 6.2 / 19.2 | 3.2 / 17.3 |
| swap rows | 9.6 / 23.4 | 4.6 / 23.3 |
| remove row | 8.5 / 29.2 | 3.3 / 27.3 |
| create 10k | 566 / 574 | 629.8 / 648.7 |
| append 1k | 24.6 / 106.5 | 30.4 / 96.2 |
| clear 1k | 7.7 / 19.8 | 5.3 / 20.6 |

Both counters hydrated and incremented (5 to 6), with no page errors on either side.

## Earlier Runs (2026-10-19 and 2026-10-20, axum Server)

The rest of this file is the first two rounds, when the rmx server was axum with rmx
rendering. Their client numbers still describe rmx's runtime at the time.

### Browser (Chromium, Windows host)

Load metrics are medians over 10 fresh page loads; operations are medians over 15 iterations.
"Commit" is click to DOM updated; "frame" adds the next paint.

| Metric | Remix 3 | rmx |
| --- | --- | --- |
| Client assets (gzip) | 42.5 KB in 41 files | 63.9 KB in 3 files (59.4 KB is the `.wasm`) |
| Hydrated at (run 1 / run 2) | 192.6 / 230.7 ms | 83.2 / 82.9 ms |
| DOMContentLoaded (run 1 / run 2) | 181.9 / 218.4 ms | 64.4 / 66.3 ms |
| JS heap after run (run 1 / run 2) | 7.4 / 26.7 MB | 2.6 / 6.0 MB (excludes WASM memory) |

| Operation, commit ms (run 1 / run 2) | Remix 3 | rmx |
| --- | --- | --- |
| create 1k rows | 19.3 / 26.9 | 33.6 / 36.1 |
| replace 1k rows | 28.5 / 37.4 | 47.2 / 45.4 |
| update every 10th row | 8.4 / 11.7 | 6.3 / 6.3 |
| select row | 10.9 / 10.8 | 5.4 / 5.4 |
| swap rows | 11.1 / 11.6 | 6.2 / 6.2 |
| remove row | 11.2 / 11.2 | 5.9 / 6.1 |
| create 10k rows | 904.9 / 1061.9 | 1020.0 / 972.7 |
| append 1k rows | 36.3 / 34.8 | 26.3 / 38.7 |
| clear 1k rows | 9.9 / 10.1 | 6.4 / 9.7 |

Both counters hydrated and incremented (5 to 6) with no page errors in either run.

### Re-run After the Parity Work (2026-10-20)

One run on the same machine and Chromium, same method (`results/browser-2026-10-20.json`;
`results/browser.json` keeps the 2026-10-19 run). The rmx runtime now carries frames,
Navigation API soft navigation and the rest of the parity work, so its
client bundle about doubled. Single runs vary by 30% or more.

| Metric | Remix 3 | rmx |
| --- | --- | --- |
| Client assets (gzip) | 42.5 KB in 41 files | 124.8 KB in 3 files (262.4 KiB raw `.wasm`) |
| Hydrated at | 256 ms | 108 ms |
| DOMContentLoaded | 243.6 ms | 76.6 ms |
| JS heap after run | 7.2 MB | 4.4 MB (excludes WASM memory) |

| Operation | Remix 3 commit / frame ms | rmx commit / frame ms |
| --- | --- | --- |
| create 1k rows | 28.7 / 118.5 | 39.4 / 131.0 |
| replace 1k rows | 39.3 / 136.4 | 52.4 / 150.6 |
| update every 10th row | 12.9 / 75.2 | 7.0 / 65.2 |
| select row | 13.6 / 19.7 | 3.3 / 13.3 |
| swap rows | 13.6 / 34.8 | 7.5 / 25.4 |
| remove row | 12.7 / 41.0 | 7.6 / 37.4 |
| create 10k rows | 1129.0 / 1162.2 | 1132.3 / 1150.7 |
| append 1k rows | 36.5 / 171.6 | 29.3 / 106.9 |
| clear 1k rows | 11.0 / 13.5 | 8.4 / 11.0 |

### Server, Linux Containers (Docker Desktop, WSL2)

Requests per second, 15 s at 50 connections. Docker run 3 (`results/docker-server.json`).

| Config | CPUs | `/` | `/ssr/100` | `/ssr/1000` | Idle / loaded memory |
| --- | --- | --- | --- | --- | --- |
| Remix, 1 Node process | 1 | 164 | 118 | 19 | 74 / 306 MB |
| rmx, 1 thread | 1 | 7,057 | 1,715 | 295 | 0.7 / 2.4 MB |
| Remix, cluster of 8 workers | 8 | 744 | 637 | 75 | 601 / 2,225 MB |
| rmx, 8 threads | 8 | 30,609 | 11,772 | 2,021 | 1.2 / 6.8 MB |

Absolute throughput moved a lot between identical Docker runs (rmx 1 thread on `/`: 4,904,
11,170 and 7,057 req/s across three runs; Remix 1 process: 109, 322 and 164). The ratios held
up better:

| rmx / Remix ratio | `/` | `/ssr/100` | `/ssr/1000` |
| --- | --- | --- | --- |
| 1 CPU, Docker runs 1 / 2 / 3 | 45x / 35x / 43x | 11x / 19x / 15x | 10x / 18x / 16x |
| 8 CPUs, Docker runs 1 / 3 | 37x / 41x | 12x / 18x | 19x / 27x |
| Windows, 1 process vs 1 thread | 33x | 6x | 8x |
| Windows, 16 workers vs 16 threads | 54x | 14x | 14x |

Docker run 1 pinned the 1-CPU configs to CPU 0; runs 2 and 3 used CPU 2.

### Server, Windows Host

Run 2 (`results/server.json`), same oha settings, servers unpinned.

| Config | `/` | `/ssr/100` | `/ssr/1000` | Idle memory |
| --- | --- | --- | --- | --- |
| Remix, 1 Node process | 147 | 274 | 33 | 110 MB |
| Remix, cluster of 16 workers | 698 | 1,189 | 185 | 1,657 MB |
| rmx, 1 thread | 4,866 | 1,711 | 277 | 16 MB |
| rmx, 16 threads | 37,385 | 16,366 | 2,627 | 17 MB |

Windows memory is the working set of the server and its child processes, which includes the
console host Windows attaches to each server (about 10 MB, estimated from the change between
runs 1 and 2); Docker memory is `docker stats`.
Run 1 (`results/server-run1.json`) used Remix's development asset setting (`watch: true`) and is
superseded.

## Where Remix Spends Its Time

In-process timings from `remix-app/diagnose.tsx`, ms per call, NODE_ENV=production:

| Call | Windows | Linux container, 1 CPU |
| --- | --- | --- |
| `router.fetch('/')` | 9.4 | 11.7 |
| `router.fetch('/ssr/0')` | 1.4 | 3.2 |
| `router.fetch('/ssr/1000')` | 35.3 | 82.5 |
| `assets.getScriptEntry(counter.tsx)` | 7.4 | 3.2 |
| `renderToString(<SsrPage n={1000}/>)` | 40.0 | 83.7 |

1. Pages with a client entry pay for `assets.getScriptEntry` on every request: the render
   middleware resolves the island's module graph, preloads and import map each time. A CPU
   profile on Windows put 63% of that call in `lstat`. On Linux the call is about half as
   expensive, which matches file-system metadata calls being cheaper there.
2. Rendering itself is the other cost. Pinning Node to one CPU roughly doubles render time
   (renderToString n=1000: 84 ms on CPU 0, 56 ms on CPU 3, 65 ms with all 16 CPUs available
   in a container, 36 to 41 ms on Windows), likely because garbage collection and JIT threads
   must share that CPU. rmx has no garbage collector or JIT, so the 1-CPU Docker rows are the strict
   "same hardware" comparison and the Windows rows are the more forgiving one for Node.

## Caveats

1. Variance is large in both environments; treat single numbers as plus or minus 30% or more
   and compare ratios.
2. Docker Desktop's VM shares the laptop with Windows and other running containers; vCPUs are
   not pinned to physical cores, so CPU pinning is only approximate.
3. The browser bench runs on the Windows host only; containers do not change client results.
4. The JS heap number excludes WASM linear memory, so it understates rmx's client memory.
5. This compares a proof of concept against a released framework. The benchmark pages use no
   frames, and rmx has no import maps, asset fingerprinting or module graph, which cost Remix
   time on every request with a client entry.
6. Remix runs TypeScript through `remix/node-tsx`, which transpiles on first load; startup
   numbers (3 to 11 s for Remix, 0.5 to 4 s for rmx in these runs) include that. Docker startup
   also includes starting the container and the probe container, so it reads higher than Windows.

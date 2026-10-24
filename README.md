<br />

<p align="center"><strong>rsmix</strong><br />Remix 3, rewritten in Rust, because of course it was.</p>

<br />

# Welcome to rsmix!

This is the source repository for rsmix, a proof of concept that ports [Remix 3](https://github.com/remix-run/remix) to Rust: the same packages, the same demos, the same component model, and very different build times. It is laid out like `remix-run/remix` (`packages/`, `demos/`, `template/`, `scripts/`, `decisions/`), so you can open the same path in both repositories and compare.

Everyone is rewriting everything in Rust right now, usually without stopping to ask whether they should. We did not stop to ask either. We did keep score, though.

## Principles (and How It's Going)

Remix 3 is built on six principles. rsmix follows them as far as Rust allows, and then a bit past the point where it should have stopped:

1. **Model-First Development.** Kept. A component is a setup scope plus a render function: state is plain fields, `handle.update()` re-renders, and there are no hooks, signals or dependency arrays. Most of this repository was written by coding agents, so the principle has at least been tested on itself.
2. **Build on Web APIs.** Bent. Rust has no Web APIs on the server, so we brought our own: `packages/fetch` (`Request`, `Response`, `FormData`, `AbortSignal`), `packages/url` and `packages/crypto` are the platform that Remix gets for free. They follow the specs' names and behavior, and the browser side uses the real APIs through `web-sys`.
3. **Religiously Runtime.** Broken, and we are only a little sorry. `rsx!` is a proc macro, route maps are checked by the type system, and a build takes minutes where Remix takes none. In our defense, what Remix decides at runtime (route matching, frame resolution, mixins, hydration) is still decided at runtime. The compiler just gets to complain first.
4. **Avoid Dependencies.** Working on it. Remix's goal is zero; `Cargo.lock` lists 168 crates from crates.io, mostly hyper, tokio, wasm-bindgen and ICU4X (the i18n demo) with their friends. The core is leaner than that sounds: `crypto`, `url`, `mime`, `tar-parser` and `terminal` have no dependencies at all, and `headers` has only `http`.
5. **Demand Composition.** Kept, and Cargo enforces it. Nearly every Remix package is its own crate, including 11 of the 12 middleware packages (async context lives in the router, which has to keep it in step), and each crate builds and tests on its own. Cargo rejects dependency cycles, so the boundaries hold whether we like it or not.
6. **Distribute Cohesively.** Kept. One dependency, `rsmix`, re-exports everything at Remix's subpath names: `remix/middleware/session` is `rsmix::middleware::session`, and `remix/node-fetch-server` is `rsmix::node_fetch_server` (no Node was harmed; it is hyper underneath). A test fails if a package is left out.

## The Component Model

Remix 3 keeps state in plain variables and re-renders when you call `handle.update()`. rsmix keeps state in plain fields and re-renders when you call `h.update()`:

```rust
pub struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rsmix::client_entry!("Counter"); // hydrates in the browser, compiled to WebAssembly

    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial }
    }

    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <button type="button" on:click={h.on(|this, h| {
                this.count += 1;
                h.update();
            })}>
                "Count: " {self.count}
            </button>
        }
    }
}
```

The same component in Remix 3:

```tsx
export const Counter = clientEntry(import.meta.url, function Counter(handle: Handle<{ initial: number }>) {
  let count = handle.props.initial
  return () => (
    <button type="button" mix={on('click', () => { count++; handle.update() })}>
      Count: {count}
    </button>
  )
})
```

More lines, more types, same idea. The full tour (`Handle`, `rsx!`, frames, navigation, what is and is not ported) is in [packages/rsmix](packages/rsmix/README.md).

## Packages

Every package below is a crate in `packages/<name>`, named `rsmix-<name>` on the Rust side, with the same job as its Remix namesake and that package's tests ported case by case. The `rsmix` crate composes them under one umbrella, like the `remix` package.

- [assets](packages/assets): Serve browser assets with access rules, fingerprints and CSS and module rewriting
- [auth](packages/auth): Browser login, OAuth and OIDC helpers (PKCE, GitHub, Google, X, Facebook, Microsoft, Okta, Auth0)
- [auth-middleware](packages/auth-middleware): Pluggable authentication middleware
- [cli](packages/cli): The `rsmix` command: `new`, `dev`, `doctor`, `completion`, `routes`, `assets`, `test`
- [component](packages/component): Component runtime with server rendering, streaming, frames and browser hydration (plus `rsx!` in [component/macros](packages/component/macros))
- [compression-middleware](packages/compression-middleware): Middleware for compressing HTTP responses
- [cookie](packages/cookie): Cookie parsing, signing and serialization
- [cop-middleware](packages/cop-middleware): Tokenless cross-origin protection
- [cors-middleware](packages/cors-middleware): CORS headers and preflight responses
- [csrf-middleware](packages/csrf-middleware): Session-backed CSRF protection
- [data-schema](packages/data-schema): Small, sync schema validation, including `FormData` and `URLSearchParams`
- [fetch-proxy](packages/fetch-proxy): An HTTP proxy for the Fetch API
- [fetch-router](packages/fetch-router): A minimal, composable router for the Fetch API (with `async-context-middleware` inside, see below)
- [file-storage](packages/file-storage): Key/value storage for files, in memory or on disk
- [form-data-middleware](packages/form-data-middleware): Middleware for parsing `FormData` from request bodies
- [fs](packages/fs): Filesystem utilities on lazy files
- [headers](packages/headers): Typed HTTP headers
- [html-template](packages/html-template): `html!` templates with auto-escaping
- [lazy-file](packages/lazy-file): Lazy, streaming files and blobs
- [logger-middleware](packages/logger-middleware): Request and response logging with Remix's format tokens
- [method-override-middleware](packages/method-override-middleware): Override request methods from form data
- [mime](packages/mime): MIME types by file extension, from the same mime-db table
- [multipart-parser](packages/multipart-parser): A streaming multipart parser
- [node-fetch-server](packages/node-fetch-server): Serve a Fetch API router over HTTP/1 and HTTP/2. The name is kept for parity; there is no Node in it, just hyper and tokio
- [render-middleware](packages/render-middleware): `ctx.render(node)` with streamed `<Frame>` resolution
- [response](packages/response): Response helpers: `html`, `redirect`, `file`, `compress`
- [rsmix](packages/rsmix): The framework in one dependency (Remix's [remix](https://github.com/remix-run/remix/tree/main/packages/remix))
- [route-pattern](packages/route-pattern): Match, rank and generate URLs
- [session](packages/session): Sessions with cookie, memory and filesystem storage
- [session-middleware](packages/session-middleware): Load and save the request's session
- [spa](packages/spa): Run the router in the browser for client-only apps
- [static-middleware](packages/static-middleware): Serve static files, with ranges, ETags and directory listings
- [tar-parser](packages/tar-parser): A streaming tar parser
- [terminal](packages/terminal): ANSI styles, color detection and terminal output
- [ui](packages/ui): Headless, accessible UI primitives and animation utilities (accordion, anchor, combobox, listbox, menu, popover, select, tabs, toggle, springs)

Packages Remix has no need for, because JavaScript ships them with the platform:

- [fetch](packages/fetch): `Request`, `Response`, streaming bodies, `FormData` (and the `form-data-parser` package's parsing) and `AbortSignal`
- [url](packages/url): `URL` and `URLSearchParams`
- [crypto](packages/crypto): The Web Crypto subset Remix uses: SHA, HMAC, PBKDF2, random bytes
- [live-reload-middleware](packages/live-reload-middleware): Reloads open pages when `rsmix dev` restarts the app, standing in for `node-hmr`

Remix packages rsmix does not port, and why:

- `data-table`, `data-table-mysql`, `data-table-postgres`, `data-table-sqlite`, `session-storage-memcache`, `session-storage-redis`, `file-storage-s3`: data access and storage backends, which Rust's existing libraries already cover well
- `async-context-middleware`: lives in `fetch-router` as `async_context` (and is re-exported as `rsmix::middleware::async_context`), because the router's `Context` has to keep the scope in step
- `form-data-parser`: `FormData::parse` in `fetch`, next to the `FormData` type
- `node-tsx`, `node-hmr`, `component-hmr`, `multiple-import-maps-polyfill`: JavaScript tooling. `rustc` is our loader, `rsmix dev` rebuilds and reloads, and one wasm bundle needs no import maps
- `assert`, `test`: `assert!` and `cargo test`, plus `wasm-bindgen-test` in Chromium for browser tests

## Demos

Every [Remix 3 demo](https://github.com/remix-run/remix/tree/main/demos) is ported in [demos/](demos), with a Playwright script (`e2e.mjs`) that checks it in a real browser: [assets](demos/assets), [bookstore](demos/bookstore), [frame-navigation](demos/frame-navigation), [frames](demos/frames), [i18n](demos/i18n), [lazy-frames](demos/lazy-frames), [social-auth](demos/social-auth), [spa](demos/spa), [sse](demos/sse), [timeboxer](demos/timeboxer), [ui](demos/ui) and [unpkg](demos/unpkg). [template](template) is the `remix new` starter app, and `rsmix new` writes it out.

```powershell
./scripts/build-client.ps1 -Crate frames-demo -Out demos/frames/public/pkg   # the wasm client
cargo run -p frames-demo                                                       # http://127.0.0.1:44110/
node demos/frames/e2e.mjs                                                      # browser checks
```

## Benchmarks

Did rewriting it in Rust make it fast? Of course it did; that is the only reason anyone does it. The [bench](bench) directory has the same app written twice, once with Remix 3.1.0 on Node and once with rsmix, serving identical HTML (an SSR table, a task list with a form and an island, and a js-framework-benchmark style client table). Measured 2026-10-24 on a fresh DigitalOcean droplet with 16 dedicated vCPUs (`c-16`, Xeon Platinum 8168, destroyed afterwards), with each server alone in a Docker container pinned to the same CPUs and the load generator on the other eight:

| Server, requests per second | `/` | `/ssr/100` | `/ssr/1000` | Memory under load |
| --- | --- | --- | --- | --- |
| Remix 3.1, 1 Node process (1 CPU) | 2,292 | 693 | 82 | 341 MB |
| rsmix, 1 thread (1 CPU) | 21,454 | 6,075 | 803 | 3 MB |
| Remix 3.1, 8 Node workers (8 CPUs) | 13,234 | 4,576 | 606 | 2,891 MB |
| rsmix, 8 threads (8 CPUs) | 121,956 | 31,433 | 5,046 | 9 MB |

That is 7x to 10x the throughput in a few megabytes. Remix 3.1 got a lot faster on the cheap page (2.9x its 3.0 figure on one process), so the old 43x headline is retired, and a clean machine is kinder to Node than a busy laptop was: its cluster scales 5.8x on eight CPUs here. In the browser (Chromium, medians, measured 2026-10-24 with the client built on nightly Rust and frames and navigation compiled out; the stable build is in the bench notes):

| | Remix 3.1 | rsmix |
| --- | --- | --- |
| Client download (gzip) | **46.4 KB** in 43 files | 87.2 KB in 3 files |
| Page hydrated | 182.45 ms | **94.3 ms** |
| Create 1,000 rows | 28.6 ms | **20.3 ms** |
| Replace 1,000 rows | 40.8 ms | **29.3 ms** |
| Create 10,000 rows | 947.8 ms | **910.8 ms** |
| Append 1,000 rows | 40.4 ms | **28.7 ms** |
| Select a row | 10.3 ms | **5.4 ms** |

Rust also lost the list benchmarks at first, which was awkward for a rewrite whose only argument is speed. Every DOM node cost several calls from wasm into JavaScript and a UTF-8 decode, so rsmix now batches those calls through one small JavaScript object of its own. Yes, the Rust rewrite got faster by writing more JavaScript. It now commits first on every list operation, by 4 to 48% against Remix 3.1, which is mostly a tie on screen: to the next painted frame the gap is a few milliseconds either way on most rows.

Then the download. Remix's `run()` starts frames and soft navigation on every page, including the bench page, which has no frames and does not need soft navigation. Rust gets to delete them at compile time: the bench client builds with `default-features = false`, and Cargo features took it from 133.6 KB to 87.2 KB. A Rust rewrite getting smaller by deleting code instead of adding it felt suspicious enough that we measured it twice.

Remix 3.1 then added prefetching and the client grew by 3.9 KB, because `run()` loads it on every page. rsmix ported prefetching behind the same `navigation` feature, so the bench client did not grow, and the gap closed by 3.9 KB without rsmix doing anything. This is the most honest way to win a size benchmark. Streaming uploads (Remix 3.1's `/future` parsers) are ported too: a 100 MiB upload reaches the handler in 0.04 ms instead of 125 ms and no longer copies the file into memory.

The router then got the treatment every Rust rewrite eventually gets: someone counted the allocations. `next.run(ctx)` used to box itself on every middleware step, so it now returns a plain future instead, and a middleware step costs 945 instructions instead of 1,384 (callgrind, in process). Over HTTP that was worth 0 to 7% on `/` on the droplet, which is what counting allocations is usually worth. The profile also found the real cost: the static files middleware resolved every request path all the way from `/` before falling through to the router. It now checks one name at a time and stops at the first one that does not exist, which made `/` 15 to 22% faster on the droplet in each round. Remix's middleware resolves the root and the path on every request too; we would mention it upstream, but that would require writing JavaScript.

The fine print, because there is always fine print: the client still ships about 1.9x more bytes, since WebAssembly brings its own runtime where Remix borrows the browser's. The nightly build trims 12.5 KB by making panics trap silently (the stable build is 99.7 KB), and with every feature on the client was 133.6 KB on nightly and 150.4 KB on stable (measured 2026-10-23, before prefetching was added). The browser numbers come from the busy Windows laptop, where single runs vary by 30% or more, and this compares a proof of concept with a released framework. The method, the raw results, where the bytes go and every earlier run are in [bench/README.md](bench/README.md).

## Getting Started

You need Rust stable with the `wasm32-unknown-unknown` target, plus `wasm-bindgen` 0.2.129 and `wasm-opt` for browser builds (details in [packages/rsmix](packages/rsmix/README.md#build-and-run)).

```sh
cargo install --path packages/cli   # the rsmix command
rsmix new my-rsmix-app                  # Remix's template, now in Rust
cd my-rsmix-app
rsmix dev                             # rebuild, restart and reload on change
```

Or depend on it directly:

```toml
[dependencies]
rsmix = { git = "https://github.com/todd-skelton/rsmix" }
```

## Repository Layout

| Path | In Remix | Here |
| --- | --- | --- |
| `packages/` | One npm package per directory | One crate per directory, same names |
| `demos/` | The demo apps | The same demos, each with a browser e2e script |
| `template/` | The `remix new` starter | The `rsmix new` starter |
| `scripts/` | Release and repo tooling | `build-client.ps1` (wasm builds), `test-browser.ps1` (browser tests), `build-bench.ps1` |
| `decisions/` | Architecture decision records | [000: Rewrite it in Rust](decisions/000-rewrite-it-in-rust.md), the plan and the requirement-by-requirement parity audit |
| `bench/` | (none) | The side-by-side benchmark apps and results |
| `Cargo.toml` | `package.json`, `pnpm-workspace.yaml` | The workspace |

## Testing

```powershell
cargo test --workspace                     # native tests, including Remix's ported test files
./scripts/test-browser.ps1                 # component runtime tests in headless Chromium
./scripts/test-browser.ps1 -Crate rsmix-ui   # UI library tests in headless Chromium
```

## License

MIT, as declared in `Cargo.toml`. Remix 3 is MIT licensed by Shopify Inc., and this port leans on its design, tests, demos and docs throughout.

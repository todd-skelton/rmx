# rmx

A proof of concept of the Remix 3 component model in idiomatic Rust: server rendering on
axum, plus interactive islands compiled to WebAssembly. This is a fun side project, not a
production framework.

## The Component Model

Remix 3 components keep state in plain variables in a setup closure and re-render only when
you call `handle.update()`. rmx maps that onto a Rust struct:

1. `setup(props, h)` runs once and returns the state struct (Remix's setup closure).
2. `render(&self, props, h)` turns state into a `Node` tree (Remix's returned render function).
3. Handlers get `&mut self`, change fields directly, and call `h.update()` to re-render.

There are no signals, hooks, dependency arrays, or change detection.

```rust
pub struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rmx::client_entry!("Counter"); // makes it an island

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

The same component in Remix 3 (from `bench/remix-app/app/public/counter.tsx`):

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

## `Handle` API

| Method | Remix 3 equivalent | Notes |
| --- | --- | --- |
| `update()` | `handle.update()` | Batched on a microtask; parents render before children. |
| `on(\|this, h\| ..)`, `on_event(..)` | `on('click', ..)` | Use with `mix={on("click", ..)}` or `on:click={..}`. |
| `queue_task(..)` | `handle.queueTask` | Runs after the next DOM commit; check `h.render_signal()` for the task signal. |
| `updated().await` | `await handle.update()` | Resolves after the DOM commit with that render's signal. |
| `render_signal()` | the `queueTask` / `update()` signal | Aborted on the next render or on removal; never aborts on the server. |
| `spawn(future)` | async work in setup | Pair with `signal()` to stop when the component is removed. |
| `signal()` | `handle.signal` | An `AbortSignal` aborted on removal. |
| `set_context` / `context` | `handle.context` | Typed by Rust type, not by key. |
| `listen(target, ..)` | `TypedEventTarget` listeners | Removed with the component. |
| `frame()`, `frames()`, `on_frame(..)` | `handle.frame`, `handle.frames` | `reload`, `reload_with`, `replace`, `reloadStart`/`reloadComplete`. |
| `id()`, `dom_id()` | `handle.id` | Stable per instance. |
| `report_error(e)` | throwing from a component | Logs and dispatches to the root's `on_error` listeners (the app's for entries and frames). |

## `rsx!`

A proc macro built on `rstml`. Templates live inside `.rs` files; there is no separate
`.rsx` file format.

1. Lowercase tags are elements, capitalized tags are components (`<RowView id={1}/>` builds `RowViewProps`).
2. Text must be quoted (`"Count: "`); expressions go in braces.
3. `mix={..}` takes one mixin or a list: `on(..)`, `css(..)`, `attrs(..)`, `node_ref(..)`, or a `HostMixin`.
4. `key={..}` enables keyed list reconciliation.
5. Child nodes of a component arrive as `props.children`.

## What Is Implemented

Each Remix 3 package is a crate under `crates/`, and, like the `remix` package with its
subpath exports, the `rmx` crate (`crates/rmx`) puts them behind one dependency: the
component model at the root (`rmx::rsx`, `rmx::Component`), the rest as modules
(`rmx::router`, `rmx::middleware`, `rmx::auth`, `rmx::session`, `rmx::ui`, ...). Server
modules exist only in native builds; a wasm build of the same crate gets the component
runtime plus `ui`, `router`, `fetch`, `spa`, `url`, `headers`, `mime` and
`route_pattern`. Every example depends on `rmx` alone (unpkg also uses
`rmx-html-template` directly, since its `html!` macro expands to that crate's path). The
crates:

1. `rmx-component` (`crates/component`): components, `rsx!`, streaming SSR with hydration markers,
   client entries hydrated by adopting server DOM, keyed reconciliation, `css` mixins,
   host mixins with insert/remove/reclaim lifecycles, `<Frame>` (blocking, non-blocking,
   named, client-mounted, custom resolvers), soft navigation through the Navigation API
   (`data-rmx-target`, `data-rmx-src`, `data-rmx-history`, `data-rmx-reset-scroll`,
   `data-rmx-document`), form submissions, redirects, scroll restoration on traversal,
   app-level error events (`App::on_error`), root-level error events (`Root::on_error`:
   `update()` from `render` or before the first commit, `update()` loops stopped after 50
   cascading renders, `Handle::report_error`, bubbling DOM `error` events), and
   `rmx::test` (`render`, `query`, `query_all`, `act`, `cleanup`), the counterpart of
   `remix/component/test`.
2. Server: `rmx-router` (routes, middleware, nested mounts, and `async_context` for
   reading the request context deeper in the call stack), `rmx-route-pattern`,
   `rmx-fetch` (Request, Response, FormData), `rmx-serve` (hyper server with
   `node-fetch-server`'s options: `host`, `protocol`, `trust_proxy` for `Forwarded` and
   `X-Forwarded-*`, `on_error` with panics answered as 500, and `ClientAddress`; plus an
   HTTP/1 `fetch` client), `rmx-fetch-proxy`, `rmx-render`, `rmx-middleware` (compression with
   br/gzip/deflate negotiation, static files with strong ETags, ranges and directory
   listings, form data with Remix's upload limits, sessions, auth, CSRF, CORS, COP, method
   override, logger with Remix's format tokens and colors), `rmx-auth` (OAuth 2 and OpenID Connect login with PKCE: GitHub,
   Google, X, Facebook, Microsoft, Okta, Auth0 and any OIDC issuer).
3. Supporting crates: `rmx-assets`, `rmx-cookie`, `rmx-crypto`, `rmx-session`,
   `rmx-file-storage`, `rmx-tar-parser`, `rmx-multipart-parser`, `rmx-html-template`,
   `rmx-headers`, `rmx-mime`, `rmx-spa`, `rmx-url`.
4. `rmx-ui`: all ten `@remix-run/ui` modules: accordion, anchor, animation (spring, tween,
   entrance, exit, FLIP layout), combobox, listbox, menu (submenus with hover aim, context
   menus), popover, select, tabs and toggle. The spring CSS output matches Remix's
   `spring.ts` byte for byte (unit tested), and each module has the matching demo in
   `examples/ui` with browser checks.

## Examples

Every Remix 3 demo has a port under `examples/`, each with a Playwright e2e script
(`examples/<name>/e2e.mjs`): assets, bookstore, frame-navigation, frames, i18n,
lazy-frames, social-auth, spa, sse, timeboxer, ui and unpkg. `examples/bench` is the
benchmark app used in `../bench/README.md`.

## Build and Run

Prereqs: Rust stable with the `wasm32-unknown-unknown` target, plus `wasm-bindgen` 0.2.129
and `wasm-opt` (the build script finds the copies cargo-leptos downloaded).

```powershell
./build-client.ps1 -Crate frames-demo -Out examples/frames/public/pkg   # wasm client
cargo run -p frames-demo                                               # http://127.0.0.1:44110/
node examples/frames/e2e.mjs                                            # browser checks
cargo test --workspace
./test-browser.ps1                                                      # component tests in headless Chromium
```

`test-browser.ps1` runs each `tests/browser_*.rs` file of a crate with `wasm-bindgen-test`
(default crate `rmx-component`; `-Test browser_errors` picks one file). It needs a
ChromeDriver matching the Chromium binary: set `CHROMEDRIVER` and `CHROMIUM_PATH`, or use
the defaults under `%LOCALAPPDATA%`.

Server env vars: `HOST` (default `127.0.0.1`), `PORT` (each demo has its own default).
The timeboxer demo needs `SESSION_SECRET`. A Linux image for the benchmark server is in
`../bench/docker/rmx.Dockerfile`.

## Differences From Remix 3

1. No import maps or module preloading: each demo ships one wasm bundle that registers its
   client entries by name (`rmx::entry::<C>()` in `RunOptions::entries`). Wasm sizes range from about 250 KiB to
   820 KiB uncompressed (measured, `wasm-release` plus `wasm-opt`); the largest is the ui
   gallery, which bundles all 17 of its demos.
2. Rust does not throw, and a panic cannot be caught in wasm32 (it aborts the module), so
   a failing `setup`, `render` or task is not turned into an error event the way a
   thrown exception is in Remix. Code reports failures with `Handle::report_error` (to
   the component's root) or `rmx::report_error` (to the app). The runtime reports its own
   errors the way Remix does: `update()` from `render` or before the first commit, and
   update loops, go to `Root::on_error`; panics and failed frame loads and navigations go
   to `App::on_error`.
3. `rmx-ui` events are plain `Event`s with the data set as properties (`rmx:select-change`
   carries `value`, `label` and `optionId`; `rmx:menu-select` carries `item`), read with
   helpers such as `select::select_change`, rather than `Event` subclasses.
4. No HMR, CLI, or `remix/test` runner: native tests use `cargo test`, component tests
   use `wasm-bindgen-test` in Chromium, and e2e checks use Playwright scripts.
5. Data access is out of scope: demos that use Remix's `data-table` use in-memory stores.
6. Some dependencies differ from Remix's zero-dependency stance: ICU4X (i18n), `ureq`
   (social-auth), `nodejs-semver` (unpkg), and the usual hyper/tokio/wasm-bindgen stack.
7. Creating large lists is slower than Remix's JS runtime (see `../bench/README.md`),
   because each DOM node costs several wasm-to-JS calls.

# Remix-style full stack Rust: feasibility and PoC plan

Date: 2026-10-19. Version data below was pulled from the GitHub releases API this session.

## 1. What "acts like Remix" means

"Remix" now names two different things:

1. Remix v1/v2 (merged into React Router v7): file-based nested routes, a `loader` and `action` per route,
   `<Form>` that works without JS, automatic revalidation after mutations, per-route error boundaries,
   `<Outlet/>` layouts, fetchers, streaming with defer. This is the "feel" most people mean.
2. Remix 3 (current `remix-run/remix`, latest `ui@0.12.1` on 2026-10-01): React-free, a set of small
   web-standards packages (fetch router, middleware, `component` with server render and hydration).
   Its stated principles are "build on Web APIs", "religiously runtime" (no reliance on compilers or
   static analysis), and few dependencies.

Recommendation: copy the v2 programming model (loaders, actions, nested routes, forms) and the Remix 3
ethos (server first, HTML and HTTP as the primitives, small composable crates). Accept one deliberate
divergence: a Rust version is compile-time heavy by nature, which is the opposite of "religiously runtime".

## 2. Feasibility verdict

A PoC is very feasible. Almost every hard piece exists already:

| Need | Existing Rust piece | Status (GitHub, measured today) |
| --- | --- | --- |
| HTTP server, middleware | axum + tower | axum 0.8.9 (2026-04) |
| JSX-like templates | Leptos `view!`, Dioxus `rsx!` macros | Leptos 0.8.22 stable, 0.9.0-rc0 (2026-10-15) |
| SSR + hydration + streaming | Leptos (Suspense, out-of-order streaming), Dioxus fullstack | Dioxus 0.8.0-alpha.3 (2026-10-16) |
| Client/server RPC | `server_fn` crate (used by Leptos and Dioxus) | stable |
| Islands (partial hydration) | Leptos `islands` feature | stable |
| Nested routes + Outlet | leptos_router, dioxus router | stable |
| Shared types across the wire | serde on one crate compiled twice | stable |

What does not exist is the Remix layer itself: route-module conventions, `loader`/`action` per route,
parallel loader execution, revalidation after actions, and file-based route discovery. That layer is the
interesting project, and it is a few thousand lines, not a framework from scratch.

## 3. The `.rsx` file question

Rust macros run inside `.rs` files on token streams. A separate `.rsx` extension needs one of:

1. `build.rs` codegen: parse `.rsx`, emit `.rs` into `OUT_DIR`, `include!` it.
2. A proc macro `rsx_file!("routes/home.rsx")` that reads the file at compile time.
3. Single-file components (Svelte/Vue style): a `.rsx` file with a Rust block plus a template block,
   split by a build step.

All three share the same problems:

1. Diagnostics: Rust has no `#line` directive and proc macros cannot create spans pointing into arbitrary
   files on stable. Type errors land in generated code, not in the `.rsx` line the user wrote.
2. rust-analyzer will not understand `.rsx`: no completion, go-to-definition, or inline errors without
   writing an LSP proxy.
3. No syntax highlighting or formatter until you write a TextMate grammar and a formatter.

Recommendation: start with an in-file macro (`view!`/`rsx!`) in `.rs` route modules, which gets full
tooling for free. Treat `.rsx` files as a phase-4 experiment via `build.rs`, and judge it on error quality.
A middle ground is a naming convention (`routes/projects.$id.rsx.rs`) that signals "route module" while
staying a real Rust file.

## 4. Rendering options (is WASM the best way?)

| Option | How it renders | Fit for Remix feel | Main costs |
| --- | --- | --- | --- |
| A. SSR HTML + no client code | Server only, forms do full POST/redirect | High: this is Remix with JS disabled | No client nav, no rich widgets |
| B. SSR + small JS runtime (hand-written or htmx 4) | Server HTML, JS swaps fragments | High | Interactive logic not in Rust |
| C. SSR + WASM islands | Server HTML, only marked components hydrate | Highest | Two builds, island boundaries, glue JS |
| D. SSR + full WASM hydration | Whole page hydrates, client router in WASM | Medium-high (closest to React Remix) | WASM size, hydration cost, slow builds |
| E. LiveView (server diffs over WebSocket) | Dioxus liveview style | Low | Stateful connection, latency, not HTTP-first |
| F. Canvas/WebGPU (Blitz, egui) | Pixels, not DOM | Very low | Accessibility, SEO, text selection, forms |
| G. Compile rsx to JS for the client | Custom compiler | Medium | Essentially writing a new Svelte; months |

Findings:

1. The server should do the rendering. HTML from the server is what makes Remix feel like Remix
   (forms that work without JS, fast first paint, plain HTTP caching).
2. WASM is a good fit for interactivity, not for rendering the page. It cannot touch the DOM directly,
   so every DOM operation goes through JS glue; that is fine for fine-grained signals (Leptos is fast here)
   but full-page hydration costs bandwidth and startup time. Realistic Leptos/Dioxus apps tend to ship
   on the order of 100-300 KB compressed WASM (estimate; measure in phase 0).
3. Per-route code splitting for WASM exists (wasm-split based lazy loading in Leptos and Dioxus) but is
   newer and less proven than JS bundler splitting. Treat it as a stretch goal.
4. Best default: C (SSR + WASM islands), with A working first so progressive enhancement is real,
   not retrofitted. D is an opt-in mode later if you want SPA-style transitions everywhere.

## 5. Proposed architecture

```
app/
  root.rs                 // document shell, <Outlet/>
  routes/
    _index.rs
    projects.rs           // layout for /projects
    projects.$id.rs       // /projects/:id
build.rs                  // scans app/routes, generates route tree
crates/
  rmx-core/               // Route trait, LoaderCtx, ActionCtx, Params, errors
  rmx-macros/             // #[loader], #[action], #[route] attribute macros
  rmx-server/             // axum integration: match, run loaders, render, PRG
  rmx-client/             // (phase 2+) link/form interception, revalidation
```

Route module sketch (built on Leptos for the PoC):

```rust
#[loader]
async fn loader(ctx: LoaderCtx, p: Params<ProjectId>) -> Result<Project, AppError> {
    db::project(ctx.db(), p.id).await
}

#[action]
async fn action(ctx: ActionCtx, form: Form<RenameProject>) -> Result<Redirect, AppError> {
    db::rename(ctx.db(), form.id, &form.name).await?;
    Ok(Redirect::see_other(format!("/projects/{}", form.id)))
}

#[route]
pub fn Page() -> impl IntoView {
    let project = use_loader_data::<Project>();
    view! {
        <h1>{project.name.clone()}</h1>
        <Form method="post">
            <input name="name" value=project.name />
            <button>"Rename"</button>
        </Form>
        <Outlet/>
    }
}
```

Request flow:

1. GET: match URL to the route chain (root > projects > projects.$id), run all loaders concurrently
   (`tokio::join!`/`try_join_all`), render nested components, embed loader data as JSON for hydration.
2. POST: run the matched route's action, then 303 redirect (POST/redirect/GET). Without JS this is a
   normal form post; with JS the client submits via fetch and revalidates.
3. Client navigation: request `?_data=<route ids>` for only the segments that changed, update signals.
4. Errors: each route can export an error boundary; a failed loader renders the nearest one.

The key design problem is server/client code separation. Loader and action bodies pull in DB crates
that do not compile to `wasm32`. The attribute macros must strip their bodies in the client build
(`cfg(feature = "ssr")` vs `cfg(feature = "hydrate")`), the way `#[server]` does in Leptos. Getting this
ergonomic is the core of the PoC.

## 6. Base options (revised after phase 0)

Goal clarified: the part of Remix 3 that matters most is its component model (plain local state,
a render function, explicit `handle.update()`), not just loaders and routes. Neither big Rust
framework works that way:

1. Leptos runs a component once (like Remix 3 setup), but state lives in signals and the DOM updates
   automatically wherever a signal is read. That is fine-grained change tracking.
2. Dioxus re-runs the component function on every render with hooks (`use_signal`), like React.
3. Yew struct components (`create`/`update`/`view`) are the closest existing Rust shape: state in a
   struct, re-render after an update. They are message-enum based and SPA-oriented, so they are prior
   art rather than a base.

Recommendation: own the component runtime (section 9) on top of axum, because it is the piece you
care about. Keep the Leptos spike as a baseline for WASM size, build times and hydration behavior.

## 7. Phased plan (revised)

| Phase | Goal | Done when |
| --- | --- | --- |
| 0. Spike | axum + Leptos SSR, one island | Done; numbers in `spike/README.md` |
| 0b. Component-model spike | `Component` trait, `Handle::update()`, same `render()` used for SSR and a WASM island | Done; `rmx/` plus the Remix 3 comparison in `bench/` (section 10) |
| 1. Routes + loaders (SSR only) | `build.rs` route scan, nested layouts, parallel loaders, loader data passed to route components as props | `/projects/1` renders 3 nested layouts with no WASM shipped |
| 2. Actions + forms | `#[action]`, plain `<form>`, POST/redirect/GET, validation errors | Create/rename works with JS disabled |
| 3. Frame navigation | Small client runtime: intercept links and forms (Navigation API), fetch server HTML, swap it in; named frames | Same app works with JS; navigation and form posts do not reload the document |
| 4. Client entries | `#[client_entry]` components hydrate from serde-serialized props | Page ships WASM only for client entries |
| 5. `.rsx` experiment | `build.rs` transpile of `.rsx` files | Compare error messages vs the in-file macro, decide |
| 6. Stretch | keyed list diffing, hydration that adopts server DOM, streaming, per-island WASM splitting | Each behind a demo route |

Phase 3 replaces the earlier JSON `_data` revalidation idea: Remix 3 navigates by fetching HTML for a
frame, so the client never needs a router or loader code, and WASM stays limited to client entries.

Demo app: a small project/task tracker with SQLite (sqlx), which exercises nested layouts, forms,
validation errors, and one interactive client entry (drag-reorder or live filter).

## 8. Risks

1. Compile times across two targets dominate DX; measure in phase 0 and budget for `cargo-leptos watch`
   or `dx serve --hotpatch`.
2. Framework churn: Leptos 0.9 RC and Dioxus 0.8 alpha both shipped in the last week.
3. Macro ergonomics for server/client stripping can produce confusing errors.
4. `.rsx` tooling (spans, rust-analyzer) is the most likely place to stall; keep it optional.
5. Philosophical mismatch with Remix 3's no-compiler stance; fine for a side project, worth knowing.
6. Hydration that adopts server-rendered DOM is fiddly; start with replace-on-mount.

## 9. Remix 3 component model in Rust

What Remix 3 does (from `packages/component/README.md`): a component receives a handle and returns a
render function; local state stays in ordinary variables; `handle.update()` explicitly schedules
another render. Client entries hydrate selectively, and frames fetch HTML for navigation.

A literal port fights the borrow checker. In JS the render closure and its click handlers share and
mutate `count` freely. In Rust a handler created inside render that mutates a captured local needs
shared mutable ownership, so `count` becomes `Rc<Cell<i32>>` and every use becomes `.get()`/`.set()`.
That loses the plain-variables feel.

Recommended shape: the runtime owns the state; render borrows it immutably and handlers get `&mut`
when they fire. Sketch (API design, not yet compiled):

```rust
#[derive(Serialize, Deserialize)] // props cross the server/browser boundary
pub struct CounterProps { pub initial: i32 }

pub struct Counter { count: i32 }

impl Component for Counter {
    type Props = CounterProps;

    // Runs once per instance, like the Remix 3 setup function.
    fn setup(props: &CounterProps, _h: &Handle<Self>) -> Self {
        Counter { count: props.initial }
    }

    // Runs on first render and after each h.update().
    fn render(&self, h: &Handle<Self>) -> Node {
        rsx! {
            <button on:click={h.handler(|this, h| { this.count += 1; h.update(); })}>
                "Count: " {self.count}
            </button>
        }
    }
}
```

How it runs:

1. Server: `setup` then `render` once, written straight to HTML. No reactivity on the server at all.
2. Browser: each instance lives in a slot (`Rc<RefCell<Counter>>`). An event borrows it mutably, runs
   the handler, and `update()` queues a re-render on a microtask. The runtime diffs the new node tree
   against the old one and patches the DOM through `web-sys`. Render and handlers never overlap, so
   the borrows never conflict.
3. Nothing tracks reads and nothing re-renders unless you call `update()`, which is the Remix 3
   behavior you want.

| Remix 3 | Rust shape |
| --- | --- |
| Locals declared in the setup function | Struct fields returned from `setup` |
| Returned render function | `fn render(&self, h)` |
| `handle.update()` | `h.update()` |
| `handle.props` | `&Self::Props` given to `setup` (an optional hook for new props from a parent is an open question) |
| `clientEntry()` | `#[client_entry]` marks a component as a WASM island |
| Frames and Navigation API interception | Phase 3 client runtime that swaps server HTML |

Open questions for phase 0b:

1. Hydration. Simplest first version: the client re-renders the island and replaces the server HTML
   (identical output, so no visible change). Adopting the existing DOM comes later.
2. Async work (fetches, timers): likely `h.spawn(async move { ... })` with tasks dropped when the
   component is removed, then `h.update()` from inside the task via a weak handle.
3. Optional sugar: a `#[component] fn` macro that lifts `let mut` locals into a generated struct so the
   code reads like the JS version. It hides the borrow rules but gives worse errors; postpone.
4. Size: a runtime without a reactive system should come in under Leptos's 72.5 KB gzip, but that is
   a guess until 0b measures it.

## 10. Phase 0b outcome (rmx vs Remix 3)

Built: `rmx/` (the component runtime, `rsx!` macro, SSR, WASM hydration with DOM adoption, keyed
diffing, delegated events) and `bench/` (a Remix 3.0.0 app with the same pages, markup, data and
islands, plus browser and server benchmarks). Method and full numbers: `bench/README.md`.

Answers to the section 9 open questions:

1. Hydration: adopting the server DOM worked from the start, so replace-on-mount was skipped.
2. Async work: still open; `queue_task` exists, but there is no `spawn` or abort signal yet.
3. `#[component] fn` sugar: not attempted. The struct form reads close enough to the JS version.
4. Size (measured): the rmx client `.wasm` is 127.4 KB raw / 59.4 KB gzip with two islands
   (counter and a 10k-row table), against 72.5 KB gzip for the one-island Leptos spike. With the JS
   glue the page ships 63.9 KB gzip, versus 42.5 KB gzip for the same Remix 3 page.

What the benchmarks say (details and caveats in `bench/README.md`):

1. Server rendering is where Rust clearly wins: one rmx thread out-serves one Node process by
   6x to 45x depending on page and environment (10x to 45x in CPU-pinned Linux containers), and
   in containers rmx idles at about 1 MB versus about 74 MB for Node.
2. In the browser it is mixed. rmx hydrates faster and wins small updates (select, swap, remove,
   update every 10th row), but loses on creating 1k rows and ships more bytes. Each DOM node costs
   several wasm-to-JS calls; batching node creation (templates plus `cloneNode`, or one call that
   builds a subtree from an encoded buffer) is the main lever.

Next phases unchanged: routes and loaders (phase 1), then frames and soft navigation (phase 3),
which is the largest Remix 3 feature rmx does not have.

## 11. Parity status (2026-10-20)

Phases 1 to 3 are done: routes and middleware, frames, and soft navigation all exist, and all
12 Remix 3 demos are ported under `rmx/examples/` with Playwright e2e scripts that mirror the
Remix demo tests where they exist. Details are in `rmx/README.md`.

Covered since phase 0b:

1. Server: router, route patterns, Fetch-style Request/Response, streaming render, the Remix
   middleware set (compression, static, form data, sessions, auth, CSRF, CORS, COP, method
   override, logger), cookies, sessions, file storage, tar parsing, assets, SPA fallback.
2. Browser: `<Frame>` in every Remix mode, Navigation API soft navigation with the
   `data-rmx-*` attributes, form submissions and redirects, `handle.signal`, `spawn`,
   context, typed event targets, host mixins with reclaim, app error events, and scroll
   handling that matches Remix (traversal restore after reconciliation shrinks the page,
   `resetScroll: false`, anchoring suppression).
3. `rmx-ui`, all ten `@remix-run/ui` modules: accordion, anchor, animation (spring output
   matches Remix's `spring.ts` byte for byte), combobox, listbox, menu (submenus, hover aim,
   context menus), popover, select, tabs and toggle. The ui gallery has every Remix ui demo,
   the two `demos/ui` cases (draggable and the Web Audio drummer) and the four component
   runtime demos, each with browser checks.
4. Package-level parity passes (this round): compression (br, gzip and deflate negotiation,
   406, weak ETags, per-chunk flushing for HTML and SSE), static files (index lists,
   directory listings, strong SHA-256 ETags, per-file range support, streamed bodies,
   symlink containment), `rmx-multipart-parser` (the streaming state machine with Remix's
   limits) and form data with Remix's defaults and upload handlers, and `rmx-auth` (OAuth 2
   and OpenID Connect with PKCE: GitHub, Google, X, Facebook, Microsoft, Okta, Auth0 and
   generic OIDC). The social-auth demo now logs in through Google, GitHub and X when their
   client credentials are set. `rmx-serve` has `node-fetch-server`'s listener options (host,
   protocol, trusted proxy headers, error handler, client address) and answers handler
   panics with 500; the logger has Remix's format tokens and colors.
5. Validation: `rmx-data-schema` (`rmx::data_schema`) ports `remix/data-schema`: the
   schema combinators, `pipe`/`refine`/`transform`, checks, coercion, `abortEarly`, error
   maps with locale, and the form-data reader, checked by ports of its test files (31 unit
   tests). Objects build Rust values through a closure instead of returning plain
   objects; JavaScript-only schemas (bigint, symbol, Map, Set, Date) are left out. Every
   form and JSON body a Remix demo validates with `data-schema` is validated with it in
   the rmx port (timeboxer, bookstore, social-auth, sse), except the bookstore admin book
   form, which keeps accepting `on` for its checkbox.
6. Controllers: `router.map(routes, controller)` from `fetch-router`. `#[derive(RouteMap)]`
   on a route struct generates `<Name>Actions` with one `Action` per direct route, so a
   controller that misses a route, names an unknown one or names a nested map fails to
   compile (Remix throws these at map time); controller and action middleware run in
   Remix's order. Every demo now maps controllers the way its `router.ts` does, and
   `router.test.ts` is ported (`crates/router/tests/router.rs`: map, verbs with action
   middleware, 404, 405 with Remix's Allow order and body, HEAD falling back to GET by
   specificity, trailing slashes, mounts), as are `router-abort.test.ts` and
   `request-abort.test.ts`: `request.signal` is an `rmx_fetch::AbortSignal` that `rmx-serve`
   aborts when the client leaves before the response finished, and an aborted request skips
   the rest of the chain and answers 499 with `AbortError` where Remix rejects. The `remix new` starter (`reference/remix/template`)
   is ported as `examples/template` with its controller test and a browser e2e.
   Route helper options that change a map's shape (`only`, `exclude`, `names`) become a
   struct you declare with `#[derive(RouteMap)]` (the bookstore's `ReadOnly` and `Users`).
7. Distribution: the `rmx` crate is the single dependency, like the `remix` package: the
   component model at the root and every other crate as a module (`rmx::router`,
   `rmx::middleware`, `rmx::auth`, ...). Server modules are native-only; wasm builds see
   the component runtime, `ui`, `router`, `fetch`, `spa` and the platform-neutral crates.
   All examples depend on `rmx` alone.

8. Component runtime behavior is checked against Remix's own test suites, ported test by
   test: 515 browser tests in 29 files (`rmx/crates/component/tests/browser_*.rs`, run in
   headless Chromium) cover the vdom, hydration, mixins (on, attrs, ref, css, link, mixin
   SSR options and server/client parity), the style manager, diff-dom, document state
   (focus restore), form navigation, navigation interception, nested client entries,
   frame CSS, child frames during parent reloads, and frame requests and resolvers; native ports cover
   server streaming (`ssr_stream.rs`, `ssr_mixins.rs`, `ssr_hydration.rs`, and the
   frames section in `rmx-render`, where frames resolve through the router). Porting found and fixed real
   gaps: host-element listeners, the constructed stylesheet, link mixin options, focus
   restore, detached form submitters, submitter encodings and `text/plain` bodies, frame
   sources after redirects, and runaway mixin expansion, document-relative origins, deferred removal of
   components whose content persists, frames inside non-blocking fallbacks that never
   loaded, doctype markup and `</template` breakouts in streamed frame content, nested
   frame statuses forced to resolved, missing reload events for frames inside a reloading
   frame, stale templates applied after a reload, and superseded response streams left
   open until their next chunk, and client frames inside client entries that kept stale
   content when an ancestor reload re-rendered the entry, and client frame reloads started
   by a superseded ancestor reload that were left running, and streamed templates cut
   short before their end marker that never applied once the document finished parsing.

Remaining gaps:

1. Import maps and per-entry module loading: rmx ships one wasm bundle per app, so the
   Remix import map and module preloader tests do not apply.
2. Errors thrown inside components: Rust does not throw, and wasm32 panics abort the
   module. Errors are reported explicitly (`Handle::report_error`) and reach
   `Root::on_error` / `App::on_error` like Remix error events.
3. Tooling: hot module replacement (wasm modules cannot be swapped in place), and the
   `remix` CLI's `completion` and `db` commands. The `rmx` binary (`rmx::cli`, in the `rmx`
   crate) has `dev` (Remix's `npm run dev` / `hmr` as rebuild, restart and page reload:
   it polls `src/`, `app/`, `public/` and `Cargo.toml`, builds the client and server, runs
   a copy of the server with `RMX_DEV_LIVE_RELOAD`, and `rmx::middleware::live_reload`
   appends an event-stream script to documents that reloads the page when the build id
   changes; measured on a new app: an edit to the home page showed in the open page 20 s
   later, 17 s of it the server rebuild), `doctor` (Remix's suites and output with rmx
   checks: environment for the `rmx` dependency, the wasm32 target and the pinned
   wasm-bindgen; project for a `RouteMap`; client for `cdylib` and a built, current wasm;
   `--json`, `--strict`; no `--fix`, and no actions suite since controllers are checked at
   compile time), `new` (writes `examples/template` as a
   standalone crate with the app's name, Remix's error codes and progress lines; an app made
   this way passed `cargo test` and the template's 10 browser checks outside the workspace),
   `version`, `help`, and `routes`, `assets` and `test`, which run the app through cargo. The
   `remix/component/test` render helper exists as `rmx::test`; `remix routes` exists as
   `rmx::router::routes::routes_command`, which every demo binary runs for `-- routes`
   (tree, `--table`, `--json`; no controller owner column, since rmx has no controller files);
   and `remix assets` exists as `rmx::assets::assets_command` over the asset server's
   `assets()` / `asset_details()` inspection (list, `inspect <url-or-file>`, with statuses
   reachable, denied, not-allowed, missing and unmapped), run by the assets and template
   demos for `-- assets`.
4. Data access (`data-table`) is intentionally left to existing Rust libraries.
5. Not yet ported: a few `frame.test.tsx` streamed template parsing edge cases that need
   a document still being parsed (`document.write` splitting a template). Cases that load
   entry modules asynchronously (a provider module arriving after its consumer, cached
   modules, `loadModule` errors) do not apply: one wasm bundle holds every entry. Top
   frame reload cases replace the whole page, so the browser tests run them on a named
   parent frame and the demo e2e scripts cover the document itself. Ported this round:
   reload results (`reload_async` resolves to the load's signal or its error), HTML after
   the first flush marker, the resolver's `signal` and streamed content, context through
   frames, and the preserved client entry, shared src, nested and blocking frame reload
   cases. The `stream.test.tsx` cases that apply are ported; import map, module preload
   and `resolveClientEntry` cases do not apply.

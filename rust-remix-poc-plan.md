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
   The navigation listener follows Remix's `navigation.ts`: a frame load started by a
   navigation is aborted with the navigation's signal, `event.scroll()` runs once the new
   content commits (not for a superseded load), WebKit's scroll position is resynchronized
   after a reset, Safari traversals read their state from `navigation.entries()`, frame
   redirects keep `resetFocus` and the stored history state, legacy boolean resets in
   history state are honored, and without `NavigationPrecommitController` a same-location
   POST is replayed as a replace navigation into the frame chosen first. About 60 of the 77
   `navigation.test.ts` cases are covered by the 57 tests in `browser_navigation.rs`
   (some tests combine cases); the rest stub a missing Navigation API or `sourceElement`,
   check internal call arguments, or redirect the top frame, which would replace the test
   page (the demo e2e scripts cover top frame redirects). The two `frame.test.ts` reload
   abort cases run there as navigations stopped with `window.stop()`: the resolver's signal
   aborts and an answer it gives after the abort is not applied, and a streamed response
   is cancelled. The top frame attribute preservation case runs in the frames demo e2e
   (its document uses `data-rmx-preserve-attrs="class data-theme"` like Remix's docs).
   `diff-dom.test.tsx`: 41 tests in `browser_diff_dom.rs`, plus the live-range focus case
   in `browser_frame_reloads.rs`, cover the 43 cases that apply (the pending marker case is
   the resolved child frame case in `browser_frame_reloads.rs`). Live client
   entry owners are attached with `rmx::test::attach_entry_owner` like Remix's
   `setClientEntryBoundaryOwner`, so the hydration range cases (moving live ranges with
   their siblings, pairing repeated ranges by identity in order, balanced boundary lists,
   nested and frame markers owned by a range) run against the real diff. The six HMR URL
   cases do not apply (rmx has no HMR); an entry's identity is its component name.
   `stream.test.tsx` escaping and styles: script text escapes `<script` and `</script` in
   any case as `\u0073` or `\u0053` (JSON and JS read it unchanged) and attribute values
   escape `'`, like Remix. Styles follow Remix's `finalizeHtml`: a document gets them at
   the end of its first `<head>` or in a new `<head>` after the `<html>` start tag, a
   fragment gets them in a `<head>` in front, and the hydration data goes before the last
   `</body>` (else `</html>`, else the end). Empty `css("")` adds no class or rule. The
   styles group runs in `ssr_stream.rs` with CSS text in place of style objects (rsx has
   no style objects; string `style` passes through). `style.test.ts` (4) is ported the same
   way in `crates/component/src/mixin.rs`: `rmxc-` classes, cached identical text, nothing
   for empty text, and nested selectors and media queries kept. Divergence: an inlined blocking
   frame's rules move to the page head, because rmx pins adopted server rules for the page
   instead of adopting them per frame; streamed frame templates still carry their own.
3. `rmx-ui`, all ten `@remix-run/ui` modules: accordion, anchor, animation (spring output
   matches Remix's `spring.ts` byte for byte), combobox, listbox, menu (submenus, hover aim,
   context menus), popover, select, tabs and toggle. The ui gallery has every Remix ui demo,
   the two `demos/ui` cases (draggable and the Web Audio drummer) and the four component
   runtime demos, each with browser checks. The browser-free logic is unit tested one test
   per Remix case: `spring.test.ts` (25), `anchor.test.ts` (15 of 16, against the same mocked
   layout; frame cancellation needs a browser) and `hover-aim.test.ts` (12, with the session
   logic in a clock-driven `Core` the DOM tracker drives). Anchor offsets take `Offset::sized`
   functions of the floating element's size, as Remix's take functions, and hover aim takes
   Remix's optional `onExpire`. `scroll-lock.test.tsx` (3, plus a no-scrollbar case) runs in
   Chromium as `crates/ui/tests/browser_scroll_lock.rs` with Remix's window mocks; calling
   one unlock twice has no counterpart, since a `ScrollLock` is released once, on drop.
   `wait-for-css-transition.test.ts` (4, plus descendant events) runs the same way in
   `browser_wait_for_css_transition.rs` with Remix's mocked `requestAnimationFrame`; its
   containers are removed one by one, since clearing the body would remove the test
   runner's output. `browser-only-mixins.test.tsx` (2) is in `crates/ui/src/lib.rs`, with
   Remix's internal `onKeyDown` and `onOutsideClick` written as plain `on` listeners.
   `animate-mixins.test.tsx` (5) is `browser_animate_mixins.rs`; its `animate` mock returns
   an object on `Animation.prototype`, since rmx checks that a tracked animation is one.
   `animate-layout-mixin.test.tsx` (8) is `browser_animate_layout.rs` with Remix's animate,
   frame and bounding-rect mocks, plus a case that tells the on-screen box of a running
   animation from its natural box, which Remix's sequence mocks cannot. Every `ui` test file
   is now ported or recorded.
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
   maps with locale, and the form-data reader. Its eight test files are ported one per case
   (125 of 149: schema 74 of 92, coerce 16 of 22, form-data 14, parse 9, checks 5, variant
   4, lazy 2, pipe 1); the 24 left out need JavaScript-only values (bigint, symbol, Map,
   Set, Date, sparse arrays, custom iterators, class hierarchies). Objects build Rust values
   through a closure instead of returning plain objects; `Fields::rest` is
   `unknownKeys: 'passthrough'` and `Object::strict` is `'error'`. Every
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
   `router-types.test.ts` (5 of 20, in `crates/router/tests/router_types.rs`: the requests its
   cases send; the rest check TypeScript context inference, which typed `ctx.get::<T>()` keys
   replace),
   `request-context.test.ts` (16 of 25; the rest are JavaScript object and type cases),
   `middleware.test.ts` and `route-map.test.ts` are ported one per case: `Context::headers_mut`
   gives the context its own copy of the request headers, like Remix's `context.headers`.
   The route helper tests (`method`, `form`, `resource`, `resources`: 29 of 31 cases plus
   the type assertions on composed routes) are ported one per case in
   `crates/router/tests/route_helpers.rs`, with `options` and `Prefix::head`/`options`
   added; `form_with(pattern, Method)` is Remix's `formMethod` option. Route helper options
   that change a map's shape (`only`, `exclude`, `names`) become a struct you declare with
   `#[derive(RouteMap)]` and fill from the helper, and the tests check the declared map's
   names, methods, paths and order; the two "both only and exclude" error cases have no
   struct equivalent and are not ported.
   `remix/response`'s `redirect.test.ts` and the init and stream cases of `html.test.ts` are
   ported in `crates/fetch/src/response.rs`: one `ResponseInit` (status, headers) is shared by
   `redirect_with`, `html_with`, `ctx.render_with` and the SPA `render_with`, and a `Location`
   or `Content-Type` in an init replaces the constructor's. `createMixin(setup, { ssr })` is
   `HostMixin` with `mixin_with(args, MixinOptions { ssr })`.
   `render-middleware`'s `render-ui.test.ts` is ported in `crates/render/src/render_ui_tests.rs`
   (12 of 15 cases plus a redirect limit and top frame header case; the client entry and
   styles cases do not apply or run elsewhere, and render errors use frame resolution errors
   since a failed component is omitted). Frame subrequests now follow `render-ui.ts`: the
   original headers minus body, conditional, hop-by-hop and `sec-fetch-*` headers (before,
   only cookies, authorization, language and host), `Accept: text/html` and
   `Accept-Encoding: identity`, `X-Remix-Top-Frame-Src` (renamed from `x-remix-top-frame`)
   honored only on frame requests, cross-origin frames get only the accept and frame headers,
   only 301, 302, 303, 307 and 308 redirect (at most 20), and the original request's abort
   signal, which also cancels the page body. A non-HTML frame response, a failed one without
   a body and too many redirects fail the page body and go to `render(RenderOptions::on_error)`
   (stderr by default) once, from the render that started the request.
   `remix/spa`: `spa.test.browser.tsx`'s resolver cases run natively in `crates/spa` against
   `resolve` (the runtime cases, initial URL and interactive fallback, are covered by the spa
   demo e2e). SPA form submissions now follow `getRequestBody`: `multipart/form-data`,
   `text/plain` (`name=value\r\n` lines) or URL encoding, with `\r\n` line breaks, and a
   POST that redirects to GET drops its content type. `rmx_spa::render(transform)` is Remix's
   `render(transform)` middleware, and a redirect without `Location` is the response.
   `vdom.insert-remove.test.tsx` is ported (`browser_insert_remove.rs`); `bool` children
   render nothing. Not ported as files: `jsx.test.tsx`, `vnode.test.ts` and
   `create-element.test.ts` (TypeScript types and JavaScript value validation that Rust's
   types enforce), `module-preloader` and `client-entry-boundary` (module ids; rmx ships one
   wasm bundle), `remix/assert` (Rust's `assert!`) and the `assets` module transform tests
   (`target`, `module-store`, `loaders`, `scripts/cjs-check` and `scripts/transform`: browser
   targets, the module graph and HMR, Node load hooks and CommonJS detection, none of which
   rmx assets has, since it does not transform JavaScript). `mime/scripts/codegen.test.ts`
   checks npm codegen output; rmx's table is generated once by `crates/mime/scripts/codegen.mjs`.
   `file.test.ts` is ported case by case (`crates/fetch/tests/response_file.rs`, 85 cases)
   against `response::file`, Remix's `createFileResponse` for any `FileLike` (an in-memory
   `File`, or a `LazyFile`, which streams only the requested range): ranges only for `GET`,
   an empty type sends no `Content-Type`, and strong ETags take a `DigestAlgorithm`
   (SHA-1/256/384/512, hand-written in `rmx-crypto`) or `Digest::with(fn)` of the file; an
   unknown algorithm name fails to parse instead of rejecting. Static files are rebuilt on
   it as Remix's `static.ts` is (`open_lazy_file` plus `response::file`), with
   `static.test.ts` ported case by case (43, symlinks included); `file_response` and
   `file_response_for` are gone.
   `remix/terminal` is `rmx-terminal` with its test files ported one per case (18 of 20; the
   entrypoint re-export and `getDefaultEnvironment` cases do not apply); the logger's colors and
   detection now come from it. Cursor helpers take unsigned and signed integers, so Remix's
   NaN and fraction normalization has nothing to port. The `rmx` CLI colors `new` progress and
   `doctor` like Remix's reporter (gray steps, green `✓`, red `✗`, yellow warnings) when
   detection allows; it closes each style rather than resetting, so the bytes differ from
   Remix's while the rendering matches.
   Compression now fails the compressed body when the source or encoder fails (it used to
   end early and look complete), with tests for that, source cancellation on drop and the
   206 skip. `remix/response/compress` now lives in `rmx_fetch::response::compress` (feature
   `compress`, re-exported by the compression middleware) with `compress_response`,
   `create_compression_options` and `compress_stream` over a `Compressor` trait, and
   `compress.test.ts` is ported case by case (83). The "Compressor interactions" cases run
   against a scripted `Compressor`: events are the pieces `write` and `end` return, an error
   event is an `Err`, `destroy()` is `Drop`, and Node's `drain` is the new
   `BodySender::ready()`, so a source chunk is read only after the previous output was
   taken (before, a fast source was read to the end in one poll). The middleware's own
   `compression.test.ts` (9) is ported too.
   `node-fetch-server`'s `request-listener.test.ts` (52) and `request-listener.http2.test.ts`
   (12) are ported case by case against real loopback servers (raw TCP and HTTP/2 clients
   for Node's mocked request events). Porting changed `rmx-serve`: request bodies are read
   by a task at most 16 KiB ahead of the handler (Node's `highWaterMark`), so a client that
   leaves mid-upload aborts the signal while the handler is not reading, and reads then fail
   with `AbortError` (a malformed body keeps its own error); a handler that panics after the
   request aborted is not reported to `on_error` (`isRequestAbortError`); a response body
   that fails after headers were sent is reported as `ServeError::ResponseBody`. `rmx-url`
   now serializes IPv6 hosts as WHATWG URL does (`[2001:db8::1]`), so equivalent `Host` and
   `:authority` values match. `node-fetch-server/test`'s `createTestServer` is
   `rmx::serve::test::create_test_server(router)`: a server on a free `127.0.0.1` port with
   `base_url` and `close()` (dropping it closes it too). Remix's `remix/test` runner itself
   (describe/it, mocks, fake timers, coverage, watch mode) is not ported: `cargo test` is the
   runner, browser tests use the wasm-bindgen harness and Playwright scripts, and mocks and
   fake timers are well served by existing crates.
   `remix/headers` tests are ported for accept, accept-encoding, accept-language, range,
   content-range, vary, if-match, if-none-match, if-range, cache-control and content-type
   (`crates/headers/tests/`). Porting changed behavior: `AcceptEncoding::accepts` always
   passes `identity` like Remix (`accepts_encoding` is gone; compression negotiates on
   `weight("identity")`), `Range::can_satisfy` rejects hand-built ranges without bounds or
   with start > end (`normalize` used to panic on them), and `Content-Range` with an empty
   unit and `Content-Type` with empty media type, charset or boundary leave those parts out.
   Header lists collect from and iterate over their values.
   The rest of the package is ported too: content-disposition (`filename*` decodes through
   the WHATWG labels for UTF-8, UTF-16, windows-1252, which `latin1` also names, and
   ISO-8859-15, falling back to the percent-decoded text), cookie, set-cookie (`Max-Age`
   reads like `parseInt`), param-values, raw-headers (`rmx_headers::raw::{parse,
   stringify, canonical_header_name}` over `http::HeaderMap`; `parse` returns an error
   where `Headers` would throw) and super-headers. `SuperHeaders` is a trait on
   `HeaderMap` rather than a subclass: `typed::<H>()`, `set_typed`, `update(|h| ...)`,
   `apply` / `apply_typed` with Remix's merge rules (`Set-Cookie` added, `Cookie` and
   `Vary` merged, list headers appended, others replaced), `get_joined` for
   `Headers.get`, and number, date, list, ETag and `Set-Cookie` helpers. Nothing is
   cached, so the lazy-parse, cache identity, proxy and stale-value cases have no
   counterpart; an edit that prints empty is not kept for the next edit the way Remix's
   cached object keeps it.
   `remix/mime` is ported with its five test files (`crates/mime/tests/`): the 1239-entry
   extension table and compressible list come from Remix's generated mime-db tables
   (`crates/mime/scripts/codegen.mjs`), with `detect_mime_type`, `detect_content_type`,
   `mime_type_to_content_type`, `is_compressible_mime_type` and `define_mime_type` (custom
   types, multi-part extensions, charset and compressibility overrides, `reset_mime_types`).
   Static files, compression, assets and the unpkg demo use it; `.xml` is now `text/xml`
   and `.ts` is `video/mp2t`, as in Remix.
   `remix/route-pattern` is ported with its tests (split, parse, serialize, route-pattern,
   join, specificity, href, match and match/limits; 355 cases in
   `crates/route-pattern/src/tests` and `crates/route-pattern/tests`). The umbrella
   package's `manifest.test.ts` maps to `crates/rmx/tests/manifest.rs`: every library crate is
   an `rmx` dependency, re-exported as `rmx::<name>` (the component model at the root) and
   listed in the crate docs, which caught `rmx::terminal` missing from the table; the README,
   guide, side-effect and package.json export checks are npm tooling. The type-level
   `types/stringify.test.ts` (13) and `types/join.test.ts` (31) run as parse, serialize and
   join checks; `types/split`, `types/parse` and `types/params` test TypeScript inference of
   the grammar the runtime split and parse tests already cover, and rmx params are runtime
   values, so they do not apply. The parser reports
   Remix's ten `ParseError` types with UTF-16 indexes and normalizes sources the same way;
   static text is decoded and canonicalized; matching runs compiled part programs behind
   static prefix, suffix and anchor indexes with the `maxPatternSize`, `maxMatcherSize` and
   `maxMatchWork` limits (exact work counts match Remix); `specificity` has `compare`,
   `less_than`, `greater_than`, `equal`, `ascending` and `descending`; hrefs return
   `HrefError` with Remix's detail types (`kind()`) and relative output for a same-origin
   `base_url`. The router's HEAD fallback now stops at the first match that is not
   `specificity::equal`, as in Remix's `router.ts`. Divergences: URLs are parsed (and
   relative references joined) with `rmx_url` before matching rather than passing a
   string and `baseURL`; the missing-params message has no `Params:` JSON line;
   `Params` leaves an unmatched optional out instead of holding `undefined`; `rmx_url`
   does no IDNA, so the punycode tests feed URLs computed by Node's WHATWG `URL`; and the
   TypeScript type tests have no counterpart. `rmx_url` paths now use the WHATWG path
   percent-encode set, so a lone `%` and lowercase escapes stay as written. The rewrite
   grew the spa client wasm from 480.2 to 532.1 KiB and timeboxer from 600.1 to 620.5 KiB
   (measured after rebuild77).
   `remix/cookie` is ported with `cookie.test.ts` (`crates/cookie/tests/cookie.rs`). This
   changed behavior to match Remix: cookies default to `Path=/` and `SameSite=Lax`, values
   are base64 of their UTF-8 bytes by default (`hello world` is `aGVsbG8gd29ybGQ=`, and a
   value that is not valid base64 or UTF-8 reads as `None`), `encode`/`decode` replace the
   codec (the lazy-frames theme cookie uses the identity codec as in Remix), `Set-Cookie` is
   printed by `rmx_headers::SetCookie` in Remix's attribute order, and `Expires` and
   `Partitioned` (which forces `Secure`) are supported. `serialize_with` takes
   `CookieProperties` overrides; `clear()` is gone, and the demos delete cookies the way
   theirs do (`serialize('', { maxAge: 0 })`). Session middleware now honors an `Expires`
   cookie as an absolute expiry rounded to the second, as Remix's does. Lowercase
   `sameSite` strings have no counterpart: `SameSite` is an enum.
   `remix/session` and `remix/session-middleware` run their Remix tests (41 session and
   storage cases in `crates/session/tests/`, 23 middleware cases in
   `crates/middleware/src/session_remix_tests.rs`, with `Date.now` mocked by a test-only
   clock). Errors and warnings use Remix's text. Remix checks after `next()` that the
   context still holds its session; rmx contexts move down the chain, so the middleware
   stores the session with `Context::set_sealed` instead, and a later `set` or `get_mut`
   of `Session` panics with the same message. Logging the `httpOnly: false` warning is
   checked through the message builder, since tests cannot capture stderr in-process.
   The middleware packages run their Remix tests as integration tests under
   `crates/middleware/tests/`: `cors` (21 cases, RegExp and boolean origin resolvers written
   as `origin_fn` closures), `csrf` (14), `cop` (15, every configuration message checked),
   `auth` (32 across `auth`, `requireAuth` and the bearer, API key and session schemes;
   `auth-types.test.ts` is TypeScript inference, which `Auth<I>` gives for free) and
   `form_data` (21); `logger` (10) and `html-template` (17) are one test per Remix case in
   place. `method-override.test.ts` (1 case) is in `crates/middleware/src/method_override.rs`, and
   the middleware now matches Remix: it overrides any request that carries a form (not only
   POST), uses the first `_method` value only when it is text, uppercases it without trimming,
   and accepts only the router's methods (`isRequestMethod`). `async-context.test.ts` (1 case)
   is `async_context_follows_the_request_into_helpers`; `get_context()` returns `None` where
   Remix's `getContext()` throws. Changes they forced: Remix's error text throughout, the bearer and API key schemes
   default to `Authorization` and `X-Api-Key` and read the header trimmed as fetch `Headers`
   does, `SchemeResult::failure()` is Remix's empty `{ status: 'failure' }`, and the file
   storage directory error reads `Path ".." is not a directory`. `remix/file-storage` runs
   `memory.test.ts` and `fs.test.ts` (15 cases, with the form-data-parser upload
   integration). `form-data-parser` (31 cases, `crates/fetch/tests/form_data.rs`) changed
   `FormData::parse`: media types other than URL-encoded and multipart (and a missing
   `Content-Type`) are a `FormDataError::Parse` before the body is read, so `text/plain`
   bodies are no longer parsed; upload handlers may return a `Result`, whose error ends
   parsing as `FormDataError::Upload` (the middleware answers 500, or an empty form with
   `suppress_errors`, as Remix's rethrow would); and `File::new` lowercases its type like
   the web `File`. Where Remix's middleware throws, rmx answers 413, 400 or 500.
   The parsers and the proxy run their Remix tests one per case: `multipart-parser` (28 of
   29 `multipart.test.ts` cases; the Web Encoding globals case has no Rust counterpart, plus
   39 request, 6 Node and 6 buffer-search cases; an empty body is now reported before a
   missing boundary), `tar-parser` (21 `tar.test.ts` cases with the lodash and npm fixtures,
   49 name and 70 of 73 size cases; the other three need values the types rule out; bodies
   grow from received bytes, and `TarParser::parse` reuses one parser across archives) and
   `fetch-proxy` (42 cases against real `rmx_serve` servers; a new `redirect` option,
   `Manual` by default, follows redirects with fetch's rules when set to `Follow`).
   `remix/auth` runs all 17 test files (50 cases, `crates/auth/tests/`), including the
   browser OIDC flow against a fake provider on a loopback port. They forced:
   `OAuthProvider::Tokens` so a custom provider keeps its own token fields through callback
   and refresh; `authorization_url` taking the transaction mutably so a provider can store
   `provider_state`; `OidcProvider::map_profile` (Remix's `mapProfile`, given the callback URL
   where Remix gives the context); `expiresAt` serialized as an ISO string; GitHub profiles
   that tell `null` from a missing `name` or `email`; and credentials `verify` receiving the
   context and free to return a `Result`. A discovery failure that rejects Remix's
   `router.fetch` is an `AuthError` the route turns into a response.
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
   `createScheduler` has no separate constructor: Remix passes one scheduler to related
   roots so their updates batch together, and rmx has one scheduler per wasm thread that
   every root and frame shares (`rmx::flush()` is `scheduler.dequeue()`). Its phase events
   and task queues are runtime plumbing that rmx exposes as mixin `on_before_update` /
   `on_commit` and `Handle::queue_task`, with the same 50-update loop guard and warning.

Remaining gaps:

1. Import maps and per-entry module loading: rmx ships one wasm bundle per app, so the
   Remix import map and module preloader tests do not apply.
2. Errors thrown inside components: Rust does not throw, and wasm32 panics abort the
   module. Errors are reported explicitly (`Handle::report_error`) and reach
   `Root::on_error` / `App::on_error` like Remix error events. A render returns
   `h.fail(error)` where Remix's would throw: the error is reported, an update keeps the
   previous DOM and a first render shows nothing, as in `vdom.errors.test.tsx` (setup
   failures are kept as state and failed in render; `browser_errors.rs` 17). On the
   server a failed component is reported and left out, where Remix rejects the stream.
3. Tooling: hot module replacement (wasm modules cannot be swapped in place), and the
   `remix` CLI's `db` command (data access is out of scope). The `rmx` binary (`rmx::cli`, in
   the `rmx` crate) has `completion` (Remix's bash/zsh script and `completion -- <index>
   <words>` protocol, with the global `--no-color` flag offered like Remix's; checked in Git Bash), the global `--no-color` (before any `--`; commands handed to cargo get `NO_COLOR=1` and `CARGO_TERM_COLOR=never`), `dev` (Remix's `npm run dev` / `hmr` as rebuild, restart and page reload:
   it polls `src/`, `app/`, `public/` and `Cargo.toml`, builds the client and server, runs
   a copy of the server with `RMX_DEV_LIVE_RELOAD`, and `rmx::middleware::live_reload`
   appends an event-stream script to documents that reloads the page when the build id
   changes; measured on a new app: an edit to the home page showed in the open page 20 s
   later, 17 s of it the server rebuild), `doctor` (Remix's suites and output with rmx
   checks: environment for the `rmx` dependency, the wasm32 target and the pinned
   wasm-bindgen; project for a `RouteMap`; client for `cdylib` and a built, current wasm;
   `--json`, `--strict`, and `--fix` like Remix's `applyDoctorFixPlans`: it adds the `rmx`
   dependency, creates or fills an empty `src/routes.rs` and declares it in `lib.rs`, and
   adds `cdylib` to the crate types, making each file once, skipping files that exist and
   refusing paths that leave the app root, even through symlinks, then checks again and
   fails on what is left; no actions suite since controllers are checked at compile time,
   and no `--config` or `remix.json`), the global `--no-color`, Remix's terminal
   preamble (a blank line, then on a terminal `RMX v0.1.0 - doctor` with one color per
   letter) and closing ANSI reset on stdout or else stderr when either is a terminal that is
   not `TERM=dumb`, Remix's status channel (on such a terminal a running step redraws
   `• Label.`, `..`, `...` every 250 ms and its result replaces it; elsewhere stable lines),
   with `doctor` reporting each suite as it runs, help colored like Remix's `formatHelpText` on a color terminal (headings
   bold light blue; flags, `[names]` and `<values>` in usage, examples and option labels light
   yellow; help under an error follows stderr), `new` (writes `examples/template` as a
   standalone crate with the app's name, Remix's error codes and progress lines; an app made
   this way passed `cargo test` and the template's 10 browser checks outside the workspace),
   `version`, `help`, and `routes`, `assets` and `test`, which run the app through cargo. The
   `remix/component/test` render helper exists as `rmx::test`; `remix routes` exists as
   `rmx::router::routes::routes_command`, which every demo binary runs for `-- routes`
   (tree, `--table`, `--json`; no controller owner column, since rmx has no controller files;
   the table pads every cell like Remix's reporter and bolds the headers on a color terminal);
   and `remix assets` exists as `rmx::assets::assets_command` over the asset server's
   `assets()` / `asset_details()` inspection (list, `inspect <url-or-file>`, with statuses
   reachable, denied, not-allowed, missing, unmapped and unsupported), run by the assets and template
   demos for `-- assets`. The asset server's mounts compile through `rmx::assets::routes`
   (Remix's `compileRoutes`, with `paths` for Windows, UNC and POSIX file paths): mount keys
   canonicalize as URL pathnames, absolute directories, query strings, fragments, encoded dot
   segments and overlapping roots are rejected with Remix's messages, and file names are
   percent-encoded like `encodeURIComponent`. `routes.test.ts` (20), `paths.test.ts` (9) and
   the fingerprint format and parse cases of `fingerprint.test.ts` are ported one per case;
   the module graph, script transforms and CommonJS checks are the TypeScript side of
   Remix's asset server, which rmx apps (Rust compiled to wasm) do not have.
   The server itself follows Remix's `createAssetServer`: it serves scripts (`.js`, `.mjs`),
   stylesheets and file assets (`files.extensions`, plus `.wasm`); other files, missing or
   denied files and stale or missing fingerprints return `None` so the router continues;
   request transforms take `name` or `name:param` with Remix's validation and messages,
   run before the global transforms (which may skip by returning `None`), and can be cached
   through a `FileCache` keyed by `cache_key`; malformed transform requests are 400s and
   other failures go to `on_error` with Remix's error codes and become a 500; CSS
   `@import`s are checked for cycles, and `url()` transform requests keep their other query
   parameters and fragments; content hashes are Remix's (six characters of base64url
   SHA-256); `preloads()` takes several roots of mixed kinds, shallowest first. 111 cases of
   `asset-server.test.ts` are ported (`crates/assets/tests/asset_server.rs`); the rest cover
   TypeScript and JSX compilation, source maps, import maps, package and bare imports, barrel
   files, module loaders, watch mode and HMR. One divergence: fingerprinted scripts import
   fingerprinted URLs directly instead of through an import map.
   `FsFileCache` is Remix's `createFsFileCache`: an LRU of checksummed records in
   `FsFileStorage` with entry, per-file and total byte limits, a `pending` marker that
   discards records after an interrupted write, and an index rebuilt from write times on
   restart; `MemoryFileStorage` and `FsFileStorage` also work as caches. All 24
   `cache.test.ts` cases are ported (`tests/fs_file_cache.rs`, the default directory case in
   its own binary and the overlapping read case as a unit test). `FileCache` methods return
   `io::Result` and a failure reaches `on_error` as `EMIT_FAILED`; the default directory is
   `target/.cache/rmx/assets` rather than `node_modules/.cache/remix/assets`. The assets demo
   caches transformed SVGs this way, like Remix's.
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

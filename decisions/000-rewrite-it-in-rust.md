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
  rsmix-core/               // Route trait, LoaderCtx, ActionCtx, Params, errors
  rsmix-macros/             // #[loader], #[action], #[route] attribute macros
  rsmix-server/             // axum integration: match, run loaders, render, PRG
  rsmix-client/             // (phase 2+) link/form interception, revalidation
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
| 0. Spike | axum + Leptos SSR, one island | Done; numbers in `bench/leptos-spike/README.md` |
| 0b. Component-model spike | `Component` trait, `Handle::update()`, same `render()` used for SSR and a WASM island | Done; `rsmix/` plus the Remix 3 comparison in `bench/` (section 10) |
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

## 10. Phase 0b outcome (rsmix vs Remix 3)

Built: `rsmix/` (the component runtime, `rsx!` macro, SSR, WASM hydration with DOM adoption, keyed
diffing, delegated events) and `bench/` (a Remix 3.0.0 app with the same pages, markup, data and
islands, plus browser and server benchmarks). Method and full numbers: `bench/README.md`.

Answers to the section 9 open questions:

1. Hydration: adopting the server DOM worked from the start, so replace-on-mount was skipped.
2. Async work: still open; `queue_task` exists, but there is no `spawn` or abort signal yet.
3. `#[component] fn` sugar: not attempted. The struct form reads close enough to the JS version.
4. Size (measured): the rsmix client `.wasm` is 127.4 KB raw / 59.4 KB gzip with two islands
   (counter and a 10k-row table), against 72.5 KB gzip for the one-island Leptos spike. With the JS
   glue the page ships 63.9 KB gzip, versus 42.5 KB gzip for the same Remix 3 page.

What the benchmarks say (details and caveats in `bench/README.md`):

1. Server rendering is where Rust clearly wins: one rsmix thread out-serves one Node process by
   6x to 45x depending on page and environment (10x to 45x in CPU-pinned Linux containers), and
   in containers rsmix idles at about 1 MB versus about 74 MB for Node.
2. In the browser it is mixed. rsmix hydrates faster and wins small updates (select, swap, remove,
   update every 10th row), but loses on creating 1k rows and ships more bytes. Each DOM node costs
   several wasm-to-JS calls; batching node creation (templates plus `cloneNode`, or one call that
   builds a subtree from an encoded buffer) is the main lever.

Next phases unchanged: routes and loaders (phase 1), then frames and soft navigation (phase 3),
which is the largest Remix 3 feature rsmix does not have.

## 11. Parity status (2026-10-20)

Phases 1 to 3 are done: routes and middleware, frames, and soft navigation all exist, and all
12 Remix 3 demos are ported under `demos/` with Playwright e2e scripts that mirror the
Remix demo tests where they exist. Details are in `packages/rsmix/README.md`.

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
   POST is replayed as a replace navigation into the frame chosen first. Upstream's
   2026-10-20 scroll fixes are ported: on iOS Safari, restorations of the previous position
   (or of a bottom the new page has had) are undone after the reset until 500 ms after the
   navigation finishes, the user interacts, or another navigation starts (9 cases in
   `browser_navigation.rs`), and a traversal keeps scroll anchoring off until the second
   frame after it finishes, dropping only its `min-height` when it finishes. Without the
   Navigation API or `NavigateEvent.sourceElement`, `navigate()` falls back to a document
   navigation that pushes or replaces like Remix's (3 cases, run with the API hidden), and
   the listeners are not started at all (2 cases). 83 of the 87 `navigation.test.ts` cases
   (upstream `2968b1e`) are covered by the 83 tests in `browser_navigation.rs` (some tests
   combine cases; dialog, `_blank` and download-request cases dispatch synthetic navigate
   events like Remix's). The other four load or redirect the top frame, which would replace
   the test page: the top frame redirect URL replacement, the public destination when the
   top frame ignores a source override, legacy state kept across a traversal redirect, and
   a marked GET form replaced across a redirect (run here without the redirect). The demo
   e2e scripts cover top frame redirects. Upstream's umbrella default-export fix
   (`remix/assert`, `remix/headers`) is JavaScript module packaging and does not apply. The two `frame.test.ts` reload
   abort cases run there as navigations stopped with `window.stop()`: the resolver's signal
   aborts and an answer it gives after the abort is not applied, and a streamed response
   is cancelled. The top frame attribute preservation case runs in the frames demo e2e
   (its document uses `data-rmx-preserve-attrs="class data-theme"` like Remix's docs).
   `diff-dom.test.tsx`: 41 tests in `browser_diff_dom.rs`, plus the live-range focus case
   in `browser_frame_reloads.rs`, cover the 43 cases that apply (the pending marker case is
   the resolved child frame case in `browser_frame_reloads.rs`). Live client
   entry owners are attached with `rsmix::test::attach_entry_owner` like Remix's
   `setClientEntryBoundaryOwner`, so the hydration range cases (moving live ranges with
   their siblings, pairing repeated ranges by identity in order, balanced boundary lists,
   nested and frame markers owned by a range) run against the real diff. The six HMR URL
   cases do not apply (rsmix has no HMR); an entry's identity is its component name.
   `stream.test.tsx` escaping and styles: script text escapes `<script` and `</script` in
   any case as `\u0073` or `\u0053` (JSON and JS read it unchanged) and attribute values
   escape `'`, like Remix. Styles follow Remix's `finalizeHtml`: a document gets them at
   the end of its first `<head>` or in a new `<head>` after the `<html>` start tag, a
   fragment gets them in a `<head>` in front, and the hydration data goes before the last
   `</body>` (else `</html>`, else the end). Empty `css("")` adds no class or rule. The
   styles group runs in `ssr_stream.rs` with CSS text in place of style objects (rsx has
   no style objects; string `style` passes through). `style.test.ts` (4) is ported the same
   way in `packages/component/src/mixin.rs`: `rmxc-` classes, cached identical text, nothing
   for empty text, and nested selectors and media queries kept. Divergence: an inlined blocking
   frame's rules move to the page head, because rsmix pins adopted server rules for the page
   instead of adopting them per frame; streamed frame templates still carry their own.
3. `rsmix-ui`, all ten `@remix-run/ui` modules: accordion, anchor, animation (spring output
   matches Remix's `spring.ts` byte for byte), combobox, listbox, menu (submenus, hover aim,
   context menus), popover, select, tabs and toggle. The ui gallery has every Remix ui demo,
   the two `demos/ui` cases (draggable and the Web Audio drummer) and the four component
   runtime demos, each with browser checks. The browser-free logic is unit tested one test
   per Remix case: `spring.test.ts` (25), `anchor.test.ts` (15 of 16, against the same mocked
   layout; frame cancellation needs a browser) and `hover-aim.test.ts` (12, with the session
   logic in a clock-driven `Core` the DOM tracker drives). Anchor offsets take `Offset::sized`
   functions of the floating element's size, as Remix's take functions, and hover aim takes
   Remix's optional `onExpire`. `scroll-lock.test.tsx` (3, plus a no-scrollbar case) runs in
   Chromium as `packages/ui/tests/browser_scroll_lock.rs` with Remix's window mocks; calling
   one unlock twice has no counterpart, since a `ScrollLock` is released once, on drop.
   `wait-for-css-transition.test.ts` (4, plus descendant events) runs the same way in
   `browser_wait_for_css_transition.rs` with Remix's mocked `requestAnimationFrame`; its
   containers are removed one by one, since clearing the body would remove the test
   runner's output. `browser-only-mixins.test.tsx` (2) is in `packages/ui/src/lib.rs`, with
   Remix's internal `onKeyDown` and `onOutsideClick` written as plain `on` listeners.
   `animate-mixins.test.tsx` (5) is `browser_animate_mixins.rs`; its `animate` mock returns
   an object on `Animation.prototype`, since rsmix checks that a tracked animation is one.
   `prefers_reduced_motion()` is Remix's new `prefersReducedMotion` (upstream 2026-10-21), with
   `prefers-reduced-motion.test.ts` (3) in `browser_prefers_reduced_motion.rs`.
   `typeahead.test.tsx`: the two matching cases are unit tests in `src/typeahead.rs` and the
   four `hidden_typeahead` cases are `browser_typeahead.rs` (the timeout case waits the real
   750 ms where Remix advances fake timers).
   `animate-layout-mixin.test.tsx` (8) is `browser_animate_layout.rs` with Remix's animate,
   frame and bounding-rect mocks, plus a case that tells the on-screen box of a running
   animation from its natural box, which Remix's sequence mocks cannot. `toggle.test.tsx` (3)
   is `browser_toggle.rs` and `tabs.test.tsx` (9) is `browser_tabs.rs` plus the server case in
   `src/tabs.rs`, `accordion.test.tsx` (10) is `browser_accordion.rs`, and `popover.test.tsx` (10) is
   `browser_popover.rs` with Remix's harness, layout mock and popover method spies.
   `select.test.tsx` (11; its two `it.todo` cases have no body) is `browser_select.rs`, waiting
   in real time where Remix advances fake timers. `listbox.test.tsx` (19) is
   `browser_listbox.rs`, plus an rsmix case for the ref's `select` skipping disabled options
   and re-entry while settling. Porting it found that an explicit `None` prop
   (`AttrValue::Absent`) blocked mixin defaults (`tabindex={None}` left the list without
   `tabindex="-1"`); `Element::attr` and `attrs(..)` now treat `Absent` as unset, like
   Remix's `undefined`. `menu.test.tsx` (23) is `browser_menu.rs`; Remix's
   `setTimeout` spies wrap `window.setTimeout` and the 60 ms flash is a real wait. It found
   two bugs: the submenu open delay started its timer on the spawned future's first poll
   rather than in the focus or pointer handler (now `event::timer`, started synchronously
   like Remix's `window.setTimeout`), and an element unmounted with its subtree kept its
   listener and handler id, so a late event there (the `toggle` that `hidePopover` queues)
   ran whichever handler had since reused the freed slot (a menu item's `click`). Listener
   expandos now carry a slot generation that `release` bumps; `browser_on_mixin.rs` covers
   it. `combobox.test.tsx` (25) is `browser_combobox.rs`: where Remix stops fake timers
   between the flash, the close transition and the 50 ms input commit, the tests poll until
   the surface closes (the two-frame transition wait leaves time to observe the closing
   state) and wait past the commit delay in real time. All 18 `@remix-run/ui` test files
   now have case-by-case ports (`anchor.test.ts` at 15 of 16, as above).
4. Package-level parity passes (this round): compression (br, gzip and deflate negotiation,
   406, weak ETags, per-chunk flushing for HTML and SSE), static files (index lists,
   directory listings, strong SHA-256 ETags, per-file range support, streamed bodies,
   symlink containment), `rsmix-multipart-parser` (the streaming state machine with Remix's
   limits) and form data with Remix's defaults and upload handlers, and `rsmix-auth` (OAuth 2
   and OpenID Connect with PKCE: GitHub, Google, X, Facebook, Microsoft, Okta, Auth0 and
   generic OIDC). The social-auth demo now logs in through Google, GitHub and X when their
   client credentials are set. `rsmix-node-fetch-server` has `node-fetch-server`'s listener options (host,
   protocol, trusted proxy headers, error handler, client address) and answers handler
   panics with 500; the logger has Remix's format tokens and colors.
5. Validation: `rsmix-data-schema` (`rsmix::data_schema`) ports `remix/data-schema`: the
   schema combinators, `pipe`/`refine`/`transform`, checks, coercion, `abortEarly`, error
   maps with locale, and the form-data reader. Its eight test files are ported one per case
   (125 of 149: schema 74 of 92, coerce 16 of 22, form-data 14, parse 9, checks 5, variant
   4, lazy 2, pipe 1); the 24 left out need JavaScript-only values (bigint, symbol, Map,
   Set, Date, sparse arrays, custom iterators, class hierarchies). Objects build Rust values
   through a closure instead of returning plain objects; `Fields::rest` is
   `unknownKeys: 'passthrough'` and `Object::strict` is `'error'`. Every
   form and JSON body a Remix demo validates with `data-schema` is validated with it in
   the rsmix port (timeboxer, bookstore, social-auth, sse), except the bookstore admin book
   form, which keeps accepting `on` for its checkbox.
6. Controllers: `router.map(routes, controller)` from `fetch-router`. `#[derive(RouteMap)]`
   on a route struct generates `<Name>Actions` with one `Action` per direct route, so a
   controller that misses a route, names an unknown one or names a nested map fails to
   compile (Remix throws these at map time); controller and action middleware run in
   Remix's order. Every demo now maps controllers the way its `router.ts` does, and
   `router.test.ts` is ported (`packages/fetch-router/tests/router.rs`: map, verbs with action
   middleware, 404, 405 with Remix's Allow order and body, HEAD falling back to GET by
   specificity, trailing slashes, mounts), as are `router-abort.test.ts` and
   `request-abort.test.ts`: `request.signal` is an `rsmix_fetch::AbortSignal` that `rsmix-node-fetch-server`
   aborts when the client leaves before the response finished, and an aborted request skips
   the rest of the chain and answers 499 with `AbortError` where Remix rejects. The `remix new` starter (`reference/remix/template`)
   is ported as `template` with its controller test and a browser e2e.
   `router-types.test.ts` (5 of 20, in `packages/fetch-router/tests/router_types.rs`: the requests its
   cases send; the rest check TypeScript context inference, which typed `ctx.get::<T>()` keys
   replace),
   `request-context.test.ts` (16 of 25; the rest are JavaScript object and type cases),
   `middleware.test.ts` and `route-map.test.ts` are ported one per case: `Context::headers_mut`
   gives the context its own copy of the request headers, like Remix's `context.headers`.
   The route helper tests (`method`, `form`, `resource`, `resources`: 29 of 31 cases plus
   the type assertions on composed routes) are ported one per case in
   `packages/fetch-router/tests/route_helpers.rs`, with `options` and `Prefix::head`/`options`
   added; `form_with(pattern, Method)` is Remix's `formMethod` option. Route helper options
   that change a map's shape (`only`, `exclude`, `names`) become a struct you declare with
   `#[derive(RouteMap)]` and fill from the helper, and the tests check the declared map's
   names, methods, paths and order; the two "both only and exclude" error cases have no
   struct equivalent and are not ported.
   `remix/response`'s `redirect.test.ts` and the init and stream cases of `html.test.ts` are
   ported in `packages/response/src/lib.rs`: one `ResponseInit` (status, headers) is shared by
   `redirect_with`, `html_with`, `ctx.render_with` and the SPA `render_with`, and a `Location`
   or `Content-Type` in an init replaces the constructor's. `createMixin(setup, { ssr })` is
   `HostMixin` with `mixin_with(args, MixinOptions { ssr })`.
   `render-middleware`'s `render-ui.test.ts` is ported in `packages/render-middleware/src/render_ui_tests.rs`
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
   `remix/spa`: `spa.test.browser.tsx`'s resolver cases run natively in `packages/spa` against
   `resolve` (the runtime cases, initial URL and interactive fallback, are covered by the spa
   demo e2e). SPA form submissions now follow `getRequestBody`: `multipart/form-data`,
   `text/plain` (`name=value\r\n` lines) or URL encoding, with `\r\n` line breaks, and a
   POST that redirects to GET drops its content type. `rsmix_spa::render(transform)` is Remix's
   `render(transform)` middleware, and a redirect without `Location` is the response.
   `vdom.insert-remove.test.tsx` is ported (`browser_insert_remove.rs`); `bool` children
   render nothing. `client-entry.test.tsx`: the entry marker and ID cases are
   `client_entry_marks_the_component_and_stores_its_id` in `tests/ssr.rs`, and an empty ID or
   props that do not serialize fail the build (the `compile_fail` examples on
   `client_entry!`, checked against compiling controls); `isEntry` on non-functions has no
   counterpart, since only `Component` types carry the constant. `nested-client-entry.test.tsx`
   is `browser_nested_entries.rs` (6 of 6). `create-element.test.ts`'s mix normalization
   (flattening, dropping falsy values) is `into_mixins_flattens_nesting_and_drops_none` in
   `src/mixin.rs`. Not ported as files: `jsx.test.tsx`, `vnode.test.ts` and the rest of
   `create-element.test.ts` (TypeScript types and JavaScript value validation that Rust's
   types enforce), `module-preloader` and `client-entry-boundary` (module ids; rsmix ships one
   wasm bundle), `remix/assert` (Rust's `assert!`) and the `assets` module transform tests
   (`target`, `module-store`, `loaders`, `scripts/cjs-check` and `scripts/transform`: browser
   targets, the module graph and HMR, Node load hooks and CommonJS detection, none of which
   rsmix assets has, since it does not transform JavaScript). `mime/scripts/codegen.test.ts`
   checks npm codegen output; rsmix's table is generated once by `packages/mime/scripts/codegen.mjs`.
   `file.test.ts` is ported case by case (`packages/response/tests/file.rs`, 85 cases)
   against `response::file`, Remix's `createFileResponse` for any `FileLike` (an in-memory
   `File`, or a `LazyFile`, which streams only the requested range): ranges only for `GET`,
   an empty type sends no `Content-Type`, and strong ETags take a `DigestAlgorithm`
   (SHA-1/256/384/512, hand-written in `rsmix-crypto`) or `Digest::with(fn)` of the file; an
   unknown algorithm name fails to parse instead of rejecting. Static files are rebuilt on
   it as Remix's `static.ts` is (`open_lazy_file` plus `response::file`), with
   `static.test.ts` ported case by case (43, symlinks included); `file_response` and
   `file_response_for` are gone.
   `remix/terminal` is `rsmix-terminal` with its test files ported one per case (18 of 20; the
   entrypoint re-export and `getDefaultEnvironment` cases do not apply); the logger's colors and
   detection now come from it. Cursor helpers take unsigned and signed integers, so Remix's
   NaN and fraction normalization has nothing to port. The `rsmix` CLI colors `new` progress and
   `doctor` like Remix's reporter (gray steps, green `✓`, red `✗`, yellow warnings) when
   detection allows; it closes each style rather than resetting, so the bytes differ from
   Remix's while the rendering matches.
   Compression now fails the compressed body when the source or encoder fails (it used to
   end early and look complete), with tests for that, source cancellation on drop and the
   206 skip. `remix/response/compress` now lives in `rsmix_fetch::response::compress` (feature
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
   for Node's mocked request events). Porting changed `rsmix-node-fetch-server`: request bodies are read
   by a task at most 16 KiB ahead of the handler (Node's `highWaterMark`), so a client that
   leaves mid-upload aborts the signal while the handler is not reading, and reads then fail
   with `AbortError` (a malformed body keeps its own error); a handler that panics after the
   request aborted is not reported to `on_error` (`isRequestAbortError`); a response body
   that fails after headers were sent is reported as `ServeError::ResponseBody`. `rsmix-url`
   now serializes IPv6 hosts as WHATWG URL does (`[2001:db8::1]`), so equivalent `Host` and
   `:authority` values match. `node-fetch-server/test`'s `createTestServer` is
   `rsmix::node_fetch_server::test::create_test_server(router)`: a server on a free `127.0.0.1` port with
   `base_url` and `close()` (dropping it closes it too). Remix's `remix/test` runner itself
   (describe/it, mocks, fake timers, coverage, watch mode) is not ported: `cargo test` is the
   runner, browser tests use the wasm-bindgen harness and Playwright scripts, and mocks and
   fake timers are well served by existing crates.
   `remix/headers` tests are ported for accept, accept-encoding, accept-language, range,
   content-range, vary, if-match, if-none-match, if-range, cache-control and content-type
   (`packages/headers/tests/`). Porting changed behavior: `AcceptEncoding::accepts` always
   passes `identity` like Remix (`accepts_encoding` is gone; compression negotiates on
   `weight("identity")`), `Range::can_satisfy` rejects hand-built ranges without bounds or
   with start > end (`normalize` used to panic on them), and `Content-Range` with an empty
   unit and `Content-Type` with empty media type, charset or boundary leave those parts out.
   Header lists collect from and iterate over their values.
   The rest of the package is ported too: content-disposition (`filename*` decodes through
   the WHATWG labels for UTF-8, UTF-16, windows-1252, which `latin1` also names, and
   ISO-8859-15, falling back to the percent-decoded text), cookie, set-cookie (`Max-Age`
   reads like `parseInt`), param-values, raw-headers (`rsmix_headers::raw::{parse,
   stringify, canonical_header_name}` over `http::HeaderMap`; `parse` returns an error
   where `Headers` would throw) and super-headers. `SuperHeaders` is a trait on
   `HeaderMap` rather than a subclass: `typed::<H>()`, `set_typed`, `update(|h| ...)`,
   `apply` / `apply_typed` with Remix's merge rules (`Set-Cookie` added, `Cookie` and
   `Vary` merged, list headers appended, others replaced), `get_joined` for
   `Headers.get`, and number, date, list, ETag and `Set-Cookie` helpers. Nothing is
   cached, so the lazy-parse, cache identity, proxy and stale-value cases have no
   counterpart; an edit that prints empty is not kept for the next edit the way Remix's
   cached object keeps it.
   `remix/mime` is ported with its five test files (`packages/mime/tests/`): the 1239-entry
   extension table and compressible list come from Remix's generated mime-db tables
   (`packages/mime/scripts/codegen.mjs`), with `detect_mime_type`, `detect_content_type`,
   `mime_type_to_content_type`, `is_compressible_mime_type` and `define_mime_type` (custom
   types, multi-part extensions, charset and compressibility overrides, `reset_mime_types`).
   Static files, compression, assets and the unpkg demo use it; `.xml` is now `text/xml`
   and `.ts` is `video/mp2t`, as in Remix.
   `remix/route-pattern` is ported with its tests (split, parse, serialize, route-pattern,
   join, specificity, href, match and match/limits; 355 cases in
   `packages/route-pattern/src/tests` and `packages/route-pattern/tests`). The umbrella
   package's `manifest.test.ts` maps to `packages/rsmix/tests/manifest.rs`: every library crate is
   an `rsmix` dependency, re-exported as `rsmix::<name>` (the component model at the root) and
   listed in the crate docs, which caught `rsmix::terminal` missing from the table; the README,
   guide, side-effect and package.json export checks are npm tooling. The type-level
   `types/stringify.test.ts` (13) and `types/join.test.ts` (31) run as parse, serialize and
   join checks; `types/split`, `types/parse` and `types/params` test TypeScript inference of
   the grammar the runtime split and parse tests already cover, and rsmix params are runtime
   values, so they do not apply. The parser reports
   Remix's ten `ParseError` types with UTF-16 indexes and normalizes sources the same way;
   static text is decoded and canonicalized; matching runs compiled part programs behind
   static prefix, suffix and anchor indexes with the `maxPatternSize`, `maxMatcherSize` and
   `maxMatchWork` limits (exact work counts match Remix); `specificity` has `compare`,
   `less_than`, `greater_than`, `equal`, `ascending` and `descending`; hrefs return
   `HrefError` with Remix's detail types (`kind()`) and relative output for a same-origin
   `base_url`. The router's HEAD fallback now stops at the first match that is not
   `specificity::equal`, as in Remix's `router.ts`. Divergences: URLs are parsed (and
   relative references joined) with `rsmix_url` before matching rather than passing a
   string and `baseURL`; the missing-params message has no `Params:` JSON line;
   `Params` leaves an unmatched optional out instead of holding `undefined`; `rsmix_url`
   does no IDNA, so the punycode tests feed URLs computed by Node's WHATWG `URL`; and the
   TypeScript type tests have no counterpart. `rsmix_url` paths now use the WHATWG path
   percent-encode set, so a lone `%` and lowercase escapes stay as written. The rewrite
   grew the spa client wasm from 480.2 to 532.1 KiB and timeboxer from 600.1 to 620.5 KiB
   (measured after rebuild77).
   `remix/cookie` is ported with `cookie.test.ts` (`packages/cookie/tests/cookie.rs`). This
   changed behavior to match Remix: cookies default to `Path=/` and `SameSite=Lax`, values
   are base64 of their UTF-8 bytes by default (`hello world` is `aGVsbG8gd29ybGQ=`, and a
   value that is not valid base64 or UTF-8 reads as `None`), `encode`/`decode` replace the
   codec (the lazy-frames theme cookie uses the identity codec as in Remix), `Set-Cookie` is
   printed by `rsmix_headers::SetCookie` in Remix's attribute order, and `Expires` and
   `Partitioned` (which forces `Secure`) are supported. `serialize_with` takes
   `CookieProperties` overrides; `clear()` is gone, and the demos delete cookies the way
   theirs do (`serialize('', { maxAge: 0 })`). Session middleware now honors an `Expires`
   cookie as an absolute expiry rounded to the second, as Remix's does. Lowercase
   `sameSite` strings have no counterpart: `SameSite` is an enum.
   `remix/session` and `remix/session-middleware` run their Remix tests (41 session and
   storage cases in `packages/session/tests/`, 23 middleware cases in
   `packages/session-middleware/src/session_remix_tests.rs`, with `Date.now` mocked by a test-only
   clock). Errors and warnings use Remix's text. Remix checks after `next()` that the
   context still holds its session; rsmix contexts move down the chain, so the middleware
   stores the session with `Context::set_sealed` instead, and a later `set` or `get_mut`
   of `Session` panics with the same message. Logging the `httpOnly: false` warning is
   checked through the message builder, since tests cannot capture stderr in-process.
   The middleware packages run their Remix tests as integration tests under
   `packages/<name>-middleware/tests/`: `cors` (21 cases, RegExp and boolean origin resolvers written
   as `origin_fn` closures), `csrf` (14), `cop` (15, every configuration message checked),
   `auth` (32 across `auth`, `requireAuth` and the bearer, API key and session schemes;
   `auth-types.test.ts` is TypeScript inference, which `Auth<I>` gives for free) and
   `form_data` (21); `logger` (10) and `html-template` (17) are one test per Remix case in
   place. `method-override.test.ts` (1 case) is in `packages/method-override-middleware/src/lib.rs`, and
   the middleware now matches Remix: it overrides any request that carries a form (not only
   POST), uses the first `_method` value only when it is text, uppercases it without trimming,
   and accepts only the router's methods (`isRequestMethod`). `async-context.test.ts` (1 case)
   is `async_context_follows_the_request_into_helpers`; `get_context()` returns `None` where
   Remix's `getContext()` throws. Changes they forced: Remix's error text throughout, the bearer and API key schemes
   default to `Authorization` and `X-Api-Key` and read the header trimmed as fetch `Headers`
   does, `SchemeResult::failure()` is Remix's empty `{ status: 'failure' }`, and the file
   storage directory error reads `Path ".." is not a directory`. `remix/file-storage` runs
   `memory.test.ts` and `fs.test.ts` (15 cases, with the form-data-parser upload
   integration). `form-data-parser` (31 cases, `packages/fetch/tests/form_data.rs`) changed
   `FormData::parse`: media types other than URL-encoded and multipart (and a missing
   `Content-Type`) are a `FormDataError::Parse` before the body is read, so `text/plain`
   bodies are no longer parsed; upload handlers may return a `Result`, whose error ends
   parsing as `FormDataError::Upload` (the middleware answers 500, or an empty form with
   `suppress_errors`, as Remix's rethrow would); and `File::new` lowercases its type like
   the web `File`. Where Remix's middleware throws, rsmix answers 413, 400 or 500.
   The parsers and the proxy run their Remix tests one per case: `multipart-parser` (28 of
   29 `multipart.test.ts` cases; the Web Encoding globals case has no Rust counterpart, plus
   39 request, 6 Node and 6 buffer-search cases; an empty body is now reported before a
   missing boundary), `tar-parser` (21 `tar.test.ts` cases with the lodash and npm fixtures,
   49 name and 70 of 73 size cases; the other three need values the types rule out; bodies
   grow from received bytes, and `TarParser::parse` reuses one parser across archives) and
   `fetch-proxy` (42 cases against real `rsmix_node_fetch_server` servers; a new `redirect` option,
   `Manual` by default, follows redirects with fetch's rules when set to `Follow`).
   `remix/auth` runs all 17 test files (50 cases, `packages/auth/tests/`), including the
   browser OIDC flow against a fake provider on a loopback port. They forced:
   `OAuthProvider::Tokens` so a custom provider keeps its own token fields through callback
   and refresh; `authorization_url` taking the transaction mutably so a provider can store
   `provider_state`; `OidcProvider::map_profile` (Remix's `mapProfile`, given the callback URL
   where Remix gives the context); `expiresAt` serialized as an ISO string; GitHub profiles
   that tell `null` from a missing `name` or `email`; and credentials `verify` receiving the
   context and free to return a `Result`. A discovery failure that rejects Remix's
   `router.fetch` is an `AuthError` the route turns into a response.
7. Distribution: the `rsmix` crate is the single dependency, like the `remix` package: the
   component model at the root and every other crate as a module (`rsmix::router`,
   `rsmix::middleware`, `rsmix::auth`, ...). Server modules are native-only; wasm builds see
   the component runtime, `ui`, `router`, `fetch`, `spa` and the platform-neutral crates.
   All examples depend on `rsmix` alone.

8. Component runtime behavior is checked against Remix's own test suites, ported test by
   test: 515 browser tests in 29 files (`packages/component/tests/browser_*.rs`, run in
   headless Chromium) cover the vdom, hydration, mixins (on, attrs, ref, css, link, mixin
   SSR options and server/client parity), the style manager, diff-dom, document state
   (focus restore), form navigation, navigation interception, nested client entries,
   frame CSS, child frames during parent reloads, and frame requests and resolvers; native ports cover
   server streaming (`ssr_stream.rs`, `ssr_mixins.rs`, `ssr_hydration.rs`, and the
   frames section in `rsmix-render-middleware`, where frames resolve through the router). Porting found and fixed real
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
   roots so their updates batch together, and rsmix has one scheduler per wasm thread that
   every root and frame shares (`rsmix::flush()` is `scheduler.dequeue()`). Its phase events
   and task queues are runtime plumbing that rsmix exposes as mixin `on_before_update` /
   `on_commit` and `Handle::queue_task`, with the same 50-update loop guard and warning.

Remaining gaps:

1. Import maps and per-entry module loading: rsmix ships one wasm bundle per app, so the
   Remix import map and module preloader tests do not apply.
2. Errors thrown inside components: Rust does not throw, and wasm32 panics abort the
   module. Errors are reported explicitly (`Handle::report_error`) and reach
   `Root::on_error` / `App::on_error` like Remix error events. A render returns
   `h.fail(error)` where Remix's would throw: the error is reported, an update keeps the
   previous DOM and a first render shows nothing, as in `vdom.errors.test.tsx` (setup
   failures are kept as state and failed in render; `browser_errors.rs` 17). On the
   server a failed component is reported and left out, where Remix rejects the stream.
3. Tooling: hot module replacement (wasm modules cannot be swapped in place), and the
   `remix` CLI's `db` command (data access is out of scope). The `rsmix` binary (`rsmix::cli`, in
   the `rsmix` crate) has `completion` (Remix's bash/zsh script and `completion -- <index>
   <words>` protocol, with the global `--no-color` flag offered like Remix's; checked in Git Bash;
   `completion.test.ts` and `commands/completion.test.ts` are in `cli/tests.rs`, as is `progress.test.ts`
   (`steps_report_running_then_success_or_failure`), and `routes`
   completion now drops flags that cannot combine with those given, like Remix's
   `completeRoutes`; the `db`, `test` runner flag and `--config` cases have no counterpart), the global `--no-color` (before any `--`; commands handed to cargo get `NO_COLOR=1` and `CARGO_TERM_COLOR=never`), `dev` (Remix's `npm run dev` / `hmr` as rebuild, restart and page reload:
   it polls `src/`, `app/`, `public/` and `Cargo.toml`, builds the client and server, runs
   a copy of the server with `RSMIX_DEV_LIVE_RELOAD`, and `rsmix::middleware::live_reload`
   appends an event-stream script to documents that reloads the page when the build id
   changes; measured on a new app: an edit to the home page showed in the open page 20 s
   later, 17 s of it the server rebuild), `doctor` (Remix's suites and output with rsmix
   checks: environment for the `rsmix` dependency, the wasm32 target and the pinned
   wasm-bindgen; project for a `RouteMap`; client for `cdylib` and a built, current wasm;
   `--json`, `--strict`, and `--fix` like Remix's `applyDoctorFixPlans`: it adds the `rsmix`
   dependency, creates or fills an empty `src/routes.rs` and declares it in `lib.rs`, and
   adds `cdylib` to the crate types, making each file once, skipping files that exist and
   refusing paths that leave the app root, even through symlinks, then checks again and
   fails on what is left; no actions suite since controllers are checked at compile time,
   and no `--config` or `remix.json`), the global `--no-color`, Remix's terminal
   preamble (a blank line, then on a terminal `RSMIX v0.1.0 - doctor` with one color per
   letter) and closing ANSI reset on stdout or else stderr when either is a terminal that is
   not `TERM=dumb`, Remix's status channel (on such a terminal a running step redraws
   `• Label.`, `..`, `...` every 250 ms and its result replaces it; elsewhere stable lines),
   with `doctor` reporting each suite as it runs, help colored like Remix's `formatHelpText` on a color terminal (headings
   bold light blue; flags, `[names]` and `<values>` in usage, examples and option labels light
   yellow; help under an error follows stderr), `new` (writes `template` as a
   standalone crate with the app's name, Remix's error codes and progress lines; an app made
   this way passed `cargo test` and the template's 10 browser checks outside the workspace),
   `version`, `help`, and `routes`, `assets` and `test`, which run the app through cargo. The
   `remix/component/test` render helper exists as `rsmix::test`; `remix routes` exists as
   `rsmix::router::routes::routes_command`, which every demo binary runs for `-- routes`
   (tree, `--table`, `--json`; no controller owner column, since rsmix has no controller files;
   the table pads every cell like Remix's reporter and bolds the headers on a color terminal);
   and `remix assets` exists as `rsmix::assets::assets_command` over the asset server's
   `assets()` / `asset_details()` inspection (list, `inspect <url-or-file>`, with statuses
   reachable, denied, not-allowed, missing, unmapped and unsupported), run by the assets and template
   demos for `-- assets`. The asset server's mounts compile through `rsmix::assets::routes`
   (Remix's `compileRoutes`, with `paths` for Windows, UNC and POSIX file paths): mount keys
   canonicalize as URL pathnames, absolute directories, query strings, fragments, encoded dot
   segments and overlapping roots are rejected with Remix's messages, and file names are
   percent-encoded like `encodeURIComponent`. `routes.test.ts` (20), `paths.test.ts` (9) and
   the fingerprint format and parse cases of `fingerprint.test.ts` are ported one per case;
   the module graph, script transforms and CommonJS checks are the TypeScript side of
   Remix's asset server, which rsmix apps (Rust compiled to wasm) do not have.
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
   `asset-server.test.ts` are ported (`packages/assets/tests/asset_server.rs`); the rest cover
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
   `target/.cache/rsmix/assets` rather than `node_modules/.cache/remix/assets`. The assets demo
   caches transformed SVGs this way, like Remix's.
4. Data access (`data-table`, `data-table-mysql`, `data-table-postgres`,
   `data-table-sqlite`) is intentionally left to existing Rust libraries, and so are the
   storage backends `file-storage-s3`, `session-storage-redis` and
   `session-storage-memcache` (rsmix's `FileStorage` and `SessionStorage` traits take any
   backend). The JavaScript tooling packages have no Rust counterpart: `node-tsx` (rsx is
   compiled by `rustc`), `node-hmr` and `component-hmr` (`rsmix dev` rebuilds, restarts and
   reloads the page instead), `multiple-import-maps-polyfill` (rsmix ships one wasm bundle
   and no import maps), `assert` (`assert!`) and `test` (`cargo test`, plus
   `rsmix_node_fetch_server::test` and `rsmix::test`). Every other Remix package has an rsmix crate or module
   (`packages/rsmix/tests/manifest.rs` checks that each crate is distributed through `rsmix`).
   The CLI's internal test files map as follows: `contained-path.test.ts` is the doctor fix
   path check (`doctor_fix_skips_existing_files_and_refuses_paths_outside_the_app`, which
   also follows symlinks); `controller-files.test.ts` and `controller-ownership.test.ts`
   plan and scan `app/actions` controller files, which rsmix replaces with the compile-time
   `<Name>Actions` check; and `remix-config.test.ts` loads `remix.json`, which rsmix does not
   have. Its sections configure things rsmix keeps elsewhere: `assets` is the asset server
   built in the app's code, `test` is `cargo test`, `db` is left to data libraries, and
   `doctor.strict` is the `--strict` flag (`--no-strict` is accepted like Remix's). The
   one `spa-response.test.ts` case (`spaResponse.create` throws outside a browser) does
   not apply: `rsmix_spa::spa_response` only stores the node beside a response, and the
   native tests build routes with it.
   Test file audit (2026-10-22): of 289 upstream test files, 86 are not cited by name in rsmix.
   Those that apply are ported under other names: the nine `hydration.*` files in
   `browser_hydration.rs`, the vdom tasks, scheduler, signals, connect and context files in
   `browser_vdom.rs`, the elements, fragments and components cases in `browser_elements.rs`,
   terminal `ansi`, `env` and `styles` in `terminal/src/tests.rs`, `raw-headers` and
   `header-names` in `headers/tests/raw_headers.rs`, `param-values` (10 of 10) in
   `param_values.rs`, the five mime files assertion for assertion, `href.test.ts` (94 of
   94), `spa.test.tsx` (the bodyless response with status, headers and node in
   `status_and_headers_carry_through`, the redirect target in
   `routes_render_nodes_and_follow_redirects`, and the rejection case), the CLI `routes`
   command in `router/tests/route_listing.rs` and `assets` in `assets/src/tests.rs`.
   `super_headers.rs` ports 12 of 36: the rest test JavaScript object behavior. The rest
   are out of scope: data-table and the memcache, redis and S3 storage backends (data
   access), `node-hmr`, `component-hmr`, `multiple-import-maps-polyfill`, `test`,
   `assert`, the asset transform pipeline, the CLI `db` and `test` commands, and the
   route-pattern `types/params` type tests.
5. `frame.test.tsx`'s streamed template parsing cases (a template parsed in two parts, with
   the observer watching or hydration starting mid-template, a sibling appended between the
   parts, and a template without its end marker waiting for parsing to finish) run in
   `browser_frame_reloads.rs` with `document.readyState` reporting `loading` and the
   template's content appended in two steps, as the parser does; `document.write` would
   replace the test page. Cases that load
   entry modules asynchronously (a provider module arriving after its consumer, cached
   modules, `loadModule` errors) do not apply: one wasm bundle holds every entry. Top
   frame reload cases replace the whole page, so the browser tests run them on a named
   parent frame and the demo e2e scripts cover the document itself. Ported this round:
   reload results (`reload_async` resolves to the load's signal or its error), HTML after
   the first flush marker, the resolver's `signal` and streamed content, context through
   frames, and the preserved client entry, shared src, nested and blocking frame reload
   cases. The `stream.test.tsx` cases that apply are ported; import map, module preload
   and `resolveClientEntry` cases do not apply.

## 12. Parity audit (2026-10-22)

Checked against upstream `2968b1e` (no newer upstream commits on 2026-10-22). Section 15
updates it for Remix 3.1.

1. Packages: upstream has 50. The 37 in scope each have an rsmix crate or module, distributed
   through the `rsmix` crate (`packages/rsmix/tests/manifest.rs`): since the 2026-10-22
   reorganization each one is a crate in a directory of the same name under `packages/`
   (`rsmix-<name>`), with three exceptions: `async-context-middleware` lives in `fetch-router`
   (`async_context`, since the router's `Context` keeps the scope in step), `form-data-parser`
   is `FormData::parse` in `fetch` (the Fetch API's `FormData` lives there), and `remix` is
   `rsmix`. The other 13 are out of scope:
   data access (`data-table` and its three adapters, the memcache and redis session
   stores, `file-storage-s3`), which existing libraries cover, and JavaScript tooling with
   no Rust counterpart (`node-tsx`, `node-hmr`, `component-hmr`,
   `multiple-import-maps-polyfill`, `assert`, `test`).
2. Demos: all 12 are in `demos/`. The 10 with a browser client, the
   `social-auth` server and the starter template pass their Playwright e2e suites
   (rebuild105, measured); `unpkg` is checked by its 13 native tests in the workspace run
   (test113, measured).
3. Test files: 289 upstream. Each one that applies is ported case by case, by name or under
   the names in section 11; the rest are listed there as out of scope or not applicable
   with the reason. Partial ports name the missing cases and why (`anchor.test.ts` 15 of
   16, `navigation.test.ts` 83 of 87, `render-ui.test.ts` 12 of 15, `super-headers` 12
   of 36).
4. Core values:
   1. Model first: components are a setup scope plus a render function, updated with
      `handle.update()`; no hooks, signals or change detection (README, "The Component
      Model").
   2. Web APIs: Fetch-style `Request`/`Response`, `Headers`, `FormData`, `URL`, the
      Navigation API, `AbortSignal` and DOM events, with Remix's names where Rust allows.
   3. Religiously runtime: a deliberate divergence. rsx and route maps compile with
      `rustc`; what Remix decides at runtime (routing, frame resolution, node trees from
      JSON, mixins) is still decided at runtime.
   4. Avoid dependencies: the core crates use the hyper, tokio and wasm-bindgen stack;
      the exceptions are listed in the README (ICU4X, `ureq`, `nodejs-semver`).
   5. Composition: small crates, middleware as composable functions, and mixins on host
      elements as in Remix.
   6. Distribute cohesively: one `rsmix` dependency re-exports every crate, checked by the
      manifest test.
5. Documented divergences, not gaps: one wasm bundle per app (no import maps), no thrown
   errors (explicit `report_error`), string `style`, plain `Event`s in `rsmix-ui`, no HMR
   (`rsmix dev` rebuilds and reloads), and slower creation of large lists than Remix's JS
   runtime (`bench/README.md`; fixed on 2026-10-23, section 13).
6. Gate at this audit (measured): workspace tests 2,586 passed, 0 failed (test113);
   browser tests `rsmix-component` 596 and `rsmix-ui` 140 passed, 0 failed (browser-all11);
   all demo e2e suites passed (rebuild105).

## 13. Client speed and size (2026-10-23)

1. The list benchmarks rsmix lost (replace 1k, create 10k, append 1k) were bound by calls
   from wasm into JavaScript, not by diffing: about 25 calls per table row, each through
   web-sys, plus a `TextDecoder` run for every dynamic string and a SipHash lookup for
   every interned one.
2. Decision: keep the vdom in Rust and route its hot DOM calls through one small JS object
   built at startup (`packages/component/src/dom/ops.rs`), combining create and append,
   passing template strings as indexes into a JS-side table, and decoding short ASCII
   strings in JS. No generated JS snippet file, so the page still loads three files.
   Keyed lists also patch their common prefix and suffix in place.
3. Result (measured, `bench/README.md`): rsmix commits faster than Remix on every list
   operation in both runs except `clear 1k` in the stable run, by 3 to 17% on the four
   creation-heavy ones.
4. Size: narrowed, not closed. 150.4 KB gzip on stable, 133.6 KB with
   `build-client.ps1 -Nightly` (`build-std` with `panic=immediate-abort`), against
   42.5 KB for Remix. The runtime's own code (vdom, frames, navigation) is about as large as
   Remix's JavaScript before compression; the rest is what wasm brings that JavaScript gets
   from the browser (core, alloc, serde_json, the allocator, the JS glue). Closing it would
   mean shipping less runtime per page or moving rendering back to JavaScript; neither is
   done.

## 14. Compiling out frames and navigation (2026-10-23)

1. After section 13 the runtime's own code was the largest part of the client: frames
   (35 KB), soft navigation (17 KB) and the DOM differ for frame reloads (9 KB), shallow
   bytes before `wasm-opt`. The bench page uses none of them; it hydrates two client entries.
2. Decision: Cargo features `frames` and `navigation` (which needs `frames`) on
   rsmix-component, forwarded by `rsmix` along with `spa` (which needs both). All are on by
   default, so existing apps do not change. With them off the browser runtime is the vdom,
   client entry hydration from `rmx-data`, styles and mixins, and the document is the only
   frame:
   1. `FrameHandle::reload` loads the document (GET assigns or reloads, POST submits a
      form), `replace` renders into the body, frame events never fire and
      `Frames::get` finds nothing.
   2. A client-rendered `<Frame>` renders its fallback, with one console warning.
   3. `navigate()` assigns `location`; links and forms are plain browser navigations.
   4. `RunOptions::resolve_frame` is ignored, with a warning.
   5. The server is unaffected and renders `<Frame>` either way.
3. A client crate opts out with `rsmix = { ..., default-features = false }`; the shared app
   crate does too, and the server crate keeps the defaults. rsmix-ui no longer turns the
   features on, and rsmix-spa asks for `navigation`.
4. Remix has no equivalent switch: its `run()` starts frames and navigation on every page.
   This is a place where compile-time features fit Rust better than a runtime option.
5. Result (measured, `bench/README.md`): the bench client went from 150.4 to 99.7 KB gzip
   on stable and from 133.6 to 87.2 KB on nightly, against 42.5 KB for Remix (2.3x and
   2.1x, from 3.5x and 3.1x). List speed did not change within noise.
6. What is left is mostly the vdom and what wasm brings (`core`, `js-sys`, serde_json,
   `alloc`, `hashbrown`, the allocator). serde_json for entry props (17 KB before
   `wasm-opt`) is the largest single item. Not done.

## 15. Parity with Remix 3.1 (2026-10-24)

Checked against the `v3.1.0` release commit `03c8414` (2026-10-23). Upstream `main` had
three commits after it on 2026-10-24, all docs or formatting (#12075, #12076).

1. Ported, one commit each, with the upstream tests ported case by case:
   1. Headers (`cc3c7f6`): Range serialization for large decimal bounds (#12025), only
      satisfiable normalized byte ranges (#12023), invalid Content-Range relationships
      rejected (#12040), weak comparison for If-None-Match (#12048), If-Range validators
      classified by syntax (#12049), backslashes kept in serialized parameter values
      (#12002) and malformed raw header lines ignored (#12027). `raw::parse` now returns a
      `HeaderMap` of the valid lines.
   2. Response and tar (`2114645`): unsupported Range units serve the full file (#12045;
      `lines=x` is now 200, `bytes=x` still 400), zero-length suffix ranges and ranges on
      empty files handled safely (#12043), and the tar parser stops after the
      end-of-archive marker while still counting what follows toward `max_total_size`
      (#12041).
   3. Streaming uploads (`584ed39`, #12030): `rsmix_multipart_parser::future` yields each
      part when its headers arrive, with content as zero-copy views of the body chunks;
      `rsmix_fetch::form_data::future` hands each `FileUpload` to the handler as it starts and
      streams it from the request; `rsmix-form-data-middleware` has a matching `future`
      module. Upstream's #12029 works around a Node.js bug (before 24.20.0 and 26.6.0)
      that kept streamed `FileUpload` contents in memory; rsmix does not run on Node, so there
      is nothing to port.
   4. Keyed Fragments in client entry props (`e2fe08b`, #12067).
   5. Prefetching (`fdc6a6a`, #12004): `data-rmx-prefetch="intent"` and `"viewport"`,
      `prefetch()`, and `RunOptions::prefetch` for a custom strategy. It is part of the
      `navigation` feature, so a client built without it carries none of it.
2. Not ported: S3 multipart uploads (#12070), data access, out of scope as in section 12.
3. Breaking for rsmix users: `RunOptions` gained a field (struct literals need
   `..Default::default()`), `raw::parse` returns `HeaderMap`, and a non-`bytes` Range unit
   is served in full instead of rejected.
4. Uploads (measured, `cargo run --release -p rsmix-fetch --example upload_bench`, median of
   7): a 100 MiB file went from 149.28 to 15.43 ms in total, 125.41 to 0.04 ms to the
   handler, and 200.1 to 0 MiB of peak heap growth.
5. Benchmarks against Remix 3.1 (measured, `bench/README.md`):
   1. Docker, pinned CPUs: rsmix serves 12x, 16x and 16x Remix's requests per second on
      `/`, `/ssr/100` and `/ssr/1000` on one CPU, and 12x, 8x and 9x on eight. Remix 3.1's
      single process is 2.9x its 3.0 figure on `/`.
   2. Browser: rsmix commits first on every list operation in both the stable and the
      nightly run, and hydrates the page in about half the time.
   3. Client: Remix 3.1 is 46.4 KB gzip (3.0 was 42.5 KB); the rsmix nightly client is still
      87.2 KB, 1.9x.
6. Gate (measured): workspace tests 2,637 passed, 0 failed (test122); native clippy and
   wasm clippy for `rsmix-component` (default, no default features, `frames` only, all
   targets), `rsmix-fetch`, `rsmix-multipart-parser` and `rsmix-form-data-middleware` with
   `-D warnings`; browser tests `rsmix-component` 612, `rsmix-ui` 140 and no-frames 1 passed,
   0 failed (browser-all16), plus `browser_prefetch` 9, `browser_prefetch_assets` 4 and
   `browser_entry_props` 2; demo e2e suites 11 of 12 in rebuild110, and the twelfth
   (bookstore, its new viewport-prefetch check) passed after scrolling the links into view.

## 16. Router steps without boxes (2026-10-24)

1. Prompted by Corten, which removed a per-request re-boxing of its middleware stack. In rsmix
   each middleware step allocated twice: `Next::run` boxed its own `async fn`, and the
   middleware's future was boxed by `Middleware::call`. Each chain also allocated an `Arc`
   for its endpoint.
2. Profile first (measured, callgrind in a Linux container,
   `packages/rsmix/examples/router_profile.rs`): a step cost about 1,384 instructions, and the
   four-middleware router took 21,479 instructions a request against 15,944 for the handler
   alone, so it showed up.
3. Decision: `Next::run` returns `NextFuture`, a named future that calls the next step on its
   first poll (so `catch_panic` still sees a panic there), checks the abort signal before and
   after calling it, and then polls the step before the signal, as `AbortSignal::race` did.
   The endpoint is held by value. `Middleware::call` still returns a `BoxFuture`: middleware
   is a list of trait objects, as in Remix, so one box per step stays.
4. Breaking: an `impl Middleware` that returned `next.run(ctx)` as its `BoxFuture` now writes
   `Box::pin(next.run(ctx))` (five crates in the repo). Closures and `async` blocks are
   unchanged.
5. Result (measured): a step costs about 945 instructions; requests use 3 to 7 fewer
   allocations and 4 to 13% fewer instructions depending on the middleware. Over HTTP in
   Docker the difference was inside the run-to-run spread on a busy host
   (`bench/README.md`).
6. Found on the way, not changed: `static_files` calls `canonicalize` on every request path,
   which costs more than all of the router's boxing on `/`.
7. Gate (measured): workspace tests 2,641 passed, 0 failed (test124); native clippy, wasm
   clippy for `rsmix-fetch-router` and `rsmix-spa` with `-D warnings`; all 12 demo e2e suites
   passed (rebuild111). `rsmix-fetch-router`'s wasm `--all-targets` clippy fails on an
   `Arc<Mutex<Option<Router>>>` in `tests/router.rs`, at the previous commit as well.

## 17. Static files without canonicalize, and a clean bench machine (2026-10-24)

1. Section 16's profile showed `static_files` resolving every request path with
   `canonicalize`, which walks the whole path from `/`, before falling through to the router.
   Remix does the same with `realpath`, on the root and the path, on every request.
2. Decision: check each component below the root (already canonical) with one
   `symlink_metadata` call and stop at the first missing name. Only a symlink or junction,
   or on Windows a name the system rewrites (trailing dots or spaces, `:`, `~`), goes
   through `canonicalize` and the containment check, so links out of the root are still
   refused. A deliberate divergence in mechanism, not in behavior; one difference: a root
   that did not exist at startup (and so could not be canonicalized) now serves files once it
   exists, as Remix's per-request `realpath` does.
3. Benchmarks moved to a fresh DigitalOcean droplet (`c-16`, 16 dedicated vCPUs, destroyed
   after the run) by `bench/docker/droplet.ps1`, since the laptop was too busy for
   differences of a few percent to show.
4. Result (measured, `bench/README.md`): on the droplet `/` went from about 17,900 to
   21,400 requests a second on one thread and from about 103,700 to 122,700 on eight
   (15 to 22% in each round); the SSR pages did not move beyond their spread. The router
   change of section 16 was worth 0 to 7% on `/`. Against Remix 3.1 on the droplet, rsmix
   serves 7x to 10x the requests per second in 3 to 9 MB against 341 MB to 2.9 GB.
5. Gate (measured): workspace tests 2,643 passed, 0 failed (test125), with two new static
   tests (directory links inside and outside the root, missing paths); the symlink tests
   failed with the link check disabled. Native clippy with `-D warnings` clean. Demo e2e
   suites: 11 of 12 in rebuild112; the ui suite's "a keyed element rendered mid-exit is
   reclaimed and retargets" check failed once (an animation timing check in the browser,
   which the static change does not touch) and passed on three reruns.

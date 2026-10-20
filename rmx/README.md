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
| `on(\|this, h\| ..)` | `on('click', ..)` | Returns an `EventHandler` for `on:event={..}`. |
| `on_event(\|this, h, e\| ..)` | handler with event arg | `e.raw()` is the underlying `web_sys::Event`. |
| `queue_task(\|this, h\| ..)` | `handle.queueTask` | Runs after the next DOM commit. |
| `id()` | `handle.id` | Stable per instance. |
| `is_connected()` | `handle.signal` (partly) | No `AbortSignal`; Remix's `update()` also returns one. |

## `rsx!`

A proc macro built on `rstml`. Templates live inside `.rs` files; there is no separate
`.rsx` file format yet.

1. Lowercase tags are elements, capitalized tags are components (`<RowView id={1}/>` builds `RowViewProps`).
2. Text must be quoted (`"Count: "`); expressions go in braces.
3. `on:click={handler}` attaches a delegated event handler.
4. `key={..}` enables keyed list reconciliation.
5. Child nodes of a component arrive as `props.children`.

## Architecture

1. `crates/rmx-macros`: the `rsx!` macro.
2. `crates/rmx/src/node.rs`: the `Node` tree (element, text, fragment, component, raw).
3. `crates/rmx/src/component.rs`: the `Component` trait and `Handle`.
4. `crates/rmx/src/ssr.rs`: `render_to_string`. Islands are wrapped in `<!--rmx:h:N-->`
   comments and their props are serialized to a `<script type="application/json" id="rmx-data">` block.
5. `crates/rmx/src/dom/mod.rs` (wasm32 only): the browser runtime. It hydrates islands by
   adopting the server DOM, reconciles keyed children with a longest-increasing-subsequence
   move pass, and uses one delegated document listener per event type.
6. `examples/bench`: `app` (shared components), `server` (axum), `client` (wasm entry that
   registers islands with `rmx::hydrate`), and `public` (static files plus generated `pkg/`).

Size: about 1,700 lines of Rust in the two crates (measured with a line count on 2026-10-19).

## Build and Run

Prereqs: Rust stable with the `wasm32-unknown-unknown` target, plus `wasm-bindgen` 0.2.129
and `wasm-opt` (the build script finds the copies cargo-leptos downloaded).

```powershell
pwsh ./build.ps1            # wasm client + release server
./target/release/bench-server.exe
# http://127.0.0.1:44101/  /ssr/100  /client
cargo test -p rmx
```

Server env vars: `HOST` (default `127.0.0.1`), `PORT` (default `44101`), `RMX_THREADS`,
`RMX_PUBLIC`. A Linux image is in `../bench/docker/rmx.Dockerfile`.

## Limitations

1. No frames or soft navigation (Remix 3's `<Frame>` and client router).
2. No streaming SSR; pages render to one `String`.
3. No async tasks, `handle.signal`, `handle.context`, or `handle.frame`.
4. No CSS or `mix` helpers beyond event handlers.
5. Islands are registered by name by hand in the client crate.
6. Creating large lists is slower than Remix's JS runtime (see `../bench/README.md`),
   because each DOM node costs several wasm-to-JS calls.

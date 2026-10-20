use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes, A},
    StaticSegment,
};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options islands=cfg!(feature = "islands")/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/rmx-spike.css"/>
        <Title text="rmx spike"/>
        <Router>
            <nav>
                <A href="/">"Tasks"</A>
                " | "
                <A href="/about">"About"</A>
            </nav>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=TasksPage/>
                    <Route path=StaticSegment("about") view=AboutPage/>
                </Routes>
            </main>
        </Router>
    }
}

#[derive(Clone)]
struct Task {
    id: u32,
    title: &'static str,
    done: bool,
}

/// Stand-in for a Remix loader; phase 1 replaces this with real data loading.
fn load_tasks() -> Vec<Task> {
    vec![
        Task { id: 1, title: "Install the Rust toolchain", done: true },
        Task { id: 2, title: "Server-render a page with axum + Leptos", done: true },
        Task { id: 3, title: "Hydrate one island", done: false },
        Task { id: 4, title: "Measure WASM size and build times", done: false },
    ]
}

/// Server-rendered only in islands mode: no WASM is generated for this component.
#[component]
fn TasksPage() -> impl IntoView {
    let tasks = load_tasks();
    let done = tasks.iter().filter(|t| t.done).count() as i32;

    view! {
        <h1>"Tasks"</h1>
        <ul>
            {tasks
                .into_iter()
                .map(|t| {
                    view! {
                        <li data-id=t.id class:done=t.done>
                            {t.title}
                        </li>
                    }
                })
                .collect_view()}
        </ul>
        <Counter initial=done/>
    }
}

/// The one interactive island. Its props are serialized into the HTML.
/// Built as a plain component when the `islands` feature is off (full-hydration comparison).
#[cfg_attr(feature = "islands", island)]
#[cfg_attr(not(feature = "islands"), component)]
fn Counter(initial: i32) -> impl IntoView {
    let count = RwSignal::new(initial);
    view! {
        <button id="counter" on:click=move |_| *count.write() += 1>
            "Completed: " {count}
        </button>
    }
}

#[component]
fn AboutPage() -> impl IntoView {
    view! {
        <h1>"About"</h1>
        <p>"Phase 0 spike: axum + Leptos SSR with one WASM island."</p>
    }
}

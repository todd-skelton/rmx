//! Client entries for the lazy-frames demo, shared by the server (HTML) and the browser (WASM).
//!
//! A port of `remix/demos/lazy-frames`. [`LazyFrame`] defers mounting a `<Frame>` until its
//! host approaches the viewport; [`ThemeToggle`] switches the document theme without
//! touching loaded frames; [`FramePlayground`] arrives inside a frame response and keeps
//! its local count when that frame reloads.

use rmx::{css, node_ref, on, rsx, Children, Component, Frame, Handle, IntoNode, Mixin, Node};
use serde::{Deserialize, Serialize};

pub const THEME_COOKIE: &str = "lazy-frames-theme";
pub const THEME_COOKIE_MAX_AGE: i64 = 60 * 60 * 24 * 365;

/// `ui/public/theme.ts`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    pub fn parse(value: Option<&str>) -> Theme {
        match value {
            Some("dark") => Theme::Dark,
            _ => Theme::Light,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    fn toggled(self) -> Theme {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }
}

// ---------------------------------------------------------------------------------------
// LazyFrame (ui/public/lazy-frame.tsx)
// ---------------------------------------------------------------------------------------

pub const DEFAULT_ROOT_MARGIN: &str = "320px 0px";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LazyFrameProps {
    pub src: String,
    pub root_margin: Option<String>,
    pub pause_animations_when_inactive: bool,
    /// Shown by the mounted Frame while its request is in flight.
    pub fallback: Option<Node>,
    /// Rendered with the page and until the host approaches the viewport.
    pub children: Children,
}

/// Defers mounting a Frame until its host approaches the viewport.
///
/// `children` render on the server and before intersection. Instances with the same
/// `root_margin` share one observer. Once the host intersects, the Frame mounts and its own
/// `fallback` covers the request; it stays mounted when it leaves the viewport. With
/// `pause_animations_when_inactive`, a second, zero-margin observer tracks visibility and a
/// host-level `css()` mixin pauses descendant animations while the host is off screen.
pub struct LazyFrame {
    requested: bool,
    active: bool,
}

impl Component for LazyFrame {
    type Props = LazyFrameProps;
    rmx::client_entry!("LazyFrame");

    fn setup(_: &LazyFrameProps, _: &Handle<Self>) -> Self {
        LazyFrame { requested: false, active: false }
    }

    fn render(&self, props: &LazyFrameProps, h: &Handle<Self>) -> Node {
        let observe = {
            let h = h.clone();
            let margin = props.root_margin.clone().unwrap_or_else(|| DEFAULT_ROOT_MARGIN.to_string());
            let track_activity = props.pause_animations_when_inactive;
            node_ref(move |node| {
                #[cfg(target_arch = "wasm32")]
                {
                    let signal = h.signal();
                    let load = h.clone();
                    intersection::observe(node.element(), &signal, &margin, move |entry| {
                        if !entry.is_intersecting() {
                            return true;
                        }
                        load.with(|this, h| {
                            this.requested = true;
                            h.update();
                        });
                        false
                    });
                    if track_activity {
                        let activity = h.clone();
                        intersection::observe(node.element(), &signal, "0px 0px 0px 0px", move |entry| {
                            let next = entry.is_intersecting();
                            activity.with(|this, h| {
                                if this.active != next {
                                    this.active = next;
                                    h.update();
                                }
                            });
                            true
                        });
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (node, &h, &margin, track_activity);
            })
        };
        let paused = (props.pause_animations_when_inactive && !self.active)
            .then(|| css("& *, & *::before, & *::after { animation-play-state: paused !important; }"));
        let content = if self.requested {
            rsx! { <Frame src={props.src.clone()} fallback={props.fallback.clone()}/> }
        } else {
            (&props.children).into_node()
        };
        rsx! { <div mix={[Some(observe), paused]}>{content}</div> }
    }
}

/// IntersectionObservers pooled by `rootMargin` (an observer's options are fixed when it
/// is constructed). Each host registers callbacks for as long as its signal is not aborted.
#[cfg(target_arch = "wasm32")]
mod intersection {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{AbortSignal, Element, IntersectionObserver, IntersectionObserverEntry, IntersectionObserverInit};

    /// Return `false` to stop observing.
    type Callback = Rc<dyn Fn(&IntersectionObserverEntry) -> bool>;

    struct Pool {
        observer: IntersectionObserver,
        targets: Vec<(Element, Vec<(u32, Callback)>)>,
        _closure: Closure<dyn Fn(js_sys::Array)>,
    }

    thread_local! {
        static POOLS: RefCell<HashMap<String, Pool>> = RefCell::new(HashMap::new());
        static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    }

    pub(crate) fn observe(
        node: &Element,
        signal: &AbortSignal,
        root_margin: &str,
        callback: impl Fn(&IntersectionObserverEntry) -> bool + 'static,
    ) {
        if signal.aborted() {
            return;
        }
        let id = NEXT_ID.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        POOLS.with(|pools| {
            let mut pools = pools.borrow_mut();
            let pool = pools.entry(root_margin.to_string()).or_insert_with(|| new_pool(root_margin));
            match pool.targets.iter_mut().find(|(el, _)| el.is_same_node(Some(node))) {
                Some((_, callbacks)) => callbacks.push((id, Rc::new(callback))),
                None => {
                    pool.targets.push((node.clone(), vec![(id, Rc::new(callback))]));
                    pool.observer.observe(node);
                }
            }
        });
        let (node, margin) = (node.clone(), root_margin.to_string());
        let on_abort = Closure::once_into_js(move || unobserve(&margin, &node, id));
        let _ = signal.add_event_listener_with_callback("abort", on_abort.unchecked_ref());
    }

    fn new_pool(root_margin: &str) -> Pool {
        let margin = root_margin.to_string();
        let closure = Closure::<dyn Fn(js_sys::Array)>::new(move |entries: js_sys::Array| {
            for entry in entries.iter() {
                let entry: IntersectionObserverEntry = entry.unchecked_into();
                let target = entry.target();
                // Collect first: callbacks may unobserve, which needs the pool.
                let callbacks: Vec<(u32, Callback)> = POOLS.with(|pools| {
                    pools
                        .borrow()
                        .get(&margin)
                        .and_then(|p| p.targets.iter().find(|(el, _)| el.is_same_node(Some(&target))))
                        .map(|(_, cbs)| cbs.clone())
                        .unwrap_or_default()
                });
                for (id, callback) in callbacks {
                    if !callback(&entry) {
                        unobserve(&margin, &target, id);
                    }
                }
            }
        });
        let init = IntersectionObserverInit::new();
        init.set_root_margin(root_margin);
        let observer = IntersectionObserver::new_with_options(closure.as_ref().unchecked_ref(), &init)
            .expect("IntersectionObserver");
        Pool { observer, targets: Vec::new(), _closure: closure }
    }

    fn unobserve(root_margin: &str, node: &Element, id: u32) {
        POOLS.with(|pools| {
            let mut pools = pools.borrow_mut();
            let Some(pool) = pools.get_mut(root_margin) else { return };
            let Some(i) = pool.targets.iter().position(|(el, _)| el.is_same_node(Some(node))) else { return };
            pool.targets[i].1.retain(|(cid, _)| *cid != id);
            if pool.targets[i].1.is_empty() {
                pool.targets.swap_remove(i);
                pool.observer.unobserve(node);
            }
            if pool.targets.is_empty() {
                pool.observer.disconnect();
                pools.remove(root_margin);
            }
        });
    }
}

// ---------------------------------------------------------------------------------------
// ThemeToggle (ui/public/theme-toggle.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ThemeToggleProps {
    pub theme: Theme,
}

/// Switches `data-theme` on the document and saves it in a cookie, without a request.
pub struct ThemeToggle {
    theme: Theme,
}

impl Component for ThemeToggle {
    type Props = ThemeToggleProps;
    rmx::client_entry!("ThemeToggle");

    fn setup(props: &ThemeToggleProps, _: &Handle<Self>) -> Self {
        ThemeToggle { theme: props.theme }
    }

    fn render(&self, _: &ThemeToggleProps, h: &Handle<Self>) -> Node {
        let next = self.theme.toggled();
        rsx! {
            <button type="button" aria-label={format!("Switch to {} mode", next.as_str())} mix={[
                theme_toggle_style(),
                on("click", h.on(move |this, h| {
                    this.theme = next;
                    apply_theme(next);
                    h.update();
                })),
            ]}>
                <span aria-hidden="true" mix={theme_icon_style()}>{if self.theme == Theme::Light { "\u{263e}" } else { "\u{2600}" }}</span>
                <span>{format!("{} mode", next.as_str())}</span>
            </button>
        }
    }
}

fn apply_theme(theme: Theme) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        if let Some(root) = document.document_element() {
            let _ = root.set_attribute("data-theme", theme.as_str());
        }
        if let Some(body) = document.body() {
            let _ = body.set_attribute("data-theme", theme.as_str());
        }
        let cookie = format!("{THEME_COOKIE}={}; Max-Age={THEME_COOKIE_MAX_AGE}; Path=/; SameSite=Lax", theme.as_str());
        let _ = document.unchecked_into::<web_sys::HtmlDocument>().set_cookie(&cookie);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = theme;
}

fn theme_toggle_style() -> Mixin {
    css("display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 36px; border: 1px solid var(--control-border); border-radius: 6px; padding: 7px 12px; background-color: var(--control-button-bg); color: var(--control-text); font: inherit; font-size: 11px; font-weight: 750; text-transform: capitalize; white-space: nowrap; cursor: pointer; transition: background-color 160ms ease, border-color 160ms ease, color 160ms ease; &:hover { border-color: var(--control-border-hover); background-color: var(--control-button-hover); } &:focus-visible { outline: 3px solid var(--focus-ring); outline-offset: 2px; }")
}
fn theme_icon_style() -> Mixin {
    css("font-size: 16px; line-height: 1;")
}

// ---------------------------------------------------------------------------------------
// FramePlayground (actions/frames/public/frame-playground.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Coral,
    Violet,
    Teal,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct FramePlaygroundProps {
    pub initial_count: i32,
    pub action_label: String,
    pub accent: Accent,
    pub served_at: String,
}

/// Loaded only when an interactive frame arrives. Its local count survives a reload of the
/// frame that contains it.
pub struct FramePlayground {
    count: i32,
    hydrated: bool,
}

impl Component for FramePlayground {
    type Props = FramePlaygroundProps;
    rmx::client_entry!("FramePlayground");

    fn setup(props: &FramePlaygroundProps, h: &Handle<Self>) -> Self {
        h.queue_task(|this, h| {
            this.hydrated = true;
            h.update();
        });
        FramePlayground { count: props.initial_count, hydrated: false }
    }

    fn render(&self, props: &FramePlaygroundProps, h: &Handle<Self>) -> Node {
        let frame = h.frame();
        let accent = match props.accent {
            Accent::Coral => css("background-color: #d4543f;"),
            Accent::Violet => css("background-color: #7557c8;"),
            Accent::Teal => css("background-color: #157c70;"),
        };
        rsx! {
            <div mix={playground_style()}>
                <div mix={status_row_style()}>
                    <span aria-hidden="true" mix={[status_dot_style(), if self.hydrated { ready_dot_style() } else { pending_dot_style() }]}></span>
                    {if self.hydrated { "Client component ready" } else { "Hydrating client component\u{2026}" }}
                </div>
                <p mix={count_style()}>"Local count: " {self.count}</p>
                <div mix={button_row_style()}>
                    <button type="button" data-action="increment" mix={[action_button_style(), accent, on("click", h.on(|this, h| {
                        this.count += 1;
                        h.update();
                    }))]}>
                        {props.action_label.clone()}
                    </button>
                    <button type="button" data-action="reload" mix={[reload_button_style(), on("click", h.on(move |_, _| frame.reload()))]}>
                        "Reload Frame"
                    </button>
                </div>
                <p mix={served_at_style()}>
                    "Frame response rendered at " <time>{props.served_at.clone()}</time>
                    ". Local count is retained when the Frame reloads."
                </p>
            </div>
        }
    }
}

fn playground_style() -> Mixin {
    css("margin-top: 24px;")
}
fn status_row_style() -> Mixin {
    css("display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 650; color: var(--interactive-muted);")
}
fn status_dot_style() -> Mixin {
    css("width: 8px; height: 8px; border-radius: 50%;")
}
fn pending_dot_style() -> Mixin {
    css("background-color: var(--interactive-pending);")
}
fn ready_dot_style() -> Mixin {
    css("background-color: #13866f; box-shadow: 0 0 0 4px rgba(19, 134, 111, 0.12);")
}
fn count_style() -> Mixin {
    css("margin: 14px 0 16px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 18px; font-weight: 700; color: var(--interactive-text);")
}
fn button_row_style() -> Mixin {
    css("display: flex; flex-wrap: wrap; gap: 10px;")
}
fn action_button_style() -> Mixin {
    css("border: 0; border-radius: 6px; padding: 11px 17px; font: inherit; font-size: 14px; font-weight: 750; color: #ffffff; cursor: pointer; &:hover { filter: brightness(0.92); } &:focus-visible { outline: 3px solid var(--interactive-text); outline-offset: 3px; }")
}
fn reload_button_style() -> Mixin {
    css("border: 1px solid var(--interactive-button-border); border-radius: 6px; padding: 10px 16px; background-color: transparent; font: inherit; font-size: 14px; font-weight: 700; color: var(--interactive-button-text); cursor: pointer; &:hover { border-color: var(--interactive-accent); background-color: var(--interactive-button-hover); } &:focus-visible { outline: 3px solid var(--interactive-accent); outline-offset: 3px; }")
}
fn served_at_style() -> Mixin {
    css("margin: 16px 0 0; font-size: 12px; line-height: 1.6; color: var(--interactive-subtle);")
}

/// Browser entry: hydrate the client entries and start soft navigation.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions {
        entries: vec![rmx::entry::<LazyFrame>(), rmx::entry::<ThemeToggle>(), rmx::entry::<FramePlayground>()],
    });
}

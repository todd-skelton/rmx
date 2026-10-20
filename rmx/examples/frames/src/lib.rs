//! Components for the frames demo, shared by the server (HTML) and the browser (WASM).
//!
//! A port of `remix/demos/frames`: the same pages, frames and client entries, written as
//! rmx components. Client entries are listed in [`start`].

use rmx::{css, on, rsx, Children, Component, Frame, FrameEvent, Handle, Mixin, Node, NodeHandle};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------------------
// Styles (ui/public/styles.ts)
// ---------------------------------------------------------------------------------------

pub fn link_style() -> Mixin {
    css("color: #b9c6ff; text-decoration: underline;")
}
pub fn page_heading_style() -> Mixin {
    css("margin-top: 16px; margin-bottom: 8px; letter-spacing: -0.02em;")
}
pub fn section_heading_style() -> Mixin {
    css("margin-top: 0; font-size: 16px;")
}
pub fn lead_style() -> Mixin {
    css("margin-top: 0; color: #b9c6ff;")
}
pub fn muted_style() -> Mixin {
    css("color: #9aa8e8;")
}
pub fn panel_style() -> Mixin {
    css("border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; padding: 16px; background: rgba(255,255,255,0.04);")
}
pub fn clock_label_style() -> Mixin {
    css("font-size: 13px; color: #b9c6ff;")
}
pub fn button_style() -> Mixin {
    css("padding: 6px 10px; border-radius: 10px; border: 1px solid rgba(255,255,255,0.18); background: rgba(255,255,255,0.06); color: #e9eefc; cursor: pointer; &:hover { background: rgba(255,255,255,0.10); }")
}
pub fn list_style() -> Mixin {
    css("margin: 0; padding-left: 18px; color: #e9eefc;")
}
pub fn fragment_style() -> Mixin {
    css("border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; padding: 10px; background: rgba(255,255,255,0.02);")
}
pub fn nested_fragment_style() -> Mixin {
    css("border: 1px solid rgba(255,255,255,0.10); border-radius: 8px; padding: 8px; background: rgba(255,255,255,0.02);")
}
pub fn fragment_label_style() -> Mixin {
    css("font-size: 12px; color: #b9c6ff;")
}
pub fn nested_frame_label_style() -> Mixin {
    css("font-size: 12px; color: #9aa8e8; margin-bottom: 6px;")
}
pub fn clock_value_style() -> Mixin {
    css("font-size: 16px; font-variant-numeric: tabular-nums; margin-top: 2px;")
}
pub fn large_clock_value_style() -> Mixin {
    css("font-size: 18px; font-variant-numeric: tabular-nums;")
}
fn section_box_style() -> Mixin {
    css("margin-top: 16px; border: 1px solid rgba(255,255,255,0.12); border-radius: 10px; padding: 12px; background: rgba(255,255,255,0.03);")
}
fn row_between_style() -> Mixin {
    css("display: flex; align-items: center; justify-content: space-between; gap: 12px;")
}

// ---------------------------------------------------------------------------------------
// Document (ui/document.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: String,
    pub max_width: Option<String>,
    pub children: Children,
}

pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        let max_width = format!("max-width: {}", props.max_width.as_deref().unwrap_or("760px"));
        rsx! {
            <html>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>{props.title.clone()}</title>
                    <script type="module" async src="/boot.js"></script>
                </head>
                <body mix={css("font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif; margin: 0; padding: 24px; background: #0b1020; color: #e9eefc;")}>
                    <div mix={css("margin: 0 auto;")} style={max_width}>{&props.children}</div>
                </body>
            </html>
        }
    }
}

// ---------------------------------------------------------------------------------------
// Client entries
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CounterProps {
    pub initial_count: i32,
    pub label: String,
}

/// `ui/public/counter.tsx`: plain state, explicit update().
pub struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rmx::client_entry!("Counter");

    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial_count }
    }

    fn render(&self, props: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={css("display: flex; gap: 12px; align-items: center;")}>
                <strong mix={css("width: 72px;")}>{props.label.clone()}</strong>
                <button type="button" mix={[button_style(), on("click", h.on(|this, h| { this.count -= 1; h.update(); }))]}>
                    "\u{2212}"
                </button>
                <span mix={css("min-width: 48px; text-align: center; font-variant-numeric: tabular-nums;")}>{self.count}</span>
                <button type="button" mix={[button_style(), on("click", h.on(|this, h| { this.count += 1; h.update(); }))]}>
                    "+"
                </button>
            </div>
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct NoProps {}

/// `ui/public/client-frame-example.tsx`: a `<Frame>` mounted by a client component.
pub struct ClientFrameExample {
    mounted: bool,
}

impl Component for ClientFrameExample {
    type Props = NoProps;
    rmx::client_entry!("ClientFrameExample");

    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        ClientFrameExample { mounted: false }
    }

    fn render(&self, _: &NoProps, h: &Handle<Self>) -> Node {
        rsx! {
            <section mix={section_box_style()}>
                <div mix={row_between_style()}>
                    <div>
                        <div mix={css("font-size: 13px; color: #b9c6ff;")}>"Client-rendered Frame"</div>
                        <div mix={css("font-size: 12px; color: #9aa8e8;")}>"Added after hydration via a client entry component."</div>
                    </div>
                    <button type="button" id="toggle-client-frame" mix={[button_style(), on("click", h.on(|this, h| { this.mounted = !this.mounted; h.update(); }))]}>
                        {if self.mounted { "Remove Frame" } else { "Mount Frame" }}
                    </button>
                </div>
                {self.mounted.then(|| rsx! {
                    <div mix={css("margin-top: 10px;")}>
                        <Frame src="/frames/client-frame-example" fallback={rsx! { <div mix={css("color: #9aa8e8;")}>"Loading client frame content\u{2026}"</div> }}/>
                    </div>
                })}
            </section>
        }
    }
}

/// `actions/frames/public/reload-time.tsx`: reloads the frame it renders in.
pub struct ReloadTime {
    pending: bool,
}

impl Component for ReloadTime {
    type Props = NoProps;
    rmx::client_entry!("ReloadTime");

    fn setup(_: &NoProps, h: &Handle<Self>) -> Self {
        let frame = h.frame();
        h.on_frame(&frame, FrameEvent::ReloadStart, |this, h| {
            this.pending = true;
            h.update();
        });
        h.on_frame(&frame, FrameEvent::ReloadComplete, |this, h| {
            this.pending = false;
            h.update();
        });
        ReloadTime { pending: false }
    }

    fn render(&self, _: &NoProps, h: &Handle<Self>) -> Node {
        let frame = h.frame();
        rsx! {
            <button type="button" class="reload-time" mix={[button_style(), on("click", h.on(move |_, _| frame.reload()))]}
                style={self.pending.then_some("background: rgba(255,255,255,0.04)")}>
                {if self.pending { "Refreshing\u{2026}" } else { "Refresh" }}
            </button>
        }
    }
}

/// `ui/public/reload-scope.tsx`: reload this frame.
pub struct ReloadScope {
    pending: bool,
}

impl Component for ReloadScope {
    type Props = NoProps;
    rmx::client_entry!("ReloadScope");

    fn setup(_: &NoProps, h: &Handle<Self>) -> Self {
        let frame = h.frame();
        h.on_frame(&frame, FrameEvent::ReloadStart, |this, h| {
            this.pending = true;
            h.update();
        });
        h.on_frame(&frame, FrameEvent::ReloadComplete, |this, h| {
            this.pending = false;
            h.update();
        });
        ReloadScope { pending: false }
    }

    fn render(&self, _: &NoProps, h: &Handle<Self>) -> Node {
        let frame = h.frame();
        rsx! {
            <div mix={css("display: flex; gap: 8px; flex-wrap: wrap;")}>
                <button type="button" class="reload-frame" mix={[button_style(), on("click", h.on(move |_, _| frame.reload()))]}
                    style={self.pending.then_some("background: rgba(255,255,255,0.04)")}>
                    {if self.pending { "Reloading frame\u{2026}" } else { "Reload this frame" }}
                </button>
            </div>
        }
    }
}

/// `ui/public/reload-scope.tsx`: reload the whole page through the top frame.
pub struct ReloadTopFrame {
    pending: bool,
}

impl Component for ReloadTopFrame {
    type Props = NoProps;
    rmx::client_entry!("ReloadTopFrame");

    fn setup(_: &NoProps, h: &Handle<Self>) -> Self {
        let top = h.frames().top();
        h.on_frame(&top, FrameEvent::ReloadStart, |this, h| {
            this.pending = true;
            h.update();
        });
        h.on_frame(&top, FrameEvent::ReloadComplete, |this, h| {
            this.pending = false;
            h.update();
        });
        ReloadTopFrame { pending: false }
    }

    fn render(&self, _: &NoProps, h: &Handle<Self>) -> Node {
        let top = h.frames().top();
        rsx! {
            <button type="button" id="reload-top" mix={[button_style(), on("click", h.on(move |_, _| top.reload()))]}
                style={self.pending.then_some("background: rgba(255,255,255,0.04)")}>
                {if self.pending { "Reloading page\u{2026}" } else { "Reload top frame" }}
            </button>
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StateSearchProps {
    pub initial_query: String,
}

/// `actions/public/state-search-page.tsx`: the frame `src` follows component state.
pub struct StateSearchPage {
    query: String,
    input: Option<NodeHandle>,
}

impl Component for StateSearchPage {
    type Props = StateSearchProps;
    rmx::client_entry!("StateSearchPage");

    fn setup(props: &StateSearchProps, _: &Handle<Self>) -> Self {
        StateSearchPage { query: props.initial_query.clone(), input: None }
    }

    fn render(&self, _: &StateSearchProps, h: &Handle<Self>) -> Node {
        let results = if self.query.trim().is_empty() {
            rsx! { <p mix={css("margin: 0; color: #9aa8e8;")}>"Enter a state name to run the frame search."</p> }
        } else {
            let src = format!("/frames/state-search-results?query={}", encode_query(&self.query));
            rsx! {
                <div mix={css("border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; padding: 12px; background: rgba(255,255,255,0.03);")}>
                    <Frame src={src} fallback={rsx! { <div mix={css("color: #9aa8e8;")}>"Searching states\u{2026}"</div> }}/>
                </div>
            }
        };
        rsx! {
            <section>
                <form id="state-search" mix={[
                    css("display: flex; gap: 8px; flex-wrap: wrap; align-items: center;"),
                    on("submit", h.on_event(|this, h, event| {
                        event.prevent_default();
                        if let Some(input) = &this.input {
                            this.query = input.value().trim().to_string();
                            input.select();
                        }
                        h.update();
                    })),
                ]}>
                    <label mix={css("display: flex; flex-direction: column; gap: 4px; flex: 1 1 320px;")}>
                        <span mix={css("font-size: 13px; color: #b9c6ff;")}>"Search states"</span>
                        <input name="query" placeholder="Try: carolina, dakota, new" mix={[
                            h.node_ref(|this, node| this.input = Some(node.clone())),
                            css("min-width: 260px; padding: 8px 10px; border-radius: 10px; border: 1px solid rgba(255,255,255,0.18); background: rgba(255,255,255,0.04); color: #e9eefc;"),
                        ]}/>
                    </label>
                    <button type="submit" mix={[button_style(), css("margin-top: 20px;")]}>"Search"</button>
                </form>
                {results}
            </section>
        }
    }
}

/// `actions/public/client-mounted-page-example.tsx`
pub struct ClientMountedPageExample {
    show: bool,
}

impl Component for ClientMountedPageExample {
    type Props = NoProps;
    rmx::client_entry!("ClientMountedPageExample");

    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        ClientMountedPageExample { show: false }
    }

    fn render(&self, _: &NoProps, h: &Handle<Self>) -> Node {
        rsx! {
            <section mix={section_box_style()}>
                <div mix={row_between_style()}>
                    <div>
                        <div mix={css("font-size: 13px; color: #b9c6ff;")}>"Client-mounted frame test"</div>
                        <div mix={css("font-size: 12px; color: #9aa8e8;")}>"Mount a frame whose server content includes a nested non-blocking frame."</div>
                    </div>
                    <button type="button" id="toggle-mounted-frame" mix={[button_style(), on("click", h.on(|this, h| { this.show = !this.show; h.update(); }))]}>
                        {if self.show { "Remove Frame" } else { "Mount Frame" }}
                    </button>
                </div>
                {self.show.then(|| rsx! {
                    <div mix={css("margin-top: 10px;")}>
                        <Frame src="/frames/client-mounted-outer" fallback={rsx! { <div mix={css("color: #9aa8e8;")}>"Loading outer mounted frame\u{2026}"</div> }}/>
                    </div>
                })}
            </section>
        }
    }
}

fn encode_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub const UNITED_STATES: [&str; 50] = [
    "Alabama", "Alaska", "Arizona", "Arkansas", "California", "Colorado", "Connecticut", "Delaware", "Florida",
    "Georgia", "Hawaii", "Idaho", "Illinois", "Indiana", "Iowa", "Kansas", "Kentucky", "Louisiana", "Maine",
    "Maryland", "Massachusetts", "Michigan", "Minnesota", "Mississippi", "Missouri", "Montana", "Nebraska",
    "Nevada", "New Hampshire", "New Jersey", "New Mexico", "New York", "North Carolina", "North Dakota", "Ohio",
    "Oklahoma", "Oregon", "Pennsylvania", "Rhode Island", "South Carolina", "South Dakota", "Tennessee", "Texas",
    "Utah", "Vermont", "Virginia", "Washington", "West Virginia", "Wisconsin", "Wyoming",
];

pub fn search_united_states(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_lowercase();
    UNITED_STATES.iter().copied().filter(|s| s.to_lowercase().contains(&q)).collect()
}

/// Browser entry: hydrate the client entries and start soft navigation.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions {
        entries: vec![
            rmx::entry::<Counter>(),
            rmx::entry::<ClientFrameExample>(),
            rmx::entry::<ReloadTime>(),
            rmx::entry::<ReloadScope>(),
            rmx::entry::<ReloadTopFrame>(),
            rmx::entry::<StateSearchPage>(),
            rmx::entry::<ClientMountedPageExample>(),
        ],
    });
}

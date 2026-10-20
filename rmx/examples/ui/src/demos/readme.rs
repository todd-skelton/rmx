//! `packages/component/src/runtime/demos/readme.demo.tsx`: the README snippets as one
//! gallery. Each Remix function component is a struct; `setup` holds what the component
//! function body held and `render` is the returned render function.

use std::cell::Cell;
use std::rc::Rc;

use rmx::browser::Interval;
use rmx::{css, node_ref, on, rsx, Children, Component, Handle, Node, NodeHandle, TypedEventTarget};

// Getting Started - Basic App Example
pub struct App {
    count: u32,
}

impl Component for App {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        App { count: 0 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <button mix={[on("click", h.on(|this, h| { this.count += 1; h.update(); }))]}>"Count: " {self.count}</button>
        }
    }
}

// Component State and Updates - Counter
pub struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Counter { count: 0 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div>
                <span>"Count: " {self.count}</span>
                <button mix={[on("click", h.on(|this, h| { this.count += 1; h.update(); }))]}>"Increment"</button>
            </div>
        }
    }
}

// Components - Greeting
#[derive(Clone, Default)]
pub struct GreetingProps {
    pub name: &'static str,
}

pub struct Greeting;

impl Component for Greeting {
    type Props = GreetingProps;
    fn setup(_: &GreetingProps, _: &Handle<Self>) -> Self {
        Greeting
    }
    fn render(&self, props: &GreetingProps, _: &Handle<Self>) -> Node {
        rsx! { <h1>"Hello, " {props.name} "!"</h1> }
    }
}

// Stateful Components - CounterWithSetup, and Setup Prop vs Props - CounterWithLabel
#[derive(Clone, Default)]
pub struct CounterProps {
    pub initial_count: i32,
    pub label: Option<&'static str>,
}

pub struct CounterWithSetup {
    count: i32,
}

impl Component for CounterWithSetup {
    type Props = CounterProps;
    // Runs once: the initial count is only read here.
    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        CounterWithSetup { count: props.initial_count }
    }
    // Runs on every update, with the latest props.
    fn render(&self, props: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div>
                {props.label.unwrap_or("Count")} ": " {self.count}
                <button mix={[on("click", h.on(|this, h| { this.count += 1; h.update(); }))]}>"Increment"</button>
            </div>
        }
    }
}

pub struct CounterWithLabel {
    count: i32,
}

impl Component for CounterWithLabel {
    type Props = CounterProps;
    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        CounterWithLabel { count: props.initial_count }
    }
    fn render(&self, props: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div>
                {props.label.unwrap_or_default()} ": " {self.count}
                <button mix={[on("click", h.on(|this, h| { this.count += 1; h.update(); }))]}>"+"</button>
            </div>
        }
    }
}

// Events - SearchInput
pub struct SearchInput {
    query: String,
    results: Vec<String>,
    loading: bool,
}

impl Component for SearchInput {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        SearchInput { query: String::new(), results: Vec::new(), loading: false }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let results = (!self.loading && !self.results.is_empty()).then(|| {
            let items: Vec<Node> = self.results.iter().map(|r| rsx! { <li>{r.clone()}</li> }).collect();
            rsx! { <ul>{items}</ul> }
        });
        rsx! {
            <div>
                <input id="search" type="text" value={self.query.clone()} placeholder="Type to search..." mix={[on("input", h.on_event(|this, h, event| {
                    this.query = event.target_value();
                    this.loading = true;
                    h.update();
                    // The signal aborts when the next input event arrives.
                    let signal = event.signal();
                    let h = h.clone();
                    h.clone().spawn(async move {
                        rmx::browser::sleep(300).await;
                        if signal.aborted() {
                            return;
                        }
                        h.with(|this, h| {
                            this.results = if this.query.is_empty() {
                                Vec::new()
                            } else {
                                vec![format!("Result for \"{}\" 1", this.query), format!("Result for \"{}\" 2", this.query)]
                            };
                            this.loading = false;
                            h.update();
                        });
                    });
                }))]}/>
                {self.loading.then(|| rsx! { <div>"Loading..."</div> })}
                {results}
            </div>
        }
    }
}

// Controlled Input - Slug Form
pub struct SlugForm {
    slug: String,
    generated_slug: String,
}

/// `crypto.randomUUID().slice(0, 8)`
fn random_slug() -> String {
    (0..8).map(|_| char::from_digit((rmx::browser::random() * 16.0) as u32 % 16, 16).unwrap_or('0')).collect()
}

impl Component for SlugForm {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        SlugForm { slug: String::new(), generated_slug: String::new() }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let value = if self.generated_slug.is_empty() { self.slug.clone() } else { self.generated_slug.clone() };
        rsx! {
            <form>
                <label mix={[css("display: flex; align-items: center; gap: 8px;")]}>
                    <input id="auto-slug" type="checkbox" mix={[on("change", h.on_event(|this, h, event| {
                        this.generated_slug = if event.target_checked() { random_slug() } else { String::new() };
                        h.update();
                    }))]}/>
                    "Auto-generate slug"
                </label>
                <label mix={[css("display: flex; flex-direction: column; gap: 4px;")]}>
                    "Slug"
                    <input id="slug" type="text" value={value} disabled={!self.generated_slug.is_empty()} mix={[on("input", h.on_event(|this, h, event| {
                        this.slug = event.target_value();
                        h.update();
                    }))]}/>
                </label>
            </form>
        }
    }
}

// Global Events - KeyboardTracker
pub struct KeyboardTracker {
    keys: Vec<String>,
}

impl Component for KeyboardTracker {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| {
            #[cfg(target_arch = "wasm32")]
            browser::on_document_keydown(h);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = h;
        });
        KeyboardTracker { keys: Vec::new() }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let keys = if self.keys.is_empty() { "(press some keys)".to_string() } else { self.keys.join(", ") };
        rsx! { <div id="keys">"Keys: " {keys}</div> }
    }
}

// CSS Prop - Button (Basic)
pub struct ButtonBasic;

impl Component for ButtonBasic {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ButtonBasic
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <button mix={[css("color: white; background-color: rgb(54, 113, 246); border: none; padding: 8px 16px; border-radius: 4px; cursor: pointer; &:hover { background-color: rgb(37, 90, 210); } &:active { transform: scale(0.98); }")]}>
                "Click me"
            </button>
        }
    }
}

// CSS Prop - Button (Advanced with nested rules)
pub struct ButtonAdvanced;

impl Component for ButtonAdvanced {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ButtonAdvanced
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <button id="button-advanced" mix={[css("
                color: white; background-color: rgb(54, 113, 246); border: none; padding: 10px 20px; border-radius: 6px;
                cursor: pointer; position: relative; display: inline-flex; align-items: center; gap: 8px;
                &:hover { background-color: rgb(37, 90, 210); }
                &::before {
                    content: \"\"; position: absolute; inset: -2px; border-radius: 8px;
                    background: linear-gradient(45deg, rgb(54, 113, 246), rgb(99, 179, 255));
                    z-index: -1; opacity: 0; transition: opacity 0.2s;
                }
                &:hover::before { opacity: 1; }
                .icon { width: 16px; height: 16px; }
            ")]}>
                <span class="icon">"\u{2605}"</span>
                "Click me"
            </button>
        }
    }
}

// Ref Mixin - Form (Basic)
pub struct FormBasic {
    input: Option<NodeHandle>,
}

impl Component for FormBasic {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        FormBasic { input: None }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div>
                <input id="select-me" type="text" placeholder="Click the button to select this"
                    mix={[h.node_ref(|this, node| this.input = Some(node.clone())), css("margin-right: 8px; padding: 4px 8px;")]}/>
                <button id="select-input" mix={[css("padding: 4px 12px;"), on("click", h.on(|this, _| {
                    if let Some(input) = &this.input {
                        input.select();
                    }
                }))]}>"Select Input"</button>
            </div>
        }
    }
}

// Ref Mixin with AbortSignal - ResizeObserver Component
pub struct ResizeComponent {
    width: i32,
    height: i32,
}

impl Component for ResizeComponent {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ResizeComponent { width: 0, height: 0 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let h2 = h.clone();
        rsx! {
            <div id="resize" mix={[
                node_ref(move |node, signal| {
                    #[cfg(target_arch = "wasm32")]
                    browser::observe_size(node, signal, h2.clone());
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = (node, signal, &h2);
                }),
                css("padding: 20px; background-color: rgba(255, 255, 255, 0.1); border-radius: 8px; resize: both; overflow: auto; min-width: 100px; min-height: 60px; border: 1px solid rgb(209, 213, 219);"),
            ]}>
                "Resize me! (" {self.width} " \u{d7} " {self.height} ")"
            </div>
        }
    }
}

// handle.update() - Player
pub struct Player {
    is_playing: bool,
    play: Option<NodeHandle>,
    stop: Option<NodeHandle>,
}

impl Component for Player {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Player { is_playing: false, play: None, stop: None }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={[css("display: flex; gap: 8px;")]}>
                <button id="play" disabled={self.is_playing} mix={[
                    h.node_ref(|this, node| this.play = Some(node.clone())),
                    css(if self.is_playing { "padding: 8px 16px; opacity: 0.5;" } else { "padding: 8px 16px; opacity: 1;" }),
                    on("click", h.on(|this, h| {
                        this.is_playing = true;
                        let h = h.clone();
                        h.clone().spawn(async move {
                            h.updated().await;
                            // Focus the enabled button after the update completes.
                            h.with(|this, _| this.stop.as_ref().map(NodeHandle::focus));
                        });
                    })),
                ]}>"\u{25b6} Play"</button>
                <button id="stop" disabled={!self.is_playing} mix={[
                    h.node_ref(|this, node| this.stop = Some(node.clone())),
                    css(if !self.is_playing { "padding: 8px 16px; opacity: 0.5;" } else { "padding: 8px 16px; opacity: 1;" }),
                    on("click", h.on(|this, h| {
                        this.is_playing = false;
                        let h = h.clone();
                        h.clone().spawn(async move {
                            h.updated().await;
                            h.with(|this, _| this.play.as_ref().map(NodeHandle::focus));
                        });
                    })),
                ]}>"\u{23f9} Stop"</button>
            </div>
        }
    }
}

// handle.queueTask - Form with scroll
pub struct FormWithScroll {
    show_details: bool,
    details: Option<NodeHandle>,
}

impl Component for FormWithScroll {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        FormWithScroll { show_details: false, details: None }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let details = self.show_details.then(|| {
            rsx! {
                <section id="details" mix={[
                    h.node_ref(|this, node| this.details = Some(node.clone())),
                    css("margin-top: 1rem; padding: 1rem; border: 1px solid rgba(255, 255, 255, 0.2); border-radius: 8px; background-color: rgba(255, 255, 255, 0.05);"),
                ]}>
                    <h3 mix={[css("margin: 0 0 0.5rem 0;")]}>"Additional Details"</h3>
                    <p mix={[css("margin: 0;")]}>"This section appears when the checkbox is checked."</p>
                </section>
            }
        });
        rsx! {
            <div>
                <label mix={[css("display: flex; align-items: center; gap: 8px; cursor: pointer;")]}>
                    <input id="show-details" type="checkbox" checked={self.show_details} mix={[on("change", h.on_event(|this, h, event| {
                        this.show_details = event.target_checked();
                        h.update();
                        if this.show_details {
                            // Scroll to the expanded section after it renders.
                            h.queue_task(|this, _| {
                                if let Some(details) = &this.details {
                                    details.scroll_into_view();
                                }
                            });
                        }
                    }))]}/>
                    "Show additional details"
                </label>
                {details}
            </div>
        }
    }
}

// handle.signal - Clock
pub struct Clock {
    /// Set once mounted in the browser. Remix renders this demo only on the client; here the
    /// gallery is server-rendered, and the server cannot know the browser's time zone, so
    /// the time appears after hydration instead of mismatching it.
    interval: Option<Interval>,
}

impl Component for Clock {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|this, h| {
            let tick = h.clone();
            // Dropping the interval (when the component goes away) clears it.
            this.interval = Some(Interval::new(1000, move || tick.update()));
            h.update();
        });
        Clock { interval: None }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let time = self.interval.is_some().then(rmx::browser::local_time_string);
        rsx! { <span id="clock">{time}</span> }
    }
}

// handle.id - LabeledInput
pub struct LabeledInput;

impl Component for LabeledInput {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        LabeledInput
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={[css("display: flex; flex-direction: column; gap: 4px;")]}>
                <label for={h.dom_id()}>"Name"</label>
                <input id={h.dom_id()} type="text" mix={[css("padding: 4px 8px; border-radius: 4px; border: 1px solid rgba(255,255,255,0.3);")]}/>
            </div>
        }
    }
}

// Context API - Theme Provider and Consumer
pub struct ThemeName {
    pub theme: &'static str,
}

pub struct ThemeProvider;

impl Component for ThemeProvider {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.set_context(ThemeName { theme: "dark" });
        ThemeProvider
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <div mix={[css("display: flex; flex-direction: column; gap: 8px;")]}><ThemedHeader/></div> }
    }
}

pub struct ThemedHeader {
    theme: &'static str,
}

impl Component for ThemedHeader {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        // Consume context from ThemeProvider.
        ThemedHeader { theme: h.context::<ThemeName>().map_or("light", |t| t.theme) }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let style = if self.theme == "dark" { "background-color: #000; color: #fff;" } else { "background-color: #fff; color: #000;" };
        rsx! { <header id="themed-header" mix={[css(style)]}>"Header"</header> }
    }
}

// Context API with EventTarget - Advanced Theme
#[derive(Clone, Default)]
pub struct Theme {
    value: Rc<Cell<&'static str>>,
    changes: TypedEventTarget<()>,
}

impl Theme {
    fn new() -> Theme {
        let theme = Theme::default();
        theme.value.set("light");
        theme
    }

    pub fn value(&self) -> &'static str {
        self.value.get()
    }

    pub fn set_value(&self, value: &'static str) {
        self.value.set(value);
        self.changes.dispatch(&());
    }
}

pub struct ThemeProviderAdvanced {
    theme: Theme,
}

impl Component for ThemeProviderAdvanced {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let theme = Theme::new();
        h.set_context(theme.clone());
        ThemeProviderAdvanced { theme }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={[css("display: flex; flex-direction: column; gap: 8px;")]}>
                <button id="toggle-theme" mix={[css("padding: 8px 16px; align-self: flex-start;"), on("click", h.on(|this, _| {
                    // No update in the parent component.
                    this.theme.set_value(if this.theme.value() == "light" { "dark" } else { "light" });
                }))]}>"Toggle Theme (EventTarget)"</button>
                <ThemedContent/>
            </div>
        }
    }
}

pub struct ThemedContent {
    theme: Option<Rc<Theme>>,
}

impl Component for ThemedContent {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let theme = h.context::<Theme>();
        // Subscribe to theme changes and update when it changes.
        if let Some(theme) = &theme {
            h.listen(&theme.changes, |_, h, _| h.update());
        }
        ThemedContent { theme }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let value = self.theme.as_ref().map_or("light", |t| t.value());
        let style = if value == "dark" { "background-color: #1a1a1a; color: #fff;" } else { "background-color: #f0f0f0; color: #000;" };
        rsx! {
            <div id="themed-content" mix={[css("padding: 12px; border-radius: 6px;"), css(style)]}>"Current theme: " {value}</div>
        }
    }
}

// Fragments - List
pub struct ListWithFragment;

impl Component for ListWithFragment {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ListWithFragment
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <ul id="fragment-list" mix={[css("margin: 0; padding-left: 20px;")]}>
                <>
                    <li>"Item 1"</li>
                    <li>"Item 2"</li>
                    <li>"Item 3"</li>
                </>
            </ul>
        }
    }
}

// Example Container Component
#[derive(Clone, Default)]
pub struct ExampleProps {
    pub title: &'static str,
    pub children: Children,
}

pub struct Example;

impl Component for Example {
    type Props = ExampleProps;
    fn setup(_: &ExampleProps, _: &Handle<Self>) -> Self {
        Example
    }
    fn render(&self, props: &ExampleProps, _: &Handle<Self>) -> Node {
        rsx! {
            <div class="example">
                <h2>{props.title}</h2>
                <div class="example-content">{&props.children}</div>
            </div>
        }
    }
}

// Main Demo App
pub struct ReadmeExamples;

impl Component for ReadmeExamples {
    type Props = ();
    rmx::client_entry!("ReadmeExamples");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ReadmeExamples
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <div class="examples-grid">
                <Example title="Getting Started - Counter"><App/></Example>
                <Example title="Component State - Counter"><Counter/></Example>
                <Example title="Greeting"><Greeting name="World"/></Example>
                <Example title="Counter with Setup"><CounterWithSetup initial_count={10} label={Some("Total")}/></Example>
                <Example title="Setup vs Props"><CounterWithLabel initial_count={5} label={Some("Score")}/></Example>
                <Example title="Events - Search Input"><SearchInput/></Example>
                <Example title="Controlled Input - Slug Form"><SlugForm/></Example>
                <Example title="Global Events - Keyboard Tracker"><KeyboardTracker/></Example>
                <Example title="CSS Prop - Basic Button"><ButtonBasic/></Example>
                <Example title="CSS Prop - Advanced Button"><ButtonAdvanced/></Example>
                <Example title="Ref Mixin - Form"><FormBasic/></Example>
                <Example title="Ref with AbortSignal - Resize Observer"><ResizeComponent/></Example>
                <Example title="handle.update() - Player"><Player/></Example>
                <Example title="handle.queueTask - Scroll to Section"><FormWithScroll/></Example>
                <Example title="handle.signal - Clock"><Clock/></Example>
                <Example title="handle.id - Labeled Input"><LabeledInput/></Example>
                <Example title="Context API - Theme Provider"><ThemeProvider/></Example>
                <Example title="Context with EventTarget"><ThemeProviderAdvanced/></Example>
                <Example title="Fragments - List"><ListWithFragment/></Example>
            </div>
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use rmx::{Handle, NodeHandle, Signal};
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    use super::{KeyboardTracker, ResizeComponent};

    /// `document.addEventListener('keydown', ..., { signal: handle.signal })`
    pub(super) fn on_document_keydown(h: &Handle<KeyboardTracker>) {
        let handle = h.clone();
        let listener = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            handle.with(|this, h| {
                this.keys.push(event.key());
                if this.keys.len() > 10 {
                    this.keys.remove(0);
                }
                h.update();
            });
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_signal(&h.signal());
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document.add_event_listener_with_callback_and_add_event_listener_options(
                "keydown",
                listener.as_ref().unchecked_ref(),
                &options,
            );
        }
        // The signal removes the listener; the closure lives as long as the page.
        listener.forget();
    }

    pub(super) fn observe_size(node: &NodeHandle, signal: &Signal, h: Handle<ResizeComponent>) {
        let callback = Closure::<dyn Fn(js_sys::Array)>::new(move |entries: js_sys::Array| {
            let Ok(entry) = entries.get(0).dyn_into::<web_sys::ResizeObserverEntry>() else { return };
            let rect = entry.content_rect();
            h.with(|this, h| {
                this.width = rect.width().round() as i32;
                this.height = rect.height().round() as i32;
                h.update();
            });
        });
        let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) else { return };
        observer.observe(node.element());
        // Clean up when the element is removed.
        signal.on_abort(move || {
            observer.disconnect();
            drop(callback);
        });
    }
}

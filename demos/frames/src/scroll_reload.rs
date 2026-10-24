//! Client entries for the scroll restoration, scroll anchoring and root reload pages
//! (`actions/public/scroll-restoration.tsx`, `scroll-anchoring.tsx`,
//! `root-reload-client-entries.tsx`).

use std::cell::Cell;

use rsmix::{css, navigate, on, rsx, Component, Frame, FrameEvent, Handle, History, Mixin, NavigateOptions, Node, ReloadOptions, Reset, TypedEventTarget};
use serde::{Deserialize, Serialize};

use crate::{lead_style, muted_style, panel_style, section_heading_style};

fn list_rows(label: &'static str) -> Node {
    let rows: Vec<Node> = (1..=48)
        .map(|item| {
            let background = if item % 2 == 0 { "rgba(255,255,255,0.04)" } else { "transparent" };
            rsx! {
                <li key={item} mix={css(&format!("min-height: 76px; display: flex; align-items: center; border: 1px solid rgba(255,255,255,0.1); border-radius: 10px; padding: 0 18px; background: {background};"))}>
                    {format!("{label} {item}")}
                </li>
            }
        })
        .collect();
    rsx! { <ol mix={css("display: grid; gap: 10px; margin: 0; padding: 0; list-style: none;")}>{rows}</ol> }
}

fn sticky_panel(margin_bottom: u32) -> Vec<Mixin> {
    vec![
        panel_style(),
        css(&format!("position: sticky; top: 12px; z-index: 1; padding: 14px; margin-bottom: {margin_bottom}px; background: #151c35; box-shadow: 0 8px 24px rgba(0,0,0,0.25);")),
    ]
}

fn newsletter_button_style() -> Mixin {
    css("border: 1px solid rgba(255,255,255,0.2); border-radius: 10px; padding: 10px 14px; color: #ffffff; background: rgba(129,140,248,0.35); cursor: pointer; font-weight: 700; &:hover { background: rgba(129,140,248,0.5); } &:focus-visible { outline: 2px solid #a5b4fc; outline-offset: 2px; }")
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LoadedAtProps {
    pub loaded_at: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct VariantProps {
    /// `"list"` or `"detail"`.
    pub variant: String,
}

// ---------------------------------------------------------------------------------------
// Scroll restoration
// ---------------------------------------------------------------------------------------

pub struct ScrollRestorationList {
    interactions: u32,
}

impl Component for ScrollRestorationList {
    type Props = LoadedAtProps;
    rsmix::client_entry!("ScrollRestorationList");

    fn setup(_: &LoadedAtProps, _: &Handle<Self>) -> Self {
        ScrollRestorationList { interactions: 0 }
    }

    fn render(&self, props: &LoadedAtProps, h: &Handle<Self>) -> Node {
        rsx! {
            <section id="scroll-restoration-list">
                <div mix={sticky_panel(12)}>
                    <strong>"Blocking frame client entry"</strong>
                    <span mix={[muted_style(), css("margin-left: 8px;")]}>{format!("resolved at {}", props.loaded_at)}</span>
                    <button type="button" mix={[on("click", h.on(|this, h| { this.interactions += 1; h.update(); })), css("margin-left: 12px;")]}>
                        {format!("Frame hydration check: {}", self.interactions)}
                    </button>
                </div>
                {list_rows("List row")}
            </section>
        }
    }
}

pub struct StoreScrollReproduction {
    interactions: u32,
}

impl Component for StoreScrollReproduction {
    type Props = VariantProps;
    rsmix::client_entry!("StoreScrollReproduction");

    fn setup(_: &VariantProps, _: &Handle<Self>) -> Self {
        StoreScrollReproduction { interactions: 0 }
    }

    fn render(&self, props: &VariantProps, h: &Handle<Self>) -> Node {
        let list = props.variant == "list";
        let body = if list {
            rsx! {
                <div>
                    <section mix={[panel_style(), css("margin-bottom: 20px;")]}>
                        <h2 mix={css("margin-top: 0; font-size: 18px;")}>"Traversal restoration"</h2>
                        <ol mix={css("margin-bottom: 0; padding-left: 22px; line-height: 1.7;")}>
                            <li>"Click the top-level hydration check to give the client entry local state."</li>
                            <li>"Scroll to the end of the collection and open the detail view."</li>
                            <li>"Use the browser Back button\u{2014}not a return link."</li>
                            <li>"Confirm the hydration count and collection scroll position are restored."</li>
                        </ol>
                    </section>
                    <Frame src="/frames/scroll-restoration-items"/>
                </div>
            }
        } else {
            let select_size = on("click", h.on(|_, _| {
                navigate(
                    "/scroll-restoration/detail?size=small",
                    NavigateOptions { history: Some(History::Replace), reset_scroll: Some(Reset::Manual), ..Default::default() },
                );
            }));
            rsx! {
                <article id="scroll-restoration-detail" mix={[panel_style(), css("min-height: 1000px; display: flex; flex-direction: column; justify-content: space-between; padding: 20px; background: rgba(255,255,255,0.03);")]}>
                    <div>
                        <h2 mix={section_heading_style()}>"Short detail view"</h2>
                        <p mix={[lead_style(), css("line-height: 1.6;")]}>
                            "The top-level client entry now renders much less content than the collection. Use the browser Back button while this short layout is present."
                        </p>
                        <button type="button" mix={[newsletter_button_style(), select_size]}>"Select size without resetting scroll"</button>
                    </div>
                    <p mix={[lead_style(), css("margin-bottom: 0;")]}>"Use the browser Back button now."</p>
                </article>
            }
        };
        rsx! {
            <section id="store-scroll-reproduction">
                <div mix={sticky_panel(20)}>
                    <strong>"Store-style top-level client entry"</strong>
                    <span mix={[muted_style(), css("margin-left: 8px;")]}>
                        {if list { "rendering the collection frame" } else { "rendering a short detail" }}
                    </span>
                    <button type="button" mix={[on("click", h.on(|this, h| { this.interactions += 1; h.update(); })), css("margin-left: 12px;")]}>
                        {format!("Hydration check: {}", self.interactions)}
                    </button>
                </div>
                {body}
            </section>
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct NewsletterProps {
    /// `"push"` or `"replace"` after a signup.
    pub history: Option<String>,
}

pub struct NewsletterSignup;

impl Component for NewsletterSignup {
    type Props = NewsletterProps;
    rsmix::client_entry!("NewsletterSignup");

    fn setup(_: &NewsletterProps, _: &Handle<Self>) -> Self {
        NewsletterSignup
    }

    fn render(&self, props: &NewsletterProps, h: &Handle<Self>) -> Node {
        // Apply the clicked button's history mode to the form before the navigation starts.
        let submit = h.on_event(|_, _, event| {
            if let Some(history) = event.submitter_attr("value").filter(|v| v == "push" || v == "replace") {
                event.set_current_attr("data-rmx-history", &history);
            }
        });
        let status = props.history.clone().map(|history| {
            rsx! {
                <p role="status" mix={css("border: 1px solid rgba(34,197,94,0.45); border-radius: 10px; padding: 12px; color: #bbf7d0; background: rgba(34,197,94,0.12);")}>
                    "Subscribed using a " <strong>{history}</strong> " navigation. The form stayed in view after the redirect."
                </p>
            }
        });
        rsx! {
            <section aria-labelledby="newsletter-heading" mix={[panel_style(), css("margin-top: 20px; padding: 20px; background: rgba(34,197,94,0.08);")]}>
                <h2 id="newsletter-heading" mix={css("margin-top: 0; font-size: 20px;")}>"Newsletter form navigation"</h2>
                <p mix={[lead_style(), css("line-height: 1.6;")]}>
                    "This form submits to another URL and redirects back here. Both buttons preserve the current scroll position with "
                    <code>"data-rmx-reset-scroll=\"false\""</code>
                    ". A userland submit handler applies the selected history mode to the form before navigation."
                </p>
                {status}
                <form method="post" action="/scroll-restoration/newsletter" data-rmx-reset-scroll="false" mix={[on("submit", submit), css("display: grid; gap: 12px;")]}>
                    <label for="newsletter-email" mix={css("display: grid; gap: 6px;")}>
                        <span mix={css("font-weight: 700;")}>"Email address"</span>
                        <input id="newsletter-email" name="email" type="email" autocomplete="email" required placeholder="you@example.com"
                            mix={css("width: 100%; max-width: 420px; box-sizing: border-box; border: 1px solid rgba(255,255,255,0.2); border-radius: 10px; padding: 10px 12px; color: #e9eefc; background: rgba(255,255,255,0.06);")}/>
                    </label>
                    <div mix={css("display: flex; flex-wrap: wrap; gap: 10px;")}>
                        <button type="submit" name="history" value="push" mix={newsletter_button_style()}>"Subscribe with push"</button>
                        <button type="submit" name="history" value="replace" mix={newsletter_button_style()}>"Subscribe with replace"</button>
                    </div>
                </form>
                <p mix={css("margin-bottom: 0; color: #9aa8e8; font-size: 14px; line-height: 1.5;")}>
                    "After a push, Back returns to the previous entry. A replace updates the current entry instead. Neither submission should jump to the top of this page."
                </p>
            </section>
        }
    }
}

// ---------------------------------------------------------------------------------------
// Scroll anchoring
// ---------------------------------------------------------------------------------------

pub struct ScrollAnchoringList;

impl Component for ScrollAnchoringList {
    type Props = LoadedAtProps;
    rsmix::client_entry!("ScrollAnchoringList");

    fn setup(_: &LoadedAtProps, _: &Handle<Self>) -> Self {
        ScrollAnchoringList
    }

    fn render(&self, props: &LoadedAtProps, _: &Handle<Self>) -> Node {
        rsx! {
            <section id="scroll-anchoring-list">
                <p mix={muted_style()}>{format!("Blocking frame resolved at {}", props.loaded_at)}</p>
                {list_rows("Anchoring row")}
            </section>
        }
    }
}

pub struct ScrollAnchoringReproduction {
    interactions: u32,
}

impl Component for ScrollAnchoringReproduction {
    type Props = VariantProps;
    rsmix::client_entry!("ScrollAnchoringReproduction");

    fn setup(_: &VariantProps, _: &Handle<Self>) -> Self {
        ScrollAnchoringReproduction { interactions: 0 }
    }

    fn render(&self, props: &VariantProps, h: &Handle<Self>) -> Node {
        let frame = (props.variant == "list").then(|| rsx! { <Frame src="/frames/scroll-anchoring-items"/> });
        rsx! {
            <section id="scroll-anchoring-reproduction">
                <div mix={sticky_panel(20)}>
                    <strong>"Preserved collection client entry"</strong>
                    <button type="button" mix={[on("click", h.on(|this, h| { this.interactions += 1; h.update(); })), css("margin-left: 12px;")]}>
                        {format!("Hydration check: {}", self.interactions)}
                    </button>
                </div>
                {frame}
            </section>
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct EmptyProps {}

pub struct ScrollAnchoringDetail;

impl Component for ScrollAnchoringDetail {
    type Props = EmptyProps;
    rsmix::client_entry!("ScrollAnchoringDetail");

    fn setup(_: &EmptyProps, _: &Handle<Self>) -> Self {
        ScrollAnchoringDetail
    }

    fn render(&self, _: &EmptyProps, _: &Handle<Self>) -> Node {
        rsx! {
            <article mix={[panel_style(), css("min-height: 1000px; display: flex; flex-direction: column; justify-content: space-between; padding: 20px; background: rgba(255,255,255,0.03);")]}>
                <div>
                    <h2 mix={section_heading_style()}>"Scroll anchoring detail"</h2>
                    <p mix={[lead_style(), css("line-height: 1.6;")]}>"This detail-only client entry is inserted before the preserved collection entry."</p>
                </div>
                <p mix={[lead_style(), css("margin-bottom: 0;")]}>"Use the browser Back button now."</p>
            </article>
        }
    }
}

// ---------------------------------------------------------------------------------------
// Root reload client entries
// ---------------------------------------------------------------------------------------

thread_local! {
    static PERSISTENT_SETUPS: Cell<u32> = const { Cell::new(0) };
    static REMOVABLE_SETUPS: Cell<u32> = const { Cell::new(0) };
    /// The removable entry was disposed (Remix dispatches a window event).
    static ENTRY_DISPOSED: TypedEventTarget<()> = TypedEventTarget::new();
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RootReloadControlsProps {
    pub include_removed: bool,
    pub server_version: String,
    pub with_removed_href: String,
    pub without_removed_href: String,
}

pub struct RootReloadControls {
    pending_href: Option<String>,
    removed_dispose_events: u32,
}

impl Component for RootReloadControls {
    type Props = RootReloadControlsProps;
    rsmix::client_entry!("RootReloadControls");

    fn setup(_: &RootReloadControlsProps, h: &Handle<Self>) -> Self {
        let disposed = ENTRY_DISPOSED.with(Clone::clone);
        h.listen(&disposed, |this, h, _| {
            this.removed_dispose_events += 1;
            h.update();
        });
        let top = h.frames().top();
        let src = top.clone();
        h.on_frame(&top, FrameEvent::ReloadStart, move |this, h| {
            this.pending_href = Some(relative(&src.src()));
            h.update();
        });
        h.on_frame(&top, FrameEvent::ReloadComplete, |this, h| {
            this.pending_href = None;
            h.update();
        });
        RootReloadControls { pending_href: None, removed_dispose_events: 0 }
    }

    fn render(&self, props: &RootReloadControlsProps, h: &Handle<Self>) -> Node {
        let reload = |href: String| {
            h.on(move |_, h| h.frames().top().reload_with(ReloadOptions { src: Some(href.clone()), ..Default::default() }))
        };
        let pending = |href: &str| self.pending_href.as_deref() == Some(href);
        rsx! {
            <section id="root-reload-controls" mix={css("border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; padding: 16px; background: rgba(255,255,255,0.04);")}>
                <h2 mix={css("margin-top: 0; font-size: 16px;")}>"Root reload controls"</h2>
                <dl mix={css("display: grid; grid-template-columns: max-content 1fr; gap: 6px 12px; margin: 0 0 14px; color: #b9c6ff;")}>
                    <dt>"Server props"</dt>
                    <dd class="server-props" mix={css("margin: 0; color: #e9eefc; font-variant-numeric: tabular-nums;")}>{props.server_version.clone()}</dd>
                    <dt>"Removable entry"</dt>
                    <dd class="removable-state" mix={css("margin: 0; color: #e9eefc;")}>{if props.include_removed { "present" } else { "absent" }}</dd>
                    <dt>"Dispose events"</dt>
                    <dd class="dispose-events" mix={css("margin: 0; color: #e9eefc; font-variant-numeric: tabular-nums;")}>{self.removed_dispose_events}</dd>
                </dl>
                <div mix={css("display: flex; gap: 8px; flex-wrap: wrap;")}>
                    <button type="button" id="reload-with" mix={[root_button_style(), on("click", reload(props.with_removed_href.clone()))]}>
                        {if pending(&props.with_removed_href) { "Reloading with entry\u{2026}" } else { "Reload with removable entry" }}
                    </button>
                    <button type="button" id="reload-without" mix={[root_button_style(), on("click", reload(props.without_removed_href.clone()))]}>
                        {if pending(&props.without_removed_href) { "Reloading without entry\u{2026}" } else { "Reload without removable entry" }}
                    </button>
                </div>
            </section>
        }
    }
}

/// `/path?query` of an absolute URL (frame sources are stored absolute in the browser).
fn relative(src: &str) -> String {
    match src.find("://").and_then(|i| src[i + 3..].find('/').map(|j| i + 3 + j)) {
        Some(start) => src[start..].to_string(),
        None => src.to_string(),
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ServerVersionProps {
    pub server_version: String,
}

pub struct PersistentRootReloadEntry {
    setup_id: u32,
    local_count: u32,
}

impl Component for PersistentRootReloadEntry {
    type Props = ServerVersionProps;
    rsmix::client_entry!("PersistentRootReloadEntry");

    fn setup(_: &ServerVersionProps, h: &Handle<Self>) -> Self {
        h.queue_task(|this, h| {
            this.setup_id = PERSISTENT_SETUPS.with(|c| {
                c.set(c.get() + 1);
                c.get()
            });
            h.update();
        });
        PersistentRootReloadEntry { setup_id: 0, local_count: 0 }
    }

    fn render(&self, props: &ServerVersionProps, h: &Handle<Self>) -> Node {
        rsx! {
            <section id="persistent-entry" mix={card_style()}>
                <h2 mix={css("margin-top: 0; font-size: 16px;")}>"Persistent entry"</h2>
                <p mix={css("margin-top: 0; color: #b9c6ff;")}>"This client entry should keep its setup and local state across root reloads while receiving fresh server props."</p>
                {entry_details(self.setup_id, self.local_count, &props.server_version)}
                <button type="button" class="increment" mix={[root_button_style(), on("click", h.on(|this, h| { this.local_count += 1; h.update(); }))]}>
                    "Increment persistent count"
                </button>
                {entry_frame("persistent", "persistent-root-reload-entry-frame", "Loading persistent entry frame\u{2026}")}
            </section>
        }
    }
}

pub struct RemovableRootReloadEntry {
    setup_id: u32,
    local_count: u32,
}

impl Component for RemovableRootReloadEntry {
    type Props = ServerVersionProps;
    rsmix::client_entry!("RemovableRootReloadEntry");

    fn setup(_: &ServerVersionProps, h: &Handle<Self>) -> Self {
        h.queue_task(|this, h| {
            this.setup_id = REMOVABLE_SETUPS.with(|c| {
                c.set(c.get() + 1);
                c.get()
            });
            h.update();
        });
        #[cfg(target_arch = "wasm32")]
        {
            let on_abort = wasm_bindgen::closure::Closure::once_into_js(|| ENTRY_DISPOSED.with(|t| t.dispatch(&())));
            let options = web_sys::AddEventListenerOptions::new();
            options.set_once(true);
            let _ = h.signal().add_event_listener_with_callback_and_add_event_listener_options("abort", wasm_bindgen::JsCast::unchecked_ref(&on_abort), &options);
        }
        RemovableRootReloadEntry { setup_id: 0, local_count: 0 }
    }

    fn render(&self, props: &ServerVersionProps, h: &Handle<Self>) -> Node {
        rsx! {
            <section id="removable-entry" mix={card_style()}>
                <h2 mix={css("margin-top: 0; font-size: 16px;")}>"Removable entry"</h2>
                <p mix={css("margin-top: 0; color: #b9c6ff;")}>"Reload without this entry. It should disappear and dispatch one dispose event."</p>
                {entry_details(self.setup_id, self.local_count, &props.server_version)}
                <button type="button" class="increment" mix={[root_button_style(), on("click", h.on(|this, h| { this.local_count += 1; h.update(); }))]}>
                    "Increment removable count"
                </button>
                {entry_frame("removable", "removable-root-reload-entry-frame", "Loading removable entry frame\u{2026}")}
            </section>
        }
    }
}

fn entry_frame(entry_label: &str, frame_name: &str, fallback_label: &'static str) -> Node {
    rsx! {
        <div mix={css("margin-top: 16px;")}>
            <h3 mix={css("margin: 0 0 6px; font-size: 14px;")}>{format!("Frame inside {entry_label} entry")}</h3>
            <p mix={css("margin: 0 0 10px; color: #b9c6ff;")}>"This frame has a stable name and src while its containing client entry remains mounted across root reloads."</p>
            <Frame name={Some(frame_name.to_string())} src="/frames/root-reload-entry-frame"
                fallback={Some(rsx! { <div mix={css("color: #9aa8e8;")}>{fallback_label}</div> })}/>
        </div>
    }
}

fn entry_details(setup_id: u32, local_count: u32, server_version: &str) -> Node {
    let setup = if setup_id == 0 { "hydrating".to_string() } else { setup_id.to_string() };
    rsx! {
        <dl mix={css("display: grid; grid-template-columns: max-content 1fr; gap: 6px 12px; margin: 0 0 14px;")}>
            <dt mix={css("color: #b9c6ff;")}>"Setup id"</dt>
            <dd class="setup-id" mix={css("margin: 0; font-variant-numeric: tabular-nums;")}>{setup}</dd>
            <dt mix={css("color: #b9c6ff;")}>"Local count"</dt>
            <dd class="local-count" mix={css("margin: 0; font-variant-numeric: tabular-nums;")}>{local_count}</dd>
            <dt mix={css("color: #b9c6ff;")}>"Server props"</dt>
            <dd class="entry-server-props" mix={css("margin: 0; font-variant-numeric: tabular-nums;")}>{server_version.to_string()}</dd>
        </dl>
    }
}

fn root_button_style() -> Mixin {
    css("padding: 8px 12px; border-radius: 10px; border: 1px solid rgba(255,255,255,0.18); background: rgba(255,255,255,0.06); color: #e9eefc; cursor: pointer; &:hover { background: rgba(255,255,255,0.10); }")
}

fn card_style() -> Mixin {
    css("border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; padding: 16px; background: rgba(255,255,255,0.03);")
}

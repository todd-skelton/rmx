//! Headless tabs (Remix's `@remix-run/ui/tabs`): app-owned markup, with roles, ids,
//! keyboard navigation and active state provided by mixins.
//!
//! ```ignore
//! use rsmix_ui::tabs;
//!
//! rsx! {
//!     <tabs::Context default_active_tab="overview">
//!         <div mix={tabs::root()}>
//!             <div aria-label="Project sections" mix={tabs::list()}>
//!                 <button mix={tabs::tab("overview")}>"Overview"</button>
//!                 <button mix={tabs::tab(tabs::TabOptions::new("settings").disabled(true))}>"Settings"</button>
//!             </div>
//!             <div mix={tabs::panel("overview")}>"..."</div>
//!             <div mix={tabs::panel("settings")}>"..."</div>
//!         </div>
//!     </tabs::Context>
//! }
//! ```
//!
//! Without `active_tab` the provider keeps its own state, starting from
//! `default_active_tab` or the first enabled tab. Activating a tab calls
//! `on_active_tab_change` and dispatches a bubbling `rmx:tabs-change` event from the
//! `root` element (read it with [`on_tabs_change`] and [`tabs_change`]).

use std::cell::RefCell;
use std::rc::Rc;

use rsmix::{mixin, on, Children, Component, Element, Event, EventHandler, Handle, HostMixin, Mixin, MixinHandle, Node};

/// The event dispatched from the root when the active tab changes.
pub const TABS_CHANGE_EVENT: &str = "rmx:tabs-change";

/// Props for [`Context`] (Remix's `TabsContextProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    /// Controlled active tab.
    pub active_tab: Option<String>,
    pub default_active_tab: Option<String>,
    pub disabled: bool,
    pub on_active_tab_change: Option<OnActiveTabChange>,
    pub children: Children,
}

/// Called with the newly active tab.
pub type OnActiveTabChange = Rc<dyn Fn(&str)>;

/// Which tab [`TabsState::activate_in_direction`] moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    First,
    Last,
    Next,
    Previous,
}

/// A tab registered by the `tab` mixin during the provider's render.
struct RegisteredTab {
    name: String,
    disabled: bool,
    node: NodeSlot,
}

/// The live element of a tab, set by its ref.
type NodeSlot = Rc<RefCell<Option<Rc<rsmix::NodeHandle>>>>;

/// The state the provider shares with its mixins (Remix's `TabsContextValue`).
pub struct TabsState {
    id: String,
    props: RefCell<ContextProps>,
    uncontrolled: RefCell<Option<Option<String>>>,
    tabs: RefCell<Vec<RegisteredTab>>,
    #[cfg(target_arch = "wasm32")]
    root: RefCell<Option<web_sys::Element>>,
    update: Box<dyn Fn()>,
}

fn encode_id_part(value: &str) -> String {
    // `encodeURIComponent(value).replace(/%/g, '-')`
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("-{b:02X}"));
        }
    }
    out
}

impl TabsState {
    /// The active tab's name.
    pub fn active_tab(&self) -> Option<String> {
        if let Some(active) = &self.props.borrow().active_tab {
            return Some(active.clone());
        }
        let mut uncontrolled = self.uncontrolled.borrow_mut();
        uncontrolled.get_or_insert_with(|| self.props.borrow().default_active_tab.clone()).clone()
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.active_tab().as_deref() == Some(name)
    }

    pub fn disabled(&self) -> bool {
        self.props.borrow().disabled
    }

    pub fn tab_id(&self, name: &str) -> String {
        format!("{}-{}-tab", self.id, encode_id_part(name))
    }

    pub fn panel_id(&self, name: &str) -> String {
        format!("{}-{}-panel", self.id, encode_id_part(name))
    }

    fn register(&self, tab: RegisteredTab) {
        let first_enabled = !tab.disabled && self.props.borrow().active_tab.is_none() && self.active_tab().is_none();
        if first_enabled {
            *self.uncontrolled.borrow_mut() = Some(Some(tab.name.clone()));
        }
        self.tabs.borrow_mut().push(tab);
    }

    /// Activate a tab (ignored when it or the tabs are disabled).
    pub fn activate(&self, name: &str) {
        if self.disabled() {
            return;
        }
        if self.tabs.borrow().iter().any(|t| t.name == name && t.disabled) {
            return;
        }
        let previous = self.active_tab();
        if previous.as_deref() == Some(name) {
            return;
        }
        let on_change = {
            let props = self.props.borrow();
            if props.active_tab.is_none() {
                *self.uncontrolled.borrow_mut() = Some(Some(name.to_string()));
                (self.update)();
            }
            props.on_active_tab_change.clone()
        };
        if let Some(f) = on_change {
            f(name);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = self.root.borrow().clone() {
            use crate::event::Field;
            crate::event::dispatch(&root, TABS_CHANGE_EVENT, &[Field::Str("activeTab", Some(name)), Field::Str("previousActiveTab", previous.as_deref())]);
        }
    }

    /// Focus and activate the first, last, next or previous enabled tab.
    pub fn activate_in_direction(&self, name: &str, direction: Direction) {
        let target = {
            let tabs = self.tabs.borrow();
            let focusable: Vec<&RegisteredTab> = tabs.iter().filter(|t| !t.disabled && t.node.borrow().is_some()).collect();
            if focusable.is_empty() {
                return;
            }
            let current = focusable.iter().position(|t| t.name == name);
            let last = focusable.len() - 1;
            let index = match direction {
                Direction::First => 0,
                Direction::Last => last,
                Direction::Previous => match current {
                    Some(i) if i > 0 => i - 1,
                    _ => last,
                },
                Direction::Next => match current {
                    Some(i) if i < last => i + 1,
                    _ => 0,
                },
            };
            let tab = focusable[index];
            let target = (tab.name.clone(), tab.node.borrow().clone());
            target
        };
        if let Some(node) = target.1 {
            node.focus();
        }
        self.activate(&target.0);
    }
}

/// The tabs provider (Remix's `tabs.Context`). Renders its children.
pub struct Context {
    state: Rc<TabsState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let handle = h.clone();
        let state = Rc::new(TabsState {
            id: h.dom_id(),
            props: RefCell::new(props.clone()),
            uncontrolled: RefCell::new(None),
            tabs: RefCell::new(Vec::new()),
            #[cfg(target_arch = "wasm32")]
            root: RefCell::new(None),
            update: Box::new(move || handle.update()),
        });
        h.set_context(state.clone());
        Context { state }
    }

    fn render(&self, props: &ContextProps, _: &Handle<Self>) -> Node {
        *self.state.props.borrow_mut() = props.clone();
        self.state.tabs.borrow_mut().clear();
        props.children.node().clone()
    }
}

fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<TabsState> {
    h.context::<Rc<TabsState>>().map(|s| (*s).clone()).expect("tabs mixins must be inside <tabs::Context>")
}

/// The element containing the tabs: change events are dispatched from it.
pub fn root() -> Mixin {
    mixin::<Root>(())
}

/// The `role="tablist"` element.
pub fn list() -> Mixin {
    mixin::<List>(())
}

/// A tab button.
pub fn tab(options: impl Into<TabOptions>) -> Mixin {
    mixin::<Tab>(options.into())
}

/// The panel for the tab named `name`.
pub fn panel(name: impl Into<String>) -> Mixin {
    mixin::<Panel>(name.into())
}

/// Listen for active tab changes on this element or its descendants.
pub fn on_tabs_change(handler: EventHandler) -> Mixin {
    on(TABS_CHANGE_EVENT, handler)
}

/// `(active_tab, previous_active_tab)` from an `rmx:tabs-change` event.
pub fn tabs_change(event: &Event) -> Option<(String, Option<String>)> {
    let active = crate::event::str_field(event, "activeTab")?;
    Some((active, crate::event::str_field(event, "previousActiveTab")))
}

/// Options for [`tab`] (Remix's `TabOptions`). A `&str` converts to an enabled tab.
#[derive(Clone)]
pub struct TabOptions {
    pub name: String,
    pub disabled: bool,
}

impl TabOptions {
    pub fn new(name: impl Into<String>) -> Self {
        TabOptions { name: name.into(), disabled: false }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl From<&str> for TabOptions {
    fn from(name: &str) -> Self {
        TabOptions::new(name)
    }
}

impl From<String> for TabOptions {
    fn from(name: String) -> Self {
        TabOptions::new(name)
    }
}

pub struct Root {
    state: Rc<TabsState>,
}

impl HostMixin for Root {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let state = state(h);
        #[cfg(target_arch = "wasm32")]
        {
            let registered = state.clone();
            h.queue_task(move |_, h, node| {
                *registered.root.borrow_mut() = Some(node.element().clone());
                let node = node.element().clone();
                let unregister = registered.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut root = unregister.root.borrow_mut();
                    if root.as_ref() == Some(&node) {
                        *root = None;
                    }
                });
            });
        }
        Root { state }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-disabled", self.state.disabled().then_some(""));
    }
}

pub struct List {
    state: Rc<TabsState>,
}

impl HostMixin for List {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        List { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("role", "tablist");
        host.set_attr("aria-disabled", self.state.disabled().then_some("true"));
    }
}

pub struct Tab {
    state: Rc<TabsState>,
    tag: &'static str,
    node: NodeSlot,
    name: String,
}

impl HostMixin for Tab {
    type Args = TabOptions;

    fn setup(h: &MixinHandle<Self>, tag: &'static str) -> Self {
        Tab { state: state(h), tag, node: Rc::default(), name: String::new() }
    }

    fn render(&mut self, options: &TabOptions, host: &mut Element, h: &MixinHandle<Self>) {
        let disabled = self.state.disabled() || options.disabled || crate::event::attr_is_true(host.attr("disabled"));
        self.name = options.name.clone();
        self.state.register(RegisteredTab { name: options.name.clone(), disabled, node: self.node.clone() });
        let active = self.state.is_active(&options.name);

        host.set_attr("aria-controls", self.state.panel_id(&options.name));
        host.set_attr("aria-selected", if active { "true" } else { "false" });
        host.set_attr("data-state", if active { "active" } else { "inactive" });
        host.set_attr("disabled", disabled);
        host.set_attr("id", self.state.tab_id(&options.name));
        host.set_attr("role", "tab");
        host.set_attr("tabindex", if active && !disabled { "0" } else { "-1" });
        if self.tag == "button" {
            host.set_attr("type", "button");
        }
        let slot = self.node.clone();
        host.mix(rsmix::node_ref(move |node, signal| {
            let node = Rc::new(node.clone());
            *slot.borrow_mut() = Some(node.clone());
            let slot = slot.clone();
            signal.on_abort(move || {
                let mut current = slot.borrow_mut();
                if current.as_ref().is_some_and(|n| Rc::ptr_eq(n, &node)) {
                    *current = None;
                }
            });
        }));
        host.mix(on("click", h.on(|this, _| this.state.activate(&this.name))));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                let direction = match event.key().as_str() {
                    "ArrowDown" | "ArrowRight" => Some(Direction::Next),
                    "ArrowUp" | "ArrowLeft" => Some(Direction::Previous),
                    "Home" => Some(Direction::First),
                    "End" => Some(Direction::Last),
                    "Enter" | " " => None,
                    _ => return,
                };
                event.prevent_default();
                match direction {
                    Some(direction) => this.state.activate_in_direction(&this.name, direction),
                    None => this.state.activate(&this.name),
                }
            }),
        ));
    }
}

pub struct Panel {
    state: Rc<TabsState>,
}

impl HostMixin for Panel {
    type Args = String;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Panel { state: state(h) }
    }

    fn render(&mut self, name: &String, host: &mut Element, _: &MixinHandle<Self>) {
        let active = self.state.is_active(name);
        host.set_attr("aria-labelledby", self.state.tab_id(name));
        host.set_attr("data-state", if active { "active" } else { "inactive" });
        host.set_attr("hidden", !active);
        host.set_attr("id", self.state.panel_id(name));
        host.set_attr("inert", !active);
        host.set_attr("role", "tabpanel");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsmix::{render_to_string, rsx};

    fn example(default_active_tab: Option<&str>) -> Node {
        let default_active_tab = default_active_tab.map(str::to_string);
        rsx! {
            <Context default_active_tab={default_active_tab}>
                <div mix={root()}>
                    <div mix={list()}>
                        <button mix={tab(TabOptions::new("disabled").disabled(true))}>"Disabled"</button>
                        <button mix={tab("first-enabled")}>"First enabled"</button>
                    </div>
                    <div mix={panel("disabled")}>"Disabled content."</div>
                    <div mix={panel("first-enabled")}>"Enabled content."</div>
                </div>
            </Context>
        }
    }

    #[test]
    fn renders_the_first_enabled_tab_active_on_the_server() {
        let html = render_to_string(&example(None));
        assert!(html.contains(r#"<div role="tablist">"#), "{html}");
        assert!(html.contains(r#"aria-selected="false" data-state="inactive" disabled"#), "{html}");
        assert!(html.contains(r#"aria-selected="true" data-state="active""#), "{html}");
        assert!(html.contains(r#"tabindex="0" type="button">First enabled"#), "{html}");
        // The disabled tab's panel is hidden and inert; the active one is not.
        assert!(html.contains(r#"data-state="inactive" hidden id="#), "{html}");
        assert!(html.contains(r#"inert role="tabpanel">Disabled content."#), "{html}");
        assert!(html.contains(r#"role="tabpanel">Enabled content."#) && !html.contains(r#"inert role="tabpanel">Enabled"#), "{html}");
    }

    #[test]
    fn default_active_tab_and_linked_ids() {
        let html = render_to_string(&example(Some("disabled")));
        // A default may name a disabled tab; it stays active until another is chosen.
        assert!(html.contains(r#"aria-selected="true" data-state="active" disabled"#), "{html}");
        let tab_id = html.split(r#"id=""#).nth(1).and_then(|s| s.split('"').next()).unwrap();
        assert!(tab_id.ends_with("-disabled-tab"), "{tab_id}");
        assert!(html.contains(&format!(r#"aria-labelledby="{tab_id}""#)), "{html}");
    }

    #[test]
    fn encodes_id_parts_like_encode_uri_component() {
        assert_eq!(encode_id_part("a b/c"), "a-20b-2Fc");
        assert_eq!(encode_id_part("ok-_.!~*'()"), "ok-_.!~*'()");
    }
}

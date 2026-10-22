//! Headless accordion (Remix's `@remix-run/ui/accordion`).
//!
//! ```ignore
//! use rmx_ui::accordion;
//!
//! rsx! {
//!     <accordion::Context default_value="shipping">
//!         <div mix={accordion::root()}>
//!             <accordion::ItemContext value="shipping">
//!                 <div mix={accordion::item()}>
//!                     <h3><button type="button" mix={accordion::trigger()}>"Shipping"</button></h3>
//!                     <div mix={accordion::content()}>"..."</div>
//!                 </div>
//!             </accordion::ItemContext>
//!         </div>
//!     </accordion::Context>
//! }
//! ```
//!
//! Single mode (the default) keeps at most one item open; `collapsible: Some(false)` keeps
//! the open item from closing. `multiple` lets any set of items be open. Pass `value` to
//! control the open items. Changes call `on_value_change` and dispatch a bubbling
//! `rmx:accordion-change` event from the `root` element (read it with
//! [`accordion_change`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{mixin, on, Children, Component, Element, Event, EventHandler, Handle, HostMixin, IntoProp, Mixin, MixinHandle, Node, NodeHandle};

/// The event dispatched from the root when items open or close.
pub const ACCORDION_CHANGE_EVENT: &str = "rmx:accordion-change";

/// Open items for the `value` and `default_value` props: at most one in single mode.
/// Converts from a `&str`, a `String`, an array of names or a `Vec<String>`, so
/// `default_value="shipping"` works in `rsx!`. `Unset` (the default) leaves `value`
/// uncontrolled; `Value::closed()` controls it with nothing open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Value {
    #[default]
    Unset,
    Items(Vec<String>),
}

impl Value {
    /// Controlled, with no item open.
    pub fn closed() -> Value {
        Value::Items(Vec::new())
    }

    fn items(&self) -> Option<&Vec<String>> {
        match self {
            Value::Unset => None,
            Value::Items(items) => Some(items),
        }
    }
}

impl IntoProp<Value> for &str {
    fn into_prop(self) -> Value {
        Value::Items(vec![self.to_string()])
    }
}

impl IntoProp<Value> for String {
    fn into_prop(self) -> Value {
        Value::Items(vec![self])
    }
}

impl<const N: usize> IntoProp<Value> for [&str; N] {
    fn into_prop(self) -> Value {
        Value::Items(self.iter().map(|s| s.to_string()).collect())
    }
}

impl IntoProp<Value> for Vec<String> {
    fn into_prop(self) -> Value {
        Value::Items(self)
    }
}

/// Called with the open items after a change.
pub type OnValueChange = Rc<dyn Fn(&[String])>;

/// Props for [`Context`] (Remix's `AccordionContextProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    /// `type: 'multiple'`: any number of items may be open.
    pub multiple: bool,
    /// Controlled open items.
    pub value: Value,
    pub default_value: Value,
    /// Single mode: whether the open item can be closed (default `true`).
    pub collapsible: Option<bool>,
    pub disabled: bool,
    pub on_value_change: Option<OnValueChange>,
    pub children: Children,
}

/// What [`AccordionState::focus_item`] moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    First,
    Last,
    Next,
    Previous,
}

type NodeSlot = Rc<RefCell<Option<Rc<NodeHandle>>>>;

struct RegisteredItem {
    value: String,
    disabled: bool,
    trigger: NodeSlot,
}

/// State shared by the provider and its mixins (Remix's `AccordionContextValue`).
pub struct AccordionState {
    props: RefCell<ContextProps>,
    uncontrolled: RefCell<Option<Vec<String>>>,
    items: RefCell<Vec<RegisteredItem>>,
    #[cfg(target_arch = "wasm32")]
    root: RefCell<Option<web_sys::Element>>,
    update: Box<dyn Fn()>,
}

impl AccordionState {
    pub fn multiple(&self) -> bool {
        self.props.borrow().multiple
    }

    pub fn disabled(&self) -> bool {
        self.props.borrow().disabled
    }

    pub fn collapsible(&self) -> bool {
        let props = self.props.borrow();
        props.multiple || props.collapsible.unwrap_or(true)
    }

    /// The open items.
    pub fn value(&self) -> Vec<String> {
        let props = self.props.borrow();
        let mut value = match props.value.items() {
            Some(v) => v.clone(),
            None => self.uncontrolled.borrow_mut().get_or_insert_with(|| props.default_value.items().cloned().unwrap_or_default()).clone(),
        };
        if !props.multiple {
            value.truncate(1);
        }
        value
    }

    pub fn is_open(&self, item: &str) -> bool {
        self.value().iter().any(|v| v == item)
    }

    /// Open or close an item (Remix's `toggleItem`).
    pub fn toggle_item(&self, item: &str) {
        if self.disabled() {
            return;
        }
        let current = self.value();
        let open = current.iter().any(|v| v == item);
        let next: Vec<String> = if self.multiple() {
            if open {
                current.into_iter().filter(|v| v != item).collect()
            } else {
                current.into_iter().chain([item.to_string()]).collect()
            }
        } else if open {
            if !self.collapsible() {
                return;
            }
            Vec::new()
        } else {
            vec![item.to_string()]
        };
        let on_change = {
            let props = self.props.borrow();
            if props.value == Value::Unset {
                *self.uncontrolled.borrow_mut() = Some(next.clone());
                (self.update)();
            }
            props.on_value_change.clone()
        };
        if let Some(f) = on_change {
            f(&next);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = self.root.borrow().clone() {
            use crate::event::Field;
            let multiple = self.multiple();
            let kind = if multiple { "multiple" } else { "single" };
            let single = next.first().map(String::as_str);
            let value = if multiple { Field::List("value", &next) } else { Field::Str("value", single) };
            crate::event::dispatch(&root, ACCORDION_CHANGE_EVENT, &[Field::Str("accordionType", Some(kind)), Field::Str("itemValue", Some(item)), value]);
        }
    }

    /// Focus another enabled trigger.
    pub fn focus_item(&self, item: &str, direction: Direction) {
        let target = {
            let items = self.items.borrow();
            let focusable: Vec<&RegisteredItem> = items.iter().filter(|i| !i.disabled && i.trigger.borrow().is_some()).collect();
            if focusable.is_empty() {
                return;
            }
            let current = focusable.iter().position(|i| i.value == item);
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
            let node = focusable[index].trigger.borrow().clone();
            node
        };
        if let Some(node) = target {
            node.focus();
        }
    }
}

/// The accordion provider (Remix's `accordion.Context`). Renders its children.
pub struct Context {
    state: Rc<AccordionState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let handle = h.clone();
        let state = Rc::new(AccordionState {
            props: RefCell::new(props.clone()),
            uncontrolled: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            #[cfg(target_arch = "wasm32")]
            root: RefCell::new(None),
            update: Box::new(move || handle.update()),
        });
        h.set_context(state.clone());
        Context { state }
    }

    fn render(&self, props: &ContextProps, _: &Handle<Self>) -> Node {
        *self.state.props.borrow_mut() = props.clone();
        self.state.items.borrow_mut().clear();
        props.children.node().clone()
    }
}

/// Props for [`ItemContext`] (Remix's `AccordionItemOptions`).
#[derive(Clone, Default)]
pub struct ItemContextProps {
    pub value: String,
    pub disabled: bool,
    pub children: Children,
}

/// One item's state, shared with the `item`, `trigger` and `content` mixins.
pub struct ItemState {
    value: RefCell<String>,
    trigger_id: String,
    panel_id: String,
    disabled: Cell<bool>,
    open: Cell<bool>,
    locked_open: Cell<bool>,
    trigger: NodeSlot,
}

/// Provides one item's state (Remix's `accordion.ItemContext`). Renders its children.
pub struct ItemContext {
    state: Rc<ItemState>,
}

impl Component for ItemContext {
    type Props = ItemContextProps;

    fn setup(props: &ItemContextProps, h: &Handle<Self>) -> Self {
        let id = h.dom_id();
        let state = Rc::new(ItemState {
            value: RefCell::new(props.value.clone()),
            trigger_id: format!("{id}-trigger"),
            panel_id: format!("{id}-panel"),
            disabled: Cell::new(false),
            open: Cell::new(false),
            locked_open: Cell::new(false),
            trigger: Rc::default(),
        });
        h.set_context(state.clone());
        ItemContext { state }
    }

    fn render(&self, props: &ItemContextProps, h: &Handle<Self>) -> Node {
        let accordion = h.context::<Rc<AccordionState>>().expect("<accordion::ItemContext> must be inside <accordion::Context>");
        let state = &self.state;
        *state.value.borrow_mut() = props.value.clone();
        let disabled = accordion.disabled() || props.disabled;
        let open = accordion.is_open(&props.value);
        state.disabled.set(disabled);
        state.open.set(open);
        state.locked_open.set(!accordion.multiple() && !accordion.collapsible() && open);
        accordion.items.borrow_mut().push(RegisteredItem { value: props.value.clone(), disabled, trigger: state.trigger.clone() });
        props.children.node().clone()
    }
}

fn accordion<M: HostMixin>(h: &MixinHandle<M>) -> Rc<AccordionState> {
    h.context::<Rc<AccordionState>>().map(|s| (*s).clone()).expect("accordion mixins must be inside <accordion::Context>")
}

fn item_state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<ItemState> {
    h.context::<Rc<ItemState>>().map(|s| (*s).clone()).expect("accordion item mixins must be inside <accordion::ItemContext>")
}

/// The element containing the items: change events are dispatched from it.
pub fn root() -> Mixin {
    mixin::<Root>(())
}

/// An item's wrapper element.
pub fn item() -> Mixin {
    mixin::<Item>(())
}

/// The button that opens and closes an item.
pub fn trigger() -> Mixin {
    mixin::<Trigger>(false)
}

/// A trigger that is disabled on its own.
pub fn trigger_disabled(disabled: bool) -> Mixin {
    mixin::<Trigger>(disabled)
}

/// An item's panel.
pub fn content() -> Mixin {
    mixin::<Content>(())
}

/// Listen for accordion changes on this element or its descendants.
pub fn on_accordion_change(handler: EventHandler) -> Mixin {
    on(ACCORDION_CHANGE_EVENT, handler)
}

/// The data of an `rmx:accordion-change` event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccordionChange {
    pub multiple: bool,
    /// The item that was toggled.
    pub item_value: String,
    /// The open items after the change.
    pub value: Vec<String>,
}

/// Read an `rmx:accordion-change` event.
pub fn accordion_change(event: &Event) -> Option<AccordionChange> {
    let multiple = crate::event::str_field(event, "accordionType")? == "multiple";
    let item_value = crate::event::str_field(event, "itemValue")?;
    let value = if multiple {
        crate::event::list_field(event, "value").unwrap_or_default()
    } else {
        crate::event::str_field(event, "value").into_iter().collect()
    };
    Some(AccordionChange { multiple, item_value, value })
}

pub struct Root {
    state: Rc<AccordionState>,
}

impl HostMixin for Root {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let state = accordion(h);
        #[cfg(target_arch = "wasm32")]
        {
            let registered = state.clone();
            h.queue_task(move |_, h, node| {
                let node = node.element().clone();
                *registered.root.borrow_mut() = Some(node.clone());
                let registered = registered.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut root = registered.root.borrow_mut();
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
        host.set_attr("data-type", if self.state.multiple() { "multiple" } else { "single" });
    }
}

pub struct Item {
    item: Rc<ItemState>,
}

impl HostMixin for Item {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Item { item: item_state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("data-disabled", self.item.disabled.get().then_some(""));
        host.set_attr("data-state", if self.item.open.get() { "open" } else { "closed" });
    }
}

pub struct Trigger {
    accordion: Rc<AccordionState>,
    item: Rc<ItemState>,
    disabled: bool,
}

impl HostMixin for Trigger {
    type Args = bool;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Trigger { accordion: accordion(h), item: item_state(h), disabled: false }
    }

    fn render(&mut self, disabled: &bool, host: &mut Element, h: &MixinHandle<Self>) {
        let item = &self.item;
        let open = item.open.get();
        self.disabled = item.disabled.get() || *disabled || crate::event::attr_is_true(host.attr("disabled"));
        host.set_attr("aria-controls", item.panel_id.clone());
        host.set_attr("aria-disabled", item.locked_open.get().then_some("true"));
        host.set_attr("aria-expanded", if open { "true" } else { "false" });
        host.set_attr("data-state", if open { "open" } else { "closed" });
        host.set_attr("disabled", self.disabled);
        host.set_attr("id", item.trigger_id.clone());
        let slot = item.trigger.clone();
        host.mix(rmx::node_ref(move |node, _| {
            *slot.borrow_mut() = Some(Rc::new(node.clone()));
        }));
        host.mix(on(
            "click",
            h.on(|this, _| {
                if this.disabled || this.item.locked_open.get() {
                    return;
                }
                let value = this.item.value.borrow().clone();
                this.accordion.toggle_item(&value);
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                let direction = match event.key().as_str() {
                    "ArrowDown" => Direction::Next,
                    "ArrowUp" => Direction::Previous,
                    "Home" => Direction::First,
                    "End" => Direction::Last,
                    _ => return,
                };
                event.prevent_default();
                let value = this.item.value.borrow().clone();
                this.accordion.focus_item(&value, direction);
            }),
        ));
    }
}

pub struct Content {
    item: Rc<ItemState>,
}

impl HostMixin for Content {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Content { item: item_state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        let open = self.item.open.get();
        host.set_attr("aria-hidden", (!open).then_some("true"));
        host.set_attr("aria-labelledby", self.item.trigger_id.clone());
        host.set_attr("data-state", if open { "open" } else { "closed" });
        host.set_attr("id", self.item.panel_id.clone());
        host.set_attr("inert", !open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::{render_to_string, rsx};

    fn example(props: ContextProps) -> Node {
        let items: Vec<Node> = [("shipping", false), ("billing", false), ("archived", true)]
            .into_iter()
            .map(|(value, disabled)| {
                rsx! {
                    <ItemContext key={value} value={value} disabled={disabled}>
                        <div mix={item()}>
                            <button type="button" mix={trigger()}>{value}</button>
                            <div mix={content()}>{format!("{value} content")}</div>
                        </div>
                    </ItemContext>
                }
            })
            .collect();
        Node::component::<Context>(None, ContextProps { children: rsx! { <div mix={root()}>{items}</div> }.into(), ..props })
    }

    fn open_items(html: &str) -> Vec<&str> {
        html.split("aria-expanded=\"true\"").skip(1).filter_map(|s| s.split('>').nth(1)?.split('<').next()).collect()
    }

    #[test]
    fn single_default_value_and_aria_links() {
        let html = render_to_string(&example(ContextProps { default_value: "shipping".into_prop(), ..Default::default() }));
        assert_eq!(open_items(&html), ["shipping"], "{html}");
        assert!(html.contains(r#"data-type="single""#), "{html}");
        assert!(html.contains(r#"data-disabled="" data-state="closed""#), "disabled item: {html}");
        let trigger_id = html.split(r#"id=""#).nth(1).and_then(|s| s.split('"').next()).unwrap();
        assert!(trigger_id.ends_with("-trigger"));
        assert!(html.contains(&format!(r#"aria-labelledby="{trigger_id}""#)), "{html}");
        assert!(html.contains(r#"aria-hidden="true""#) && html.contains(" inert"), "{html}");
    }

    #[test]
    fn multiple_and_controlled_values() {
        let html = render_to_string(&example(ContextProps { multiple: true, default_value: ["shipping", "billing"].into_prop(), ..Default::default() }));
        assert_eq!(open_items(&html), ["shipping", "billing"], "{html}");
        let html = render_to_string(&example(ContextProps { value: "billing".into_prop(), default_value: "shipping".into_prop(), ..Default::default() }));
        assert_eq!(open_items(&html), ["billing"], "{html}");
        // Single mode keeps at most one item open.
        let html = render_to_string(&example(ContextProps { default_value: ["shipping", "billing"].into_prop(), ..Default::default() }));
        assert_eq!(open_items(&html), ["shipping"], "{html}");
    }

    #[test]
    fn non_collapsible_single_item_is_locked_open() {
        let html = render_to_string(&example(ContextProps { default_value: "shipping".into_prop(), collapsible: Some(false), ..Default::default() }));
        assert!(html.contains(r#"aria-disabled="true" aria-expanded="true""#), "{html}");
    }
}

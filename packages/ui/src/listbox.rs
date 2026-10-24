//! Selection and highlight behavior for a list of options (Remix's
//! `@remix-run/ui/listbox`). The app owns `value` and `active_value`; the listbox calls
//! `on_select` and `on_highlight` and handles keyboard navigation, typeahead, hover
//! highlighting and scrolling the active option into view.
//!
//! ```ignore
//! use rsmix_ui::listbox::{self, ListboxOption};
//!
//! rsx! {
//!     <listbox::Context value={self.value.clone()} active_value={self.active.clone()} on_select={select} on_highlight={highlight}>
//!         <div aria-label="Framework" tabindex="0" mix={listbox::list()}>
//!             <div mix={listbox::option(ListboxOption::new("remix", "Remix"))}>"Remix"</div>
//!         </div>
//!     </listbox::Context>
//! }
//! ```

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use rsmix::{mixin, on, Children, Component, Element, Handle, HostMixin, Mixin, MixinHandle, Node, NodeHandle};

use crate::typeahead::{hidden_typeahead, match_next_by_search_text};

/// An option's data (Remix's `ListboxOption` without the id, which the mixin assigns).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListboxOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
    /// Text typeahead matches against (defaults to the label).
    pub text_value: Vec<String>,
}

impl ListboxOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        ListboxOption { value: value.into(), label: label.into(), ..Default::default() }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn text_value(mut self, values: &[&str]) -> Self {
        self.text_value = values.iter().map(|s| s.to_string()).collect();
        self
    }

    pub(crate) fn search_values(&self) -> Vec<String> {
        if self.text_value.is_empty() {
            vec![self.label.clone()]
        } else {
            self.text_value.clone()
        }
    }
}

type NodeSlot = Rc<RefCell<Option<NodeHandle>>>;

/// An option rendered in the list (Remix's `ListboxRegisteredOption`).
#[derive(Clone)]
pub struct RegisteredOption {
    pub id: String,
    pub option: ListboxOption,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    node: NodeSlot,
}

impl RegisteredOption {
    /// The option element, once it is in the document.
    #[cfg(target_arch = "wasm32")]
    pub fn element(&self) -> Option<web_sys::HtmlElement> {
        let node = self.node.borrow();
        wasm_bindgen::JsCast::dyn_into(node.as_ref()?.element().clone()).ok()
    }

    /// Hidden with the `hidden` attribute.
    pub fn hidden(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.element().is_some_and(|el| el.hidden())
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    fn visible(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.element().is_some_and(|el| el.is_connected() && !el.hidden())
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    fn interactable(&self) -> bool {
        self.visible() && !self.option.disabled
    }

    fn scroll_into_view(&self) {
        #[cfg(target_arch = "wasm32")]
        if self.visible() {
            if let Some(el) = self.element() {
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                el.scroll_into_view_with_scroll_into_view_options(&options);
            }
        }
    }
}

/// `(value, option)` for `on_select` and `on_highlight`.
pub type OnChange = Rc<dyn Fn(Option<&str>, Option<&RegisteredOption>)>;
/// Runs after a selection (and its flash); the next selection waits for it.
pub type OnSelectSettled = Rc<dyn Fn(Option<String>, Option<RegisteredOption>) -> Pin<Box<dyn Future<Output = ()>>>>;
/// Receives the listbox (Remix's `ref` prop).
pub type OnRef = Rc<dyn Fn(Rc<ListboxState>)>;

/// Props for [`Context`] (Remix's `ListboxProviderProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    pub value: Option<String>,
    pub active_value: Option<String>,
    /// Briefly set an attribute on the selected option before `on_select_settled`.
    pub flash_selection: bool,
    /// The attribute for `flash_selection` (default `data-listbox-flash`).
    pub selection_flash_attribute: Option<String>,
    pub on_select: Option<OnChange>,
    pub on_select_settled: Option<OnSelectSettled>,
    pub on_highlight: Option<OnChange>,
    pub on_ref: Option<OnRef>,
    pub children: Children,
}

/// Which option [`ListboxState::navigate`] highlights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigate {
    Next,
    Previous,
    First,
    Last,
}

/// The listbox state shared with its mixins and handed out by `on_ref` (Remix's
/// `ListboxContext` and `ListboxRef`).
pub struct ListboxState {
    props: RefCell<ContextProps>,
    options: RefCell<Vec<RegisteredOption>>,
    selecting: Cell<bool>,
}

impl ListboxState {
    pub fn value(&self) -> Option<String> {
        self.props.borrow().value.clone()
    }

    pub fn active_value(&self) -> Option<String> {
        self.props.borrow().active_value.clone()
    }

    /// The options rendered in the last render.
    pub fn options(&self) -> Vec<RegisteredOption> {
        self.options.borrow().clone()
    }

    fn option(&self, value: Option<&str>) -> Option<RegisteredOption> {
        let value = value?;
        self.options.borrow().iter().find(|o| o.option.value == value).cloned()
    }

    pub fn active(&self) -> Option<RegisteredOption> {
        self.option(self.active_value().as_deref())
    }

    pub fn selected(&self) -> Option<RegisteredOption> {
        self.option(self.value().as_deref())
    }

    fn interactable(&self) -> Vec<RegisteredOption> {
        self.options.borrow().iter().filter(|o| o.interactable()).cloned().collect()
    }

    fn emit(&self, f: fn(&ContextProps) -> Option<OnChange>, value: Option<&str>, option: Option<&RegisteredOption>) {
        let callback = f(&self.props.borrow());
        if let Some(callback) = callback {
            callback(value, option);
        }
    }

    /// The next interactable option after `from` whose text starts with `text`.
    pub fn match_search_text(&self, text: &str, from: Option<&str>) -> Option<RegisteredOption> {
        let options = self.interactable();
        let from_index = from.and_then(|v| options.iter().position(|o| o.option.value == v));
        match_next_by_search_text(text, &options, from_index, |o| o.option.search_values()).cloned()
    }

    /// Select an option: `on_select`, the flash, then `on_select_settled`. Ignored while a
    /// selection is settling and for disabled or hidden options.
    pub async fn select(self: &Rc<Self>, value: Option<&str>) {
        if self.selecting.replace(true) {
            return;
        }
        let option = self.option(value).filter(|o| o.interactable());
        let Some(option) = option else {
            self.selecting.set(false);
            return;
        };
        self.emit(|p| p.on_select.clone(), value, Some(&option));
        let (flash, attribute, settled) = {
            let props = self.props.borrow();
            let attribute = props.selection_flash_attribute.clone().unwrap_or_else(|| "data-listbox-flash".into());
            (props.flash_selection, attribute, props.on_select_settled.clone())
        };
        #[cfg(target_arch = "wasm32")]
        if flash {
            if let Some(el) = option.element() {
                let _ = el.set_attribute(&attribute, "true");
                crate::event::sleep(60).await;
                let _ = el.remove_attribute(&attribute);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (flash, attribute);
        if let Some(settled) = settled {
            settled(value.map(str::to_string), Some(option)).await;
        }
        self.selecting.set(false);
    }

    /// Select the highlighted option.
    pub async fn select_active(self: &Rc<Self>) {
        let active = self.active_value();
        self.select(active.as_deref()).await;
    }

    pub fn highlight(&self, value: Option<&str>) {
        if self.selecting.get() {
            return;
        }
        let option = self.option(value);
        self.emit(|p| p.on_highlight.clone(), value, option.as_ref());
    }

    pub fn highlight_search_match(&self, text: &str) {
        if self.selecting.get() {
            return;
        }
        if let Some(option) = self.match_search_text(text, self.active_value().as_deref()) {
            self.emit(|p| p.on_highlight.clone(), Some(&option.option.value), Some(&option));
            option.scroll_into_view();
        }
    }

    pub fn navigate(&self, to: Navigate) {
        if self.selecting.get() {
            return;
        }
        let options = self.interactable();
        let active = self.active_value();
        let index = options.iter().position(|o| Some(&o.option.value) == active.as_ref());
        let option = match to {
            Navigate::Next => index.and_then(|i| options.get(i + 1)).or(options.first()),
            Navigate::Previous => match index {
                None => options.last(),
                Some(i) => i.checked_sub(1).and_then(|i| options.get(i)),
            },
            Navigate::First => options.first(),
            Navigate::Last => options.last(),
        };
        if let Some(option) = option {
            self.emit(|p| p.on_highlight.clone(), Some(&option.option.value), Some(option));
            option.scroll_into_view();
        }
    }

    pub fn scroll_active_into_view(&self) {
        if let Some(option) = self.active() {
            option.scroll_into_view();
        }
    }
}

/// The listbox provider (Remix's `listbox.Context`). Renders its children.
pub struct Context {
    state: Rc<ListboxState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let state = Rc::new(ListboxState { props: RefCell::new(props.clone()), options: RefCell::default(), selecting: Cell::new(false) });
        h.set_context(state.clone());
        h.queue_task(|this, _| {
            let on_ref = this.state.props.borrow().on_ref.clone();
            if let Some(f) = on_ref {
                f(this.state.clone());
            }
        });
        Context { state }
    }

    fn render(&self, props: &ContextProps, _: &Handle<Self>) -> Node {
        *self.state.props.borrow_mut() = props.clone();
        self.state.options.borrow_mut().clear();
        props.children.node().clone()
    }
}

pub(crate) fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<ListboxState> {
    h.context::<Rc<ListboxState>>().map(|s| (*s).clone()).expect("listbox mixins must be inside <listbox::Context>")
}

/// The `role="listbox"` element: keyboard navigation and typeahead.
pub fn list() -> Mixin {
    mixin::<List>(())
}

/// An option.
pub fn option(option: ListboxOption) -> Mixin {
    mixin::<OptionMixin>(option)
}

pub struct List {
    state: Rc<ListboxState>,
}

impl HostMixin for List {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        List { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        if host.attr("tabindex").is_none() {
            host.set_attr("tabindex", "-1");
        }
        if host.attr("role").is_none() {
            host.set_attr("role", "listbox");
        }
        host.mix(on("focus", h.on(|this, _| this.state.scroll_active_into_view())));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                let to = match event.key().as_str() {
                    "ArrowDown" => Navigate::Next,
                    "ArrowUp" => Navigate::Previous,
                    "Tab" | "Home" => Navigate::First,
                    "End" => Navigate::Last,
                    "Enter" | " " => {
                        event.prevent_default();
                        let state = this.state.clone();
                        spawn(async move { state.select_active().await });
                        return;
                    }
                    _ => return,
                };
                event.prevent_default();
                this.state.navigate(to);
            }),
        ));
        let state = self.state.clone();
        host.mix(hidden_typeahead(move |text| state.highlight_search_match(text)));
    }
}

pub(crate) fn spawn(fut: impl Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(fut);
    #[cfg(not(target_arch = "wasm32"))]
    drop(fut);
}

pub struct OptionMixin {
    state: Rc<ListboxState>,
    node: NodeSlot,
    value: String,
}

impl HostMixin for OptionMixin {
    type Args = ListboxOption;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let node: NodeSlot = Rc::default();
        let slot = node.clone();
        h.queue_task(move |_, _, n| *slot.borrow_mut() = Some(n.clone()));
        OptionMixin { state: state(h), node, value: String::new() }
    }

    fn render(&mut self, option: &ListboxOption, host: &mut Element, h: &MixinHandle<Self>) {
        let id = h.id().to_string();
        self.value = option.value.clone();
        self.state.options.borrow_mut().push(RegisteredOption { id: id.clone(), option: option.clone(), node: self.node.clone() });
        let value = self.state.value();
        let active = self.state.active_value();
        host.set_attr("role", "option");
        host.set_attr("id", id);
        host.set_attr("aria-selected", if value.as_deref() == Some(&option.value) { "true" } else { "false" });
        host.set_attr("aria-disabled", if option.disabled { "true" } else { "false" });
        host.set_attr("data-highlighted", if active.as_deref() == Some(&option.value) { "true" } else { "false" });
        if option.disabled {
            return;
        }
        host.mix(on(
            "click",
            h.on(|this, _| {
                let (state, value) = (this.state.clone(), this.value.clone());
                spawn(async move { state.select(Some(&value)).await });
            }),
        ));
        host.mix(on(
            "mousemove",
            h.on(|this, _| {
                if this.state.active_value().as_deref() != Some(&this.value) {
                    this.state.highlight(Some(&this.value));
                }
            }),
        ));
        host.mix(on(
            "mouseleave",
            h.on(|this, _| {
                if this.state.active_value().as_deref() == Some(&this.value) {
                    this.state.highlight(None);
                }
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsmix::{render_to_string, rsx};

    #[test]
    fn renders_roles_and_selection_state() {
        let options: Vec<Node> = [ListboxOption::new("remix", "Remix"), ListboxOption::new("react", "React"), ListboxOption::new("archived", "Archived").disabled(true)]
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={option(o.clone())}>{o.label.clone()}</div> })
            .collect();
        let html = render_to_string(&Node::component::<Context>(
            None,
            ContextProps { value: Some("remix".into()), active_value: Some("react".into()), children: rsx! { <div aria-label="Framework" mix={list()}>{options}</div> }.into(), ..Default::default() },
        ));
        assert!(html.starts_with(r#"<div aria-label="Framework" tabindex="-1" role="listbox">"#), "{html}");
        assert!(html.contains(r#"aria-selected="true" aria-disabled="false" data-highlighted="false">Remix"#), "{html}");
        assert!(html.contains(r#"aria-selected="false" aria-disabled="false" data-highlighted="true">React"#), "{html}");
        assert!(html.contains(r#"aria-disabled="true" data-highlighted="false">Archived"#), "{html}");
        assert_eq!(html.matches(r#"role="option" id=""#).count(), 3, "{html}");
    }
}

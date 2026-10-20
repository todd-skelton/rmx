//! A select built from a trigger button, a popover and a listbox (Remix's
//! `@remix-run/ui/select`).
//!
//! ```ignore
//! use rmx_ui::{listbox::ListboxOption, popover, select};
//!
//! rsx! {
//!     <select::Context default_label="Choose framework" default_value="remix" name="framework">
//!         <button type="button" mix={select::trigger()}><SelectLabel/></button>
//!         <popover::Context>
//!             <div mix={select::popover()}>
//!                 <div mix={select::list()}>
//!                     <div mix={select::option(ListboxOption::new("remix", "Remix"))}>"Remix"</div>
//!                 </div>
//!             </div>
//!         </popover::Context>
//!         <input mix={select::hidden_input()}/>
//!     </select::Context>
//! }
//! ```
//!
//! The trigger opens the list (click, ArrowDown, ArrowUp) and selects by typeahead while
//! closed. Choosing an option flashes it, closes the popover, waits for its CSS
//! transition and `label_swap_delay_ms`, then shows the new label and dispatches a
//! bubbling `rmx:select-change` event (read it with [`select_change`]). The hidden input
//! carries the value in form submissions.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{mixin, on, Children, Component, Element, Event, EventHandler, Handle, HostMixin, Mixin, MixinHandle, Node};

use crate::listbox::{self, ListboxOption, ListboxState, RegisteredOption};
use crate::typeahead::hidden_typeahead;

/// The event dispatched when the selection settles.
pub const SELECT_CHANGE_EVENT: &str = "rmx:select-change";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const DEFAULT_LABEL_SWAP_DELAY_MS: u32 = 75;

/// Props for [`Context`] (Remix's `SelectContextProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    /// The trigger label before anything is selected.
    pub default_label: String,
    pub default_value: Option<String>,
    pub disabled: bool,
    pub label_swap_delay_ms: Option<u32>,
    /// The hidden input's name.
    pub name: Option<String>,
    pub children: Children,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Initializing,
    Closed,
    Open,
    Selecting,
}

/// The data of an `rmx:select-change` event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectChange {
    pub label: Option<String>,
    pub option_id: Option<String>,
    pub value: Option<String>,
}

/// Select state shared with the mixins and with components that show the label
/// (Remix's `SelectContextValue`).
pub struct SelectState {
    handle: Handle<Context>,
    props: RefCell<ContextProps>,
    phase: Cell<Phase>,
    value: RefCell<Option<String>>,
    active_value: RefCell<Option<String>>,
    active_id: RefCell<Option<String>>,
    selected_id: RefCell<Option<String>>,
    selected_label: RefCell<String>,
    displayed_label: RefCell<String>,
    pending: RefCell<Option<SelectChange>>,
    list_id: String,
    listbox: RefCell<Option<Rc<ListboxState>>>,
    #[cfg(target_arch = "wasm32")]
    trigger: RefCell<Option<web_sys::HtmlElement>>,
    #[cfg(target_arch = "wasm32")]
    surface: RefCell<Option<web_sys::HtmlElement>>,
    #[cfg(target_arch = "wasm32")]
    popover: RefCell<Option<Rc<crate::popover::PopoverState>>>,
}

impl SelectState {
    /// The label the trigger shows.
    pub fn displayed_label(&self) -> String {
        self.displayed_label.borrow().clone()
    }

    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }

    pub fn disabled(&self) -> bool {
        self.props.borrow().disabled
    }

    pub fn is_open(&self) -> bool {
        self.phase.get() == Phase::Open
    }

    pub fn is_expanded(&self) -> bool {
        matches!(self.phase.get(), Phase::Open | Phase::Selecting)
    }

    pub fn open(&self) {
        if self.phase.get() != Phase::Closed || self.disabled() {
            return;
        }
        self.phase.set(Phase::Open);
        *self.active_value.borrow_mut() = self.value();
        self.handle.update();
    }

    pub fn close(&self) {
        if self.phase.get() != Phase::Open {
            return;
        }
        self.phase.set(Phase::Closed);
        self.handle.update();
    }

    fn set_selected(&self, value: Option<&str>, option: Option<&RegisteredOption>, sync_label: bool) {
        *self.value.borrow_mut() = value.map(str::to_string);
        *self.active_value.borrow_mut() = value.map(str::to_string);
        *self.active_id.borrow_mut() = option.map(|o| o.id.clone());
        *self.selected_id.borrow_mut() = option.map(|o| o.id.clone());
        *self.selected_label.borrow_mut() = option.map_or_else(|| self.props.borrow().default_label.clone(), |o| o.option.label.clone());
        self.sync_popover();
        if sync_label {
            *self.displayed_label.borrow_mut() = self.selected_label.borrow().clone();
        }
    }

    fn pending_change(&self, value: Option<&str>, option: Option<&RegisteredOption>) -> Option<SelectChange> {
        let option = option?;
        if self.value.borrow().as_deref() == value {
            return None;
        }
        Some(SelectChange { label: Some(option.option.label.clone()), option_id: Some(option.id.clone()), value: Some(option.option.value.clone()) })
    }

    fn dispatch(&self, change: Option<SelectChange>) {
        let Some(change) = change else { return };
        #[cfg(target_arch = "wasm32")]
        {
            use crate::event::Field;
            let target = self.trigger.borrow().clone().or_else(|| self.surface.borrow().clone());
            if let Some(target) = target {
                crate::event::dispatch(
                    &target,
                    SELECT_CHANGE_EVENT,
                    &[Field::Str("label", change.label.as_deref()), Field::Str("optionId", change.option_id.as_deref()), Field::Str("value", change.value.as_deref())],
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = change;
    }

    fn select_option(&self, value: Option<&str>, option: Option<&RegisteredOption>) {
        if self.phase.get() != Phase::Open {
            return;
        }
        *self.pending.borrow_mut() = self.pending_change(value, option);
        self.set_selected(value, option, false);
        self.handle.update();
    }

    /// After the listbox flash: close, wait for the close transition and the label delay,
    /// then show the label and dispatch the change.
    async fn settle(self: Rc<Self>) {
        if self.phase.get() != Phase::Open {
            return;
        }
        let change = self.pending.borrow_mut().take();
        self.phase.set(Phase::Selecting);
        #[cfg(target_arch = "wasm32")]
        {
            let surface = self.surface.borrow().clone();
            if let Some(surface) = surface {
                let signal = self.handle.signal();
                let updated = self.handle.updated();
                let transition = crate::event::wait_for_css_transition(surface, signal.clone());
                futures_join(updated, transition).await;
                let delay = self.props.borrow().label_swap_delay_ms.unwrap_or(DEFAULT_LABEL_SWAP_DELAY_MS);
                crate::event::sleep(delay as i32).await;
                if signal.aborted() {
                    return;
                }
            }
        }
        *self.displayed_label.borrow_mut() = self.selected_label.borrow().clone();
        self.phase.set(Phase::Closed);
        self.handle.updated().await;
        if !self.handle.is_connected() {
            return;
        }
        self.dispatch(change);
    }

    fn select_typeahead_match(self: &Rc<Self>, text: &str) {
        if self.phase.get() != Phase::Closed || self.disabled() {
            return;
        }
        let listbox = self.listbox.borrow().clone();
        let Some(option) = listbox.and_then(|l| l.match_search_text(text, self.value().as_deref())) else { return };
        let change = self.pending_change(Some(&option.option.value), Some(&option));
        *self.pending.borrow_mut() = None;
        self.set_selected(Some(&option.option.value), Some(&option), true);
        let this = self.clone();
        listbox::spawn(async move {
            this.handle.updated().await;
            if this.handle.is_connected() {
                this.dispatch(change);
            }
        });
    }

    fn highlight(&self, value: Option<&str>, option: Option<&RegisteredOption>) {
        if self.phase.get() != Phase::Open {
            return;
        }
        *self.active_value.borrow_mut() = value.map(str::to_string);
        *self.active_id.borrow_mut() = option.map(|o| o.id.clone());
        self.handle.update();
    }

    /// Point the popover at the trigger, aligned so the selected option overlays it.
    fn sync_popover(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::anchor::{AnchorOptions, AnchorTarget, Placement};
            let Some(popover) = self.popover.borrow().clone() else { return };
            let trigger = self.trigger.borrow().clone();
            popover.set_hide_focus_target(trigger.clone());
            match trigger {
                Some(trigger) => {
                    let relative = self.selected_id.borrow().as_ref().map_or_else(|| "[role=\"option\"]".to_string(), |id| format!("[id=\"{id}\"]"));
                    popover.set_anchor(AnchorTarget::Element(trigger), AnchorOptions::new(Placement::Left).inset(true).relative_to(relative));
                }
                None => popover.clear_anchor(),
            }
        }
    }

    /// The selected option's id, read from the rendered list once the surface exists
    /// (the server renders `aria-selected` from `default_value`).
    #[cfg(target_arch = "wasm32")]
    fn learn_selected_id(&self) {
        if self.selected_id.borrow().is_some() {
            return;
        }
        let Some(surface) = self.surface.borrow().clone() else { return };
        if let Ok(Some(selected)) = surface.query_selector("[aria-selected=\"true\"]") {
            *self.selected_id.borrow_mut() = Some(selected.id());
            self.sync_popover();
            self.handle.update();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn sync_min_width(&self) {
        if self.phase.get() != Phase::Open {
            return;
        }
        if let (Some(surface), Some(trigger)) = (self.surface.borrow().as_ref(), self.trigger.borrow().as_ref()) {
            let _ = surface.style().set_property("min-width", &format!("{}px", trigger.offset_width()));
        }
    }
}

#[cfg(target_arch = "wasm32")]
async fn futures_join(a: impl std::future::Future<Output = ()>, b: impl std::future::Future<Output = ()> + 'static) {
    // Both futures make progress through browser callbacks, so awaiting them in turn
    // finishes when the later one does.
    let b = wasm_bindgen_futures::future_to_promise(async move {
        b.await;
        Ok(wasm_bindgen::JsValue::UNDEFINED)
    });
    a.await;
    let _ = wasm_bindgen_futures::JsFuture::from(b).await;
}

/// The select provider (Remix's `select.Context`).
pub struct Context {
    state: Rc<SelectState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let state = Rc::new(SelectState {
            handle: h.clone(),
            props: RefCell::new(props.clone()),
            phase: Cell::new(Phase::Initializing),
            value: RefCell::new(None),
            active_value: RefCell::new(None),
            active_id: RefCell::new(None),
            selected_id: RefCell::new(None),
            selected_label: RefCell::new(String::new()),
            displayed_label: RefCell::new(String::new()),
            pending: RefCell::new(None),
            list_id: format!("{}-list", h.dom_id()),
            listbox: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            trigger: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            surface: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            popover: RefCell::new(None),
        });
        h.set_context(state.clone());
        Context { state }
    }

    fn render(&self, props: &ContextProps, h: &Handle<Self>) -> Node {
        let state = &self.state;
        *state.props.borrow_mut() = props.clone();
        if state.phase.get() == Phase::Initializing {
            *state.selected_label.borrow_mut() = props.default_label.clone();
            *state.displayed_label.borrow_mut() = props.default_label.clone();
            *state.value.borrow_mut() = props.default_value.clone();
            *state.active_value.borrow_mut() = props.default_value.clone();
            state.phase.set(Phase::Closed);
            // Learn the selected option's id from the rendered list.
            h.queue_task(|this, _| {
                #[cfg(target_arch = "wasm32")]
                this.state.learn_selected_id();
                let _ = this;
            });
        }
        let (s1, s2, s3, s4) = (state.clone(), state.clone(), state.clone(), state.clone());
        Node::component::<listbox::Context>(
            None,
            listbox::ContextProps {
                value: state.value(),
                active_value: state.active_value.borrow().clone(),
                flash_selection: true,
                selection_flash_attribute: Some("data-select-flash".into()),
                on_select: Some(Rc::new(move |value, option| s1.select_option(value, option))),
                on_select_settled: Some(Rc::new(move |_, _| Box::pin(s2.clone().settle()))),
                on_highlight: Some(Rc::new(move |value, option| s3.highlight(value, option))),
                on_ref: Some(Rc::new(move |listbox| *s4.listbox.borrow_mut() = Some(listbox))),
                children: props.children.clone(),
            },
        )
    }
}

fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<SelectState> {
    h.context::<Rc<SelectState>>().map(|s| (*s).clone()).expect("select mixins must be inside <select::Context>")
}

/// The button that shows the label and opens the list.
pub fn trigger() -> Mixin {
    mixin::<Trigger>(())
}

/// The popover surface holding the list.
pub fn popover() -> Mixin {
    mixin::<Popover>(())
}

/// The list element.
pub fn list() -> Mixin {
    mixin::<List>(())
}

/// An option (the listbox option mixin).
pub fn option(option: ListboxOption) -> Mixin {
    listbox::option(option)
}

/// A hidden input carrying the value in forms.
pub fn hidden_input() -> Mixin {
    mixin::<HiddenInput>(())
}

/// Listen for selection changes on this element or its ancestors' descendants.
pub fn on_select_change(handler: EventHandler) -> Mixin {
    on(SELECT_CHANGE_EVENT, handler)
}

/// Read an `rmx:select-change` event.
pub fn select_change(event: &Event) -> Option<SelectChange> {
    let value = crate::event::str_field(event, "value");
    let label = crate::event::str_field(event, "label");
    let option_id = crate::event::str_field(event, "optionId");
    (value.is_some() || label.is_some()).then_some(SelectChange { label, option_id, value })
}

pub struct Trigger {
    state: Rc<SelectState>,
}

impl HostMixin for Trigger {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Trigger { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        let s = &self.state;
        host.set_attr("aria-haspopup", "listbox");
        host.set_attr("aria-expanded", if s.is_expanded() { "true" } else { "false" });
        host.set_attr("aria-controls", s.list_id.clone());
        host.set_attr("aria-describedby", s.selected_id.borrow().clone());
        if s.disabled() {
            host.set_attr("disabled", true);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let registered = s.clone();
            host.mix(rmx::node_ref(move |node, signal| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                *registered.trigger.borrow_mut() = Some(el.clone());
                registered.sync_popover();
                let registered = registered.clone();
                signal.on_abort(move || {
                    let mut trigger = registered.trigger.borrow_mut();
                    if trigger.as_ref() == Some(&el) {
                        *trigger = None;
                        drop(trigger);
                        registered.sync_popover();
                    }
                });
            }));
        }
        let typeahead = s.clone();
        host.mix(hidden_typeahead(move |text| typeahead.select_typeahead_match(text)));
        host.mix(on("click", h.on(|this, _| this.state.open())));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                if matches!(event.key().as_str(), "ArrowDown" | "ArrowUp") {
                    event.prevent_default();
                    this.state.open();
                }
            }),
        ));
    }
}

pub struct Popover {
    state: Rc<SelectState>,
}

impl HostMixin for Popover {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let select = state(h);
            let popover = h.context::<Rc<crate::popover::PopoverState>>().map(|p| (*p).clone());
            h.queue_task(move |_, h, node| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                *select.surface.borrow_mut() = Some(el.clone());
                *select.popover.borrow_mut() = popover.clone();
                select.sync_popover();
                select.learn_selected_id();
                let select = select.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut surface = select.surface.borrow_mut();
                    if surface.as_ref() == Some(&el) {
                        *surface = None;
                    }
                    drop(surface);
                    if let Some(popover) = select.popover.borrow_mut().take() {
                        popover.clear_anchor();
                        popover.set_hide_focus_target(None);
                    }
                });
            });
        }
        Popover { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        let close = self.state.clone();
        host.mix(crate::popover::surface(crate::popover::SurfaceOptions::new(self.state.is_open(), move |_| close.close())));
        host.mix(on(
            "beforetoggle",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                if js_sys::Reflect::get(event.raw(), &"newState".into()).ok().and_then(|v| v.as_string()).as_deref() == Some("open") {
                    this.state.sync_min_width();
                }
                let _ = (this, event);
            }),
        ));
    }
}

pub struct List {
    state: Rc<SelectState>,
}

impl HostMixin for List {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        List { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("id", self.state.list_id.clone());
        host.set_attr("aria-activedescendant", self.state.active_id.borrow().clone());
        host.mix(crate::popover::focus_on_show());
        host.mix(listbox::list());
    }
}

pub struct HiddenInput {
    state: Rc<SelectState>,
}

impl HostMixin for HiddenInput {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        HiddenInput { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        let s = &self.state;
        host.set_attr("disabled", s.disabled());
        host.set_attr("name", s.props.borrow().name.clone());
        host.set_attr("type", "hidden");
        host.set_attr("value", s.value().unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::{render_to_string, rsx};

    struct Label;

    impl Component for Label {
        type Props = ();

        fn setup(_: &(), _: &Handle<Self>) -> Self {
            Label
        }

        fn render(&self, _: &(), h: &Handle<Self>) -> Node {
            let label = h.context::<Rc<SelectState>>().map(|s| s.displayed_label()).unwrap_or_default();
            rsx! { <span>{label}</span> }
        }
    }

    #[test]
    fn default_value_selects_the_option_while_the_trigger_keeps_the_default_label() {
        let options: Vec<Node> = [ListboxOption::new("remix", "Remix"), ListboxOption::new("react", "React")]
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={option(o.clone())}>{o.label.clone()}</div> })
            .collect();
        let html = render_to_string(&rsx! {
            <Context default_label="Choose framework" default_value="remix" name="framework">
                <button type="button" mix={trigger()}><Label/></button>
                <crate::popover::Context>
                    <div mix={popover()}><div mix={list()}>{options}</div></div>
                </crate::popover::Context>
                <input mix={hidden_input()}/>
            </Context>
        });
        assert!(html.contains(r#"aria-haspopup="listbox" aria-expanded="false" aria-controls=""#), "{html}");
        assert!(html.contains("<span>Choose framework</span>"), "{html}");
        assert!(html.contains(r#"aria-selected="true" aria-disabled="false" data-highlighted="true">Remix"#), "{html}");
        assert!(html.contains(r#"<input name="framework" type="hidden" value="remix">"#), "{html}");
        assert!(html.contains(r#"popover="manual""#) && html.contains(r#"role="listbox""#), "{html}");
    }
}

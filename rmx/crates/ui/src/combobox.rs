//! An editable combobox: a text input that filters a popover list of options (Remix's
//! `@remix-run/ui/combobox`).
//!
//! ```ignore
//! use rmx_ui::{combobox, listbox::ListboxOption};
//!
//! rsx! {
//!     <combobox::Context name="framework">
//!         <input mix={combobox::input()} placeholder="Search frameworks"/>
//!         <div mix={combobox::popover()}>
//!             <div mix={combobox::list()}>
//!                 <div mix={combobox::option(ListboxOption::new("remix", "Remix").text_value(&["remix", "rmx"]))}>"Remix"</div>
//!             </div>
//!         </div>
//!         <input mix={combobox::hidden_input()}/>
//!     </combobox::Context>
//! }
//! ```
//!
//! Typing clears the committed selection, filters the options by prefix and shows the
//! list as a hint. Arrow keys open and navigate the list while focus stays on the input;
//! Enter selects, flashes the option, closes the list and then writes the label into the
//! input. Blur and Escape keep an exact match and clear anything else. Changes dispatch a
//! bubbling `rmx:combobox-change` event (read it with [`combobox_change`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rmx::{mixin, on, Children, Component, Element, Event, EventHandler, Handle, HostMixin, Mixin, MixinHandle, Node};

use crate::listbox::{self, ListboxOption, ListboxState, Navigate, RegisteredOption};
use crate::typeahead::matches_search_text;

/// The event dispatched when the committed selection changes.
pub const COMBOBOX_CHANGE_EVENT: &str = "rmx:combobox-change";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const INPUT_COMMIT_DELAY_MS: i32 = 50;

/// Which option is active when the list opens (Remix's `ComboboxOpenStrategy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenStrategy {
    /// The option exactly matching the input, else the selected one, else the first.
    #[default]
    Selected,
    /// The selected option if it matches, else none.
    SelectedOrNone,
    First,
    Last,
}

/// Why the list is showing: `Hint` while typing filters it, `Nav` when opened or
/// navigated with the keyboard or pointer. Rendered as `data-show-reason` on the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowReason {
    Hint,
    Nav,
}

impl ShowReason {
    fn as_str(self) -> &'static str {
        match self {
            ShowReason::Hint => "hint",
            ShowReason::Nav => "nav",
        }
    }
}

/// Receives the combobox state on first render (Remix's `ref` prop and `ComboboxHandle`).
pub type OnRef = Rc<dyn Fn(Rc<ComboboxState>)>;

/// Props for [`Context`] (Remix's `ComboboxContextProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    /// The committed value; it is also the initial input text.
    pub default_value: Option<String>,
    pub disabled: bool,
    /// The hidden input's name.
    pub name: Option<String>,
    pub on_ref: Option<OnRef>,
    pub children: Children,
}

/// The data of an `rmx:combobox-change` event. All fields are `None` when the selection
/// was cleared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComboboxChange {
    pub label: Option<String>,
    pub option_id: Option<String>,
    pub value: Option<String>,
}

impl ComboboxChange {
    fn of(option: &RegisteredOption) -> Self {
        ComboboxChange { label: Some(option.option.label.clone()), option_id: Some(option.id.clone()), value: Some(option.option.value.clone()) }
    }
}

fn matches_filter(option: &ListboxOption, text: &str) -> bool {
    text.is_empty() || matches_search_text(&option.search_values(), text)
}

fn matches_exactly(option: &ListboxOption, text: &str) -> bool {
    let text = text.to_lowercase();
    !text.is_empty() && option.search_values().iter().any(|v| v.to_lowercase() == text)
}

/// Combobox state shared with the mixins and handed out by `on_ref` (Remix's
/// `ComboboxContextValue` and `ComboboxHandle`).
pub struct ComboboxState {
    handle: Handle<Context>,
    props: RefCell<ContextProps>,
    initialized: Cell<bool>,
    value: RefCell<Option<String>>,
    selected_id: RefCell<Option<String>>,
    selected_label: RefCell<String>,
    active_value: RefCell<Option<String>>,
    active_id: RefCell<Option<String>>,
    input_text: RefCell<String>,
    filter_text: RefCell<String>,
    open: Cell<bool>,
    surface_visible: Cell<bool>,
    show_reason: Cell<ShowReason>,
    pending_input_value: RefCell<Option<String>>,
    pending_change: RefCell<Option<ComboboxChange>>,
    select_input_after_close: Cell<bool>,
    close_sequence: Cell<u32>,
    list_id: String,
    listbox: RefCell<Option<Rc<ListboxState>>>,
    #[cfg(target_arch = "wasm32")]
    input: RefCell<Option<web_sys::HtmlInputElement>>,
    #[cfg(target_arch = "wasm32")]
    surface: RefCell<Option<web_sys::HtmlElement>>,
}

impl ComboboxState {
    /// The committed value.
    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }

    /// The committed option's label.
    pub fn label(&self) -> Option<String> {
        let label = self.selected_label.borrow();
        (!label.is_empty()).then(|| label.clone())
    }

    /// The text the user typed (or the committed label).
    pub fn input_text(&self) -> String {
        self.input_text.borrow().clone()
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// The highlighted option's element id.
    pub fn active_option_id(&self) -> Option<String> {
        self.active_id.borrow().clone()
    }

    /// The list element's id.
    pub fn id(&self) -> &str {
        &self.list_id
    }

    pub fn disabled(&self) -> bool {
        self.props.borrow().disabled
    }

    /// Open the list with the active option chosen by `strategy`.
    pub fn open(self: &Rc<Self>, strategy: OpenStrategy) {
        let this = self.clone();
        listbox::spawn(async move { this.open_popover(strategy, ShowReason::Nav).await });
    }

    pub fn close(self: &Rc<Self>) {
        let this = self.clone();
        listbox::spawn(async move { this.close_popover().await });
    }

    fn options(&self) -> Vec<RegisteredOption> {
        self.listbox.borrow().as_ref().map(|l| l.options()).unwrap_or_default()
    }

    fn selected_option(&self) -> Option<RegisteredOption> {
        let value = self.value()?;
        self.options().into_iter().find(|o| o.option.value == value)
    }

    fn matching_options(&self, text: &str) -> Vec<RegisteredOption> {
        self.options().into_iter().filter(|o| matches_filter(&o.option, text)).collect()
    }

    fn exact_input_match(&self, text: &str) -> Option<RegisteredOption> {
        self.options().into_iter().find(|o| !o.option.disabled && matches_exactly(&o.option, text))
    }

    /// The filter applied when the list opens: none for an empty input, an exact match
    /// or text that matches nothing, so the whole list shows.
    fn open_filter_text(&self) -> String {
        let text = self.input_text();
        if text.is_empty() || self.exact_input_match(&text).is_some() || self.matching_options(&text).is_empty() {
            return String::new();
        }
        text
    }

    fn resolve_open_option(&self, strategy: OpenStrategy, filter: &str) -> Option<RegisteredOption> {
        let enabled: Vec<RegisteredOption> = self.matching_options(filter).into_iter().filter(|o| !o.option.disabled).collect();
        if enabled.is_empty() {
            return None;
        }
        let usable_selected = || self.selected_option().filter(|o| !o.option.disabled && matches_filter(&o.option, filter));
        match strategy {
            OpenStrategy::SelectedOrNone => usable_selected(),
            OpenStrategy::Selected => self
                .exact_input_match(&self.input_text())
                .filter(|o| matches_filter(&o.option, filter))
                .or_else(usable_selected)
                .or_else(|| enabled.first().cloned()),
            OpenStrategy::First => enabled.first().cloned(),
            OpenStrategy::Last => enabled.last().cloned(),
        }
    }

    fn set_input_value(&self, value: &str) {
        #[cfg(target_arch = "wasm32")]
        if let Some(input) = self.input.borrow().as_ref() {
            if input.value() != value {
                input.set_value(value);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = value;
    }

    /// Collapse a text selection in the input to a cursor at the end.
    fn clear_input_selection(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(input) = self.input.borrow().as_ref() {
            let (Ok(Some(start)), Ok(Some(end))) = (input.selection_start(), input.selection_end()) else { return };
            let cursor = input.value().encode_utf16().count() as u32;
            if start != cursor || end != cursor {
                let _ = input.set_selection_range(cursor, cursor);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn sync_min_width(&self) {
        if let (Some(surface), Some(input)) = (self.surface.borrow().as_ref(), self.input.borrow().as_ref()) {
            let width = input.offset_width();
            if width > 0 {
                let _ = surface.style().set_property("min-width", &format!("{width}px"));
            }
        }
    }

    fn dispatch(&self, change: &ComboboxChange) {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::event::Field;
            let target: Option<web_sys::EventTarget> =
                self.input.borrow().clone().map(Into::into).or_else(|| self.surface.borrow().clone().map(Into::into));
            if let Some(target) = target {
                crate::event::dispatch(
                    &target,
                    COMBOBOX_CHANGE_EVENT,
                    &[Field::Str("label", change.label.as_deref()), Field::Str("optionId", change.option_id.as_deref()), Field::Str("value", change.value.as_deref())],
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = change;
    }

    /// Forget the committed selection; whether there was one.
    fn clear_committed_selection(&self) -> bool {
        let changed = self.value.borrow_mut().take().is_some() | self.selected_id.borrow_mut().take().is_some();
        self.selected_label.borrow_mut().clear();
        changed
    }

    fn cancel_pending(&self) {
        self.close_sequence.set(self.close_sequence.get() + 1);
        *self.pending_input_value.borrow_mut() = None;
        *self.pending_change.borrow_mut() = None;
        self.select_input_after_close.set(false);
    }

    fn clear_active(&self) {
        *self.active_value.borrow_mut() = None;
        *self.active_id.borrow_mut() = None;
    }

    fn commit(&self, option: &RegisteredOption) {
        *self.value.borrow_mut() = Some(option.option.value.clone());
        *self.selected_id.borrow_mut() = Some(option.id.clone());
        *self.selected_label.borrow_mut() = option.option.label.clone();
        *self.active_value.borrow_mut() = Some(option.option.value.clone());
        *self.active_id.borrow_mut() = Some(option.id.clone());
    }

    fn clear_input_and_selection(&self) {
        self.cancel_pending();
        self.input_text.borrow_mut().clear();
        self.clear_active();
        if !self.open.get() {
            self.filter_text.borrow_mut().clear();
        }
        let changed = self.clear_committed_selection();
        self.set_input_value("");
        if changed {
            self.dispatch(&ComboboxChange::default());
        }
    }

    /// Whether the close identified by `id` was superseded.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn close_superseded(&self, id: u32) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.handle.signal().aborted() {
            return true;
        }
        id != self.close_sequence.get() || self.open.get()
    }

    /// Close the list. After a keyboard or pointer selection, wait for the surface's
    /// close transition before clearing the filter, then write the chosen label into the
    /// input and select its text.
    async fn close_popover(self: Rc<Self>) {
        if !self.open.get() {
            return;
        }
        let id = self.close_sequence.get() + 1;
        self.close_sequence.set(id);
        self.open.set(false);
        if self.show_reason.get() != ShowReason::Nav {
            self.surface_visible.set(false);
            self.filter_text.borrow_mut().clear();
            self.clear_active();
            self.handle.updated().await;
            return;
        }
        self.handle.updated().await;
        if self.close_superseded(id) {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let surface = self.surface.borrow().clone().filter(|s| s.is_connected());
            if let Some(surface) = surface {
                crate::event::wait_for_css_transition(surface, self.handle.signal()).await;
            }
            if self.close_superseded(id) {
                return;
            }
        }
        self.surface_visible.set(false);
        self.filter_text.borrow_mut().clear();
        self.clear_active();
        self.handle.updated().await;
        if self.close_superseded(id) {
            return;
        }
        let pending = self.pending_input_value.borrow().clone();
        if let Some(next) = pending {
            #[cfg(target_arch = "wasm32")]
            crate::event::sleep(INPUT_COMMIT_DELAY_MS).await;
            if self.close_superseded(id) || self.pending_input_value.borrow().as_ref() != Some(&next) {
                return;
            }
            self.set_input_value(&next);
            *self.pending_input_value.borrow_mut() = None;
        }
        #[cfg(target_arch = "wasm32")]
        if self.select_input_after_close.get() {
            if let Some(input) = self.input.borrow().as_ref().filter(|i| i.is_connected()) {
                let focused = input.owner_document().and_then(|d| d.active_element());
                if focused.as_ref() == Some(input.as_ref()) {
                    input.select();
                }
            }
        }
        self.select_input_after_close.set(false);
    }

    async fn open_popover(self: Rc<Self>, strategy: OpenStrategy, reason: ShowReason) {
        if self.disabled() {
            return;
        }
        let pending = self.pending_input_value.borrow_mut().take();
        if let Some(pending) = pending {
            self.set_input_value(&pending);
        }
        self.close_sequence.set(self.close_sequence.get() + 1);
        self.select_input_after_close.set(false);
        let filter = self.open_filter_text();
        if self.matching_options(&filter).is_empty() {
            return;
        }
        let active = self.resolve_open_option(strategy, &filter);
        *self.filter_text.borrow_mut() = filter;
        self.show_reason.set(reason);
        self.open.set(true);
        self.surface_visible.set(true);
        *self.active_value.borrow_mut() = active.as_ref().map(|o| o.option.value.clone());
        *self.active_id.borrow_mut() = active.map(|o| o.id);
        self.handle.updated().await;
        #[cfg(target_arch = "wasm32")]
        if self.handle.signal().aborted() {
            return;
        }
        let listbox = self.listbox.borrow().clone();
        if let Some(listbox) = listbox {
            listbox.scroll_active_into_view();
        }
    }

    /// The listbox chose an option (before its flash).
    fn begin_selection(&self, option: Option<&RegisteredOption>) {
        let Some(option) = option else { return };
        let changed = self.value.borrow().as_deref() != Some(&option.option.value);
        self.commit(option);
        *self.input_text.borrow_mut() = option.option.label.clone();
        self.show_reason.set(ShowReason::Nav);
        #[cfg(target_arch = "wasm32")]
        let shown = self.input.borrow().as_ref().map(|i| i.value());
        #[cfg(not(target_arch = "wasm32"))]
        let shown: Option<String> = None;
        *self.pending_input_value.borrow_mut() = (shown.as_deref() != Some(&option.option.label)).then(|| option.option.label.clone());
        *self.pending_change.borrow_mut() = changed.then(|| ComboboxChange::of(option));
        self.select_input_after_close.set(true);
        self.handle.update();
    }

    /// The selection flash finished.
    async fn finish_selection(self: Rc<Self>) {
        let change = self.pending_change.borrow_mut().take();
        if let Some(change) = change {
            self.dispatch(&change);
        }
        self.close_popover().await;
    }

    async fn set_input_text(self: Rc<Self>, text: String) {
        self.cancel_pending();
        let changed = self.clear_committed_selection();
        *self.input_text.borrow_mut() = text.clone();
        self.show_reason.set(ShowReason::Hint);
        self.clear_active();
        let open = self.open.get();
        if !text.is_empty() || !open {
            *self.filter_text.borrow_mut() = text.clone();
        }
        let matches = !self.matching_options(&text).is_empty();
        if changed {
            self.dispatch(&ComboboxChange::default());
        }
        if text.is_empty() || !matches {
            if open {
                self.open.set(false);
                self.surface_visible.set(false);
            } else {
                self.filter_text.borrow_mut().clear();
            }
            self.handle.updated().await;
            return;
        }
        self.open.set(true);
        self.surface_visible.set(true);
        self.handle.updated().await;
    }

    /// Keep an exact match (committing it), clear anything else, and close.
    fn handle_blur(self: &Rc<Self>) {
        match self.exact_input_match(&self.input_text()) {
            None => self.clear_input_and_selection(),
            Some(exact) => {
                let changed = self.value.borrow().as_deref() != Some(&exact.option.value);
                self.commit(&exact);
                if changed {
                    self.dispatch(&ComboboxChange::of(&exact));
                }
            }
        }
        self.close();
    }

    fn handle_escape(self: &Rc<Self>) {
        if self.exact_input_match(&self.input_text()).is_none() {
            self.clear_input_and_selection();
        }
        if self.open.get() {
            self.close();
        }
    }

    fn open_from_arrow(self: &Rc<Self>, direction: OpenStrategy) {
        let strategy = if self.exact_input_match(&self.input_text()).is_some() { OpenStrategy::Selected } else { direction };
        self.clear_input_selection();
        self.open(strategy);
    }

    fn open_from_input_activation(self: &Rc<Self>) {
        self.clear_input_selection();
        self.open(OpenStrategy::SelectedOrNone);
    }

    fn navigate(&self, to: Navigate) {
        self.clear_input_selection();
        if let Some(listbox) = self.listbox.borrow().clone() {
            listbox.navigate(to);
        }
    }

    fn select_active(&self) {
        if let Some(listbox) = self.listbox.borrow().clone() {
            listbox::spawn(async move { listbox.select_active().await });
        }
    }

    fn highlight(&self, value: Option<&str>, option: Option<&RegisteredOption>) {
        if !self.open.get() {
            return;
        }
        *self.active_value.borrow_mut() = value.map(str::to_string);
        *self.active_id.borrow_mut() = option.map(|o| o.id.clone());
        if option.is_some() {
            self.show_reason.set(ShowReason::Nav);
        }
        self.handle.update();
    }
}

/// The combobox provider (Remix's `combobox.Context`). It provides the popover and
/// listbox contexts its mixins use.
pub struct Context {
    state: Rc<ComboboxState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let state = Rc::new(ComboboxState {
            handle: h.clone(),
            props: RefCell::new(props.clone()),
            initialized: Cell::new(false),
            value: RefCell::new(None),
            selected_id: RefCell::new(None),
            selected_label: RefCell::new(String::new()),
            active_value: RefCell::new(None),
            active_id: RefCell::new(None),
            input_text: RefCell::new(String::new()),
            filter_text: RefCell::new(String::new()),
            open: Cell::new(false),
            surface_visible: Cell::new(false),
            show_reason: Cell::new(ShowReason::Nav),
            pending_input_value: RefCell::new(None),
            pending_change: RefCell::new(None),
            select_input_after_close: Cell::new(false),
            close_sequence: Cell::new(0),
            list_id: format!("{}-combobox-list", h.dom_id()),
            listbox: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            input: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            surface: RefCell::new(None),
        });
        h.set_context(state.clone());
        h.queue_task(|this, _| {
            let on_ref = this.state.props.borrow().on_ref.clone();
            if let Some(on_ref) = on_ref {
                on_ref(this.state.clone());
            }
        });
        Context { state }
    }

    fn render(&self, props: &ContextProps, h: &Handle<Self>) -> Node {
        let state = &self.state;
        *state.props.borrow_mut() = props.clone();
        if !state.initialized.replace(true) {
            *state.value.borrow_mut() = props.default_value.clone();
            *state.input_text.borrow_mut() = props.default_value.clone().unwrap_or_default();
        }
        h.queue_task(|this, _| {
            let state = &this.state;
            match state.selected_option() {
                Some(selected) => {
                    *state.selected_id.borrow_mut() = Some(selected.id.clone());
                    *state.selected_label.borrow_mut() = selected.option.label.clone();
                }
                None if state.value.borrow().is_none() => {
                    *state.selected_id.borrow_mut() = None;
                    state.selected_label.borrow_mut().clear();
                }
                None => {}
            }
        });
        let (s1, s2, s3, s4) = (state.clone(), state.clone(), state.clone(), state.clone());
        let listbox = Node::component::<listbox::Context>(
            None,
            listbox::ContextProps {
                value: state.value(),
                active_value: state.active_value.borrow().clone(),
                flash_selection: true,
                selection_flash_attribute: Some("data-combobox-flash".into()),
                on_select: Some(Rc::new(move |_, option| s1.begin_selection(option))),
                on_select_settled: Some(Rc::new(move |_, _| Box::pin(s2.clone().finish_selection()))),
                on_highlight: Some(Rc::new(move |value, option| s3.highlight(value, option))),
                on_ref: Some(Rc::new(move |listbox| *s4.listbox.borrow_mut() = Some(listbox))),
                children: props.children.clone(),
            },
        );
        Node::component::<crate::popover::Context>(None, crate::popover::ContextProps { children: listbox.into() })
    }
}

fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<ComboboxState> {
    h.context::<Rc<ComboboxState>>().map(|s| (*s).clone()).expect("combobox mixins must be inside <combobox::Context>")
}

/// The text input (`role="combobox"`).
pub fn input() -> Mixin {
    mixin::<Input>(())
}

/// The popover surface holding the list.
pub fn popover() -> Mixin {
    mixin::<Popover>(())
}

/// The list element.
pub fn list() -> Mixin {
    mixin::<List>(())
}

/// An option; hidden while it does not match the typed text. Its `text_value` is what
/// typing matches against (Remix's `searchValue`).
pub fn option(option: ListboxOption) -> Mixin {
    mixin::<OptionMixin>(option)
}

/// A hidden input carrying the committed value in forms.
pub fn hidden_input() -> Mixin {
    mixin::<HiddenInput>(())
}

/// Listen for committed-selection changes on this element or its descendants.
pub fn on_combobox_change(handler: EventHandler) -> Mixin {
    on(COMBOBOX_CHANGE_EVENT, handler)
}

/// Read an `rmx:combobox-change` event.
pub fn combobox_change(event: &Event) -> ComboboxChange {
    ComboboxChange {
        label: crate::event::str_field(event, "label"),
        option_id: crate::event::str_field(event, "optionId"),
        value: crate::event::str_field(event, "value"),
    }
}

pub struct Input {
    state: Rc<ComboboxState>,
}

impl HostMixin for Input {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Input { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        let s = &self.state;
        host.set_attr("aria-activedescendant", s.active_id.borrow().clone());
        host.set_attr("aria-autocomplete", "list");
        host.set_attr("aria-controls", s.list_id.clone());
        host.set_attr("aria-expanded", if s.open.get() { "true" } else { "false" });
        host.set_attr("aria-haspopup", "listbox");
        if host.attr("autocomplete").is_none() {
            host.set_attr("autocomplete", "off");
        }
        if s.surface_visible.get() {
            host.set_attr("data-surface-visible", "true");
        }
        if s.disabled() {
            host.set_attr("disabled", true);
        }
        host.set_attr("role", "combobox");
        if host.attr("type").is_none() {
            host.set_attr("type", "text");
        }
        #[cfg(target_arch = "wasm32")]
        {
            let registered = s.clone();
            host.mix(rmx::node_ref(move |node, signal| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(node.element().clone()) else { return };
                *registered.input.borrow_mut() = Some(el.clone());
                registered.set_input_value(&registered.input_text());
                let registered = registered.clone();
                signal.on_abort(move || {
                    let mut input = registered.input.borrow_mut();
                    if input.as_ref() == Some(&el) {
                        *input = None;
                    }
                });
            }));
        }
        host.mix(crate::popover::anchor(crate::anchor::AnchorOptions::new(crate::anchor::Placement::BottomStart)));
        host.mix(on(
            "click",
            h.on(|this, _| {
                if !this.state.is_open() {
                    this.state.open_from_input_activation();
                }
            }),
        ));
        host.mix(on(
            "input",
            h.on_event(|this, _, event| {
                let state = this.state.clone();
                let text = event.target_value();
                listbox::spawn(async move { state.set_input_text(text).await });
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                let s = &this.state;
                match event.key().as_str() {
                    "ArrowDown" => {
                        event.prevent_default();
                        if s.is_open() {
                            s.navigate(Navigate::Next);
                        } else {
                            s.open_from_arrow(OpenStrategy::First);
                        }
                    }
                    "ArrowUp" => {
                        event.prevent_default();
                        if s.is_open() {
                            s.navigate(Navigate::Previous);
                        } else {
                            s.open_from_arrow(OpenStrategy::Last);
                        }
                    }
                    "Enter" if s.is_open() => {
                        event.prevent_default();
                        s.select_active();
                    }
                    "Escape" if s.is_open() || !s.input_text.borrow().is_empty() => {
                        event.prevent_default();
                        s.handle_escape();
                    }
                    _ => {}
                }
            }),
        ));
        host.mix(on("blur", h.on(|this, _| this.state.handle_blur())));
    }
}

pub struct Popover {
    state: Rc<ComboboxState>,
}

impl HostMixin for Popover {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let combobox = state(h);
            h.queue_task(move |_, h, node| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                *combobox.surface.borrow_mut() = Some(el.clone());
                let combobox = combobox.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut surface = combobox.surface.borrow_mut();
                    if surface.as_ref() == Some(&el) {
                        *surface = None;
                    }
                });
            });
        }
        Popover { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        host.set_attr("data-show-reason", self.state.show_reason.get().as_str());
        let close = self.state.clone();
        host.mix(crate::popover::surface(crate::popover::SurfaceOptions::new(self.state.is_open(), move |_| close.close()).close_on_anchor_click(false)));
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
    state: Rc<ComboboxState>,
}

impl HostMixin for List {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        List { state: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("id", self.state.list_id.clone());
        host.mix(listbox::list());
    }
}

pub struct OptionMixin {
    state: Rc<ComboboxState>,
}

impl HostMixin for OptionMixin {
    type Args = ListboxOption;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        OptionMixin { state: state(h) }
    }

    fn render(&mut self, option: &ListboxOption, host: &mut Element, _: &MixinHandle<Self>) {
        if !matches_filter(option, &self.state.filter_text.borrow()) {
            host.set_attr("hidden", true);
        }
        // Keep focus in the input when an option is pressed.
        host.mix(on(
            "pointerdown",
            EventHandler::new(|event: &Event| {
                #[cfg(target_arch = "wasm32")]
                if js_sys::Reflect::get(event.raw(), &"button".into()).ok().and_then(|b| b.as_f64()) == Some(0.0) {
                    event.prevent_default();
                }
                let _ = event;
            }),
        ));
        host.mix(listbox::option(option.clone()));
    }
}

pub struct HiddenInput {
    state: Rc<ComboboxState>,
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

    #[test]
    fn exact_and_prefix_matching_use_search_values() {
        let remix = ListboxOption::new("remix", "Remix").text_value(&["remix", "rmx"]);
        assert!(matches_filter(&remix, ""));
        assert!(matches_filter(&remix, "RM"));
        assert!(!matches_filter(&remix, "emix"));
        assert!(matches_exactly(&remix, "RMX"));
        assert!(!matches_exactly(&remix, "rm"));
        assert!(!matches_exactly(&remix, ""));
        let react = ListboxOption::new("react", "React");
        assert!(matches_exactly(&react, "react"));
    }

    #[test]
    fn default_value_is_committed_and_rendered_in_the_hidden_input() {
        let options: Vec<Node> = [ListboxOption::new("remix", "Remix"), ListboxOption::new("react", "React")]
            .into_iter()
            .map(|o| rsx! { <div key={o.value.clone()} mix={option(o.clone())}>{o.label.clone()}</div> })
            .collect();
        let html = render_to_string(&rsx! {
            <Context default_value="remix" name="framework">
                <input mix={input()}/>
                <div mix={popover()}><div mix={list()}>{options}</div></div>
                <input mix={hidden_input()}/>
            </Context>
        });
        assert!(html.contains(r#"aria-autocomplete="list""#) && html.contains(r#"aria-expanded="false""#), "{html}");
        assert!(html.contains(r#"role="combobox" type="text""#), "{html}");
        assert!(html.contains(r#"data-show-reason="nav""#) && html.contains(r#"popover="manual""#), "{html}");
        assert!(html.contains(r#"aria-selected="true""#), "{html}");
        assert!(html.contains(r#"<input name="framework" type="hidden" value="remix">"#), "{html}");
        assert!(!html.contains("hidden=\"\""), "no option is filtered out: {html}");
    }
}

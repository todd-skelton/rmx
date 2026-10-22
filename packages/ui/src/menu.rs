//! Button-triggered and context menus with keyboard navigation, checkbox and radio items,
//! a bubbling selection event and nested submenus (Remix's `@remix-run/ui/menu`).
//!
//! ```ignore
//! use rmx_ui::menu::{self, ItemOptions, SubmenuTriggerOptions};
//!
//! rsx! {
//!     <menu::Context label="Project actions">
//!         <button type="button" mix={menu::trigger()}>"Actions"</button>
//!         <div mix={menu::popover()}>
//!             <div mix={menu::list()}>
//!                 <div mix={menu::item(ItemOptions::new("rename"))}>"Rename"</div>
//!                 <menu::Context>
//!                     <div mix={menu::submenu_trigger(SubmenuTriggerOptions::default())}>"Zoom"</div>
//!                     <div mix={menu::popover()}>
//!                         <div mix={menu::list()}>
//!                             <div mix={menu::item(ItemOptions::new("zoom").radio("100", true))}>"100%"</div>
//!                         </div>
//!                     </div>
//!                 </menu::Context>
//!             </div>
//!         </div>
//!     </menu::Context>
//! }
//! ```
//!
//! A submenu is a nested [`Context`] inside the parent's list whose first child carries
//! [`submenu_trigger`]. Selecting an item flashes it, dispatches one bubbling
//! `rmx:menu-select` event from the item (read it with [`menu_select`]), fades the whole
//! tree out and focuses the root trigger.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use rmx::{mixin, on, Children, Component, Element, Event, EventHandler, Handle, HostMixin, Mixin, MixinHandle, Node, NodeHandle};

use crate::anchor::{AnchorOptions, Placement};
use crate::listbox::spawn;
use crate::typeahead::{hidden_typeahead, match_next_by_search_text};

/// The event dispatched from a selected item.
pub const MENU_SELECT_EVENT: &str = "rmx:menu-select";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const MENU_FLASH_DURATION_MS: i32 = 60;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const SUBMENU_OPEN_DELAY_MS: i32 = 300;
const SUBMENU_ANCHOR_RELATIVE_TO: &str = "[role=\"menu\"] > [role^=\"menuitem\"]";

/// An item's kind, which sets its role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ItemType {
    #[default]
    Item,
    Checkbox,
    Radio,
}

impl ItemType {
    fn role(self) -> &'static str {
        match self {
            ItemType::Item => "menuitem",
            ItemType::Checkbox => "menuitemcheckbox",
            ItemType::Radio => "menuitemradio",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ItemType::Item => "item",
            ItemType::Checkbox => "checkbox",
            ItemType::Radio => "radio",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseAnimation {
    None,
    Fade,
}

/// Where focus goes when a menu opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenStrategy {
    /// The first enabled item.
    First,
    /// The last enabled item.
    Last,
    /// The list itself, nothing highlighted.
    #[default]
    List,
    /// Open without moving focus.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Next,
    Previous,
    First,
    Last,
}

/// What a menu is doing (rendered as `data-submenu-state` on submenu triggers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// Fading out after a selection or dismissal.
    Dismissing,
    /// Flashing the chosen item.
    Selecting,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Dismissing => "dismissing",
            Phase::Selecting => "selecting",
        }
    }
}

/// The item an `rmx:menu-select` event describes (Remix's `MenuSelectItem`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuSelectItem {
    /// The committed checked state for checkbox and radio items.
    pub checked: Option<bool>,
    pub id: String,
    pub label: String,
    pub name: String,
    pub item_type: ItemType,
    pub value: Option<String>,
}

/// Props for [`Context`] (Remix's `MenuProviderProps`).
#[derive(Clone, Default)]
pub struct ContextProps {
    /// The list's `aria-label`; without it the list is labelled by its trigger.
    pub label: Option<String>,
    pub children: Children,
}

/// Options for [`item`] (Remix's `MenuItemOptions`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemOptions {
    pub name: String,
    pub item_type: ItemType,
    pub checked: bool,
    pub disabled: bool,
    /// The label in events and for typeahead (defaults to the item's text).
    pub label: Option<String>,
    /// Text typeahead matches against (defaults to the label).
    pub search_value: Vec<String>,
    pub value: Option<String>,
}

impl ItemOptions {
    pub fn new(name: impl Into<String>) -> Self {
        ItemOptions { name: name.into(), ..Default::default() }
    }

    pub fn checkbox(mut self, checked: bool) -> Self {
        self.item_type = ItemType::Checkbox;
        self.checked = checked;
        self
    }

    pub fn radio(mut self, value: impl Into<String>, checked: bool) -> Self {
        self.item_type = ItemType::Radio;
        self.value = Some(value.into());
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn search_value(mut self, values: &[&str]) -> Self {
        self.search_value = values.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}

/// Options for [`submenu_trigger`] (Remix's `SubmenuTriggerOptions`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubmenuTriggerOptions {
    pub disabled: bool,
    pub label: Option<String>,
    pub search_value: Vec<String>,
    pub value: Option<String>,
}

impl SubmenuTriggerOptions {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn search_value(mut self, values: &[&str]) -> Self {
        self.search_value = values.iter().map(|s| s.to_string()).collect();
        self
    }
}

type NodeSlot = Rc<RefCell<Option<NodeHandle>>>;

#[derive(Clone)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct RegisteredItem {
    id: String,
    item_type: ItemType,
    checked: bool,
    disabled: bool,
    search_value: Vec<String>,
    label: Option<String>,
    name: Option<String>,
    value: Option<String>,
    submenu: Option<Weak<MenuState>>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    node: NodeSlot,
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl RegisteredItem {
    #[cfg(target_arch = "wasm32")]
    fn element(&self) -> Option<web_sys::HtmlElement> {
        let node = self.node.borrow();
        wasm_bindgen::JsCast::dyn_into(node.as_ref()?.element().clone()).ok()
    }

    fn interactable(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            !self.disabled && self.element().is_some_and(|el| el.is_connected() && !el.hidden())
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    fn label(&self) -> String {
        if let Some(label) = &self.label {
            return normalize(label);
        }
        #[cfg(target_arch = "wasm32")]
        {
            normalize(&self.element().and_then(|el| el.text_content()).unwrap_or_default())
        }
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }

    fn search_values(&self) -> Vec<String> {
        if self.search_value.is_empty() {
            vec![self.label()]
        } else {
            self.search_value.clone()
        }
    }

    fn submenu(&self) -> Option<Rc<MenuState>> {
        self.submenu.as_ref().and_then(Weak::upgrade)
    }
}

/// A menu's state, shared with its mixins and with nested submenus (Remix's
/// `MenuContextValue`).
pub struct MenuState {
    handle: Handle<Context>,
    parent: Option<Rc<MenuState>>,
    label: RefCell<Option<String>>,
    active_id: RefCell<Option<String>>,
    children: RefCell<Vec<Weak<MenuState>>>,
    close_animation: Cell<CloseAnimation>,
    /// The flashing item and the checked state it shows.
    flash: RefCell<Option<(String, Option<bool>)>>,
    items: RefCell<Vec<RegisteredItem>>,
    open: Cell<bool>,
    phase: Cell<Phase>,
    suppress_pointer_leave_clear: Cell<bool>,
    list_id: String,
    trigger_id: RefCell<Option<String>>,
    #[cfg(target_arch = "wasm32")]
    list: RefCell<Option<web_sys::HtmlElement>>,
    #[cfg(target_arch = "wasm32")]
    surface: RefCell<Option<web_sys::HtmlElement>>,
    #[cfg(target_arch = "wasm32")]
    trigger: RefCell<Option<web_sys::HtmlElement>>,
    #[cfg(target_arch = "wasm32")]
    hover_aim: crate::hover_aim::HoverAim,
}

impl MenuState {
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    pub fn phase(&self) -> Phase {
        self.phase.get()
    }

    /// The highlighted item's id.
    pub fn active_id(&self) -> Option<String> {
        self.active_id.borrow().clone()
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Open the menu with focus placed by `strategy`.
    pub fn open(self: &Rc<Self>, strategy: OpenStrategy) {
        let this = self.clone();
        spawn(async move { this.open_menu(strategy, strategy != OpenStrategy::None).await });
    }

    /// Fade out the whole menu tree and focus the root trigger.
    pub fn close(self: &Rc<Self>) {
        let this = self.clone();
        spawn(async move { this.close_all(true).await });
    }

    fn root(self: &Rc<Self>) -> Rc<MenuState> {
        let mut menu = self.clone();
        while let Some(parent) = menu.parent.clone() {
            menu = parent;
        }
        menu
    }

    fn item(&self, id: Option<&str>) -> Option<RegisteredItem> {
        let id = id?;
        self.items.borrow().iter().find(|i| i.id == id).cloned()
    }

    fn interactable(&self) -> Vec<RegisteredItem> {
        self.items.borrow().iter().filter(|i| i.interactable()).cloned().collect()
    }

    fn child_menus(&self) -> Vec<Rc<MenuState>> {
        self.children.borrow().iter().filter_map(Weak::upgrade).collect()
    }

    fn open_child(&self) -> Option<Rc<MenuState>> {
        self.child_menus().into_iter().find(|c| c.is_open())
    }

    /// Register an item for this render, replacing an earlier registration of the same id
    /// (a submenu re-rendering on its own registers its trigger again).
    fn register_item(&self, item: RegisteredItem) {
        let mut items = self.items.borrow_mut();
        match items.iter_mut().find(|i| i.id == item.id) {
            Some(existing) => *existing = item,
            None => items.push(item),
        }
    }

    fn register_child(&self, child: &Rc<MenuState>) {
        let mut children = self.children.borrow_mut();
        if !children.iter().any(|c| c.as_ptr() == Rc::as_ptr(child)) {
            children.push(Rc::downgrade(child));
        }
    }

    /// Render, then whether that render's signal is still live (Remix's
    /// `let signal = await handle.update(); if (signal.aborted) return`).
    async fn flushed(&self) -> bool {
        !self.handle.updated().await.aborted()
    }

    fn close_children_sync(&self, animation: CloseAnimation) -> bool {
        let mut updated = false;
        for child in self.child_menus() {
            updated |= child.close_sync(animation);
        }
        updated
    }

    async fn open_menu(self: Rc<Self>, strategy: OpenStrategy, focus: bool) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let mut updated = self.close_children_sync(CloseAnimation::None);
        let items = self.interactable();
        let next = match strategy {
            OpenStrategy::First => items.first().map(|i| i.id.clone()),
            OpenStrategy::Last => items.last().map(|i| i.id.clone()),
            OpenStrategy::List | OpenStrategy::None => None,
        };
        let changed = !self.open.get() || *self.active_id.borrow() != next;
        self.open.set(true);
        *self.active_id.borrow_mut() = next.clone();
        if changed {
            self.handle.update();
            updated = true;
        }
        if updated && !self.flushed().await {
            return;
        }
        if focus {
            #[cfg(target_arch = "wasm32")]
            {
                let item = self.item(next.as_deref()).filter(|i| i.interactable() && matches!(strategy, OpenStrategy::First | OpenStrategy::Last));
                match item {
                    Some(item) => crate::event::focus_node(item.element().as_ref()),
                    None => crate::event::focus_node(self.list.borrow().as_ref()),
                }
            }
        }
    }

    /// Close this menu and its children without waiting; whether a render was scheduled.
    fn close_sync(&self, animation: CloseAnimation) -> bool {
        let was_open = self.open.get();
        self.close_animation.set(animation);
        let mut updated = self.close_children_sync(animation);
        if !was_open && (animation == CloseAnimation::Fade || self.active_id.borrow().is_none()) {
            return updated;
        }
        self.open.set(false);
        if animation == CloseAnimation::None {
            *self.active_id.borrow_mut() = None;
            self.phase.set(Phase::Idle);
        } else if was_open && self.phase.get() == Phase::Idle {
            self.phase.set(Phase::Dismissing);
        }
        self.handle.update();
        updated = true;
        updated
    }

    fn finish_dismissal_sync(&self) -> bool {
        let mut updated = false;
        for child in self.child_menus() {
            updated |= child.finish_dismissal_sync();
        }
        if self.open.get() {
            return updated;
        }
        let mut changed = self.active_id.borrow_mut().take().is_some();
        if self.phase.replace(Phase::Idle) != Phase::Idle {
            changed = true;
        }
        if self.close_animation.replace(CloseAnimation::None) != CloseAnimation::None {
            changed = true;
        }
        if changed {
            self.handle.update();
        }
        updated || changed
    }

    async fn close_branch(self: Rc<Self>, focus_trigger: bool) {
        if self.close_sync(CloseAnimation::None) && !self.flushed().await {
            return;
        }
        if focus_trigger {
            if let Some(parent) = &self.parent {
                parent.suppress_pointer_leave_clear.set(true);
            }
            #[cfg(target_arch = "wasm32")]
            crate::event::focus_node(self.trigger.borrow().as_ref());
        }
    }

    async fn close_all(self: Rc<Self>, focus_root: bool) {
        let root = self.root();
        #[cfg(target_arch = "wasm32")]
        let surface = root.surface.borrow().clone().filter(|s| root.open.get() && s.is_connected());
        if root.close_sync(CloseAnimation::Fade) && !root.flushed().await {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(surface) = surface.filter(|s| s.is_connected()) {
            crate::event::wait_for_css_transition(surface, root.handle.signal()).await;
        }
        if root.finish_dismissal_sync() && !root.flushed().await {
            return;
        }
        if focus_root {
            #[cfg(target_arch = "wasm32")]
            crate::event::focus_node(root.trigger.borrow().as_ref());
        }
    }

    fn highlight(self: &Rc<Self>, id: Option<&str>, focus: bool) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let next = self.item(id);
        if id.is_some() && !next.as_ref().is_some_and(RegisteredItem::interactable) {
            return;
        }
        let mut updated = false;
        if let Some(open_child) = self.open_child() {
            let keeps_child = next.as_ref().and_then(RegisteredItem::submenu).is_some_and(|s| Rc::ptr_eq(&s, &open_child));
            if !keeps_child {
                updated |= open_child.close_sync(CloseAnimation::None);
            }
        }
        if self.active_id.borrow().as_deref() != id {
            *self.active_id.borrow_mut() = id.map(str::to_string);
            self.handle.update();
            updated = true;
        }
        if !focus {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let target = next.and_then(|n| n.element());
            if !updated {
                crate::event::focus_node(target.as_ref());
                return;
            }
            let this = self.clone();
            spawn(async move {
                if this.flushed().await {
                    crate::event::focus_node(target.as_ref());
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (next, updated);
    }

    fn navigate(self: &Rc<Self>, nav: Nav) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let items = self.interactable();
        let active = self.active_id();
        let index = items.iter().position(|i| Some(&i.id) == active.as_ref());
        let next = match (nav, index) {
            (Nav::Next, None) | (Nav::First, _) => items.first(),
            (Nav::Next, Some(i)) => items.get(i + 1).or(items.get(i)),
            (Nav::Previous, None) | (Nav::Last, _) => items.last(),
            (Nav::Previous, Some(i)) => i.checked_sub(1).and_then(|i| items.get(i)),
        };
        if let Some(next) = next {
            self.highlight(Some(&next.id), true);
        }
    }

    fn highlight_search_match(self: &Rc<Self>, text: &str) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let items = self.interactable();
        let active = self.active_id();
        let index = items.iter().position(|i| Some(&i.id) == active.as_ref());
        if let Some(next) = match_next_by_search_text(text, &items, index, RegisteredItem::search_values) {
            self.highlight(Some(&next.id), true);
        }
    }

    async fn activate_item(self: Rc<Self>, id: String) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let Some(item) = self.item(Some(&id)).filter(RegisteredItem::interactable) else { return };
        if let Some(submenu) = item.submenu() {
            submenu.open_menu(OpenStrategy::First, true).await;
            return;
        }
        let checked = match item.item_type {
            ItemType::Item => None,
            ItemType::Checkbox => Some(!item.checked),
            ItemType::Radio => Some(true),
        };
        self.phase.set(Phase::Selecting);
        *self.active_id.borrow_mut() = Some(item.id.clone());
        *self.flash.borrow_mut() = Some((item.id.clone(), checked));
        if !self.flushed().await {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let selected = MenuSelectItem {
                checked,
                id: item.id.clone(),
                label: item.label(),
                name: item.name.clone().unwrap_or_default(),
                item_type: item.item_type,
                value: item.value.clone(),
            };
            if let Some(el) = item.element() {
                dispatch_menu_select(&el, &selected);
            }
            crate::event::sleep(MENU_FLASH_DURATION_MS).await;
            if !self.handle.is_connected() {
                return;
            }
        }
        *self.flash.borrow_mut() = None;
        if !self.flushed().await {
            return;
        }
        self.clone().close_all(true).await;
        self.phase.set(Phase::Idle);
        self.handle.update();
    }

    fn activate_active(self: &Rc<Self>) {
        if let Some(id) = self.active_id() {
            spawn(self.clone().activate_item(id));
        }
    }

    fn open_active_submenu(&self) {
        if self.phase.get() != Phase::Idle {
            return;
        }
        let Some(item) = self.item(self.active_id().as_deref()).filter(RegisteredItem::interactable) else { return };
        if let Some(submenu) = item.submenu() {
            spawn(async move { submenu.open_menu(OpenStrategy::First, true).await });
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn register_trigger(self: &Rc<Self>, node: &NodeHandle, signal: &rmx::Signal) {
        let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
        *self.trigger.borrow_mut() = Some(el.clone());
        let menu = self.clone();
        signal.on_abort(move || {
            let mut trigger = menu.trigger.borrow_mut();
            if trigger.as_ref() == Some(&el) {
                *trigger = None;
            }
        });
    }
}

/// Dispatch a bubbling `rmx:menu-select` event whose `item` describes the selection (Remix's
/// `new MenuSelectEvent(item)`), for re-dispatching a selection from another element.
#[cfg(target_arch = "wasm32")]
pub fn dispatch_menu_select(target: &web_sys::EventTarget, item: &MenuSelectItem) {
    use wasm_bindgen::JsValue;
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    let Ok(event) = web_sys::Event::new_with_event_init_dict(MENU_SELECT_EVENT, &init) else { return };
    let object = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(&object, &key.into(), &value);
    };
    set("checked", item.checked.map_or(JsValue::UNDEFINED, JsValue::from_bool));
    set("id", item.id.as_str().into());
    set("label", item.label.as_str().into());
    set("name", item.name.as_str().into());
    set("type", item.item_type.as_str().into());
    set("value", item.value.as_deref().map_or(JsValue::NULL, JsValue::from_str));
    let _ = js_sys::Reflect::set(&event, &"item".into(), &object);
    let _ = target.dispatch_event(&event);
}

/// Read an `rmx:menu-select` event.
pub fn menu_select(event: &Event) -> Option<MenuSelectItem> {
    #[cfg(target_arch = "wasm32")]
    {
        let item = js_sys::Reflect::get(event.raw(), &"item".into()).ok().filter(|v| v.is_object())?;
        let get = |key: &str| js_sys::Reflect::get(&item, &key.into()).ok();
        let string = |key: &str| get(key).and_then(|v| v.as_string());
        let item_type = match string("type").as_deref() {
            Some("checkbox") => ItemType::Checkbox,
            Some("radio") => ItemType::Radio,
            _ => ItemType::Item,
        };
        Some(MenuSelectItem {
            checked: get("checked").and_then(|v| v.as_bool()),
            id: string("id").unwrap_or_default(),
            label: string("label").unwrap_or_default(),
            name: string("name").unwrap_or_default(),
            item_type,
            value: string("value"),
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        None
    }
}

/// Listen for menu selections on this element or its descendants.
pub fn on_menu_select(handler: EventHandler) -> Mixin {
    on(MENU_SELECT_EVENT, handler)
}

/// The menu provider (Remix's `menu.Context`). Nested inside another menu's list it is
/// a submenu. It provides the popover context its surface uses.
pub struct Context {
    state: Rc<MenuState>,
}

impl Component for Context {
    type Props = ContextProps;

    fn setup(props: &ContextProps, h: &Handle<Self>) -> Self {
        let parent = h.context::<Rc<MenuState>>().map(|p| (*p).clone());
        let state = Rc::new(MenuState {
            handle: h.clone(),
            parent,
            label: RefCell::new(props.label.clone()),
            active_id: RefCell::new(None),
            children: RefCell::default(),
            close_animation: Cell::new(CloseAnimation::None),
            flash: RefCell::new(None),
            items: RefCell::default(),
            open: Cell::new(false),
            phase: Cell::new(Phase::Idle),
            suppress_pointer_leave_clear: Cell::new(false),
            list_id: format!("{}-list", h.dom_id()),
            trigger_id: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            list: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            surface: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            trigger: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            hover_aim: Default::default(),
        });
        h.set_context(state.clone());
        Context { state }
    }

    fn render(&self, props: &ContextProps, _: &Handle<Self>) -> Node {
        let state = &self.state;
        *state.label.borrow_mut() = props.label.clone();
        state.children.borrow_mut().clear();
        state.items.borrow_mut().clear();
        if let Some(parent) = &state.parent {
            parent.register_child(state);
        }
        Node::component::<crate::popover::Context>(None, crate::popover::ContextProps { children: props.children.clone() })
    }
}

fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<MenuState> {
    h.context::<Rc<MenuState>>().map(|s| (*s).clone()).expect("menu mixins must be inside <menu::Context>")
}

/// The button that opens the root menu, anchored `bottom-start`.
pub fn trigger() -> Mixin {
    trigger_with(AnchorOptions::new(Placement::BottomStart))
}

/// [`trigger`] with custom anchoring.
pub fn trigger_with(options: AnchorOptions) -> Mixin {
    mixin::<Trigger>(options)
}

/// An element that opens the root menu at the pointer on right click, or at its own
/// bounds with the ContextMenu key or Shift+F10.
pub fn context_trigger() -> Mixin {
    context_trigger_with(AnchorOptions::new(Placement::BottomStart))
}

/// [`context_trigger`] with custom anchoring.
pub fn context_trigger_with(options: AnchorOptions) -> Mixin {
    mixin::<ContextTrigger>(options)
}

/// The popover surface holding a menu's list.
pub fn popover() -> Mixin {
    mixin::<Popover>(())
}

/// The `role="menu"` list: keyboard navigation and typeahead.
pub fn list() -> Mixin {
    mixin::<List>(())
}

/// An item.
pub fn item(options: ItemOptions) -> Mixin {
    mixin::<Item>(options)
}

/// The item in the parent list that opens this (nested) menu.
pub fn submenu_trigger(options: SubmenuTriggerOptions) -> Mixin {
    mixin::<SubmenuTrigger>(options)
}

/// The host's own `id`, else `fallback` set on it (Remix's `attrs` only fills props the
/// element leaves unset).
fn host_id(host: &mut Element, fallback: &str) -> String {
    if let Some(rmx::AttrValue::Str(id)) = host.attr("id") {
        return id.to_string();
    }
    host.set_attr("id", fallback.to_string());
    fallback.to_string()
}

fn set_trigger_attrs(menu: &MenuState, host: &mut Element, id: &str) {
    let id = host_id(host, id);
    *menu.trigger_id.borrow_mut() = Some(id);
    host.set_attr("aria-controls", menu.list_id.clone());
    host.set_attr("aria-expanded", if menu.is_open() { "true" } else { "false" });
    host.set_attr("aria-haspopup", "menu");
}

pub struct Trigger {
    menu: Rc<MenuState>,
}

impl HostMixin for Trigger {
    type Args = AnchorOptions;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Trigger { menu: state(h) }
    }

    fn render(&mut self, options: &AnchorOptions, host: &mut Element, h: &MixinHandle<Self>) {
        set_trigger_attrs(&self.menu, host, h.id());
        #[cfg(target_arch = "wasm32")]
        {
            let menu = self.menu.clone();
            host.mix(rmx::node_ref(move |node, signal| menu.register_trigger(node, signal)));
        }
        host.mix(crate::popover::anchor(options.clone()));
        host.mix(on(
            "click",
            h.on(|this, _| {
                if this.menu.is_open() {
                    this.menu.close();
                } else {
                    this.menu.open(OpenStrategy::List);
                }
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                let strategy = match event.key().as_str() {
                    "ArrowDown" => OpenStrategy::First,
                    "ArrowUp" => OpenStrategy::Last,
                    "Enter" | " " => OpenStrategy::List,
                    _ => return,
                };
                event.prevent_default();
                event.stop_propagation();
                this.menu.open(strategy);
            }),
        ));
    }
}

pub struct ContextTrigger {
    menu: Rc<MenuState>,
    #[cfg(target_arch = "wasm32")]
    popover: Option<Rc<crate::popover::PopoverState>>,
    #[cfg(target_arch = "wasm32")]
    point: Rc<Cell<crate::anchor::AnchorPoint>>,
    options: Option<AnchorOptions>,
}

impl ContextTrigger {
    #[cfg(target_arch = "wasm32")]
    fn open_at(&self, point: crate::anchor::AnchorPoint) {
        self.point.set(point);
        if let (Some(popover), Some(options)) = (&self.popover, &self.options) {
            popover.set_anchor(crate::anchor::AnchorTarget::Point(self.point.clone()), options.clone());
        }
        self.menu.open(OpenStrategy::List);
    }
}

impl HostMixin for ContextTrigger {
    type Args = AnchorOptions;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        ContextTrigger {
            menu: state(h),
            #[cfg(target_arch = "wasm32")]
            popover: h.context::<Rc<crate::popover::PopoverState>>().map(|p| (*p).clone()),
            #[cfg(target_arch = "wasm32")]
            point: Rc::default(),
            options: None,
        }
    }

    fn render(&mut self, options: &AnchorOptions, host: &mut Element, h: &MixinHandle<Self>) {
        self.options = Some(options.clone());
        set_trigger_attrs(&self.menu, host, h.id());
        #[cfg(target_arch = "wasm32")]
        {
            let menu = self.menu.clone();
            host.mix(rmx::node_ref(move |node, signal| menu.register_trigger(node, signal)));
        }
        host.mix(on(
            "contextmenu",
            h.on_event(|this, _, event| {
                event.prevent_default();
                event.stop_propagation();
                #[cfg(target_arch = "wasm32")]
                {
                    let raw = event.raw();
                    let (x, y) = (crate::event::num_prop(raw, "clientX"), crate::event::num_prop(raw, "clientY"));
                    this.open_at(crate::anchor::AnchorPoint { x, y, width: 0.0, height: 0.0 });
                }
                let _ = this;
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                {
                    let key = event.key();
                    if key != "ContextMenu" && !(key == "F10" && crate::event::bool_prop(event.raw(), "shiftKey")) {
                        return;
                    }
                    event.prevent_default();
                    event.stop_propagation();
                    let Some(el) = event.current_element() else { return };
                    let r = el.get_bounding_client_rect();
                    this.open_at(crate::anchor::AnchorPoint { x: r.left(), y: r.top(), width: r.width(), height: r.height() });
                }
                let _ = (this, event);
            }),
        ));
    }
}

pub struct Popover {
    menu: Rc<MenuState>,
}

impl HostMixin for Popover {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let menu = state(h);
        #[cfg(target_arch = "wasm32")]
        {
            let menu = menu.clone();
            h.queue_task(move |_, h, node| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                *menu.surface.borrow_mut() = Some(el.clone());
                let menu = menu.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut surface = menu.surface.borrow_mut();
                    if surface.as_ref() == Some(&el) {
                        *surface = None;
                    }
                });
            });
        }
        Popover { menu }
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        let menu = &self.menu;
        if menu.close_animation.get() == CloseAnimation::None {
            host.set_attr("data-close-animation", "none");
        }
        if !menu.is_root() {
            host.set_attr("data-menu-submenu", "true");
        }
        let hide = menu.clone();
        let options = crate::popover::SurfaceOptions::new(menu.is_open(), move |request| {
            use crate::popover::HideReason;
            if hide.is_root() || request.reason == HideReason::EscapeKey {
                hide.close();
            } else if request.reason == HideReason::OutsideClick {
                #[cfg(target_arch = "wasm32")]
                {
                let root = hide.root();
                let inside_root = match (root.surface.borrow().as_ref(), request.target.as_ref()) {
                    (Some(surface), Some(target)) => surface.contains(Some(target)),
                    _ => false,
                };
                if inside_root {
                    spawn(hide.clone().close_branch(false));
                }
                }
            }
        })
        .close_on_anchor_click(false)
        .restore_focus_on_hide(false)
        .stop_outside_click_propagation(false);
        host.mix(crate::popover::surface(options));
    }
}

/// Whether a pointer leaving an item should clear its highlight: not when it moves into
/// the item or onto another item of the same menu.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
fn should_clear_on_pointer_leave(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Some(current) = event.current_element() else { return false };
        let related = crate::event::related_target(event.raw());
        if related.as_ref().is_some_and(|r| current.contains(Some(r))) {
            return false;
        }
        let Some(related) = related.and_then(|r| r.dyn_into::<web_sys::Element>().ok()) else { return true };
        let menu = current.closest("[role=\"menu\"]").ok().flatten();
        let next_item = related.closest("[role^=\"menuitem\"]").ok().flatten();
        match (menu, next_item) {
            (Some(menu), Some(next)) => next.closest("[role=\"menu\"]").ok().flatten().as_ref() != Some(&menu),
            _ => true,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// Whether a key event comes from this list rather than a nested submenu's list.
fn belongs_to_current_menu(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let target = event.raw().target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        let owner = target.and_then(|t| t.closest("[role=\"menu\"]").ok().flatten());
        owner.is_some() && owner == event.current_element()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        false
    }
}

fn has_modifier(event: &Event) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        ["ctrlKey", "altKey", "metaKey"].iter().any(|k| crate::event::bool_prop(event.raw(), k))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = event;
        false
    }
}

#[cfg(target_arch = "wasm32")]
fn allows_pointer(menu: &MenuState, event: &Event) -> bool {
    menu.hover_aim.accepts(event.raw())
}

pub struct List {
    menu: Rc<MenuState>,
}

impl HostMixin for List {
    type Args = ();

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        List { menu: state(h) }
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        let menu = &self.menu;
        let label = menu.label.borrow().clone();
        host.set_attr("id", menu.list_id.clone());
        host.set_attr("role", "menu");
        host.set_attr("tabindex", "-1");
        if label.is_none() {
            host.set_attr("aria-labelledby", menu.trigger_id.borrow().clone());
        }
        host.set_attr("aria-label", label);
        #[cfg(target_arch = "wasm32")]
        {
            let registered = menu.clone();
            host.mix(rmx::node_ref(move |node, signal| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                *registered.list.borrow_mut() = Some(el.clone());
                let registered = registered.clone();
                signal.on_abort(move || {
                    let mut list = registered.list.borrow_mut();
                    if list.as_ref() == Some(&el) {
                        *list = None;
                    }
                });
            }));
        }
        host.mix(on(
            "pointerleave",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                {
                    let related = crate::event::related_target(event.raw());
                    if let (Some(current), Some(related)) = (event.current_element(), related) {
                        if current.contains(Some(&related)) {
                            return;
                        }
                    }
                }
                let _ = event;
                if this.menu.open_child().is_none() {
                    if this.menu.suppress_pointer_leave_clear.replace(false) {
                        return;
                    }
                    this.menu.highlight(None, false);
                }
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                if !belongs_to_current_menu(event) {
                    return;
                }
                let key = event.key();
                if (key.chars().count() == 1 && !has_modifier(event)) || key == "Backspace" {
                    // Typeahead (on this list) only.
                    event.stop_propagation();
                    return;
                }
                let menu = this.menu.clone();
                let handled = |event: &Event| {
                    event.prevent_default();
                    event.stop_propagation();
                };
                match key.as_str() {
                        "ArrowDown" => {
                            handled(event);
                            menu.navigate(Nav::Next);
                        }
                        "ArrowUp" => {
                            handled(event);
                            menu.navigate(Nav::Previous);
                        }
                        "Home" => {
                            handled(event);
                            menu.navigate(Nav::First);
                        }
                        "End" => {
                            handled(event);
                            menu.navigate(Nav::Last);
                        }
                        "Enter" | " " => {
                            handled(event);
                            menu.activate_active();
                        }
                        "ArrowRight" => {
                            handled(event);
                            menu.open_active_submenu();
                        }
                        "ArrowLeft" if !menu.is_root() => {
                            handled(event);
                            spawn(menu.close_branch(true));
                        }
                        "Escape" => {
                            handled(event);
                            menu.close();
                        }
                        "Tab" => {
                            event.stop_propagation();
                            spawn(menu.close_all(false));
                        }
                        _ => {}
                    }
            }),
        ));
        let typeahead = menu.clone();
        host.mix(hidden_typeahead(move |text| typeahead.highlight_search_match(text)));
    }
}

fn node_slot<M: HostMixin>(h: &MixinHandle<M>) -> NodeSlot {
    let node: NodeSlot = Rc::default();
    let slot = node.clone();
    h.queue_task(move |_, _, n| *slot.borrow_mut() = Some(n.clone()));
    node
}

pub struct Item {
    menu: Rc<MenuState>,
    node: NodeSlot,
    id: String,
}

impl HostMixin for Item {
    type Args = ItemOptions;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Item { menu: state(h), node: node_slot(h), id: h.id().to_string() }
    }

    fn render(&mut self, options: &ItemOptions, host: &mut Element, h: &MixinHandle<Self>) {
        let menu = &self.menu;
        let id = self.id.clone();
        menu.register_item(RegisteredItem {
            id: id.clone(),
            item_type: options.item_type,
            checked: options.checked,
            disabled: options.disabled,
            search_value: options.search_value.clone(),
            label: options.label.clone(),
            name: Some(options.name.clone()),
            value: options.value.clone(),
            submenu: None,
            node: self.node.clone(),
        });
        let flash = menu.flash.borrow().clone().filter(|(f, _)| *f == id);
        host_id(host, &id);
        host.set_attr("role", options.item_type.role());
        host.set_attr("tabindex", "-1");
        if options.item_type != ItemType::Item {
            let checked = match &flash {
                Some((_, checked)) => checked.unwrap_or(false),
                None => options.checked,
            };
            host.set_attr("aria-checked", if checked { "true" } else { "false" });
        }
        if options.disabled {
            host.set_attr("aria-disabled", "true");
        }
        if flash.is_some() {
            host.set_attr("data-menu-flash", "true");
        }
        if menu.active_id.borrow().as_deref() == Some(id.as_str()) {
            host.set_attr("data-highlighted", "true");
        }
        if options.disabled {
            return;
        }
        host.mix(on("click", h.on(|this, _| spawn(this.menu.clone().activate_item(this.id.clone())))));
        host.mix(on(
            "pointermove",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                if !allows_pointer(&this.menu, event) {
                    return;
                }
                let _ = event;
                this.menu.highlight(Some(&this.id), true);
            }),
        ));
        host.mix(on(
            "pointerleave",
            h.on_event(|this, _, event| {
                if this.menu.active_id().as_deref() == Some(this.id.as_str()) && should_clear_on_pointer_leave(event) {
                    this.menu.highlight(None, false);
                }
            }),
        ));
        host.mix(on("focus", h.on(|this, _| this.menu.highlight(Some(&this.id), false))));
    }
}

pub struct SubmenuTrigger {
    /// The menu this trigger opens.
    child: Rc<MenuState>,
    node: NodeSlot,
    id: String,
    /// A hover or focus open is pending or has fired (Remix's non-zero `openTimeoutId`).
    scheduled: bool,
    generation: u32,
    disabled: bool,
}

impl SubmenuTrigger {
    fn clear_scheduled_open(&mut self) {
        self.scheduled = false;
        self.generation += 1;
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn schedule_open(&mut self, h: &MixinHandle<Self>) {
        self.clear_scheduled_open();
        if self.disabled || self.child.is_open() {
            return;
        }
        self.scheduled = true;
        #[cfg(target_arch = "wasm32")]
        {
            let (h, generation) = (h.clone(), self.generation);
            // Started here, as Remix calls `window.setTimeout` from the handler.
            let fired = crate::event::timer(SUBMENU_OPEN_DELAY_MS);
            spawn(async move {
                let _ = fired.await;
                h.with(|this, _| {
                    let active = this.child.parent.as_ref().and_then(|p| p.active_id());
                    if this.generation == generation && active.as_deref() == Some(this.id.as_str()) {
                        let child = this.child.clone();
                        spawn(async move { child.open_menu(OpenStrategy::None, false).await });
                    }
                });
            });
        }
    }
}

impl HostMixin for SubmenuTrigger {
    type Args = SubmenuTriggerOptions;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        SubmenuTrigger { child: state(h), node: node_slot(h), id: h.id().to_string(), scheduled: false, generation: 0, disabled: false }
    }

    fn render(&mut self, options: &SubmenuTriggerOptions, host: &mut Element, h: &MixinHandle<Self>) {
        self.disabled = options.disabled;
        let Some(parent) = self.child.parent.clone() else { return };
        let child = &self.child;
        let id = self.id.clone();
        parent.register_item(RegisteredItem {
            id: id.clone(),
            item_type: ItemType::Item,
            checked: false,
            disabled: options.disabled,
            search_value: options.search_value.clone(),
            label: options.label.clone(),
            name: None,
            value: options.value.clone(),
            submenu: Some(Rc::downgrade(child)),
            node: self.node.clone(),
        });
        *child.trigger_id.borrow_mut() = Some(host_id(host, &id));
        host.set_attr("role", "menuitem");
        host.set_attr("tabindex", "-1");
        host.set_attr("aria-controls", child.list_id.clone());
        if options.disabled {
            host.set_attr("aria-disabled", "true");
        }
        host.set_attr("aria-expanded", if child.is_open() { "true" } else { "false" });
        host.set_attr("aria-haspopup", "menu");
        if child.phase() != Phase::Idle {
            host.set_attr("data-submenu-state", child.phase().as_str());
        }
        if parent.active_id().as_deref() == Some(id.as_str()) {
            host.set_attr("data-highlighted", "true");
        }
        #[cfg(target_arch = "wasm32")]
        {
            let menu = child.clone();
            host.mix(rmx::node_ref(move |node, signal| menu.register_trigger(node, signal)));
        }
        host.mix(crate::popover::anchor(AnchorOptions::new(Placement::RightStart).relative_to(SUBMENU_ANCHOR_RELATIVE_TO)));
        if options.disabled {
            return;
        }
        host.mix(on(
            "click",
            h.on(|this, _| {
                if let Some(parent) = &this.child.parent {
                    parent.highlight(Some(&this.id), true);
                }
                let child = this.child.clone();
                spawn(async move { child.open_menu(OpenStrategy::None, false).await });
            }),
        ));
        host.mix(on(
            "pointermove",
            h.on_event(|this, h, event| {
                let Some(parent) = this.child.parent.clone() else { return };
                #[cfg(target_arch = "wasm32")]
                {
                    if !allows_pointer(&parent, event) {
                        return;
                    }
                    let focused = this.node.borrow().as_ref().is_some_and(|n| {
                        let el = n.element();
                        el.owner_document().and_then(|d| d.active_element()).as_ref() == Some(el)
                    });
                    let should_schedule = focused && !this.scheduled;
                    parent.highlight(Some(&this.id), true);
                    if should_schedule {
                        this.schedule_open(h);
                    }
                }
                let _ = (parent, h, event);
            }),
        ));
        host.mix(on(
            "pointerleave",
            h.on_event(|this, _, event| {
                this.clear_scheduled_open();
                let Some(parent) = this.child.parent.clone() else { return };
                #[cfg(target_arch = "wasm32")]
                if this.child.is_open() {
                    let source = this.node.borrow().as_ref().and_then(|n| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(n.element().clone()).ok());
                    let target = this.child.surface.borrow().clone();
                    if let (Some(source), Some(target)) = (source, target) {
                        if parent.hover_aim.start(&source, &target, Some(event.raw()), None) {
                            return;
                        }
                    }
                }
                if parent.active_id().as_deref() == Some(this.id.as_str()) && should_clear_on_pointer_leave(event) {
                    parent.highlight(None, false);
                }
            }),
        ));
        host.mix(on(
            "focus",
            h.on_event(|this, h, event| {
                if let Some(parent) = &this.child.parent {
                    parent.highlight(Some(&this.id), false);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::JsCast;
                    // Focus returning from the submenu (ArrowLeft) does not reopen it.
                    let from = crate::event::related_target(event.raw()).and_then(|n| n.dyn_into::<web_sys::Element>().ok());
                    let from_menu = from.and_then(|el| el.closest("[role=\"menu\"]").ok().flatten());
                    if from_menu.is_some_and(|m| m.id() == this.child.list_id) {
                        return;
                    }
                }
                let _ = event;
                this.schedule_open(h);
            }),
        ));
        host.mix(on("blur", h.on(|this, _| this.clear_scheduled_open())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::{render_to_string, rsx};

    #[test]
    fn renders_roles_checked_state_and_labelling() {
        let html = render_to_string(&rsx! {
            <Context>
                <button type="button" mix={trigger()}>"View"</button>
                <div mix={popover()}>
                    <div mix={list()}>
                        <div mix={item(ItemOptions::new("wordWrap").checkbox(true))}>"Word wrap"</div>
                        <div mix={item(ItemOptions::new("minimap").disabled(true))}>"Minimap"</div>
                        <Context label="Zoom">
                            <div mix={submenu_trigger(SubmenuTriggerOptions::default())}>"Zoom"</div>
                            <div mix={popover()}>
                                <div mix={list()}>
                                    <div mix={item(ItemOptions::new("zoom").radio("100", false))}>"100%"</div>
                                </div>
                            </div>
                        </Context>
                    </div>
                </div>
            </Context>
        });
        assert!(html.contains(r#"aria-expanded="false" aria-haspopup="menu""#), "{html}");
        assert!(html.contains(r#"role="menu" tabindex="-1" aria-labelledby="rmx-m"#), "the root list is labelled by its trigger: {html}");
        assert!(html.contains(r#"role="menuitemcheckbox" tabindex="-1" aria-checked="true""#), "{html}");
        assert!(html.contains(r#"role="menuitem" tabindex="-1" aria-disabled="true""#), "{html}");
        assert!(html.contains(r#"role="menuitemradio" tabindex="-1" aria-checked="false""#), "{html}");
        assert!(html.contains(r#"aria-label="Zoom""#) && html.contains(r#"data-menu-submenu="true""#), "{html}");
        assert!(html.contains(r#"role="menuitem" tabindex="-1" aria-controls=""#), "{html}");
        assert_eq!(html.matches(r#"popover="manual""#).count(), 2, "{html}");
    }

    #[test]
    fn whitespace_in_labels_is_normalized() {
        assert_eq!(normalize("  Word \n  wrap "), "Word wrap");
    }
}

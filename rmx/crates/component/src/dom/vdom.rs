//! Component instances, mounting, keyed reconciliation, hydration and update scheduling.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::{intern, JsCast};

use super::{document, frames, styles, Scope};
use crate::component::{Component, Handle, HandleShared, Instance};
use crate::mixin::{NodeHandle, NodeRef};
use crate::node::{AttrValue, Element, Event, EventHandler, FrameNode, Key, Node, Str};

// ---------------------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------------------

/// Runtime bookkeeping stored in every live component instance.
pub(crate) struct Live {
    pub(crate) alive: Cell<bool>,
    dirty: Cell<bool>,
    depth: u32,
    /// Scope handed to the components this one renders.
    child_scope: Scope,
    /// What the component's last render produced. Never empty: an empty render mounts an
    /// empty text node so the component always has a DOM position.
    root: RefCell<Vec<Mounted>>,
}

pub(crate) trait AnyInstance {
    fn as_any(&self) -> &dyn Any;
    fn component_type(&self) -> TypeId;
    fn live(&self) -> &Live;
    fn shared(&self) -> &HandleShared;
    fn render_node(&self) -> Node;
}

impl<C: Component> AnyInstance for Instance<C> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn component_type(&self) -> TypeId {
        TypeId::of::<C>()
    }

    fn live(&self) -> &Live {
        &self.live
    }

    fn shared(&self) -> &HandleShared {
        &self.handle.shared
    }

    fn render_node(&self) -> Node {
        let state = self.state.borrow();
        let props = self.props.borrow();
        state.render(&props, &self.handle)
    }
}

/// Run `setup` for a new instance. The caller renders and mounts (or hydrates) it.
pub(crate) fn new_instance<C: Component>(props: C::Props, scope: &Scope) -> Rc<dyn AnyInstance> {
    Rc::<Instance<C>>::new_cyclic(|weak| {
        let handle = Handle::attached(weak.clone(), scope);
        let state = C::setup(&props, &handle);
        let parent: Weak<dyn AnyInstance> = weak.clone();
        Instance {
            state: RefCell::new(state),
            props: RefCell::new(props),
            handle,
            live: Live {
                alive: Cell::new(true),
                dirty: Cell::new(false),
                depth: scope.depth,
                child_scope: Scope { parent: Some(parent), frame: scope.frame.clone(), depth: scope.depth + 1 },
                root: RefCell::new(Vec::new()),
            },
        }
    })
}

fn rendered_list(inst: &dyn AnyInstance) -> Vec<Node> {
    let mut flat = Vec::new();
    flatten(inst.render_node(), &mut flat);
    if flat.is_empty() {
        flat.push(empty_text());
    }
    flat
}

/// Re-render an instance and patch its DOM in place.
pub(crate) fn rerender(inst: &dyn AnyInstance) {
    let live = inst.live();
    if !live.alive.get() {
        return;
    }
    live.dirty.set(false);
    let new = rendered_list(inst);
    let (parent, end) = {
        let root = live.root.borrow();
        let first = root.first().expect("mounted component has DOM").first_dom();
        let last = root.last().expect("mounted component has DOM").last_dom();
        let Some(parent) = first.parent_node() else { return };
        (parent, last.next_sibling())
    };
    let mut root = live.root.borrow_mut();
    patch_children(&parent, &mut root, new, end.as_ref(), false, &live.child_scope);
}

/// Remove a root instance from the runtime (its DOM is removed by the caller).
pub(crate) fn unmount_root(inst: &dyn AnyInstance) {
    let live = inst.live();
    if live.alive.replace(false) {
        inst.shared().dispose();
        live.root.borrow().iter().for_each(unmount);
    }
}

// ---------------------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------------------

thread_local! {
    static DIRTY: RefCell<Vec<Rc<dyn AnyInstance>>> = const { RefCell::new(Vec::new()) };
    static TASKS: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    static SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static FLUSH: js_sys::Function = Closure::<dyn FnMut()>::new(flush).into_js_value().unchecked_into();
}

pub(crate) fn schedule_update<C: Component>(inst: Rc<Instance<C>>) {
    let live = &inst.live;
    if !live.alive.get() || live.dirty.replace(true) {
        return;
    }
    DIRTY.with(|d| d.borrow_mut().push(inst));
    schedule_flush();
}

pub(crate) fn queue_task(task: Box<dyn FnOnce()>) {
    TASKS.with(|t| t.borrow_mut().push(task));
    schedule_flush();
}

fn schedule_flush() {
    if !SCHEDULED.replace(true) {
        FLUSH.with(|f| super::window().queue_microtask(f));
    }
}

/// Render every component that called `update()`, parents before children, then run
/// queued tasks. Runs automatically on a microtask; call it to flush synchronously.
pub fn flush() {
    SCHEDULED.set(false);
    // A component that updates itself from every render would loop forever; cap the passes.
    for _ in 0..100 {
        let mut batch = DIRTY.with(|d| std::mem::take(&mut *d.borrow_mut()));
        if batch.is_empty() {
            break;
        }
        batch.sort_by_key(|i| i.live().depth);
        for inst in &batch {
            // A parent re-render already refreshed children it passed new props to.
            if inst.live().dirty.get() {
                rerender(&**inst);
            }
        }
    }
    // Tasks may queue more tasks or updates; those run in a later flush.
    let tasks = TASKS.with(|t| std::mem::take(&mut *t.borrow_mut()));
    for task in tasks {
        task();
    }
}

// ---------------------------------------------------------------------------------------
// Mounted tree
// ---------------------------------------------------------------------------------------

pub(crate) enum Mounted {
    Element(Box<MElement>),
    Text(MText),
    Component(MComponent),
    Frame(Box<MFrame>),
}

pub(crate) struct MElement {
    tag: &'static str,
    key: Option<Key>,
    attrs: Vec<(&'static str, AttrValue)>,
    listeners: Vec<Listener>,
    el: web_sys::Element,
    children: Vec<Mounted>,
}

pub(crate) struct MText {
    text: Str,
    node: web_sys::Text,
}

pub(crate) struct MComponent {
    key: Option<Key>,
    inst: Rc<dyn AnyInstance>,
}

/// A `<Frame>` rendered by a component: a comment-bounded range the frame owns.
pub(crate) struct MFrame {
    src: String,
    name: Option<String>,
    state: Rc<frames::FrameState>,
}

/// Event handling is delegated: one document listener per event type, and each element
/// stores the id of its handler in an expando property. Creating a row costs one property
/// write instead of a JS closure plus addEventListener, and re-renders only swap the
/// handler in the Rust-side table.
struct Listener {
    name: &'static str,
    id: u32,
}

#[derive(Default)]
struct HandlerTable {
    items: Vec<Option<EventHandler>>,
    free: Vec<u32>,
}

thread_local! {
    static HANDLERS: RefCell<HandlerTable> = RefCell::new(HandlerTable::default());
    static DELEGATED: RefCell<HashMap<&'static str, JsValue>> = RefCell::new(HashMap::new());
}

/// The expando property name for an event, registering the document listener on first use.
fn event_key(name: &'static str) -> JsValue {
    DELEGATED.with(|d| {
        if let Some(key) = d.borrow().get(name) {
            return key.clone();
        }
        let key = JsValue::from_str(&format!("__rmx_{name}"));
        let dispatch_key = key.clone();
        let dispatch = Closure::<dyn FnMut(web_sys::Event)>::new(move |raw: web_sys::Event| {
            dispatch(&raw, &dispatch_key);
        });
        // Events that do not bubble are caught on the way down instead.
        let capture = matches!(
            name,
            "focus" | "blur" | "mouseenter" | "mouseleave" | "load" | "error" | "scroll" | "toggle" | "invalid"
        );
        document()
            .add_event_listener_with_callback_and_bool(name, dispatch.as_ref().unchecked_ref(), capture)
            .expect("addEventListener");
        dispatch.forget();
        d.borrow_mut().insert(name, key.clone());
        key
    })
}

/// Walk from the target to the root, calling every handler registered for this event.
fn dispatch(raw: &web_sys::Event, key: &JsValue) {
    let mut node = raw.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
    let event = Event { raw: raw.clone() };
    while let Some(current) = node {
        if let Some(id) = js_sys::Reflect::get(&current, key).ok().and_then(|v| v.as_f64()) {
            let handler = HANDLERS.with(|h| h.borrow().items.get(id as usize).cloned().flatten());
            if let Some(handler) = handler {
                (handler.0)(&event);
            }
            if raw.cancel_bubble() {
                break;
            }
        }
        node = current.parent_node();
    }
}

impl Listener {
    fn attach(el: &web_sys::Element, name: &'static str, handler: EventHandler) -> Listener {
        let id = HANDLERS.with(|h| {
            let mut table = h.borrow_mut();
            match table.free.pop() {
                Some(id) => {
                    table.items[id as usize] = Some(handler);
                    id
                }
                None => {
                    table.items.push(Some(handler));
                    (table.items.len() - 1) as u32
                }
            }
        });
        let _ = js_sys::Reflect::set(el, &event_key(name), &JsValue::from(id));
        Listener { name, id }
    }

    fn replace(&self, handler: EventHandler) {
        HANDLERS.with(|h| h.borrow_mut().items[self.id as usize] = Some(handler));
    }

    /// Free the handler slot. `el` is given when the element stays in the document.
    fn release(&self, el: Option<&web_sys::Element>) {
        HANDLERS.with(|h| {
            let mut table = h.borrow_mut();
            table.items[self.id as usize] = None;
            table.free.push(self.id);
        });
        if let Some(el) = el {
            let _ = js_sys::Reflect::delete_property(el, &event_key(self.name).unchecked_into());
        }
    }
}

/// Attach several handlers for the same event as one listener that calls them in order.
fn attach_events(el: &web_sys::Element, events: Vec<(&'static str, EventHandler)>) -> Vec<Listener> {
    merge_events(events).into_iter().map(|(n, h)| Listener::attach(el, n, h)).collect()
}

fn merge_events(events: Vec<(&'static str, EventHandler)>) -> Vec<(&'static str, EventHandler)> {
    if events.len() < 2 {
        return events;
    }
    let mut merged: Vec<(&'static str, Vec<EventHandler>)> = Vec::new();
    for (name, handler) in events {
        match merged.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(handler),
            None => merged.push((name, vec![handler])),
        }
    }
    merged
        .into_iter()
        .map(|(name, mut list)| {
            if list.len() == 1 {
                (name, list.pop().expect("one handler"))
            } else {
                (name, EventHandler(Rc::new(move |e: &Event| list.iter().for_each(|h| (h.0)(e)))))
            }
        })
        .collect()
}

impl Mounted {
    fn key(&self) -> Option<&Key> {
        match self {
            Mounted::Element(e) => e.key.as_ref(),
            Mounted::Component(c) => c.key.as_ref(),
            Mounted::Text(_) | Mounted::Frame(_) => None,
        }
    }

    pub(crate) fn first_dom(&self) -> web_sys::Node {
        match self {
            Mounted::Element(e) => e.el.clone().into(),
            Mounted::Text(t) => t.node.clone().into(),
            Mounted::Component(c) => c.inst.live().root.borrow()[0].first_dom(),
            Mounted::Frame(f) => f.state.start().into(),
        }
    }

    fn last_dom(&self) -> web_sys::Node {
        match self {
            Mounted::Element(e) => e.el.clone().into(),
            Mounted::Text(t) => t.node.clone().into(),
            Mounted::Component(c) => c.inst.live().root.borrow().last().expect("component has DOM").last_dom(),
            Mounted::Frame(f) => f.state.end().into(),
        }
    }

    fn for_each_dom(&self, f: &mut dyn FnMut(&web_sys::Node)) {
        match self {
            Mounted::Element(e) => f(e.el.unchecked_ref()),
            Mounted::Text(t) => f(t.node.unchecked_ref()),
            Mounted::Component(c) => c.inst.live().root.borrow().iter().for_each(|m| m.for_each_dom(f)),
            Mounted::Frame(m) => {
                let start: web_sys::Node = m.state.start().into();
                // A frame created detached keeps its range in a fragment until inserted.
                match start.parent_node() {
                    Some(p) if p.node_type() == web_sys::Node::DOCUMENT_FRAGMENT_NODE => f(&p),
                    _ => frames::range_nodes(&m.state.start(), &m.state.end(), true).iter().for_each(f),
                }
            }
        }
    }

    fn same_kind(&self, node: &Node) -> bool {
        match (self, node) {
            (Mounted::Element(m), Node::Element(n)) => m.tag == n.tag,
            (Mounted::Text(_), Node::Text(_)) => true,
            (Mounted::Component(m), Node::Component(n)) => m.inst.component_type() == n.inner.component_type(),
            (Mounted::Frame(_), Node::Frame(_)) => true,
            _ => false,
        }
    }
}

fn empty_text() -> Node {
    Node::Text(Cow::Borrowed(""))
}

/// Expand fragments and shared subtrees into a flat child list. Server-only nodes and
/// `Empty` become empty text nodes so conditional children keep a stable position.
fn flatten(node: Node, out: &mut Vec<Node>) {
    match node {
        Node::Fragment(nodes) => nodes.into_iter().for_each(|n| flatten(n, out)),
        Node::Shared(rc) => flatten(Rc::unwrap_or_clone(rc), out),
        Node::Empty | Node::Raw(_) => out.push(empty_text()),
        other => out.push(other),
    }
}

fn flatten_children(children: Vec<Node>) -> Vec<Node> {
    let mut flat = Vec::with_capacity(children.len());
    children.into_iter().for_each(|c| flatten(c, &mut flat));
    flat
}

/// Mark a subtree's component instances as removed and free its event handlers. Does not
/// touch the DOM.
fn unmount(m: &Mounted) {
    match m {
        Mounted::Element(e) => {
            e.listeners.iter().for_each(|l| l.release(None));
            e.children.iter().for_each(unmount);
        }
        Mounted::Text(_) => {}
        Mounted::Component(c) => unmount_root(&*c.inst),
        Mounted::Frame(f) => {
            for node in frames::range_nodes(&f.state.start(), &f.state.end(), true) {
                super::diff::dispose_subtree(&node);
            }
        }
    }
}

fn insert(parent: &web_sys::Node, m: &Mounted, before: Option<&web_sys::Node>) {
    m.for_each_dom(&mut |n| {
        parent.insert_before(n, before).expect("insertBefore");
    });
}

fn remove(parent: &web_sys::Node, m: &Mounted) {
    unmount(m);
    m.for_each_dom(&mut |n| {
        let _ = parent.remove_child(n);
    });
}

/// Template strings (tags, attribute names, static text) are interned so repeated rows reuse
/// one JS string instead of re-decoding UTF-8 on every call; dynamic text is not.
fn interned(s: &Str) -> &str {
    match s {
        Cow::Borrowed(b) => intern(b),
        Cow::Owned(o) => o,
    }
}

fn set_attr(el: &web_sys::Element, name: &'static str, value: &AttrValue) {
    let name = intern(name);
    match value {
        AttrValue::Str(v) => {
            let _ = el.set_attribute(name, interned(v));
            if name == "value" {
                let _ = js_sys::Reflect::set(el, &JsValue::from_str(name), &JsValue::from_str(v));
            }
        }
        AttrValue::Bool(true) => {
            let _ = el.set_attribute(name, "");
            if name == "checked" || name == "selected" {
                let _ = js_sys::Reflect::set(el, &JsValue::from_str(name), &JsValue::TRUE);
            }
        }
        AttrValue::Bool(false) | AttrValue::Absent => {
            let _ = el.remove_attribute(name);
            if name == "checked" || name == "selected" {
                let _ = js_sys::Reflect::set(el, &JsValue::from_str(name), &JsValue::FALSE);
            }
        }
    }
}

/// Insert an element's `css()` rules and, for a newly mounted element, schedule its
/// `node_ref` callbacks to run after the commit.
fn apply_extra(el: &web_sys::Element, extra: Option<Box<crate::node::ElementExtra>>, mounted: bool) {
    let Some(extra) = extra else { return };
    for rule in &extra.styles {
        styles::insert(rule);
    }
    if mounted && !extra.refs.is_empty() {
        let el = el.clone();
        let refs: Vec<NodeRef> = extra.refs;
        queue_task(Box::new(move || {
            let handle = NodeHandle { el };
            for r in &refs {
                (r.0)(&handle);
            }
        }));
    }
}

// ---------------------------------------------------------------------------------------
// Create and patch
// ---------------------------------------------------------------------------------------

/// Build detached DOM for a node. The caller inserts it.
fn create(node: Node, scope: &Scope) -> Mounted {
    match node {
        Node::Element(el) => {
            let Element { tag, key, attrs, events, children, extra } = *el;
            let dom = if is_svg(tag) {
                document().create_element_ns(Some("http://www.w3.org/2000/svg"), intern(tag)).expect("createElementNS")
            } else {
                document().create_element(intern(tag)).expect("createElement")
            };
            for (name, value) in &attrs {
                set_attr(&dom, name, value);
            }
            apply_extra(&dom, extra, true);
            let listeners = attach_events(&dom, events);
            let mut kids = Vec::with_capacity(children.len());
            for child in flatten_children(children) {
                let m = create(child, scope);
                m.for_each_dom(&mut |n| {
                    dom.append_child(n).expect("appendChild");
                });
                kids.push(m);
            }
            Mounted::Element(Box::new(MElement { tag, key, attrs, listeners, el: dom, children: kids }))
        }
        Node::Text(text) => {
            let node = document().create_text_node(interned(&text));
            Mounted::Text(MText { text, node })
        }
        Node::Component(c) => {
            let inst = c.inner.instantiate(scope);
            let child_scope = &inst.live().child_scope;
            let root = rendered_list(&*inst).into_iter().map(|n| create(n, child_scope)).collect();
            *inst.live().root.borrow_mut() = root;
            Mounted::Component(MComponent { key: c.key, inst })
        }
        Node::Frame(f) => create_frame(&f, scope),
        other => {
            let mut flat = Vec::new();
            flatten(other, &mut flat);
            create(flat.into_iter().next().unwrap_or_else(empty_text), scope)
        }
    }
}

/// Tags that only exist in the SVG namespace (shared names like `a` stay HTML).
fn is_svg(tag: &str) -> bool {
    matches!(
        tag,
        "svg" | "path" | "circle" | "rect" | "line" | "polyline" | "polygon" | "ellipse" | "g" | "defs" | "use"
            | "symbol" | "text" | "tspan" | "clipPath" | "mask" | "pattern" | "linearGradient"
            | "radialGradient" | "stop" | "foreignObject" | "marker" | "filter"
    )
}

fn create_frame(f: &FrameNode, scope: &Scope) -> Mounted {
    let fallback = f.fallback.as_ref().map(crate::ssr::render_to_string).unwrap_or_default();
    let state = frames::create_client_frame(&f.src, f.name.clone(), scope.frame.clone(), &fallback);
    Mounted::Frame(Box::new(MFrame { src: f.src.clone(), name: f.name.clone(), state }))
}

fn patch(parent: &web_sys::Node, old: &mut Mounted, new: Node, scope: &Scope) {
    match (&mut *old, new) {
        (Mounted::Element(m), Node::Element(n)) if m.tag == n.tag => patch_element(m, *n, scope),
        (Mounted::Text(m), Node::Text(t)) => {
            if m.text != t {
                m.node.set_data(&t);
                m.text = t;
            }
        }
        (Mounted::Component(m), Node::Component(c)) if m.inst.component_type() == c.inner.component_type() => {
            m.key = c.key;
            c.inner.update_instance(&*m.inst);
        }
        (Mounted::Frame(m), Node::Frame(f)) => {
            if m.name != f.name {
                m.state.set_name(f.name.clone());
                m.name = f.name.clone();
            }
            if m.src != f.src {
                m.src = f.src.clone();
                m.state.set_src(&f.src);
                frames::spawn_reload(m.state.clone());
            }
        }
        (_, new) => {
            let replacement = create(new, scope);
            insert(parent, &replacement, Some(&old.first_dom()));
            remove(parent, old);
            *old = replacement;
        }
    }
}

fn patch_element(m: &mut MElement, n: Element, scope: &Scope) {
    let Element { key, attrs, events, children, extra, .. } = n;
    m.key = key;

    if m.attrs != attrs {
        for (name, value) in &attrs {
            let unchanged = m.attrs.iter().any(|(n, v)| n == name && v == value);
            if !unchanged {
                set_attr(&m.el, name, value);
            }
        }
        for (name, _) in &m.attrs {
            if !attrs.iter().any(|(n, _)| n == name) {
                let _ = m.el.remove_attribute(name);
            }
        }
        m.attrs = attrs;
    }
    apply_extra(&m.el, extra, false);

    let events = merge_events(events);
    let same_events =
        m.listeners.len() == events.len() && m.listeners.iter().zip(&events).all(|(l, (name, _))| l.name == *name);
    if same_events {
        for (listener, (_, handler)) in m.listeners.iter().zip(events) {
            listener.replace(handler);
        }
    } else {
        for listener in m.listeners.drain(..) {
            listener.release(Some(&m.el));
        }
        m.listeners = events.into_iter().map(|(n, h)| Listener::attach(&m.el, n, h)).collect();
    }

    if !children.is_empty() || !m.children.is_empty() {
        let parent: web_sys::Node = m.el.clone().into();
        patch_children(&parent, &mut m.children, flatten_children(children), None, true, scope);
    }
}

/// Reconcile a flat child list. `end` is the DOM node after this list (None = end of
/// parent); `whole` means the list is every child of `parent`, which allows fast clears.
fn patch_children(
    parent: &web_sys::Node,
    old: &mut Vec<Mounted>,
    new: Vec<Node>,
    end: Option<&web_sys::Node>,
    whole: bool,
    scope: &Scope,
) {
    if new.is_empty() {
        if whole {
            old.drain(..).for_each(|m| unmount(&m));
            parent.set_text_content(None);
        } else {
            old.drain(..).for_each(|m| remove(parent, &m));
        }
        return;
    }
    if old.is_empty() {
        for node in new {
            let m = create(node, scope);
            insert(parent, &m, end);
            old.push(m);
        }
        return;
    }

    let keyed = new.iter().any(|n| n.key().is_some()) && old.iter().any(|m| m.key().is_some());
    let same_key_order = old.len() == new.len() && old.iter().zip(&new).all(|(o, n)| o.key() == n.key());
    if !keyed || same_key_order {
        patch_indexed(parent, old, new, end, scope);
    } else {
        patch_keyed(parent, old, new, end, whole, scope);
    }
}

fn patch_indexed(parent: &web_sys::Node, old: &mut Vec<Mounted>, new: Vec<Node>, end: Option<&web_sys::Node>, scope: &Scope) {
    let new_len = new.len();
    for (i, node) in new.into_iter().enumerate() {
        if i < old.len() {
            patch(parent, &mut old[i], node, scope);
        } else {
            let m = create(node, scope);
            insert(parent, &m, end);
            old.push(m);
        }
    }
    if old.len() > new_len {
        old.drain(new_len..).for_each(|m| remove(parent, &m));
    }
}

fn patch_keyed(
    parent: &web_sys::Node,
    old: &mut Vec<Mounted>,
    new: Vec<Node>,
    end: Option<&web_sys::Node>,
    whole: bool,
    scope: &Scope,
) {
    let mut slots: Vec<Option<Mounted>> = old.drain(..).map(Some).collect();
    let mut by_key: HashMap<Key, usize> = HashMap::with_capacity(slots.len());
    for (i, slot) in slots.iter().enumerate() {
        if let Some(key) = slot.as_ref().and_then(Mounted::key) {
            by_key.insert(key.clone(), i);
        }
    }

    // Match new children to old ones by key; unmatched ones are created detached.
    let mut matched: Vec<(Option<Mounted>, Node, Option<usize>)> = Vec::with_capacity(new.len());
    let mut any_reused = false;
    for node in new {
        let found = node.key().and_then(|k| by_key.remove(k));
        match found.and_then(|i| slots[i].take().map(|m| (i, m))) {
            Some((i, m)) if m.same_kind(&node) => {
                any_reused = true;
                matched.push((Some(m), node, Some(i)));
            }
            Some((_, m)) => {
                slots.push(Some(m)); // removed below with the other leftovers
                matched.push((None, node, None));
            }
            None => matched.push((None, node, None)),
        }
    }

    // Nothing reused and the list owns the parent: clear in one DOM call.
    if !any_reused && whole {
        slots.into_iter().flatten().for_each(|m| unmount(&m));
        parent.set_text_content(None);
    } else {
        slots.into_iter().flatten().for_each(|m| remove(parent, &m));
    }

    let mut result = Vec::with_capacity(matched.len());
    let mut sources = Vec::with_capacity(matched.len());
    for (existing, node, source) in matched {
        let m = match existing {
            Some(mut m) => {
                patch(parent, &mut m, node, scope);
                m
            }
            None => create(node, scope),
        };
        result.push(m);
        sources.push(source.map_or(-1, |s| s as i64));
    }

    // Children on the longest increasing run of old positions stay put; move the rest.
    let stay = longest_increasing(&sources);
    let mut next: Option<web_sys::Node> = end.cloned();
    for i in (0..result.len()).rev() {
        if !stay[i] {
            insert(parent, &result[i], next.as_ref());
        }
        next = Some(result[i].first_dom());
    }
    *old = result;
}

/// Mask of positions on a longest strictly increasing subsequence of `sources`
/// (negative entries are new nodes and never count).
fn longest_increasing(sources: &[i64]) -> Vec<bool> {
    let mut stay = vec![false; sources.len()];
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; sources.len()];
    for (i, &s) in sources.iter().enumerate() {
        if s < 0 {
            continue;
        }
        let pos = tails.partition_point(|&t| sources[t] < s);
        if pos > 0 {
            prev[i] = tails[pos - 1];
        }
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
    }
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        stay[i] = true;
        cur = (prev[i] != usize::MAX).then_some(prev[i]);
    }
    stay
}

// ---------------------------------------------------------------------------------------
// Hydration
// ---------------------------------------------------------------------------------------

/// Render an instance and adopt the server DOM starting at `cursor`.
pub(crate) fn hydrate_instance(inst: &dyn AnyInstance, parent: &web_sys::Node, cursor: &mut Option<web_sys::Node>) {
    let scope = &inst.live().child_scope;
    let root = rendered_list(inst).into_iter().map(|n| hydrate_node(n, parent, cursor, scope)).collect();
    *inst.live().root.borrow_mut() = root;
}

/// Adopt the server DOM at `cursor` for `node`, or create and insert DOM on a mismatch.
fn hydrate_node(node: Node, parent: &web_sys::Node, cursor: &mut Option<web_sys::Node>, scope: &Scope) -> Mounted {
    match node {
        Node::Element(el) => {
            let adopt = cursor
                .as_ref()
                .and_then(|c| c.dyn_ref::<web_sys::Element>())
                .filter(|e| e.tag_name().eq_ignore_ascii_case(el.tag))
                .cloned();
            let Some(dom) = adopt else {
                return create_at(Node::Element(el), parent, cursor, scope);
            };
            *cursor = dom.next_sibling();
            let Element { tag, key, attrs, events, children, extra } = *el;
            // Server and client attributes can differ (ids from h.id(), dynamic values).
            for (name, value) in &attrs {
                let current = dom.get_attribute(name);
                let matches = match value {
                    AttrValue::Str(v) => current.as_deref() == Some(&**v),
                    AttrValue::Bool(true) => current.is_some(),
                    AttrValue::Bool(false) | AttrValue::Absent => current.is_none(),
                };
                if !matches {
                    set_attr(&dom, name, value);
                }
            }
            apply_extra(&dom, extra, true);
            let listeners = attach_events(&dom, events);
            let dom_node: web_sys::Node = dom.clone().into();
            let mut child_cursor = dom.first_child();
            let kids = flatten_children(children)
                .into_iter()
                .map(|c| hydrate_node(c, &dom_node, &mut child_cursor, scope))
                .collect();
            while let Some(extra) = child_cursor.take() {
                child_cursor = extra.next_sibling();
                let _ = dom_node.remove_child(&extra);
            }
            Mounted::Element(Box::new(MElement { tag, key, attrs, listeners, el: dom, children: kids }))
        }
        Node::Text(text) if !text.is_empty() => {
            let adopt = cursor.as_ref().and_then(|c| c.dyn_ref::<web_sys::Text>()).cloned();
            let Some(dom) = adopt else {
                return create_at(Node::Text(text), parent, cursor, scope);
            };
            let data = dom.data();
            if data == *text {
                *cursor = dom.next_sibling();
            } else if data.starts_with(&*text) {
                // Adjacent text children arrive merged into one DOM text node; split them.
                let len = text.encode_utf16().count() as u32;
                *cursor = Some(dom.split_text(len).expect("splitText").into());
            } else {
                dom.set_data(&text);
                *cursor = dom.next_sibling();
            }
            Mounted::Text(MText { text, node: dom })
        }
        Node::Component(c) => {
            let inst = c.inner.instantiate(scope);
            hydrate_instance(&*inst, parent, cursor);
            Mounted::Component(MComponent { key: c.key, inst })
        }
        Node::Frame(f) => {
            let start = cursor
                .as_ref()
                .and_then(|c| c.dyn_ref::<web_sys::Comment>())
                .filter(|c| c.data().starts_with("rmx:f:"))
                .cloned();
            let state = start.and_then(|s| frames::adopt_server_frame(&s, scope.frame.clone()));
            let Some(state) = state else {
                return create_at(Node::Frame(f), parent, cursor, scope);
            };
            *cursor = state.end().next_sibling();
            Mounted::Frame(Box::new(MFrame { src: f.src.clone(), name: f.name.clone(), state }))
        }
        other => create_at(other, parent, cursor, scope),
    }
}

fn create_at(node: Node, parent: &web_sys::Node, cursor: &Option<web_sys::Node>, scope: &Scope) -> Mounted {
    let m = create(node, scope);
    insert(parent, &m, cursor.as_ref());
    m
}

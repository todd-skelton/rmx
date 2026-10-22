//! Component instances, mounting, keyed reconciliation, hydration and update scheduling.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::{intern, JsCast};

use super::document_state::DocumentState;
use super::{document, frames, styles, Scope};
use crate::component::{Component, Handle, HandleShared, Instance, Phase};
use crate::host_mixin::has_mixins;
use super::mixin_host::MixinHost;
use crate::node::{AttrValue, Element, ElementExtra, Event, EventHandler, FrameNode, Key, Node, Str, StyleRule};

// ---------------------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------------------

/// Runtime bookkeeping stored in every live component instance.
pub(crate) struct Live {
    pub(crate) alive: Cell<bool>,
    dirty: Cell<bool>,
    depth: u32,
    /// (event loop turn, renders in that turn) for the update-loop check.
    cascade: Cell<(u32, u32)>,
    /// Scope handed to the components this one renders.
    child_scope: Scope,
    /// What the component's last render produced. Never empty: an empty render mounts an
    /// empty text node so the component always has a DOM position.
    root: RefCell<Vec<Mounted>>,
    /// Elements with stateful mixins this component rendered, for `beforeUpdate`/`commit`.
    pub(crate) hosts: RefCell<Vec<Weak<MixinHost>>>,
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
        let shared = &self.handle.shared;
        let outer = shared.begin_render();
        let node = self.state.borrow().render(&self.props.borrow(), &self.handle);
        shared.end_render(outer);
        node
    }
}

/// Run `setup` for a new instance. The caller renders and mounts (or hydrates) it.
pub(crate) fn new_instance<C: Component>(props: C::Props, scope: &Scope) -> Rc<dyn AnyInstance> {
    Rc::<Instance<C>>::new_cyclic(|weak| {
        let handle = Handle::attached(weak.clone(), scope);
        let state = C::setup(&props, &handle);
        handle.shared.phase.set(Phase::Idle);
        let parent: Weak<dyn AnyInstance> = weak.clone();
        Instance {
            state: RefCell::new(state),
            props: RefCell::new(props),
            handle,
            live: Live {
                alive: Cell::new(true),
                dirty: Cell::new(false),
                depth: scope.depth,
                cascade: Cell::new((0, 0)),
                child_scope: Scope {
                    parent: Some(parent),
                    frame: scope.frame.clone(),
                    depth: scope.depth + 1,
                    errors: scope.errors.clone(),
                    svg: scope.svg,
                },
                root: RefCell::new(Vec::new()),
                hosts: RefCell::new(Vec::new()),
            },
        }
    })
}

/// Render an instance into a flat node list, or `None` when the render failed
/// ([`crate::Handle::fail`]).
fn render_output(inst: &dyn AnyInstance) -> Option<Vec<Node>> {
    let node = inst.render_node();
    if inst.shared().failed.take() {
        return None;
    }
    let mut flat = Vec::new();
    flatten(node, &mut flat);
    if flat.is_empty() {
        flat.push(empty_text());
    }
    Some(flat)
}

/// A first render's nodes: a failed one renders nothing.
fn rendered_list(inst: &dyn AnyInstance) -> Vec<Node> {
    render_output(inst).unwrap_or_else(|| vec![empty_text()])
}

/// Re-render an instance and patch its DOM in place.
pub(crate) fn rerender(inst: &dyn AnyInstance) {
    let live = inst.live();
    if !live.alive.get() {
        return;
    }
    live.dirty.set(false);
    let hosts = live_hosts(live);
    hosts.iter().for_each(|h| h.before_update());
    // A failed render keeps the DOM of the last one.
    let Some(new) = render_output(inst) else {
        hosts.iter().for_each(|h| h.commit());
        return;
    };
    let (parent, end) = {
        let root = live.root.borrow();
        let first = root.first().expect("mounted component has DOM").first_dom();
        let last = root.last().expect("mounted component has DOM").last_dom();
        let Some(parent) = first.parent_node() else { return };
        (parent, last.next_sibling())
    };
    {
        let mut root = live.root.borrow_mut();
        patch_children(&parent, &mut root, new, end.as_ref(), false, &live.child_scope);
    }
    hosts.iter().for_each(|h| h.commit());
}

/// The live mixin hosts a component rendered (dropping ones that are gone).
fn live_hosts(live: &Live) -> Vec<Rc<MixinHost>> {
    let mut list = live.hosts.borrow_mut();
    if list.is_empty() {
        return Vec::new();
    }
    list.retain(|w| w.upgrade().is_some_and(|h| h.is_alive()));
    list.iter().filter_map(Weak::upgrade).collect()
}

/// Remove a root instance from the runtime and its DOM from the document.
pub(crate) fn remove_root(inst: &dyn AnyInstance) {
    unmount_root(inst);
    for m in inst.live().root.borrow().iter() {
        m.for_each_dom(&mut |n| {
            if let Some(parent) = n.parent_node() {
                let _ = parent.remove_child(n);
            }
        });
    }
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
    /// Commit-phase callbacks (mixin `insert` events), run before `TASKS`.
    static COMMITS: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    /// Mixin hosts that called `update()`.
    static DIRTY_HOSTS: RefCell<Vec<Rc<MixinHost>>> = const { RefCell::new(Vec::new()) };
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

/// Queue a commit-phase callback: it runs after the pending renders, before tasks.
pub(crate) fn queue_commit(task: Box<dyn FnOnce()>) {
    COMMITS.with(|t| t.borrow_mut().push(task));
    schedule_flush();
}

pub(crate) fn queue_task(task: Box<dyn FnOnce()>) {
    TASKS.with(|t| t.borrow_mut().push(task));
    schedule_flush();
}

/// Resolves after the next flush (renders, then queued tasks).
pub(crate) async fn after_flush() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        queue_task(Box::new(move || {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        }));
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

pub(super) fn schedule_host_update(host: Rc<MixinHost>) {
    DIRTY_HOSTS.with(|d| d.borrow_mut().push(host));
    schedule_flush();
}

fn schedule_flush() {
    if !SCHEDULED.replace(true) {
        FLUSH.with(|f| super::window().queue_microtask(f));
    }
}

/// Updates per event loop turn before the runtime warns, and per component before it
/// stops a loop (Remix's scheduler limits).
const CASCADING_UPDATE_WARN_THRESHOLD: u32 = 50;
const MAX_CASCADING_COMPONENT_UPDATES: u32 = 50;

/// Component renders in the current event loop turn, to catch `update()` loops.
#[derive(Default)]
struct Cascade {
    /// Which event loop turn this is; instance counts from older turns are stale.
    turn: u32,
    total: u32,
    /// Renders per component name, in first-seen order.
    per_name: Vec<(&'static str, u32)>,
    warned: bool,
    reset_scheduled: bool,
}

thread_local! {
    static CASCADE: RefCell<Cascade> = RefCell::new(Cascade::default());
    static RESET_CASCADE: js_sys::Function = Closure::<dyn FnMut()>::new(|| {
        CASCADE.with(|c| {
            let mut c = c.borrow_mut();
            *c = Cascade { turn: c.turn.wrapping_add(1), ..Cascade::default() };
        });
    })
    .into_js_value()
    .unchecked_into();
}

fn format_counts(counts: &[(&'static str, u32)]) -> String {
    counts.iter().map(|(name, n)| format!("{name} x{n}")).collect::<Vec<_>>().join(", ")
}

/// Count a render of `inst`. Returns an error message when it has looped too often.
fn track_cascading_update(inst: &dyn AnyInstance) -> Result<(), String> {
    CASCADE.with(|c| {
        let mut c = c.borrow_mut();
        c.total += 1;
        let name = inst.shared().name();
        match c.per_name.iter_mut().find(|(n, _)| *n == name) {
            Some((_, n)) => *n += 1,
            None => c.per_name.push((name, 1)),
        }
        let (turn, n) = inst.live().cascade.get();
        let count = if turn == c.turn { n + 1 } else { 1 };
        inst.live().cascade.set((c.turn, count));
        if !c.reset_scheduled {
            c.reset_scheduled = true;
            // Reset when control returns to the event loop; microtask flushes in the same
            // turn still count as cascading.
            RESET_CASCADE.with(|f| {
                let _ = super::window().set_timeout_with_callback_and_timeout_and_arguments_0(f, 0);
            });
        }
        if !c.warned && c.total >= CASCADING_UPDATE_WARN_THRESHOLD {
            c.warned = true;
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "{} cascading component updates detected in one event loop turn. Consider reducing hydration regions. Components: {}",
                c.total,
                format_counts(&c.per_name)
            )));
        }
        if count > MAX_CASCADING_COMPONENT_UPDATES {
            return Err(format!(
                "handle.update() infinite loop detected in {name} after {count} cascading updates. Components: {}",
                format_counts(&c.per_name)
            ));
        }
        Ok(())
    })
}

/// Render every component that called `update()`, parents before children, then run
/// queued tasks. Runs automatically on a microtask; call it to flush synchronously.
///
/// A component that keeps updating itself (for example from a task queued by every
/// render) is stopped after 50 renders in one event loop turn, and the loop is reported
/// as an error.
pub fn flush() {
    SCHEDULED.set(false);
    // Tasks may update components or queue more tasks; keep going until everything has
    // settled, as Remix's scheduler does. The update-loop check bounds component renders;
    // the pass cap bounds anything else that keeps rescheduling itself.
    for _ in 0..1000 {
        if !flush_pass() {
            return;
        }
        let idle = DIRTY.with(|d| d.borrow().is_empty())
            && DIRTY_HOSTS.with(|d| d.borrow().is_empty())
            && COMMITS.with(|t| t.borrow().is_empty())
            && TASKS.with(|t| t.borrow().is_empty());
        if idle {
            return;
        }
    }
}

/// Render dirty components and mixin hosts, then run commit callbacks and tasks. Returns
/// `false` when an update loop was stopped.
fn flush_pass() -> bool {
    // Keyed moves can blur the focused element; put focus and selection back before
    // commit callbacks run, as Remix's scheduler does.
    let focus = DocumentState::capture();
    // Mixin hosts are not tracked per instance; cap the passes so a host that updates
    // itself on every rerun cannot hang the page.
    for _ in 0..1000 {
        let mut batch = DIRTY.with(|d| std::mem::take(&mut *d.borrow_mut()));
        let hosts = DIRTY_HOSTS.with(|d| std::mem::take(&mut *d.borrow_mut()));
        if batch.is_empty() && hosts.is_empty() {
            break;
        }
        batch.sort_by_key(|i| i.live().depth);
        for (i, inst) in batch.iter().enumerate() {
            // A parent re-render already refreshed children it passed new props to.
            if !inst.live().dirty.get() {
                continue;
            }
            if let Err(message) = track_cascading_update(&**inst) {
                // Drop this batch and everything still pending, as Remix does.
                batch[i..].iter().for_each(|inst| inst.live().dirty.set(false));
                DIRTY.with(|d| d.borrow_mut().drain(..).for_each(|inst| inst.live().dirty.set(false)));
                crate::app::report_to(inst.shared().scope.errors.as_ref(), message);
                focus.restore();
                return false;
            }
            rerender(&**inst);
        }
        for host in hosts {
            host.rerun();
        }
    }
    focus.restore();
    let commits = COMMITS.with(|t| std::mem::take(&mut *t.borrow_mut()));
    for commit in commits {
        commit();
    }
    let tasks = TASKS.with(|t| std::mem::take(&mut *t.borrow_mut()));
    for task in tasks {
        task();
    }
    true
}

// ---------------------------------------------------------------------------------------
// Mounted tree
// ---------------------------------------------------------------------------------------

pub(crate) enum Mounted {
    Element(Box<MElement>),
    /// An explicit `<head>`: its children render into `document.head` (Remix's head
    /// support); the text node marks its position in the tree.
    Head(Box<MElement>, web_sys::Text),
    Text(MText),
    Component(MComponent),
    Frame(Box<MFrame>),
}

/// What has been applied to a DOM element: attributes, listeners and the `css()` rules
/// it holds a reference to.
#[derive(Default)]
pub(crate) struct Applied {
    pub(super) attrs: Vec<(&'static str, AttrValue)>,
    pub(super) listeners: Vec<Listener>,
    pub(super) styles: Vec<Rc<StyleRule>>,
}

impl Applied {
    /// Free the listener slots and style references. `el` is given when the element
    /// stays in the document, so its listeners are removed too.
    pub(super) fn release(&self, el: Option<&web_sys::Element>) {
        self.listeners.iter().for_each(|l| l.release(el));
        release_styles(&self.styles);
    }
}

pub(crate) struct MElement {
    tag: &'static str,
    key: Option<Key>,
    /// Empty when `host` owns the element's props.
    props: Applied,
    el: web_sys::Element,
    children: Vec<Mounted>,
    /// Stateful mixins bound to this element.
    host: Option<Rc<MixinHost>>,
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

/// Event listeners live on their host element, as in Remix (so an ancestor's
/// `stopPropagation` does not hide them, and capture listeners work). Every element shares
/// one JS function per event type and phase, and stores the id of its Rust handler in an
/// expando property, so binding costs one call into JS and a re-render only swaps the
/// handler in the Rust-side table.
pub(crate) struct Listener {
    name: &'static str,
    capture: bool,
    id: u32,
}

#[derive(Default)]
struct HandlerTable {
    items: Vec<Option<EventHandler>>,
    /// Bumped when a slot is freed. An element unmounted without unbinding keeps its
    /// listener and expando, so the expando carries the generation it was bound with and a
    /// late event there (a `toggle` after `hidePopover`) cannot reach the slot's next owner.
    generations: Vec<u32>,
    free: Vec<u32>,
    /// Signals handed out by `Event::signal`, aborted on the next dispatch or release.
    signals: HashMap<u32, web_sys::AbortController>,
}

/// The signal for the current run of a listener.
pub(crate) fn listener_signal(id: u32) -> web_sys::AbortSignal {
    HANDLERS.with(|h| {
        let mut table = h.borrow_mut();
        table.signals.entry(id).or_insert_with(|| web_sys::AbortController::new().expect("AbortController")).signal()
    })
}

fn abort_listener_signal(table: &mut HandlerTable, id: u32) {
    if let Some(c) = table.signals.remove(&id) {
        c.abort();
    }
}

/// The shared functions for one event type and phase. `bind` and `unbind` run with the
/// element as `this`: they set or delete the expando and add or remove the listener.
struct EventType {
    bind: js_sys::Function,
    unbind: js_sys::Function,
}

thread_local! {
    static HANDLERS: RefCell<HandlerTable> = RefCell::new(HandlerTable::default());
    static EVENT_TYPES: RefCell<HashMap<(&'static str, bool), EventType>> = RefCell::new(HashMap::new());
}

/// Call `f` with the shared functions for `name` in this phase, creating them on first use.
fn with_event_type<R>(name: &'static str, capture: bool, f: impl FnOnce(&EventType) -> R) -> R {
    EVENT_TYPES.with(|types| {
        if let Some(t) = types.borrow().get(&(name, capture)) {
            return f(t);
        }
        let key = JsValue::from_str(&format!("__rmx_{}{name}", if capture { "c_" } else { "" }));
        let dispatch_key = key.clone();
        // `Fn`: a handler may dispatch another event of the same type synchronously.
        let listener: js_sys::Function = Closure::<dyn Fn(web_sys::Event)>::new(move |raw: web_sys::Event| dispatch(&raw, &dispatch_key))
            .into_js_value()
            .unchecked_into();
        let make = js_sys::Function::new_with_args(
            "f,k,n,c",
            "return [function(id){this[k]=id;this.addEventListener(n,f,c)},function(){delete this[k];this.removeEventListener(n,f,c)}]",
        );
        let pair: js_sys::Array = make
            .apply(&JsValue::NULL, &js_sys::Array::of4(&listener, &key, &JsValue::from_str(name), &JsValue::from_bool(capture)))
            .expect("event binders")
            .unchecked_into();
        let t = EventType { bind: pair.get(0).unchecked_into(), unbind: pair.get(1).unchecked_into() };
        let out = f(&t);
        types.borrow_mut().insert((name, capture), t);
        out
    })
}

/// Expando values are `id + generation * 2^32`; generations wrap below 2^21 so the value
/// stays an exact integer in an `f64`.
const GENERATION_UNIT: f64 = 4_294_967_296.0;
const GENERATION_MASK: u32 = (1 << 21) - 1;

/// Run the handler of the element the event is at (`currentTarget`).
fn dispatch(raw: &web_sys::Event, key: &JsValue) {
    let Some(current) = raw.current_target() else { return };
    let Some(value) = js_sys::Reflect::get(&current, key).ok().and_then(|v| v.as_f64()) else { return };
    let (id, generation) = ((value % GENERATION_UNIT) as u32, (value / GENERATION_UNIT) as u32);
    let handler = HANDLERS.with(|h| {
        let mut table = h.borrow_mut();
        if table.generations.get(id as usize) != Some(&generation) {
            return None;
        }
        if !table.signals.is_empty() {
            abort_listener_signal(&mut table, id);
        }
        table.items.get(id as usize).cloned().flatten()
    });
    if let Some(handler) = handler {
        (handler.0)(&Event { raw: raw.clone(), current: current.dyn_into().ok(), listener: Some(id) });
    }
}

impl Listener {
    fn attach(el: &web_sys::Element, name: &'static str, handler: EventHandler) -> Listener {
        let capture = handler.1;
        let (id, generation) = HANDLERS.with(|h| {
            let mut table = h.borrow_mut();
            let id = match table.free.pop() {
                Some(id) => {
                    table.items[id as usize] = Some(handler);
                    id
                }
                None => {
                    table.items.push(Some(handler));
                    table.generations.push(0);
                    (table.items.len() - 1) as u32
                }
            };
            (id, table.generations[id as usize])
        });
        let value = f64::from(id) + f64::from(generation) * GENERATION_UNIT;
        with_event_type(name, capture, |t| {
            let _ = t.bind.call1(el, &JsValue::from(value));
        });
        Listener { name, capture, id }
    }

    fn replace(&self, handler: EventHandler) {
        HANDLERS.with(|h| h.borrow_mut().items[self.id as usize] = Some(handler));
    }

    /// Free the handler slot. `el` is given when the element stays in the document.
    pub(super) fn release(&self, el: Option<&web_sys::Element>) {
        HANDLERS.with(|h| {
            let mut table = h.borrow_mut();
            table.items[self.id as usize] = None;
            let generation = &mut table.generations[self.id as usize];
            *generation = (*generation + 1) & GENERATION_MASK;
            table.free.push(self.id);
            if !table.signals.is_empty() {
                abort_listener_signal(&mut table, self.id);
            }
        });
        if let Some(el) = el {
            with_event_type(self.name, self.capture, |t| {
                let _ = t.unbind.call0(el);
            });
        }
    }
}

/// Attach several handlers for the same event as one listener that calls them in order.
pub(super) fn attach_events(el: &web_sys::Element, events: Vec<(&'static str, EventHandler)>) -> Vec<Listener> {
    merge_events(events).into_iter().map(|(n, h)| Listener::attach(el, n, h)).collect()
}

fn merge_events(events: Vec<(&'static str, EventHandler)>) -> Vec<(&'static str, EventHandler)> {
    if events.len() < 2 {
        return events;
    }
    let mut merged: Vec<(&'static str, Vec<EventHandler>)> = Vec::new();
    for (name, handler) in events {
        match merged.iter_mut().find(|(n, list)| *n == name && list[0].1 == handler.1) {
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
                let capture = list[0].1;
                (name, EventHandler(Rc::new(move |e: &Event| list.iter().for_each(|h| (h.0)(e))), capture))
            }
        })
        .collect()
}

impl Mounted {
    fn key(&self) -> Option<&Key> {
        match self {
            Mounted::Element(e) | Mounted::Head(e, _) => e.key.as_ref(),
            Mounted::Component(c) => c.key.as_ref(),
            Mounted::Text(_) | Mounted::Frame(_) => None,
        }
    }

    pub(crate) fn first_dom(&self) -> web_sys::Node {
        match self {
            Mounted::Element(e) => e.el.clone().into(),
            Mounted::Head(_, marker) => marker.clone().into(),
            Mounted::Text(t) => t.node.clone().into(),
            Mounted::Component(c) => c.inst.live().root.borrow()[0].first_dom(),
            Mounted::Frame(f) => f.state.start().into(),
        }
    }

    fn last_dom(&self) -> web_sys::Node {
        match self {
            Mounted::Element(e) => e.el.clone().into(),
            Mounted::Head(_, marker) => marker.clone().into(),
            Mounted::Text(t) => t.node.clone().into(),
            Mounted::Component(c) => c.inst.live().root.borrow().last().expect("component has DOM").last_dom(),
            Mounted::Frame(f) => f.state.end().into(),
        }
    }

    fn for_each_dom(&self, f: &mut dyn FnMut(&web_sys::Node)) {
        match self {
            Mounted::Element(e) => f(e.el.unchecked_ref()),
            Mounted::Head(_, marker) => f(marker.unchecked_ref()),
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
            (Mounted::Head(..), Node::Element(n)) => n.tag == "head",
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
            e.props.release(None);
            if matches!(e.tag, "input" | "textarea" | "select") {
                untrack_controlled(&e.el);
            }
            if let Some(host) = &e.host {
                host.dispose();
            }
            e.children.iter().for_each(unmount);
        }
        Mounted::Head(e, _) => {
            // `document.head` stays: detach this tree's listeners and remove its children.
            let head: &web_sys::Node = e.el.unchecked_ref();
            e.props.release(Some(&e.el));
            if let Some(host) = &e.host {
                host.dispose();
            }
            for child in &e.children {
                unmount(child);
                child.for_each_dom(&mut |n| {
                    let _ = head.remove_child(n);
                });
            }
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

fn remove(parent: &web_sys::Node, m: Mounted) {
    let m = match m {
        Mounted::Element(e) if persists_element(&e) => {
            let host = e.host.clone().expect("persists_element checked the host");
            let token = NEXT_PERSIST_TOKEN.with(|t| {
                let n = t.get().wrapping_add(1);
                t.set(n);
                n
            });
            if host.persist_removal(move || finish_persisted(token)) {
                // The element stays in the document, still mounted, until its teardown
                // finishes or the same keyed element is rendered again and reclaims it.
                PERSISTED.with(|p| p.borrow_mut().push(Persisted { parent: parent.clone(), token, element: e }));
                return;
            }
            Mounted::Element(e)
        }
        // A removed component removes what it rendered one node at a time, so a host at
        // its top level can still defer its removal (Remix removes component content
        // before the component).
        Mounted::Component(c) if c.inst.live().root.borrow().iter().any(persists) => {
            let live = c.inst.live();
            let was_alive = live.alive.replace(false);
            let content = std::mem::take(&mut *live.root.borrow_mut());
            content.into_iter().for_each(|child| remove(parent, child));
            if was_alive {
                c.inst.shared().dispose();
            }
            return;
        }
        other => other,
    };
    unmount(&m);
    m.for_each_dom(&mut |n| {
        let _ = parent.remove_child(n);
    });
}

/// An element whose removal a mixin deferred with `persist_node` (Remix's persisted
/// mixin nodes). It is tracked by DOM parent so a keyed element rendered again under the
/// same parent before the teardown finishes reuses the node instead of creating a new one.
struct Persisted {
    parent: web_sys::Node,
    token: u32,
    element: Box<MElement>,
}

thread_local! {
    static PERSISTED: RefCell<Vec<Persisted>> = const { RefCell::new(Vec::new()) };
    static NEXT_PERSIST_TOKEN: Cell<u32> = const { Cell::new(0) };
}

/// The teardown settled: remove the element unless it was reclaimed meanwhile.
fn finish_persisted(token: u32) {
    let entry = PERSISTED.with(|p| {
        let mut p = p.borrow_mut();
        p.iter().position(|e| e.token == token).map(|i| p.swap_remove(i))
    });
    if let Some(entry) = entry {
        let el = entry.element.el.clone();
        unmount(&Mounted::Element(entry.element));
        if let Some(parent) = el.parent_node() {
            let _ = parent.remove_child(&el);
        }
    }
}

/// Whether `parent` still holds elements waiting for their exit teardown.
fn has_persisted(parent: &web_sys::Node) -> bool {
    PERSISTED.with(|p| p.borrow().iter().any(|e| e.parent.is_same_node(Some(parent))))
}

fn take_persisted(parent: &web_sys::Node, tag: &str, key: &Key) -> Option<Box<MElement>> {
    PERSISTED.with(|p| {
        let mut p = p.borrow_mut();
        let i = p.iter().position(|e| {
            e.element.tag == tag && e.element.key.as_ref() == Some(key) && e.parent.is_same_node(Some(parent))
        })?;
        Some(p.remove(i).element)
    })
}

/// Create a node that will be inserted into `parent`. A keyed element matching a persisted
/// one under the same parent reclaims it: the pending teardown is aborted, the node is
/// patched in place and its mixins get `reclaimed` instead of `insert`.
fn create_in(parent: &web_sys::Node, node: Node, scope: &Scope) -> Mounted {
    let reclaimed = match &node {
        Node::Element(el) => el.key.as_ref().and_then(|key| take_persisted(parent, el.tag, key)),
        _ => None,
    };
    match (reclaimed, node) {
        (Some(mut m), Node::Element(el)) => {
            if let Some(host) = &m.host {
                host.cancel_removal();
            }
            patch_element(&mut m, *el, scope);
            if let Some(host) = &m.host {
                host.queue_reclaimed();
            }
            Mounted::Element(m)
        }
        (_, node) => create(node, scope),
    }
}

/// Template strings (tags, attribute names, static text) are interned so repeated rows reuse
/// one JS string instead of re-decoding UTF-8 on every call; dynamic text is not.
fn interned(s: &Str) -> &str {
    match s {
        Cow::Borrowed(b) => intern(b),
        Cow::Owned(o) => o,
    }
}

/// Whether an element's content comes from `innerHTML={unsafe_html(..)}` instead of children.
pub(super) fn has_inner_html(attrs: &[(&'static str, AttrValue)]) -> bool {
    attrs.iter().any(|(n, v)| *n == "innerHTML" && matches!(v, AttrValue::Html(_)))
}

/// Whether removing this node may be deferred by a `beforeRemove` listener.
/// Whether removing `m` can be deferred: a host whose mixins listen for `beforeRemove`,
/// directly or at the top level of a component.
fn persists(m: &Mounted) -> bool {
    match m {
        Mounted::Element(e) => persists_element(e),
        Mounted::Component(c) => c.inst.live().root.borrow().iter().any(persists),
        _ => false,
    }
}

fn persists_element(e: &MElement) -> bool {
    e.host.as_ref().is_some_and(|h| h.listens_before_remove())
}

pub(super) fn set_attr(el: &web_sys::Element, tag: &str, name: &'static str, value: &AttrValue) {
    if name == "innerHTML" {
        match value {
            AttrValue::Html(html) => {
                if el.inner_html() != **html {
                    el.set_inner_html(html);
                }
            }
            AttrValue::Absent | AttrValue::Bool(false) => el.set_inner_html(""),
            _ => super::warn("rmx: innerHTML needs a value from rmx::unsafe_html()"),
        }
        return;
    }
    if name == "defaultValue" || name == "defaultChecked" {
        // Uncontrolled initial values are DOM properties (they reflect to `value`/`checked`).
        let v = match value {
            AttrValue::Str(v) | AttrValue::Html(v) => JsValue::from_str(v),
            AttrValue::Bool(b) => JsValue::from_bool(*b),
            AttrValue::Absent => JsValue::from_bool(false),
        };
        let v = if name == "defaultValue" && v.is_falsy() && v.as_string().is_none() { JsValue::from_str("") } else { v };
        let _ = js_sys::Reflect::set(el, &JsValue::from_str(name), &v);
        return;
    }
    if name == "value" || name == "checked" {
        track_controlled(el, name, value);
    }
    if !crate::attrs::is_allowed_name(name) {
        return;
    }
    if is_form_property(el, name) {
        // Live state, not markup: Remix sets these as properties, so the attribute (the
        // server-rendered default) is left alone.
        let v = match value {
            AttrValue::Str(v) | AttrValue::Html(v) => JsValue::from_str(v),
            AttrValue::Bool(b) => JsValue::from_bool(*b),
            AttrValue::Absent if name == "value" => JsValue::from_str(""),
            AttrValue::Absent => JsValue::FALSE,
        };
        let _ = js_sys::Reflect::set(el, &JsValue::from_str(name), &v);
        if matches!(value, AttrValue::Absent) {
            let _ = el.remove_attribute(name);
        }
        return;
    }
    if name.starts_with("xlink:") || name.starts_with("xml:") {
        return set_namespaced_attr(el, name, value);
    }
    if tag.contains('-') && set_custom_element_property(el, name, value) {
        return;
    }
    let name = intern(name);
    match value {
        AttrValue::Html(v) => {
            let _ = el.set_attribute(name, &crate::attrs::sanitize_url(tag, name, v));
        }
        AttrValue::Bool(b) if crate::attrs::is_booleanish_string(name) => {
            let _ = el.set_attribute(name, if *b { "true" } else { "false" });
        }
        AttrValue::Str(v) => {
            match name {
                "href" | "src" | "action" | "formaction" | "data" => {
                    let _ = el.set_attribute(name, &crate::attrs::sanitize_url(tag, name, v));
                }
                _ => {
                    let _ = el.set_attribute(name, interned(v));
                }
            }
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

/// Remix sets a prop the element defines (`name in element`) as a property. For built-in
/// elements the attribute reflects the same value, so rmx only does this for custom
/// elements, whose properties need not reflect. Returns whether the property was set.
fn set_custom_element_property(el: &web_sys::Element, name: &'static str, value: &AttrValue) -> bool {
    if name.contains('-') || matches!(name, "class" | "style" | "id") || crate::attrs::is_booleanish_string(name) {
        return false;
    }
    let key = JsValue::from_str(name);
    if !js_sys::Reflect::has(el, &key).unwrap_or(false) {
        return false;
    }
    let v = match value {
        AttrValue::Str(v) | AttrValue::Html(v) => JsValue::from_str(v),
        AttrValue::Bool(b) => JsValue::from_bool(*b),
        AttrValue::Absent => {
            let _ = el.remove_attribute(name);
            return true;
        }
    };
    js_sys::Reflect::set(el, &key, &v).is_ok()
}

/// `xlink:href`, `xml:lang` and friends go in their XML namespaces, as Remix sets them.
fn set_namespaced_attr(el: &web_sys::Element, name: &str, value: &AttrValue) {
    let (prefix, local) = name.split_once(':').expect("prefixed name");
    let ns = if prefix == "xlink" { "http://www.w3.org/1999/xlink" } else { "http://www.w3.org/XML/1998/namespace" };
    match value {
        AttrValue::Str(v) | AttrValue::Html(v) => {
            let _ = el.set_attribute_ns(Some(ns), name, &crate::attrs::sanitize_url("", name, v));
        }
        AttrValue::Bool(true) => {
            let _ = el.set_attribute_ns(Some(ns), name, "");
        }
        AttrValue::Bool(false) | AttrValue::Absent => {
            let _ = el.remove_attribute_ns(Some(ns), local);
        }
    }
}

/// `value` on text controls, `checked` on inputs and `selected` on options: state the
/// browser keeps as a property, separate from the attribute.
fn is_form_property(el: &web_sys::Element, name: &str) -> bool {
    match name {
        "value" => matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"),
        "checked" => el.tag_name() == "INPUT",
        "selected" => el.tag_name() == "OPTION",
        _ => false,
    }
}

const CONTROLLED_VALUE: &str = "__rmx_cv";
const CONTROLLED_CHECKED: &str = "__rmx_cc";

/// Remember a form control's controlled `value`/`checked` so user edits the app does not
/// accept (no `update()`, or an update that keeps the old value) are reverted.
fn track_controlled(el: &web_sys::Element, name: &str, value: &AttrValue) {
    if !matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") {
        return;
    }
    let key = JsValue::from_str(if name == "value" { CONTROLLED_VALUE } else { CONTROLLED_CHECKED });
    let tracked = match (name, value) {
        ("value", AttrValue::Str(v) | AttrValue::Html(v)) => Some(JsValue::from_str(v)),
        ("checked", AttrValue::Bool(b)) => Some(JsValue::from_bool(*b)),
        _ => None,
    };
    match tracked {
        Some(v) => {
            let value_key = JsValue::from_str(CONTROLLED_VALUE);
            let checked_key = JsValue::from_str(CONTROLLED_CHECKED);
            let first = !js_sys::Reflect::has(el, &value_key).unwrap_or(false) && !js_sys::Reflect::has(el, &checked_key).unwrap_or(false);
            let _ = js_sys::Reflect::set(el, &key, &v);
            if first {
                // Like Remix, listen after the commit, so the element's own handlers run first
                // and a render they schedule settles the value before the restore.
                let el = el.clone();
                queue_commit(Box::new(move || {
                    RESTORE.with(|f| {
                        let _ = el.add_event_listener_with_callback("input", f);
                        let _ = el.add_event_listener_with_callback("change", f);
                    });
                }));
            }
        }
        None => {
            let _ = js_sys::Reflect::delete_property(el.unchecked_ref::<js_sys::Object>(), &key);
        }
    }
}

/// Stop restoring a removed control's value (Remix's `teardownControlledReflection`).
fn untrack_controlled(el: &web_sys::Element) {
    for key in [CONTROLLED_VALUE, CONTROLLED_CHECKED] {
        let _ = js_sys::Reflect::delete_property(el.unchecked_ref::<js_sys::Object>(), &JsValue::from_str(key));
    }
    RESTORE.with(|f| {
        let _ = el.remove_event_listener_with_callback("input", f);
        let _ = el.remove_event_listener_with_callback("change", f);
    });
}

thread_local! {
    /// The `input`/`change` listener of controlled form controls.
    static RESTORE: js_sys::Function = Closure::<dyn Fn(web_sys::Event)>::new(|raw: web_sys::Event| restore_controlled(&raw))
        .into_js_value()
        .unchecked_into();
}

/// After an `input`/`change` event, put a controlled control back to its rendered value
/// unless a render (which runs first, on an earlier microtask) changed it.
fn restore_controlled(raw: &web_sys::Event) {
    let Some(target) = raw.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
    let value_key = JsValue::from_str(CONTROLLED_VALUE);
    let checked_key = JsValue::from_str(CONTROLLED_CHECKED);
    let has_value = js_sys::Reflect::has(&target, &value_key).unwrap_or(false);
    let has_checked = js_sys::Reflect::has(&target, &checked_key).unwrap_or(false);
    if !has_value && !has_checked {
        return;
    }
    // Checkboxes and selects settle on `change`; restoring on `input` would race it.
    if raw.type_() == "input" && (has_checked || target.tag_name() == "SELECT") {
        return;
    }
    let restore = Closure::once_into_js(move || {
        for (key, prop) in [(value_key, "value"), (checked_key, "checked")] {
            let Ok(want) = js_sys::Reflect::get(&target, &key) else { continue };
            if want.is_undefined() {
                continue;
            }
            let prop = JsValue::from_str(prop);
            if js_sys::Reflect::get(&target, &prop).ok().as_ref() != Some(&want) {
                let _ = js_sys::Reflect::set(&target, &prop, &want);
            }
        }
    });
    super::window().queue_microtask(restore.unchecked_ref());
}

/// A `<select>`'s `value` only sticks once its options exist: apply it after children.
fn sync_select_value(el: &web_sys::Element) {
    if el.tag_name() == "SELECT" {
        if let Ok(v) = js_sys::Reflect::get(el, &JsValue::from_str(CONTROLLED_VALUE)) {
            if v.is_string() {
                let _ = js_sys::Reflect::set(el, &JsValue::from_str("value"), &v);
            }
        }
    }
}

/// Point an element's `css()` rules at `extra`'s: references to new rules are taken
/// before old ones are dropped, so a rule kept across the update is never deleted.
pub(super) fn apply_styles(extra: Option<Box<ElementExtra>>, applied: &mut Vec<Rc<StyleRule>>) {
    let next = extra.map(|x| x.styles).unwrap_or_default();
    if next.len() == applied.len() && next.iter().zip(applied.iter()).all(|(a, b)| a.class == b.class) {
        return;
    }
    next.iter().for_each(|r| styles::retain(r));
    release_styles(applied);
    *applied = next;
}

pub(super) fn release_styles(applied: &[Rc<StyleRule>]) {
    applied.iter().for_each(|r| styles::release(&r.class));
}

// ---------------------------------------------------------------------------------------
// Create and patch
// ---------------------------------------------------------------------------------------

/// Build detached DOM for a node. The caller inserts it.
fn create(node: Node, scope: &Scope) -> Mounted {
    match node {
        Node::Element(mut el) => {
            let tag = el.tag;
            let head = if tag == "head" { document().head() } else { None };
            let child_scope = element_scope(scope, tag);
            let scope = &*child_scope;
            let dom = if let Some(head) = &head {
                head.clone().into()
            } else if in_svg(scope, tag) {
                document().create_element_ns(Some("http://www.w3.org/2000/svg"), intern(tag)).expect("createElementNS")
            } else {
                document().create_element(intern(tag)).expect("createElement")
            };
            let children = std::mem::take(&mut el.children);
            let (key, props, host, inner_html) = if has_mixins(&el) {
                let key = el.key.clone();
                let host = MixinHost::new(dom.clone(), scope, tag, key.clone());
                let inner_html = host.apply(*el, true);
                (key, Applied::default(), Some(host), inner_html)
            } else {
                let Element { key, attrs, events, extra, .. } = *el;
                for (name, value) in &attrs {
                    set_attr(&dom, tag, name, value);
                }
                let mut styles = Vec::new();
                apply_styles(extra, &mut styles);
                let listeners = attach_events(&dom, events);
                let inner_html = has_inner_html(&attrs);
                (key, Applied { attrs, listeners, styles }, None, inner_html)
            };
            let children = if inner_html { Vec::new() } else { children };
            let mut kids = Vec::with_capacity(children.len());
            let children = flatten_children(children);
            warn_duplicate_keys(&children);
            for child in children {
                let m = create(child, scope);
                m.for_each_dom(&mut |n| {
                    dom.append_child(n).expect("appendChild");
                });
                kids.push(m);
            }
            if tag == "select" {
                sync_select_value(&dom);
            }
            let element = Box::new(MElement { tag, key, props, el: dom, children: kids, host });
            match head {
                Some(_) => Mounted::Head(element, document().create_text_node("")),
                None => Mounted::Element(element),
            }
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
            inst.shared().committed.set(true);
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

/// Whether an element is created in the SVG namespace: inside `<svg>`, or a tag that only
/// exists in SVG (for components that render SVG parts without an `<svg>` ancestor).
/// `scope` is the scope for the element's children ([`element_scope`]).
fn in_svg(scope: &Scope, tag: &str) -> bool {
    scope.svg || tag == "foreignObject" || is_svg(tag)
}

/// The scope for an element's children: SVG inside `<svg>`, HTML again inside
/// `<foreignObject>`.
fn element_scope<'a>(scope: &'a Scope, tag: &str) -> Cow<'a, Scope> {
    let svg = tag != "foreignObject" && (tag == "svg" || scope.svg || is_svg(tag));
    if svg == scope.svg {
        Cow::Borrowed(scope)
    } else {
        Cow::Owned(Scope { svg, ..scope.clone() })
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
    state.set_owner(scope.parent.clone());
    Mounted::Frame(Box::new(MFrame { src: f.src.clone(), name: f.name.clone(), state }))
}

fn patch(parent: &web_sys::Node, old: &mut Mounted, new: Node, scope: &Scope) {
    match (&mut *old, new) {
        (Mounted::Element(m), Node::Element(n)) if m.tag == n.tag => patch_element(m, *n, scope),
        (Mounted::Head(m, _), Node::Element(n)) if n.tag == "head" => patch_element(m, *n, scope),
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
                frames::spawn_reload(m.state.clone(), f.fallback.is_none());
            } else if frames::in_server_reload() {
                // Re-rendered by a frame load (a client entry got new server props, or node
                // content was replaced): reload along with the ancestor, like Remix.
                frames::spawn_reload(m.state.clone(), f.fallback.is_none());
            }
        }
        (_, new) => {
            let replacement = create_in(parent, new, scope);
            insert(parent, &replacement, Some(&old.first_dom()));
            let previous = std::mem::replace(old, replacement);
            remove(parent, previous);
        }
    }
}

fn patch_element(m: &mut MElement, mut n: Element, scope: &Scope) {
    let children = std::mem::take(&mut n.children);
    m.key = n.key.clone();
    if m.host.is_none() && has_mixins(&n) {
        // Stateful mixins added to a mounted element: the host takes over its props.
        let host = MixinHost::new(m.el.clone(), scope, m.tag, m.key.clone());
        host.adopt(std::mem::take(&mut m.props));
        m.host = Some(host);
    }
    let inner_html = match &m.host {
        Some(host) => {
            host.set_key(m.key.clone());
            host.apply(n, false)
        }
        None => {
            let Element { attrs, events, extra, .. } = n;
            patch_props(&m.el, m.tag, &mut m.props, attrs, events, extra);
            has_inner_html(&m.props.attrs)
        }
    };

    if inner_html {
        m.children.iter().for_each(unmount);
        m.children.clear();
    } else if !children.is_empty() || !m.children.is_empty() {
        let parent: web_sys::Node = m.el.clone().into();
        // `document.head` holds more than this tree, so never clear it wholesale.
        let whole = m.tag != "head";
        let scope = element_scope(scope, m.tag);
        patch_children(&parent, &mut m.children, flatten_children(children), None, whole, &scope);
    }
    if m.tag == "select" {
        sync_select_value(&m.el);
    }
}

/// Patch an element's attributes, `css()` rules and event handlers.
pub(super) fn patch_props(
    el: &web_sys::Element,
    tag: &str,
    applied: &mut Applied,
    attrs: Vec<(&'static str, AttrValue)>,
    events: Vec<(&'static str, EventHandler)>,
    extra: Option<Box<ElementExtra>>,
) {
    let Applied { attrs: old_attrs, listeners, styles } = applied;
    if *old_attrs != attrs {
        for (name, value) in &attrs {
            let unchanged = old_attrs.iter().any(|(n, v)| n == name && v == value);
            if !unchanged {
                set_attr(el, tag, name, value);
            }
        }
        for (name, _) in old_attrs.iter() {
            if !attrs.iter().any(|(n, _)| n == name) {
                set_attr(el, tag, name, &AttrValue::Absent);
            }
        }
        *old_attrs = attrs;
    }
    apply_styles(extra, styles);

    let events = merge_events(events);
    let same_events = listeners.len() == events.len()
        && listeners.iter().zip(&events).all(|(l, (name, h))| l.name == *name && l.capture == h.1);
    if same_events {
        for (listener, (_, handler)) in listeners.iter().zip(events) {
            listener.replace(handler);
        }
    } else {
        for listener in listeners.drain(..) {
            listener.release(Some(el));
        }
        *listeners = events.into_iter().map(|(n, h)| Listener::attach(el, n, h)).collect();
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
        if whole && !old.iter().any(persists) && !has_persisted(parent) {
            old.drain(..).for_each(|m| unmount(&m));
            parent.set_text_content(None);
        } else {
            old.drain(..).for_each(|m| remove(parent, m));
        }
        return;
    }
    if old.is_empty() {
        warn_duplicate_keys(&new);
        for node in new {
            let m = create_in(parent, node, scope);
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
        warn_duplicate_keys(&new);
        patch_keyed(parent, old, new, end, whole, scope);
    }
}

/// Warn when siblings share a key (the last one wins), as Remix does.
fn warn_duplicate_keys(nodes: &[Node]) {
    if nodes.len() < 2 || nodes.iter().all(|n| n.key().is_none()) {
        return;
    }
    // Sort references rather than hash clones: no allocation for string keys.
    let mut keys: Vec<&Key> = nodes.iter().filter_map(Node::key).collect();
    keys.sort_unstable();
    let mut duplicates: Vec<&Key> = keys.windows(2).filter(|w| w[0] == w[1]).map(|w| w[0]).collect();
    if duplicates.is_empty() {
        return;
    }
    duplicates.dedup();
    let quoted: Vec<String> = duplicates
        .iter()
        .map(|k| match k {
            Key::Int(i) => format!("\"{i}\""),
            Key::Str(s) => format!("\"{s}\""),
        })
        .collect();
    super::warn(&format!("Duplicate keys detected in siblings: {}. Keys should be unique.", quoted.join(", ")));
}

fn patch_indexed(parent: &web_sys::Node, old: &mut Vec<Mounted>, new: Vec<Node>, end: Option<&web_sys::Node>, scope: &Scope) {
    let new_len = new.len();
    for (i, node) in new.into_iter().enumerate() {
        if i < old.len() {
            patch(parent, &mut old[i], node, scope);
        } else {
            let m = create_in(parent, node, scope);
            insert(parent, &m, end);
            old.push(m);
        }
    }
    if old.len() > new_len {
        old.drain(new_len..).for_each(|m| remove(parent, m));
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
    if !any_reused && whole && !slots.iter().flatten().any(persists) && !has_persisted(parent) {
        slots.into_iter().flatten().for_each(|m| unmount(&m));
        parent.set_text_content(None);
    } else {
        slots.into_iter().flatten().for_each(|m| remove(parent, m));
    }

    let mut result = Vec::with_capacity(matched.len());
    let mut sources = Vec::with_capacity(matched.len());
    for (existing, node, source) in matched {
        let m = match existing {
            Some(mut m) => {
                patch(parent, &mut m, node, scope);
                m
            }
            None => create_in(parent, node, scope),
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

/// Render an instance and adopt the server DOM starting at `cursor`, up to `end` (a range
/// root's end marker; `None` for the end of `parent`).
pub(crate) fn hydrate_instance(inst: &dyn AnyInstance, parent: &web_sys::Node, cursor: &mut Option<web_sys::Node>, end: Option<&web_sys::Node>) {
    let scope = &inst.live().child_scope;
    let root = rendered_list(inst).into_iter().map(|n| hydrate_node(n, parent, cursor, end, scope)).collect();
    *inst.live().root.borrow_mut() = root;
    inst.shared().committed.set(true);
}

/// The node an element or text can claim at `cursor`: comments are skipped (they are
/// markers, not content), except a frame's start; nothing at or past `end`.
fn claimable(cursor: Option<web_sys::Node>, end: Option<&web_sys::Node>) -> Option<web_sys::Node> {
    let mut node = cursor;
    while let Some(n) = node {
        if end.is_some_and(|e| e.is_same_node(Some(&n))) {
            return None;
        }
        match n.dyn_ref::<web_sys::Comment>() {
            Some(c) if !c.data().trim().starts_with("rmx:f:") => node = n.next_sibling(),
            _ => return Some(n),
        }
    }
    None
}

/// Server DOM whose tag is `tag`, as hydration compares it (SVG tags are case-sensitive).
fn same_tag(node: &web_sys::Node, tag: &str) -> Option<web_sys::Element> {
    let el = node.dyn_ref::<web_sys::Element>()?;
    let matches = if el.namespace_uri().as_deref() == Some("http://www.w3.org/2000/svg") {
        el.tag_name() == tag
    } else {
        el.tag_name().eq_ignore_ascii_case(tag)
    };
    matches.then(|| el.clone())
}

fn log_mismatch(kind: &str, server: &str, client: &str) {
    web_sys::console::error_4(&"Hydration mismatch:".into(), &kind.into(), &server.into(), &client.into());
}

/// Adopt the server DOM at `cursor` for `node`, or create and insert DOM on a mismatch.
/// Like Remix, nodes hydration does not claim are left in place (browser extensions inject
/// content), and a tag mismatch skips one node before giving up.
fn hydrate_node(node: Node, parent: &web_sys::Node, cursor: &mut Option<web_sys::Node>, end: Option<&web_sys::Node>, scope: &Scope) -> Mounted {
    match node {
        // A `<head>` renders into `document.head`; there is nothing in place to adopt.
        Node::Element(el) if el.tag != "head" => {
            let at = claimable(cursor.clone(), end);
            let mut adopt = at.as_ref().and_then(|n| same_tag(n, el.tag));
            if adopt.is_none() {
                if let Some(at) = at.as_ref().filter(|n| n.node_type() == web_sys::Node::ELEMENT_NODE) {
                    // An injected element in front of ours: try the next one, leaving it in place.
                    adopt = claimable(at.next_sibling(), end).and_then(|n| same_tag(&n, el.tag));
                    if adopt.is_none() {
                        log_mismatch("tag", &at.unchecked_ref::<web_sys::Element>().tag_name().to_ascii_lowercase(), el.tag);
                    }
                }
            }
            let Some(dom) = adopt else {
                *cursor = at;
                return create_at(Node::Element(el), parent, &cursor.clone().or_else(|| end.cloned()), scope);
            };
            *cursor = dom.next_sibling();
            let mut el = el;
            let tag = el.tag;
            let children = std::mem::take(&mut el.children);
            let (key, props, host, inner_html) = if has_mixins(&el) {
                let key = el.key.clone();
                let host = MixinHost::new(dom.clone(), scope, tag, key.clone());
                let inner_html = host.hydrate(*el);
                (key, Applied::default(), Some(host), inner_html)
            } else {
                let Element { key, attrs, events, extra, .. } = *el;
                hydrate_attrs(&dom, tag, &attrs);
                let mut styles = Vec::new();
                apply_styles(extra, &mut styles);
                let listeners = attach_events(&dom, events);
                let inner_html = has_inner_html(&attrs);
                (key, Applied { attrs, listeners, styles }, None, inner_html)
            };
            let dom_node: web_sys::Node = dom.clone().into();
            let children = if inner_html { Vec::new() } else { children };
            let mut child_cursor = if inner_html { None } else { dom.first_child() };
            let child_scope = element_scope(scope, tag);
            let kids = flatten_children(children)
                .into_iter()
                .map(|c| hydrate_node(c, &dom_node, &mut child_cursor, None, &child_scope))
                .collect();
            if tag == "select" {
                sync_select_value(&dom);
            }
            Mounted::Element(Box::new(MElement { tag, key, props, el: dom, children: kids, host }))
        }
        Node::Text(text) if !text.is_empty() => {
            let at = claimable(cursor.clone(), end);
            let Some(dom) = at.as_ref().and_then(|n| n.dyn_ref::<web_sys::Text>()).cloned() else {
                *cursor = at;
                return create_at(Node::Text(text), parent, &cursor.clone().or_else(|| end.cloned()), scope);
            };
            let data = dom.data();
            *cursor = dom.next_sibling();
            if data != *text {
                if data.starts_with(&*text) {
                    // Adjacent text children arrive merged into one DOM text node; split them.
                    let len = text.encode_utf16().count() as u32;
                    *cursor = Some(dom.split_text(len).expect("splitText").into());
                } else if let Some(rest) = text.strip_prefix(data.as_str()) {
                    // The parser split a long text into several DOM text nodes; merge them.
                    let mut rest = rest.to_string();
                    let mut sibling = dom.next_sibling();
                    while let Some(t) = sibling.as_ref().and_then(|s| s.dyn_ref::<web_sys::Text>()).cloned() {
                        let chunk = t.data();
                        if rest.is_empty() || !rest.starts_with(chunk.as_str()) {
                            break;
                        }
                        rest.drain(..chunk.len());
                        sibling = t.next_sibling();
                        t.remove();
                    }
                    if !rest.is_empty() {
                        // A chunk can also hold the start of the next text child.
                        if let Some(t) = sibling.as_ref().and_then(|s| s.dyn_ref::<web_sys::Text>()).filter(|t| t.data().starts_with(rest.as_str())) {
                            let _ = t.delete_data(0, rest.encode_utf16().count() as u32);
                        }
                    }
                    dom.set_data(&text);
                    *cursor = sibling;
                } else {
                    log_mismatch("text mismatch", &data, &text);
                    dom.set_data(&text);
                }
            }
            Mounted::Text(MText { text, node: dom })
        }
        Node::Component(c) => {
            let inst = c.inner.instantiate(scope);
            hydrate_instance(&*inst, parent, cursor, end);
            Mounted::Component(MComponent { key: c.key, inst })
        }
        Node::Frame(f) => {
            let start = claimable(cursor.clone(), end)
                .and_then(|n| n.dyn_into::<web_sys::Comment>().ok())
                .filter(|c| c.data().trim().starts_with("rmx:f:"));
            let state = start.and_then(|s| frames::adopt_server_frame(&s, scope.frame.clone()));
            let Some(state) = state else {
                return create_at(Node::Frame(f), parent, &cursor.clone().or_else(|| end.cloned()), scope);
            };
            *cursor = state.end().next_sibling();
            state.set_owner(scope.parent.clone());
            // The region's own client entries and frames hydrate with it, as when the
            // region is in the page outside any entry.
            frames::hydrate_region(&state);
            Mounted::Frame(Box::new(MFrame { src: f.src.clone(), name: f.name.clone(), state }))
        }
        other => create_at(other, parent, &cursor.clone().or_else(|| end.cloned()), scope),
    }
}

/// Server and client attributes can differ (ids from h.id(), dynamic values): fix the
/// ones that do.
pub(super) fn hydrate_attrs(dom: &web_sys::Element, tag: &str, attrs: &[(&'static str, AttrValue)]) {
    for (name, value) in attrs {
        if *name == "value" || *name == "checked" {
            track_controlled(dom, name, value);
        }
        if *name == "defaultValue" || *name == "defaultChecked" {
            // The server wrote these as `value`/`checked`; the DOM already matches.
            continue;
        }
        let current = dom.get_attribute(name);
        let matches = match value {
            AttrValue::Html(v) if *name == "innerHTML" => dom.inner_html() == **v,
            AttrValue::Str(v) | AttrValue::Html(v) => current.as_deref() == Some(&**v),
            AttrValue::Bool(true) => current.is_some(),
            AttrValue::Bool(false) | AttrValue::Absent => current.is_none(),
        };
        if !matches {
            set_attr(dom, tag, name, value);
        }
    }
}

fn create_at(node: Node, parent: &web_sys::Node, cursor: &Option<web_sys::Node>, scope: &Scope) -> Mounted {
    let m = create(node, scope);
    insert(parent, &m, cursor.as_ref());
    m
}

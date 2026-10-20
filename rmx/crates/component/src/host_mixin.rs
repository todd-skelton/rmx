//! Stateful mixins (Remix's `createMixin`).
//!
//! A [`HostMixin`] is to one element what a [`Component`](crate::Component) is to a
//! subtree. `setup` runs once per mixin slot on a host element and returns plain state.
//! `render` runs on every render of the host and patches it (attributes, events, nested
//! mixins). [`MixinHandle::update`] re-runs the host's mixins without re-rendering the
//! component that owns the element.
//!
//! ```ignore
//! struct ClickCount { count: u32 }
//!
//! impl HostMixin for ClickCount {
//!     type Args = &'static str;
//!
//!     fn setup(_: &MixinHandle<Self>, _tag: &'static str) -> Self {
//!         ClickCount { count: 0 }
//!     }
//!
//!     fn render(&mut self, label: &&'static str, host: &mut Element, h: &MixinHandle<Self>) {
//!         host.set_attr("aria-label", format!("{label}: {} clicks", self.count));
//!         host.mix(on("click", h.on(|this, h| { this.count += 1; h.update(); })));
//!     }
//! }
//!
//! fn click_count(label: &'static str) -> Mixin { mixin::<ClickCount>(label) }
//! // rsx! { <button mix={click_count("Save")}>"Save"</button> }
//! ```
//!
//! Lifecycle listeners are registered in `setup` and only fire in the browser:
//! [`on_insert`](MixinHandle::on_insert), [`on_remove`](MixinHandle::on_remove),
//! [`on_before_update`](MixinHandle::on_before_update),
//! [`on_commit`](MixinHandle::on_commit) and
//! [`on_before_remove`](MixinHandle::on_before_remove), whose event can keep the element in
//! the document until an exit animation finishes.

// The lifecycle plumbing is only driven by the browser runtime.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::frame::FrameHandle;
use crate::mixin::{Mixin, NodeHandle};
use crate::node::{Element, Event, EventHandler, Key};

/// A mixin with its own state and DOM lifecycle.
pub trait HostMixin: Sized + 'static {
    /// The arguments passed to the mixin's factory, handed to every `render`.
    type Args: 'static;

    /// `false` makes this a browser-only mixin (Remix's `{ ssr: false }`): the server skips
    /// its setup and render.
    const SSR: bool = true;

    /// Runs once per slot. `tag` is the host element's tag name.
    fn setup(h: &MixinHandle<Self>, tag: &'static str) -> Self;

    /// Runs on every render of the host, after the mixins before it in `mix`. Patch the
    /// host's attributes and events or add mixins with [`Element::mix`]; the host's children
    /// belong to its owner and are not visible here. It takes `&mut self` so a mixin can
    /// keep its latest arguments (Remix mixins assign them to setup variables).
    fn render(&mut self, _args: &Self::Args, _host: &mut Element, _h: &MixinHandle<Self>) {}
}

/// A `mix={...}` value for a [`HostMixin`] with these arguments.
pub fn mixin<M: HostMixin>(args: M::Args) -> Mixin {
    Mixin::stateful(Rc::new(Descriptor::<M>(args)))
}

/// The element a lifecycle event concerns.
pub struct InsertEvent {
    /// The host element.
    pub node: NodeHandle,
    /// The host's key, when it has one.
    pub key: Option<Key>,
}

/// Dispatched when removal of the host begins. Call [`BeforeRemoveEvent::persist_node`]
/// from the listener to keep the element in the document until teardown work finishes.
#[derive(Default)]
pub struct BeforeRemoveEvent {
    #[cfg(target_arch = "wasm32")]
    pub(crate) teardowns: RefCell<Vec<Teardown>>,
}

#[cfg(target_arch = "wasm32")]
pub(crate) type Teardown = Box<dyn FnOnce(web_sys::AbortSignal) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>>;

impl BeforeRemoveEvent {
    /// Keep the host in the document until `teardown` completes. The signal aborts if the
    /// pending removal is cancelled.
    #[cfg(target_arch = "wasm32")]
    pub fn persist_node<F, Fut>(&self, teardown: F)
    where
        F: FnOnce(web_sys::AbortSignal) -> Fut + 'static,
        Fut: std::future::Future<Output = ()> + 'static,
    {
        self.teardowns.borrow_mut().push(Box::new(move |signal| Box::pin(teardown(signal))));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lifecycle {
    Insert,
    Remove,
    BeforeUpdate,
    Commit,
    BeforeRemove,
}

pub(crate) enum LifecycleEvent<'a> {
    Insert(&'a InsertEvent),
    Remove,
    BeforeUpdate(&'a NodeHandle),
    Commit(&'a NodeHandle),
    BeforeRemove(&'a BeforeRemoveEvent),
}

impl LifecycleEvent<'_> {
    fn kind(&self) -> Lifecycle {
        match self {
            LifecycleEvent::Insert(_) => Lifecycle::Insert,
            LifecycleEvent::Remove => Lifecycle::Remove,
            LifecycleEvent::BeforeUpdate(_) => Lifecycle::BeforeUpdate,
            LifecycleEvent::Commit(_) => Lifecycle::Commit,
            LifecycleEvent::BeforeRemove(_) => Lifecycle::BeforeRemove,
        }
    }
}

type Listener<M> = Rc<dyn Fn(&mut M, &MixinHandle<M>, &LifecycleEvent<'_>)>;

/// What a new slot needs from its host.
pub(crate) struct SlotContext {
    pub(crate) tag: &'static str,
    pub(crate) id: Rc<str>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) host: Weak<crate::dom::MixinHost>,
}

/// A mixin descriptor with its type erased.
pub(crate) trait ErasedMixin {
    fn mixin_type(&self) -> TypeId;
    fn ssr(&self) -> bool;
    fn setup(&self, ctx: &SlotContext) -> Rc<dyn AnySlot>;
    fn render(&self, slot: &dyn AnySlot, host: &mut Element);
}

/// A live mixin slot with its type erased.
pub(crate) trait AnySlot {
    fn as_any(&self) -> &dyn Any;
    fn dispatch(&self, event: &LifecycleEvent<'_>);
    fn listens(&self, kind: Lifecycle) -> bool;
    fn is_disposed(&self) -> bool;
    /// The slot received its `insert` event.
    fn bind(&self);
    fn is_bound(&self) -> bool;
    /// Abort the slot's signal and dispatch `remove`.
    fn dispose(&self);
}

struct Descriptor<M: HostMixin>(M::Args);

impl<M: HostMixin> ErasedMixin for Descriptor<M> {
    fn mixin_type(&self) -> TypeId {
        TypeId::of::<M>()
    }

    fn ssr(&self) -> bool {
        M::SSR
    }

    fn setup(&self, ctx: &SlotContext) -> Rc<dyn AnySlot> {
        Slot::<M>::create(ctx)
    }

    fn render(&self, slot: &dyn AnySlot, host: &mut Element) {
        let slot = slot.as_any().downcast_ref::<Slot<M>>().expect("mixin slot type checked before render");
        let mut state = slot.state.borrow_mut();
        state.render(&self.0, host, &slot.handle);
    }
}

struct Slot<M: HostMixin> {
    state: RefCell<M>,
    handle: MixinHandle<M>,
}

impl<M: HostMixin> Slot<M> {
    fn create(ctx: &SlotContext) -> Rc<dyn AnySlot> {
        let shared = Rc::new(SlotShared {
            id: ctx.id.clone(),
            #[cfg(target_arch = "wasm32")]
            host: ctx.host.clone(),
            #[cfg(target_arch = "wasm32")]
            controller: RefCell::new(None),
            disposed: Cell::new(false),
            bound: Cell::new(false),
            listeners: RefCell::new(Vec::new()),
        });
        let slot: Rc<Slot<M>> = Rc::new_cyclic(|weak| {
            let handle = MixinHandle { slot: weak.clone(), shared };
            let state = M::setup(&handle, ctx.tag);
            Slot { state: RefCell::new(state), handle }
        });
        slot
    }
}

impl<M: HostMixin> AnySlot for Slot<M> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn dispatch(&self, event: &LifecycleEvent<'_>) {
        let kind = event.kind();
        let listeners: Vec<Listener<M>> =
            self.handle.shared.listeners.borrow().iter().filter(|(k, _)| *k == kind).map(|(_, f)| f.clone()).collect();
        for f in listeners {
            // A listener that fires while the state is borrowed (from inside render) is skipped.
            if let Ok(mut state) = self.state.try_borrow_mut() {
                f(&mut state, &self.handle, event);
            }
        }
    }

    fn listens(&self, kind: Lifecycle) -> bool {
        self.handle.shared.listeners.borrow().iter().any(|(k, _)| *k == kind)
    }

    fn is_disposed(&self) -> bool {
        self.handle.shared.disposed.get()
    }

    fn bind(&self) {
        self.handle.shared.bound.set(true);
    }

    fn is_bound(&self) -> bool {
        self.handle.shared.bound.get()
    }

    fn dispose(&self) {
        if self.handle.shared.disposed.replace(true) {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(c) = self.handle.shared.controller.borrow().as_ref() {
            c.abort();
        }
        self.dispatch(&LifecycleEvent::Remove);
        self.handle.shared.listeners.borrow_mut().clear();
    }
}

struct SlotShared<M: HostMixin> {
    id: Rc<str>,
    #[cfg(target_arch = "wasm32")]
    host: Weak<crate::dom::MixinHost>,
    #[cfg(target_arch = "wasm32")]
    controller: RefCell<Option<web_sys::AbortController>>,
    disposed: Cell<bool>,
    bound: Cell<bool>,
    listeners: RefCell<Vec<(Lifecycle, Listener<M>)>>,
}

/// A mixin slot's connection to the runtime (Remix's `MixinHandle`).
pub struct MixinHandle<M: HostMixin> {
    slot: Weak<Slot<M>>,
    shared: Rc<SlotShared<M>>,
}

impl<M: HostMixin> Clone for MixinHandle<M> {
    fn clone(&self) -> Self {
        MixinHandle { slot: self.slot.clone(), shared: self.shared.clone() }
    }
}

impl<M: HostMixin> MixinHandle<M> {
    /// An id shared by every mixin on the same host, usable as an HTML id.
    pub fn id(&self) -> &str {
        &self.shared.id
    }

    /// Re-run the host's mixins (not its owner component) on the next flush. No-op on the
    /// server.
    pub fn update(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(host) = self.shared.host.upgrade() {
            host.schedule_update();
        }
    }

    /// Schedule an update and resolve once it has been applied. Resolves at once on the
    /// server.
    pub async fn updated(&self) {
        self.update();
        #[cfg(target_arch = "wasm32")]
        crate::dom::after_flush().await;
    }

    /// Whether the slot is still bound to a mounted host.
    pub fn is_connected(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            !self.shared.disposed.get() && self.shared.host.upgrade().is_some_and(|h| h.is_alive())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// An event handler with mutable access to the mixin state.
    pub fn on(&self, f: impl Fn(&mut M, &MixinHandle<M>) + 'static) -> EventHandler {
        self.on_event(move |state, h, _| f(state, h))
    }

    /// Like [`MixinHandle::on`], with access to the DOM event.
    pub fn on_event(&self, f: impl Fn(&mut M, &MixinHandle<M>, &Event) + 'static) -> EventHandler {
        #[cfg(target_arch = "wasm32")]
        {
            let slot = self.slot.clone();
            EventHandler(Rc::new(move |event: &Event| {
                if let Some(slot) = slot.upgrade() {
                    if let Ok(mut state) = slot.state.try_borrow_mut() {
                        f(&mut state, &slot.handle, event);
                    }
                }
            }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop(f);
            EventHandler(Rc::new(|_| {}))
        }
    }

    /// Run `f` with mutable state while the slot is live (from timers and async tasks).
    pub fn with<R>(&self, f: impl FnOnce(&mut M, &MixinHandle<M>) -> R) -> Option<R> {
        let slot = self.slot.upgrade()?;
        if slot.handle.shared.disposed.get() {
            return None;
        }
        let mut state = slot.state.try_borrow_mut().ok()?;
        Some(f(&mut state, &slot.handle))
    }

    /// Run `f` with the host element after pending DOM updates (`handle.queueTask`).
    /// Dropped on the server.
    pub fn queue_task(&self, f: impl FnOnce(&mut M, &MixinHandle<M>, &NodeHandle) + 'static) {
        #[cfg(target_arch = "wasm32")]
        {
            let slot = self.slot.clone();
            crate::dom::queue_task(Box::new(move || {
                let Some(slot) = slot.upgrade() else { return };
                let Some(host) = slot.handle.shared.host.upgrade() else { return };
                if slot.handle.shared.disposed.get() || !host.is_alive() {
                    return;
                }
                let node = NodeHandle { el: host.el.clone() };
                let Ok(mut state) = slot.state.try_borrow_mut() else { return };
                f(&mut state, &slot.handle, &node);
                drop(state);
            }));
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(f);
    }

    /// Aborted when this slot is disposed, even if the host stays mounted.
    #[cfg(target_arch = "wasm32")]
    pub fn signal(&self) -> web_sys::AbortSignal {
        let mut slot = self.shared.controller.borrow_mut();
        let controller = slot.get_or_insert_with(|| web_sys::AbortController::new().expect("AbortController"));
        if self.shared.disposed.get() {
            controller.abort();
        }
        controller.signal()
    }

    /// Run an async task in the browser. Dropped on the server.
    pub fn spawn<F: std::future::Future<Output = ()> + 'static>(&self, fut: F) {
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(fut);
        #[cfg(not(target_arch = "wasm32"))]
        drop(fut);
    }

    /// Context provided by an ancestor component (`handle.context.get`).
    pub fn context<T: 'static>(&self) -> Option<Rc<T>> {
        let want = TypeId::of::<T>();
        #[cfg(target_arch = "wasm32")]
        if let Some(host) = self.shared.host.upgrade() {
            return crate::dom::find_context(host.scope.parent.clone(), want).and_then(|v| v.downcast::<T>().ok());
        }
        crate::component::server_context(want).and_then(|v| v.downcast::<T>().ok())
    }

    /// The frame containing the host.
    pub fn frame(&self) -> FrameHandle {
        #[cfg(target_arch = "wasm32")]
        {
            let frame = self.shared.host.upgrade().and_then(|h| h.scope.frame.clone());
            FrameHandle { frame: Some(frame.unwrap_or_else(crate::dom::frames::top)) }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            FrameHandle::server(crate::component::server_frame_src().0)
        }
    }

    fn listen(&self, kind: Lifecycle, f: impl Fn(&mut M, &MixinHandle<M>, &LifecycleEvent<'_>) + 'static) {
        if cfg!(target_arch = "wasm32") {
            self.shared.listeners.borrow_mut().push((kind, Rc::new(f)));
        }
    }

    /// After the mixin binds to its host in the commit phase, including when it is added to
    /// an element that is already mounted.
    pub fn on_insert(&self, f: impl Fn(&mut M, &MixinHandle<M>, &InsertEvent) + 'static) {
        self.listen(Lifecycle::Insert, move |s, h, e| {
            if let LifecycleEvent::Insert(ev) = e {
                f(s, h, ev)
            }
        });
    }

    /// When the slot is disposed: the host left the document, or this mixin left its `mix`.
    pub fn on_remove(&self, f: impl Fn(&mut M, &MixinHandle<M>) + 'static) {
        self.listen(Lifecycle::Remove, move |s, h, _| f(s, h));
    }

    /// Before DOM changes in the host's update scope (its owner re-rendering, or
    /// `update()`). Measure the old layout here.
    pub fn on_before_update(&self, f: impl Fn(&mut M, &MixinHandle<M>, &NodeHandle) + 'static) {
        self.listen(Lifecycle::BeforeUpdate, move |s, h, e| {
            if let LifecycleEvent::BeforeUpdate(node) = e {
                f(s, h, node)
            }
        });
    }

    /// After DOM changes in the host's update scope have been applied.
    pub fn on_commit(&self, f: impl Fn(&mut M, &MixinHandle<M>, &NodeHandle) + 'static) {
        self.listen(Lifecycle::Commit, move |s, h, e| {
            if let LifecycleEvent::Commit(node) = e {
                f(s, h, node)
            }
        });
    }

    /// When removal of the host begins, while it can still be deferred with
    /// [`BeforeRemoveEvent::persist_node`].
    pub fn on_before_remove(&self, f: impl Fn(&mut M, &MixinHandle<M>, &BeforeRemoveEvent) + 'static) {
        self.listen(Lifecycle::BeforeRemove, move |s, h, e| {
            if let LifecycleEvent::BeforeRemove(ev) = e {
                f(s, h, ev)
            }
        });
    }
}

/// Take the stateful mixins queued on an element by `mix`.
pub(crate) fn take_mixins(el: &mut Element) -> Vec<Rc<dyn ErasedMixin>> {
    el.extra.as_mut().map(|x| std::mem::take(&mut x.mixins)).unwrap_or_default()
}

pub(crate) fn has_mixins(el: &Element) -> bool {
    el.extra.as_ref().is_some_and(|x| !x.mixins.is_empty())
}

thread_local! {
    static NEXT_HOST: Cell<u32> = const { Cell::new(0) };
}

pub(crate) fn next_host_id() -> Rc<str> {
    let n = NEXT_HOST.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        n
    });
    format!("rmx-m{n}").into()
}

/// Server rendering: run the element's server-enabled mixins once, in order, and return
/// the element they produce.
pub(crate) fn resolve_server(mut el: Element) -> Element {
    let ctx = SlotContext {
        tag: el.tag,
        id: next_host_id(),
        #[cfg(target_arch = "wasm32")]
        host: Weak::new(),
    };
    let mut queue = take_mixins(&mut el);
    let mut i = 0;
    while i < queue.len() {
        let d = queue[i].clone();
        if d.ssr() {
            let slot = d.setup(&ctx);
            d.render(&*slot, &mut el);
            queue.extend(take_mixins(&mut el));
        }
        i += 1;
    }
    el
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{on, rsx, render_to_string};

    struct Label {
        count: u32,
    }

    impl HostMixin for Label {
        type Args = &'static str;

        fn setup(_: &MixinHandle<Self>, tag: &'static str) -> Self {
            Label { count: if tag == "button" { 1 } else { 0 } }
        }

        fn render(&mut self, label: &&'static str, host: &mut Element, h: &MixinHandle<Self>) {
            host.set_attr("aria-label", format!("{label}: {}", self.count));
            host.set_attr("data-host", h.id().to_string());
            host.mix(on("click", h.on(|this, h| {
                this.count += 1;
                h.update();
            })));
            host.mix(mixin::<Pressed>(()));
        }
    }

    struct Pressed;

    impl HostMixin for Pressed {
        type Args = ();
        fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
            Pressed
        }
        fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
            host.set_attr("aria-pressed", "false");
        }
    }

    struct BrowserOnly;

    impl HostMixin for BrowserOnly {
        type Args = ();
        const SSR: bool = false;
        fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
            panic!("browser-only mixins do not run on the server")
        }
    }

    #[test]
    fn server_runs_setup_render_and_nested_mixins() {
        let html = render_to_string(&rsx! {
            <button aria-label="ignored" mix={[mixin::<Label>("Save"), mixin::<BrowserOnly>(())]}>"Save"</button>
        });
        assert!(html.starts_with("<button aria-label=\"Save: 1\" data-host=\"rmx-m"), "{html}");
        assert!(html.ends_with("aria-pressed=\"false\">Save</button>"), "{html}");
    }
}

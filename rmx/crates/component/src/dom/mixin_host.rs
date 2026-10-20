//! The browser runtime for stateful mixins: one [`MixinHost`] per element whose `mix`
//! contains a [`HostMixin`](crate::HostMixin). It owns the mixin slots and the element's
//! applied attributes, listeners and styles, so `update()` can re-run the mixins and patch the
//! element without re-rendering its owner.

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};


use super::vdom::{apply_styles, attach_events, has_inner_html, hydrate_attrs, patch_props, queue_commit, schedule_host_update, set_attr, Applied};
use super::Scope;
use crate::host_mixin::{take_mixins, AnySlot, BeforeRemoveEvent, InsertEvent, Lifecycle, LifecycleEvent, SlotContext};
use crate::mixin::NodeHandle;
use crate::node::{Element, Key};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Create,
    Hydrate,
    Patch,
}

pub(crate) struct MixinHost {
    me: Weak<MixinHost>,
    pub(crate) el: web_sys::Element,
    pub(crate) scope: Scope,
    tag: &'static str,
    id: Rc<str>,
    key: RefCell<Option<Key>>,
    alive: Cell<bool>,
    dirty: Cell<bool>,
    slots: RefCell<Vec<(TypeId, Rc<dyn AnySlot>)>>,
    /// The element as its owner last rendered it, without children, for `update()`.
    base: RefCell<Option<Element>>,
    props: RefCell<Applied>,
    /// Aborts pending `persist_node` teardowns.
    removal: RefCell<Option<web_sys::AbortController>>,
}

impl MixinHost {
    pub(crate) fn new(el: web_sys::Element, scope: &Scope, tag: &'static str, key: Option<Key>) -> Rc<MixinHost> {
        let host = Rc::new_cyclic(|me| MixinHost {
            me: me.clone(),
            el,
            scope: scope.clone(),
            tag,
            id: crate::host_mixin::next_host_id(),
            key: RefCell::new(key),
            alive: Cell::new(true),
            dirty: Cell::new(false),
            slots: RefCell::new(Vec::new()),
            base: RefCell::new(None),
            props: RefCell::new(Applied::default()),
            removal: RefCell::new(None),
        });
        if let Some(owner) = scope.parent.as_ref().and_then(Weak::upgrade) {
            owner.live().hosts.borrow_mut().push(host.me.clone());
        }
        host
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.get()
    }

    pub(crate) fn set_key(&self, key: Option<Key>) {
        *self.key.borrow_mut() = key;
    }

    /// Take over the props of an element that was mounted before it had stateful mixins.
    pub(crate) fn adopt(&self, props: Applied) {
        *self.props.borrow_mut() = props;
    }

    /// Run the mixins over the owner's element (children already taken) and apply the
    /// result. Returns whether the element's content comes from `innerHTML`.
    pub(crate) fn apply(&self, el: Element, mounted: bool) -> bool {
        let mode = if mounted { Mode::Create } else { Mode::Patch };
        self.apply_mode(el, mode)
    }

    pub(crate) fn hydrate(&self, el: Element) -> bool {
        self.apply_mode(el, Mode::Hydrate)
    }

    fn apply_mode(&self, el: Element, mode: Mode) -> bool {
        *self.base.borrow_mut() = Some(el.clone());
        self.run(el, mode)
    }

    fn run(&self, mut el: Element, mode: Mode) -> bool {
        let ctx = SlotContext { tag: self.tag, id: self.id.clone(), host: self.me.clone() };
        let old = std::mem::take(&mut *self.slots.borrow_mut());
        let mut queue = take_mixins(&mut el);
        let held = crate::mixin::withhold(&mut el);
        let mut slots: Vec<(TypeId, Rc<dyn AnySlot>)> = Vec::with_capacity(queue.len());
        let mut inserted = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            let d = queue[i].clone();
            let ty = d.mixin_type();
            let slot = match old.get(i) {
                Some((t, s)) if *t == ty && !s.is_disposed() => s.clone(),
                _ => {
                    let s = d.setup(&ctx);
                    inserted.push(s.clone());
                    s
                }
            };
            d.render(&*slot, &mut el);
            queue.extend(take_mixins(&mut el));
            slots.push((ty, slot));
            i += 1;
        }
        for (i, (_, s)) in old.iter().enumerate() {
            if !slots.get(i).is_some_and(|(_, n)| Rc::ptr_eq(n, s)) {
                s.dispose();
            }
        }
        *self.slots.borrow_mut() = slots;
        crate::mixin::restore(&mut el, held);

        let Element { attrs, events, extra, .. } = el;
        let inner_html = has_inner_html(&attrs);
        match mode {
            Mode::Create | Mode::Hydrate => {
                if mode == Mode::Create {
                    for (name, value) in &attrs {
                        set_attr(&self.el, self.tag, name, value);
                    }
                } else {
                    hydrate_attrs(&self.el, self.tag, &attrs);
                }
                let mut props = self.props.borrow_mut();
                apply_styles(extra, &mut props.styles);
                props.listeners = attach_events(&self.el, events);
                props.attrs = attrs;
            }
            Mode::Patch => {
                patch_props(&self.el, self.tag, &mut self.props.borrow_mut(), attrs, events, extra);
            }
        }

        if !inserted.is_empty() {
            let me = self.me.clone();
            queue_commit(Box::new(move || {
                let Some(host) = me.upgrade() else { return };
                if !host.alive.get() {
                    return;
                }
                let event = InsertEvent { node: NodeHandle { el: host.el.clone() }, key: host.key.borrow().clone() };
                for slot in inserted.iter().filter(|s| !s.is_disposed()) {
                    slot.bind();
                    slot.dispatch(&LifecycleEvent::Insert(&event));
                }
            }));
        }
        inner_html
    }

    pub(crate) fn schedule_update(&self) {
        if self.alive.get() && !self.dirty.replace(true) {
            if let Some(me) = self.me.upgrade() {
                schedule_host_update(me);
            }
        }
    }

    /// Re-run the mixins from the owner's last element (`handle.update()`).
    pub(crate) fn rerun(&self) {
        if !self.dirty.replace(false) || !self.alive.get() {
            return;
        }
        self.before_update();
        let base = self.base.borrow().clone();
        if let Some(el) = base {
            self.run(el, Mode::Patch);
        }
        self.commit();
    }

    fn dispatch_where(&self, kind: Lifecycle, event: &LifecycleEvent<'_>) {
        // Update events go to slots that have had their `insert`.
        let bound_only = matches!(kind, Lifecycle::BeforeUpdate | Lifecycle::Commit);
        let slots: Vec<Rc<dyn AnySlot>> = self
            .slots
            .borrow()
            .iter()
            .filter(|(_, s)| s.listens(kind) && (!bound_only || s.is_bound()))
            .map(|(_, s)| s.clone())
            .collect();
        for slot in slots {
            slot.dispatch(event);
        }
    }

    /// A host waiting for its exit teardown is no longer part of its owner's updates.
    fn is_exiting(&self) -> bool {
        self.removal.borrow().is_some()
    }

    pub(crate) fn before_update(&self) {
        if self.is_exiting() {
            return;
        }
        let node = NodeHandle { el: self.el.clone() };
        self.dispatch_where(Lifecycle::BeforeUpdate, &LifecycleEvent::BeforeUpdate(&node));
    }

    pub(crate) fn commit(&self) {
        if self.alive.get() && !self.is_exiting() {
            let node = NodeHandle { el: self.el.clone() };
            self.dispatch_where(Lifecycle::Commit, &LifecycleEvent::Commit(&node));
        }
    }

    pub(crate) fn listens_before_remove(&self) -> bool {
        self.slots.borrow().iter().any(|(_, s)| s.listens(Lifecycle::BeforeRemove))
    }

    /// Dispose every slot (`remove` events), release listeners and cancel pending teardown.
    pub(crate) fn dispose(&self) {
        if !self.alive.replace(false) {
            return;
        }
        if let Some(c) = self.removal.borrow_mut().take() {
            c.abort();
        }
        let slots = std::mem::take(&mut *self.slots.borrow_mut());
        for (_, slot) in slots {
            slot.dispose();
        }
        std::mem::take(&mut *self.props.borrow_mut()).release(None);
    }

    /// Removal is starting: dispatch `beforeRemove`. When a listener persisted the node,
    /// returns `true` and calls `done` once every teardown settles (or was cancelled).
    pub(crate) fn persist_removal(&self, done: impl FnOnce() + 'static) -> bool {
        if !self.alive.get() || !self.listens_before_remove() {
            return false;
        }
        let event = BeforeRemoveEvent::default();
        self.dispatch_where(Lifecycle::BeforeRemove, &LifecycleEvent::BeforeRemove(&event));
        let teardowns = event.teardowns.take();
        if teardowns.is_empty() {
            return false;
        }
        let controller = web_sys::AbortController::new().expect("AbortController");
        let signal = controller.signal();
        *self.removal.borrow_mut() = Some(controller);
        let pending = Rc::new(Cell::new(teardowns.len()));
        let done = Rc::new(Cell::new(Some(done)));
        for teardown in teardowns {
            let fut = teardown(signal.clone());
            let (pending, done) = (pending.clone(), done.clone());
            wasm_bindgen_futures::spawn_local(async move {
                fut.await;
                pending.set(pending.get() - 1);
                if pending.get() == 0 {
                    if let Some(done) = done.take() {
                        done();
                    }
                }
            });
        }
        true
    }

    /// The persisted host is being reused: abort its pending teardown signal.
    pub(crate) fn cancel_removal(&self) {
        if let Some(c) = self.removal.borrow_mut().take() {
            c.abort();
        }
    }

    /// Dispatch `reclaimed` in the commit phase, after the reused host was patched.
    pub(crate) fn queue_reclaimed(&self) {
        let me = self.me.clone();
        queue_commit(Box::new(move || {
            let Some(host) = me.upgrade() else { return };
            if host.alive.get() {
                let node = NodeHandle { el: host.el.clone() };
                host.dispatch_where(Lifecycle::Reclaimed, &LifecycleEvent::Reclaimed(&node));
            }
        }));
    }
}

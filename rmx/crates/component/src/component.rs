// Parts of the instance and node types are only read by the browser runtime.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::frame::{FrameHandle, Frames};
use crate::node::{ComponentNode, Event, EventHandler, Key, Node};
use crate::ssr::Ssr;

/// A Remix 3 style component.
///
/// `setup` runs once per instance and returns the component's state as a plain struct.
/// `render` runs for the first render and again after each [`Handle::update`]. Event
/// handlers get `&mut self`, mutate fields directly, then call `update()`.
///
/// ```ignore
/// impl Component for Counter {
///     type Props = CounterProps;
///
///     fn setup(props: &CounterProps, _h: &Handle<Self>) -> Self {
///         Counter { count: props.initial }
///     }
///
///     fn render(&self, _props: &CounterProps, h: &Handle<Self>) -> Node {
///         rsx! {
///             <button on:click={h.on(|this, h| { this.count += 1; h.update(); })}>
///                 "Count: " {self.count}
///             </button>
///         }
///     }
/// }
/// ```
pub trait Component: Sized + 'static {
    /// Props from the parent. `render` always receives the latest value.
    type Props: Clone + 'static;

    fn setup(props: &Self::Props, h: &Handle<Self>) -> Self;

    fn render(&self, props: &Self::Props, h: &Handle<Self>) -> Node;

    #[doc(hidden)]
    const CLIENT_ENTRY: Option<&'static str> = None;

    #[doc(hidden)]
    fn props_json(_props: &Self::Props) -> Option<String> {
        None
    }
}

/// Context values a component provides to its descendants.
pub(crate) type ContextList = RefCell<Vec<(TypeId, Rc<dyn Any>)>>;

/// A live component instance. Only the browser runtime creates these.
pub(crate) struct Instance<C: Component> {
    pub(crate) state: RefCell<C>,
    pub(crate) props: RefCell<C::Props>,
    pub(crate) handle: Handle<C>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) live: crate::dom::Live,
}

/// The component's connection to the runtime (Remix's `handle`).
pub struct Handle<C: Component> {
    pub(crate) inst: Weak<Instance<C>>,
    pub(crate) shared: Rc<HandleShared>,
}

/// Per-instance data that exists before `setup` runs, so context, frame and signal work
/// inside `setup`.
pub(crate) struct HandleShared {
    id: u32,
    /// Context this component provides (`set_context`).
    pub(crate) contexts: ContextList,
    #[cfg(target_arch = "wasm32")]
    pub(crate) scope: crate::dom::Scope,
    #[cfg(target_arch = "wasm32")]
    pub(crate) controller: RefCell<Option<web_sys::AbortController>>,
    /// Aborted when the component renders again or is removed (the render signal).
    #[cfg(target_arch = "wasm32")]
    pub(crate) render_controller: RefCell<Option<web_sys::AbortController>>,
    /// What the component is running now, to catch `update()` from setup or render.
    #[cfg(target_arch = "wasm32")]
    pub(crate) phase: Cell<Phase>,
    /// Set once the first render's DOM exists; `update()` before that is an error.
    #[cfg(target_arch = "wasm32")]
    pub(crate) committed: Cell<bool>,
    /// The component's full type name (`std::any::type_name`), for runtime messages.
    type_name: &'static str,
    pub(crate) removed: Cell<bool>,
}

#[cfg(target_arch = "wasm32")]
type ScopeRef = crate::dom::Scope;
#[cfg(not(target_arch = "wasm32"))]
type ScopeRef = ();

/// The part of its lifecycle a component is running.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Setup,
    Render,
    Idle,
}

/// `my_app::ui::Counter<T>` -> `Counter`.
fn short_type_name(full: &'static str) -> &'static str {
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base)
}

impl HandleShared {
    /// Not generic, so it is compiled once rather than inlined per component type.
    /// `scope` is `None` for a detached (server) handle.
    #[inline(never)]
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn new(type_name: &'static str, scope: Option<&ScopeRef>) -> Rc<HandleShared> {
        Rc::new(HandleShared {
            id: next_id(),
            contexts: ContextList::default(),
            #[cfg(target_arch = "wasm32")]
            scope: scope.cloned().unwrap_or_default(),
            #[cfg(target_arch = "wasm32")]
            controller: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            render_controller: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            phase: Cell::new(if scope.is_some() { Phase::Setup } else { Phase::Idle }),
            #[cfg(target_arch = "wasm32")]
            committed: Cell::new(false),
            type_name,
            removed: Cell::new(false),
        })
    }

    /// A render is starting: abort the previous render's signal and enter the render
    /// phase. Returns the phase to restore with [`HandleShared::end_render`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn begin_render(&self) -> Phase {
        self.abort_render_signal();
        self.phase.replace(Phase::Render)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn end_render(&self, outer: Phase) {
        self.phase.set(outer);
    }

    /// The component's name for messages (`Counter`, like a JS function's `name`).
    pub(crate) fn name(&self) -> &'static str {
        short_type_name(self.type_name)
    }

    /// The component left the document: abort its signal.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn dispose(&self) {
        self.removed.set(true);
        if let Some(c) = self.controller.borrow().as_ref() {
            c.abort();
        }
        self.abort_render_signal();
    }

    /// Whether `update()` may schedule a render now; warns or reports when it may not.
    /// Not generic, so the messages are compiled once rather than per component type.
    #[cfg(target_arch = "wasm32")]
    #[inline(never)]
    pub(crate) fn may_update(&self) -> bool {
        if self.removed.get() {
            return false;
        }
        let name = self.name();
        let message = match self.phase.get() {
            Phase::Idle if self.committed.get() => return true,
            Phase::Setup => {
                web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
                    "Ignored handle.update() while {name} is running its setup function. The initial render includes setup changes."
                )));
                return false;
            }
            Phase::Render => format!(
                "Cannot call handle.update() while {name} is running its render function. Call it from an event handler or handle.queueTask() instead."
            ),
            Phase::Idle => format!(
                "Cannot call handle.update() before {name}'s initial render commits. Call it from an event handler or handle.queueTask() instead."
            ),
        };
        crate::app::report_to(self.scope.errors.as_ref(), message);
        false
    }

    /// A new render is starting: the previous render's signal is done.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn abort_render_signal(&self) {
        if let Some(c) = self.render_controller.borrow_mut().take() {
            c.abort();
        }
    }
}

impl<C: Component> Clone for Handle<C> {
    fn clone(&self) -> Self {
        Handle { inst: self.inst.clone(), shared: self.shared.clone() }
    }
}

thread_local! {
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    #[cfg(not(target_arch = "wasm32"))]
    static NOOP_HANDLER: EventHandler = EventHandler(Rc::new(|_| {}));
    /// Server rendering: contexts provided by the components currently being rendered.
    static SSR_CONTEXT: RefCell<Vec<Rc<HandleShared>>> = const { RefCell::new(Vec::new()) };
    /// Server rendering: (frame src, top frame src) for `h.frame()` / `h.frames().top()`.
    static SSR_FRAME: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
}

/// Server rendering: the nearest context value of type `want`.
pub(crate) fn server_context(want: TypeId) -> Option<Rc<dyn Any>> {
    SSR_CONTEXT.with(|stack| {
        stack.borrow().iter().rev().find_map(|shared| {
            shared.contexts.borrow().iter().find(|(t, _)| *t == want).map(|(_, v)| v.clone())
        })
    })
}

fn next_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id.wrapping_add(1));
        id
    })
}

/// Set the frame URLs server-rendered components see through `h.frame().src()` and
/// `h.frames().top().src()`. The render middleware calls this around each render.
pub fn set_server_frame_src(frame_src: &str, top_src: &str) {
    SSR_FRAME.with(|f| *f.borrow_mut() = (frame_src.to_string(), top_src.to_string()));
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn server_frame_src() -> (String, String) {
    SSR_FRAME.with(|f| f.borrow().clone())
}

impl<C: Component> Handle<C> {
    /// A handle with no instance behind it, used for server rendering.
    pub(crate) fn detached() -> Self {
        Handle { inst: Weak::new(), shared: HandleShared::new(std::any::type_name::<C>(), None) }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn attached(inst: Weak<Instance<C>>, scope: &crate::dom::Scope) -> Self {
        Handle { inst, shared: HandleShared::new(std::any::type_name::<C>(), Some(scope)) }
    }

    /// Stable per-instance id, useful for `id`/`for` attribute pairs.
    pub fn id(&self) -> u32 {
        self.shared.id
    }

    /// A string id for HTML attributes, like `"rmx-12"`.
    pub fn dom_id(&self) -> String {
        format!("rmx-{}", self.shared.id)
    }

    /// Schedule a re-render of this component. Renders are batched on a microtask, so
    /// several `update()` calls in one handler produce one render. No-op on the server.
    ///
    /// Call it from event handlers, tasks and async work. As in Remix, a call from `setup`
    /// is ignored with a warning (the first render already sees setup's changes), and a
    /// call from `render`, or before the first render is in the DOM, is reported as an
    /// error (see [`crate::Root::on_error`]) and ignored.
    pub fn update(&self) {
        #[cfg(target_arch = "wasm32")]
        if self.shared.may_update() {
            if let Some(inst) = self.inst.upgrade() {
                crate::dom::schedule_update(inst);
            }
        }
    }

    /// Report an error from this component: log it with `console.error` and dispatch it
    /// to the error listeners of the root it renders in ([`crate::Root::on_error`]), or of
    /// the app ([`crate::App::on_error`]) for client entries and frames. This is the Rust
    /// counterpart of throwing from a Remix component, since a panic cannot be caught in
    /// WebAssembly. On the server it logs to stderr.
    pub fn report_error(&self, error: impl std::fmt::Display) {
        #[cfg(target_arch = "wasm32")]
        crate::app::report_to(self.shared.scope.errors.as_ref(), error);
        #[cfg(not(target_arch = "wasm32"))]
        crate::report_error(error);
    }

    /// The latest props (`handle.props`), for callbacks that run outside `render`. `None`
    /// on the server and after the component is removed.
    pub fn props(&self) -> Option<C::Props> {
        self.inst.upgrade().map(|inst| inst.props.borrow().clone())
    }

    /// Whether the component is still mounted in the document.
    pub fn is_connected(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inst.upgrade().is_some_and(|i| i.live.alive.get())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// An `AbortSignal` aborted when the component is removed (`handle.signal`). Pass it to
    /// `fetch` or `addEventListener` so work stops with the component.
    #[cfg(target_arch = "wasm32")]
    pub fn signal(&self) -> web_sys::AbortSignal {
        let mut slot = self.shared.controller.borrow_mut();
        let controller = slot.get_or_insert_with(|| web_sys::AbortController::new().expect("AbortController"));
        if self.shared.removed.get() {
            controller.abort();
        }
        controller.signal()
    }

    /// A signal for the current render, aborted when the component renders again or is
    /// removed: the signal Remix passes to `queueTask` tasks and resolves `handle.update()`
    /// with. Read it in a queued task or after [`Handle::updated`] to drop work that a newer
    /// render made stale. On the server it never aborts.
    pub fn render_signal(&self) -> crate::Signal {
        #[cfg(target_arch = "wasm32")]
        {
            let controller = web_sys::AbortController::new().expect("AbortController");
            if self.shared.removed.get() {
                controller.abort();
                return crate::Signal::from_raw(controller.signal());
            }
            let mut slot = self.shared.render_controller.borrow_mut();
            crate::Signal::from_raw(slot.get_or_insert(controller).signal())
        }
        #[cfg(not(target_arch = "wasm32"))]
        crate::Signal::never()
    }

    /// An event handler that gets mutable access to the component state.
    pub fn on(&self, f: impl Fn(&mut C, &Handle<C>) + 'static) -> EventHandler {
        self.on_event(move |state, h, _event| f(state, h))
    }

    /// A [`crate::node_ref`] mixin whose callback gets the component state, for keeping
    /// an element around: `mix={h.node_ref(|this, node| this.input = Some(node.clone()))}`.
    pub fn node_ref(&self, f: impl Fn(&mut C, &crate::NodeHandle) + 'static) -> crate::Mixin {
        let inst = self.inst.clone();
        crate::node_ref(move |node, _| {
            if let Some(inst) = inst.upgrade() {
                if let Ok(mut state) = inst.state.try_borrow_mut() {
                    f(&mut state, node);
                }
            }
        })
    }

    /// Like [`Handle::on`], with access to the DOM event.
    pub fn on_event(&self, f: impl Fn(&mut C, &Handle<C>, &Event) + 'static) -> EventHandler {
        #[cfg(target_arch = "wasm32")]
        {
            let inst = self.inst.clone();
            let f = Rc::new(f);
            EventHandler(Rc::new(move |event: &Event| {
                let Some(inst) = inst.upgrade() else { return };
                if let Ok(mut state) = inst.state.try_borrow_mut() {
                    f(&mut state, &inst.handle, event);
                    return;
                }
                // Re-entrant: this component's own code dispatched the event synchronously
                // (`node.focus()` fires `focus`). Run the handler once that code returns.
                let (inst, f, event) = (Rc::downgrade(&inst), f.clone(), event.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    if let Some(inst) = inst.upgrade() {
                        if inst.live.alive.get() {
                            if let Ok(mut state) = inst.state.try_borrow_mut() {
                                f(&mut state, &inst.handle, &event);
                            }
                        }
                    }
                });
            }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop(f);
            NOOP_HANDLER.with(Clone::clone)
        }
    }

    /// Run `f` with mutable state, if the component is still mounted. Use it from async
    /// tasks and timers: `h.with(|this, h| { this.data = data; h.update(); })`.
    pub fn with<R>(&self, f: impl FnOnce(&mut C, &Handle<C>) -> R) -> Option<R> {
        #[cfg(target_arch = "wasm32")]
        {
            let inst = self.inst.upgrade()?;
            if !inst.live.alive.get() {
                return None;
            }
            let mut state = inst.state.try_borrow_mut().ok()?;
            Some(f(&mut state, &inst.handle))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop(f);
            None
        }
    }

    /// Run `f` after the next commit to the DOM (Remix's `handle.queueTask`). Calling it
    /// from `setup` runs it after the first render or hydration. Remix passes the task a
    /// signal; read [`Handle::render_signal`] inside `f` for the same check. Dropped on the
    /// server.
    pub fn queue_task(&self, f: impl FnOnce(&mut C, &Handle<C>) + 'static) {
        #[cfg(target_arch = "wasm32")]
        {
            let inst = self.inst.clone();
            crate::dom::queue_task(Box::new(move || {
                if let Some(inst) = inst.upgrade() {
                    if inst.live.alive.get() {
                        let mut state = inst.state.borrow_mut();
                        f(&mut state, &inst.handle);
                    }
                }
            }));
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(f);
    }

    /// Run an async task in the browser (fetches, timers). Use [`Handle::with`] inside it to
    /// touch state, and [`Handle::signal`] to stop with the component. Dropped on the server.
    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(fut);
        #[cfg(not(target_arch = "wasm32"))]
        drop(fut);
    }

    /// Schedule an update now and resolve once it has been applied to the DOM, with that
    /// render's [signal](Handle::render_signal) (Remix's `let signal = await handle.update()`).
    /// Like Remix, the update is scheduled when this is called, not when the future is
    /// first polled. The signal is already aborted if the component was removed. Resolves
    /// at once on the server.
    pub fn updated(&self) -> impl std::future::Future<Output = crate::Signal> + 'static {
        self.update();
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let h = self.clone();
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                let h = h.clone();
                // Read the signal when the flush runs tasks, as Remix's update task does.
                crate::dom::queue_task(Box::new(move || {
                    let _ = resolve.call1(&wasm_bindgen::JsValue::UNDEFINED, h.render_signal().raw());
                }));
            });
            async move {
                let value = wasm_bindgen_futures::JsFuture::from(promise).await.unwrap_or_default();
                crate::Signal::from(value.unchecked_into::<web_sys::AbortSignal>())
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::future::ready(crate::Signal::never())
    }

    /// Call `f` with the component state each time `target` dispatches, until the
    /// component is removed (`target.addEventListener(type, fn, { signal: handle.signal })`).
    pub fn listen<E: 'static>(&self, target: &crate::TypedEventTarget<E>, f: impl Fn(&mut C, &Handle<C>, &E) + 'static) {
        #[cfg(target_arch = "wasm32")]
        {
            let inst = self.inst.clone();
            let id = target.add_listener(move |event| {
                if let Some(inst) = inst.upgrade() {
                    if !inst.live.alive.get() {
                        return;
                    }
                    if let Ok(mut state) = inst.state.try_borrow_mut() {
                        f(&mut state, &inst.handle, event);
                    }
                }
            });
            let target = target.clone();
            crate::Signal::from_raw(self.signal()).on_abort(move || target.remove_listener(id));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (target, f);
    }

    /// Provide a value to descendants (`handle.context.set`). Does not trigger an update.
    pub fn set_context<T: 'static>(&self, value: T) {
        let mut list = self.shared.contexts.borrow_mut();
        list.retain(|(t, _)| *t != TypeId::of::<T>());
        list.push((TypeId::of::<T>(), Rc::new(value)));
    }

    /// The nearest ancestor's value of type `T` (`handle.context.get`).
    pub fn context<T: 'static>(&self) -> Option<Rc<T>> {
        let want = TypeId::of::<T>();
        #[cfg(target_arch = "wasm32")]
        if self.shared.scope.parent.is_some() {
            return crate::dom::find_context(self.shared.scope.parent.clone(), want).and_then(|v| v.downcast::<T>().ok());
        }
        server_context(want).and_then(|v| v.downcast::<T>().ok())
    }

    /// Run `f` when `frame` emits `event`, for as long as this component is mounted
    /// (`handle.frame.addEventListener(type, fn, { signal: handle.signal })`).
    ///
    /// ```ignore
    /// h.on_frame(&h.frame(), FrameEvent::ReloadStart, |this, h| { this.pending = true; h.update(); });
    /// ```
    pub fn on_frame(&self, frame: &FrameHandle, event: crate::FrameEvent, f: impl Fn(&mut C, &Handle<C>) + 'static) {
        #[cfg(target_arch = "wasm32")]
        if let Some(state) = &frame.frame {
            state.add_listener(event.name(), self.on(f), &self.signal());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (frame, event, f);
    }

    /// The frame this component renders in (`handle.frame`).
    pub fn frame(&self) -> FrameHandle {
        #[cfg(target_arch = "wasm32")]
        {
            let frame = self.shared.scope.frame.clone();
            FrameHandle { frame: Some(frame.unwrap_or_else(crate::dom::frames::top)) }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            FrameHandle::server(server_frame_src().0)
        }
    }

    /// Frame lookup (`handle.frames.top`, `handle.frames.get(name)`).
    pub fn frames(&self) -> Frames {
        Frames
    }
}

impl Node {
    /// A component node. `rsx!` emits this for capitalized tags.
    pub fn component<C: Component>(key: Option<Key>, props: C::Props) -> Node {
        Node::Component(ComponentNode { key, inner: Box::new(PropsOf::<C>(props)) })
    }
}

/// Type-erased props plus the component type they belong to.
pub(crate) trait ErasedComponent {
    fn clone_box(&self) -> Box<dyn ErasedComponent>;
    fn component_type(&self) -> TypeId;
    fn ssr(&self, out: &mut Ssr);
    /// Run setup and render once, detached, for serializing the output.
    fn render_static(&self) -> Node;

    #[cfg(target_arch = "wasm32")]
    fn instantiate(self: Box<Self>, scope: &crate::dom::Scope) -> Rc<dyn crate::dom::AnyInstance>;

    /// Hand new props to an existing instance of the same type and re-render it.
    #[cfg(target_arch = "wasm32")]
    fn update_instance(self: Box<Self>, inst: &dyn crate::dom::AnyInstance);
}

impl Clone for Box<dyn ErasedComponent> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

struct PropsOf<C: Component>(C::Props);

impl<C: Component> ErasedComponent for PropsOf<C> {
    fn clone_box(&self) -> Box<dyn ErasedComponent> {
        Box::new(PropsOf::<C>(self.0.clone()))
    }

    fn component_type(&self) -> TypeId {
        TypeId::of::<C>()
    }

    fn render_static(&self) -> Node {
        let handle = Handle::<C>::detached();
        C::setup(&self.0, &handle).render(&self.0, &handle)
    }

    fn ssr(&self, out: &mut Ssr) {
        let handle = Handle::<C>::detached();
        let state = C::setup(&self.0, &handle);
        let node = state.render(&self.0, &handle);
        let provides = !handle.shared.contexts.borrow().is_empty();
        if provides {
            SSR_CONTEXT.with(|s| s.borrow_mut().push(handle.shared.clone()));
        }
        match C::CLIENT_ENTRY {
            Some(name) => out.client_entry(name, || C::props_json(&self.0).unwrap_or_default(), &node),
            None => out.write(&node),
        }
        if provides {
            SSR_CONTEXT.with(|s| s.borrow_mut().pop());
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn instantiate(self: Box<Self>, scope: &crate::dom::Scope) -> Rc<dyn crate::dom::AnyInstance> {
        crate::dom::new_instance::<C>(self.0, scope)
    }

    #[cfg(target_arch = "wasm32")]
    fn update_instance(self: Box<Self>, inst: &dyn crate::dom::AnyInstance) {
        let inst = inst
            .as_any()
            .downcast_ref::<Instance<C>>()
            .expect("component type checked before update");
        *inst.props.borrow_mut() = self.0;
        crate::dom::rerender(inst);
    }
}

/// Props for [`Fragment`].
#[derive(Clone, Default)]
pub struct FragmentProps {
    pub children: crate::Children,
}

/// Groups children without a wrapper element. Use it where a fragment needs a key, so a
/// keyed list can move the whole group: `<Fragment key={id}><dt/><dd/></Fragment>`
/// (Remix's `<Fragment key>`). Unkeyed groups can use `<>...</>`.
pub struct Fragment;

impl Component for Fragment {
    type Props = FragmentProps;

    fn setup(_: &FragmentProps, _: &Handle<Self>) -> Self {
        Fragment
    }

    fn render(&self, props: &FragmentProps, _: &Handle<Self>) -> Node {
        crate::IntoNode::into_node(&props.children)
    }
}

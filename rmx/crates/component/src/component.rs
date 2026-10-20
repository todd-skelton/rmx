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
    id: u32,
    /// Context this component provides (`set_context`).
    pub(crate) contexts: Rc<ContextList>,
}

impl<C: Component> Clone for Handle<C> {
    fn clone(&self) -> Self {
        Handle { inst: self.inst.clone(), id: self.id, contexts: self.contexts.clone() }
    }
}

thread_local! {
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    #[cfg(not(target_arch = "wasm32"))]
    static NOOP_HANDLER: EventHandler = EventHandler(Rc::new(|_| {}));
    /// Server rendering: contexts provided by the components currently being rendered.
    static SSR_CONTEXT: RefCell<Vec<Rc<ContextList>>> = const { RefCell::new(Vec::new()) };
    /// Server rendering: (frame src, top frame src) for `h.frame()` / `h.frames().top()`.
    static SSR_FRAME: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
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

pub(crate) fn server_frame_src() -> (String, String) {
    SSR_FRAME.with(|f| f.borrow().clone())
}

impl<C: Component> Handle<C> {
    /// A handle with no instance behind it, used for server rendering.
    pub(crate) fn detached() -> Self {
        Handle { inst: Weak::new(), id: next_id(), contexts: Rc::default() }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn attached(inst: Weak<Instance<C>>) -> Self {
        Handle { inst, id: next_id(), contexts: Rc::default() }
    }

    /// Stable per-instance id, useful for `id`/`for` attribute pairs.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// A string id for HTML attributes, like `"rmx-12"`.
    pub fn dom_id(&self) -> String {
        format!("rmx-{}", self.id)
    }

    /// Schedule a re-render of this component. Renders are batched on a microtask, so
    /// several `update()` calls in one handler produce one render. No-op on the server.
    pub fn update(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(inst) = self.inst.upgrade() {
            crate::dom::schedule_update(inst);
        }
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
        match self.inst.upgrade() {
            Some(inst) => inst.live.signal(),
            None => {
                let c = web_sys::AbortController::new().expect("AbortController");
                c.abort();
                c.signal()
            }
        }
    }

    /// An event handler that gets mutable access to the component state.
    pub fn on(&self, f: impl Fn(&mut C, &Handle<C>) + 'static) -> EventHandler {
        self.on_event(move |state, h, _event| f(state, h))
    }

    /// Like [`Handle::on`], with access to the DOM event.
    pub fn on_event(&self, f: impl Fn(&mut C, &Handle<C>, &Event) + 'static) -> EventHandler {
        #[cfg(target_arch = "wasm32")]
        {
            let inst = self.inst.clone();
            EventHandler(Rc::new(move |event: &Event| {
                if let Some(inst) = inst.upgrade() {
                    let mut state = inst.state.borrow_mut();
                    f(&mut state, &inst.handle, event);
                }
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
    /// from `setup` runs it after the first render or hydration. Dropped on the server.
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

    /// Provide a value to descendants (`handle.context.set`). Does not trigger an update.
    pub fn set_context<T: 'static>(&self, value: T) {
        let mut list = self.contexts.borrow_mut();
        list.retain(|(t, _)| *t != TypeId::of::<T>());
        list.push((TypeId::of::<T>(), Rc::new(value)));
    }

    /// The nearest ancestor's value of type `T` (`handle.context.get`).
    pub fn context<T: 'static>(&self) -> Option<Rc<T>> {
        let want = TypeId::of::<T>();
        #[cfg(target_arch = "wasm32")]
        if let Some(inst) = self.inst.upgrade() {
            return crate::dom::find_context(inst.live.parent.as_ref(), want).and_then(|v| v.downcast::<T>().ok());
        }
        SSR_CONTEXT.with(|stack| {
            stack.borrow().iter().rev().find_map(|list| {
                list.borrow().iter().find(|(t, _)| *t == want).map(|(_, v)| v.clone())
            })
        })
        .and_then(|v| v.downcast::<T>().ok())
    }

    /// The frame this component renders in (`handle.frame`).
    pub fn frame(&self) -> FrameHandle {
        #[cfg(target_arch = "wasm32")]
        {
            let frame = self.inst.upgrade().and_then(|i| i.live.frame.clone());
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

    fn ssr(&self, out: &mut Ssr) {
        let handle = Handle::<C>::detached();
        let state = C::setup(&self.0, &handle);
        let node = state.render(&self.0, &handle);
        let provides = !handle.contexts.borrow().is_empty();
        if provides {
            SSR_CONTEXT.with(|s| s.borrow_mut().push(handle.contexts.clone()));
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

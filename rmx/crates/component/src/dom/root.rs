//! Standalone roots (Remix's `createRoot` and `createRangeRoot`): render a node tree into
//! a container element or between two sibling nodes, without `run()` or server markers.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::frames::FrameState;
use super::vdom::{hydrate_instance, remove_root};
use super::{flush, rerender, AnyInstance, Scope};
use crate::component::{Component, Handle, Instance};
use crate::node::{Children, IntoNode, Node};

/// A root returned by [`create_root`] or [`create_range_root`].
///
/// ```ignore
/// let root = rmx::create_root(&container);
/// root.render(rsx! { <App/> });
/// ```
#[derive(Clone)]
pub struct Root(Rc<RootState>);

struct RootState {
    parent: web_sys::Node,
    /// The frame components in this root see as `h.frame()` (`None` = the top frame).
    frame: Option<Rc<FrameState>>,
    /// The node before the root's content (`None` = the start of the container).
    start: Option<web_sys::Node>,
    /// The node after the root's content (`None` = the end of the container).
    end: Option<web_sys::Node>,
    inst: RefCell<Option<Rc<dyn AnyInstance>>>,
    current: RefCell<Option<Children>>,
    /// Errors from this root's components (`None` for frame roots, which report to the app).
    errors: Option<crate::TypedEventTarget<crate::AppError>>,
    /// Forwards bubbling DOM `error` events from the container to `errors`.
    forward: RefCell<Option<js_sys::Function>>,
}

/// A root that renders into `container`. The first render hydrates existing content (for
/// example server HTML) and replaces what does not match.
pub fn create_root(container: &web_sys::Element) -> Root {
    super::styles::adopt_server_styles(container.as_ref());
    Root::in_container(container.clone().into(), None).standalone()
}

/// A root that owns the nodes between `start` and `end` (exclusive), which must be
/// siblings. Useful for rendering into part of an element, between two comments.
///
/// # Panics
///
/// If `start` and `end` do not have the same parent node ("Boundaries must share parent").
pub fn create_range_root(start: &web_sys::Node, end: &web_sys::Node) -> Root {
    Root::in_range(start, end, None).standalone()
}

impl Root {
    pub(crate) fn in_container(parent: web_sys::Node, frame: Option<Rc<FrameState>>) -> Root {
        Root(Rc::new(RootState {
            parent,
            frame,
            start: None,
            end: None,
            inst: RefCell::new(None),
            current: RefCell::new(None),
            errors: None,
            forward: RefCell::new(None),
        }))
    }

    pub(crate) fn in_range(start: &web_sys::Node, end: &web_sys::Node, frame: Option<Rc<FrameState>>) -> Root {
        let parent = match (start.parent_node(), end.parent_node()) {
            (Some(a), Some(b)) if a.is_same_node(Some(&b)) => a,
            _ => panic!("Boundaries must share parent"),
        };
        Root(Rc::new(RootState {
            parent,
            frame,
            start: Some(start.clone()),
            end: Some(end.clone()),
            inst: RefCell::new(None),
            current: RefCell::new(None),
            errors: None,
            forward: RefCell::new(None),
        }))
    }

    /// Give the root its own error listeners (Remix's `createRoot` is an event target).
    fn standalone(self) -> Root {
        let mut state = Rc::try_unwrap(self.0).ok().expect("new root");
        state.errors = Some(crate::TypedEventTarget::new());
        let root = Root(Rc::new(state));
        root.attach_error_forwarding();
        root
    }

    /// Forward bubbling DOM `error` events (an `ErrorEvent` dispatched inside the root)
    /// to the root's listeners, until [`Root::dispose`].
    fn attach_error_forwarding(&self) {
        let Some(errors) = self.0.errors.clone() else { return };
        if self.0.forward.borrow().is_some() {
            return;
        }
        let forward: js_sys::Function = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
            let message = match event.dyn_ref::<web_sys::ErrorEvent>() {
                Some(e) => error_message(&e.error()).unwrap_or_else(|| e.message()),
                None => "error".to_string(),
            };
            errors.dispatch(&crate::AppError { message });
        })
        .into_js_value()
        .unchecked_into();
        let _ = self.0.parent.add_event_listener_with_callback("error", &forward);
        *self.0.forward.borrow_mut() = Some(forward);
    }

    /// Call `f` for each error reported by this root's components: an `update()` from
    /// `render` or before the first render committed, an update loop, a
    /// [`Handle::report_error`](crate::Handle::report_error), or a bubbling DOM `error`
    /// event (`root.addEventListener('error', ...)`). Roots that frames create report to
    /// the app instead.
    pub fn on_error(&self, f: impl Fn(&crate::AppError) + 'static) -> crate::ListenerId {
        match &self.0.errors {
            Some(errors) => errors.add_listener(f),
            None => crate::app::add_listener(f),
        }
    }

    pub fn remove_error_listener(&self, id: crate::ListenerId) {
        match &self.0.errors {
            Some(errors) => errors.remove_listener(id),
            None => crate::app::remove_listener(id),
        }
    }

    /// Stop managing the content (its components are released, the DOM is left in place).
    pub(crate) fn release(&self) {
        if let Some(inst) = self.0.inst.borrow_mut().take() {
            super::vdom::unmount_root(&*inst);
        }
        self.0.current.borrow_mut().take();
    }

    /// Render a tree, reusing DOM and component instances from the previous render.
    pub fn render(&self, node: impl IntoNode) {
        let children = Children::from(node.into_node());
        *self.0.current.borrow_mut() = Some(children.clone());
        let existing = self.0.inst.borrow().clone();
        match existing {
            Some(inst) => {
                let host = inst.as_any().downcast_ref::<Instance<RootHost>>().expect("root host");
                *host.props.borrow_mut() = children;
                let focus = super::document_state::DocumentState::capture();
                rerender(&*inst);
                focus.restore();
            }
            None => {
                if self.0.errors.is_some() {
                    self.attach_error_forwarding();
                }
                let scope = Scope { frame: self.0.frame.clone(), errors: self.0.errors.clone(), ..Scope::default() };
                let inst = super::new_instance::<RootHost>(children, &scope);
                let state = &self.0;
                let mut cursor = match &state.start {
                    Some(start) => start.next_sibling(),
                    None => state.parent.first_child(),
                };
                hydrate_instance(&*inst, &state.parent, &mut cursor, state.end.as_ref());
                // Drop leftover content the render did not claim.
                while let Some(node) = cursor.take() {
                    if state.end.as_ref().is_some_and(|end| end.is_same_node(Some(&node))) {
                        break;
                    }
                    cursor = node.next_sibling();
                    let _ = state.parent.remove_child(&node);
                }
                *state.inst.borrow_mut() = Some(inst);
            }
        }
    }

    /// Render the most recent tree again (after outside state it reads has changed).
    pub fn reconcile(&self) {
        let current = self.0.current.borrow().clone();
        if let Some(children) = current {
            self.render(children);
        }
    }

    /// Remove the rendered tree and release its components. The root stops forwarding
    /// DOM error events until it renders again.
    pub fn dispose(&self) {
        if let Some(forward) = self.0.forward.borrow_mut().take() {
            let _ = self.0.parent.remove_event_listener_with_callback("error", &forward);
        }
        if let Some(inst) = self.0.inst.borrow_mut().take() {
            remove_root(&*inst);
        }
        self.0.current.borrow_mut().take();
    }

    /// Run pending renders and queued tasks now instead of on the next microtask.
    pub fn flush(&self) {
        flush();
    }
}

/// The component at the top of a root: renders whatever the root was given.
struct RootHost;

impl Component for RootHost {
    type Props = Children;

    fn setup(_: &Children, _: &Handle<Self>) -> Self {
        RootHost
    }

    fn render(&self, props: &Children, _: &Handle<Self>) -> Node {
        props.into_node()
    }
}

/// The message of a thrown value: an `Error`'s `message`, or a string.
fn error_message(value: &JsValue) -> Option<String> {
    if let Some(e) = value.dyn_ref::<js_sys::Error>() {
        return Some(e.message().into());
    }
    value.as_string()
}

//! Standalone roots (Remix's `createRoot` and `createRangeRoot`): render a node tree into
//! a container element or between two sibling nodes, without `run()` or server markers.

use std::cell::RefCell;
use std::rc::Rc;

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
}

/// A root that renders into `container`. The first render hydrates existing content (for
/// example server HTML) and replaces what does not match.
pub fn create_root(container: &web_sys::Element) -> Root {
    super::styles::adopt_server_styles(container.as_ref());
    Root::in_container(container.clone().into(), None)
}

/// A root that owns the nodes between `start` and `end` (exclusive), which must be
/// siblings. Useful for rendering into part of an element, between two comments.
pub fn create_range_root(start: &web_sys::Node, end: &web_sys::Node) -> Root {
    Root::in_range(start, end, None)
}

impl Root {
    pub(crate) fn in_container(parent: web_sys::Node, frame: Option<Rc<FrameState>>) -> Root {
        Root(Rc::new(RootState { parent, frame, start: None, end: None, inst: RefCell::new(None), current: RefCell::new(None) }))
    }

    pub(crate) fn in_range(start: &web_sys::Node, end: &web_sys::Node, frame: Option<Rc<FrameState>>) -> Root {
        let parent = end.parent_node().expect("range root boundaries need a parent");
        Root(Rc::new(RootState {
            parent,
            frame,
            start: Some(start.clone()),
            end: Some(end.clone()),
            inst: RefCell::new(None),
            current: RefCell::new(None),
        }))
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
                rerender(&*inst);
            }
            None => {
                let scope = Scope { frame: self.0.frame.clone(), ..Scope::default() };
                let inst = super::new_instance::<RootHost>(children, &scope);
                let state = &self.0;
                let mut cursor = match &state.start {
                    Some(start) => start.next_sibling(),
                    None => state.parent.first_child(),
                };
                hydrate_instance(&*inst, &state.parent, &mut cursor);
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

    /// Remove the rendered tree and release its components.
    pub fn dispose(&self) {
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

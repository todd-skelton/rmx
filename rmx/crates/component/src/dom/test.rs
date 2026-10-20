//! Component testing helpers (Remix's `remix/component/test`): mount a node tree into a
//! container in the page, query it, and flush updates between steps. Use them from
//! `wasm-bindgen-test` tests that run in a browser.
//!
//! ```ignore
//! use rmx::test::render;
//!
//! #[wasm_bindgen_test]
//! fn increments() {
//!     let t = render(rsx! { <Counter/> });
//!     t.act(|| t.query("[data-action=increment]").unwrap().click());
//!     assert_eq!(t.text("[data-testid=count]"), "1");
//! }
//! ```

use std::cell::RefCell;
use std::future::Future;

use wasm_bindgen::JsCast;

use super::{create_root, document, Root};

/// Focus and selection capture around DOM moves (Remix's `createDocumentState`), for tests.
#[doc(hidden)]
pub use super::document_state::DocumentState;
use crate::node::IntoNode;

/// Options for [`render_with`].
#[derive(Default)]
pub struct RenderOptions {
    /// The element to mount into. Defaults to a new `div` appended to `document.body`.
    pub container: Option<web_sys::HtmlElement>,
}

/// A mounted tree returned by [`render`]. Dropping it cleans up, like calling
/// [`RenderResult::cleanup`].
pub struct RenderResult {
    container: RefCell<Option<web_sys::HtmlElement>>,
    root: RefCell<Option<Root>>,
}

/// Mount `node` into a new `div` appended to `document.body` and flush its first
/// render.
pub fn render(node: impl IntoNode) -> RenderResult {
    render_with(node, RenderOptions::default())
}

/// [`render`] with options, for example to mount into an existing element.
pub fn render_with(node: impl IntoNode, options: RenderOptions) -> RenderResult {
    let container = options.container.unwrap_or_else(|| {
        let div: web_sys::HtmlElement = document().create_element("div").expect("div").unchecked_into();
        document().body().expect("body").append_child(&div).expect("append container");
        div
    });
    let root = create_root(&container);
    root.render(node);
    root.flush();
    RenderResult { container: RefCell::new(Some(container)), root: RefCell::new(Some(root)) }
}

impl RenderResult {
    /// The element the tree is mounted into.
    pub fn container(&self) -> web_sys::HtmlElement {
        self.container.borrow().clone().expect("Test container has already been cleaned up")
    }

    /// The root the tree renders in, for `on_error`, `render` or `flush`.
    pub fn root(&self) -> Root {
        self.root.borrow().clone().expect("Test root has already been cleaned up")
    }

    /// The first matching element (`$(selector)`).
    pub fn query(&self, selector: &str) -> Option<web_sys::HtmlElement> {
        self.container().query_selector(selector).ok().flatten().and_then(|e| e.dyn_into().ok())
    }

    /// Every matching element (`$$(selector)`).
    pub fn query_all(&self, selector: &str) -> Vec<web_sys::HtmlElement> {
        let Ok(list) = self.container().query_selector_all(selector) else { return Vec::new() };
        (0..list.length()).filter_map(|i| list.item(i)?.dyn_into().ok()).collect()
    }

    /// The text content of the first matching element, or `""` when there is none.
    pub fn text(&self, selector: &str) -> String {
        self.query(selector).and_then(|e| e.text_content()).unwrap_or_default()
    }

    /// The container's `innerHTML`.
    pub fn html(&self) -> String {
        self.container().inner_html()
    }

    /// Run `f`, then flush pending updates so the DOM reflects them.
    pub fn act(&self, f: impl FnOnce()) {
        f();
        self.root().flush();
    }

    /// Await `f`, then flush pending updates (`await act(async () => ...)`).
    pub async fn act_async(&self, f: impl Future<Output = ()>) {
        f.await;
        self.root().flush();
    }

    /// Dispose the root and remove the container from the document. Safe to call twice.
    pub fn cleanup(&self) {
        if let Some(root) = self.root.borrow_mut().take() {
            root.dispose();
        }
        if let Some(container) = self.container.borrow_mut().take() {
            container.remove();
        }
    }
}

impl Drop for RenderResult {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Reconcile `container`'s children to `next_html` the way a navigation or frame reload
/// does (Remix's `diffNodes`). For tests of the DOM diff itself.
#[doc(hidden)]
pub fn diff_html(container: &web_sys::Element, next_html: &str) {
    let template: web_sys::HtmlTemplateElement = document().create_element("template").expect("template").unchecked_into();
    template.set_inner_html(next_html);
    let parent: &web_sys::Node = container.as_ref();
    let next = super::frames::child_nodes(template.content().as_ref());
    super::diff::diff_nodes(parent, super::frames::child_nodes(parent), next, None);
}

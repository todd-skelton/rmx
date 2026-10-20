//! `<Frame>`: server content loaded from a URL, streamed into the page and reloadable from
//! the browser without a full navigation.
//!
//! ```ignore
//! rsx! {
//!     <Frame src="/sidebar" fallback={rsx! { <p>"Loading..."</p> }}/>
//!     <Frame name="cart" src="/cart-summary"/>
//! }
//! ```
//!
//! Without a `fallback` the server waits for the frame before sending the page; with one,
//! the fallback is sent first and the content streams in when ready.

use std::rc::Rc;

use crate::component::{Component, Handle};
use crate::node::{FrameNode, Node};

#[derive(Clone, Default)]
pub struct FrameProps {
    /// URL of the frame content (resolved against the page URL).
    pub src: String,
    /// Registers the frame for `h.frames().get(name)` and `data-rmx-target="name"`.
    pub name: Option<String>,
    /// Shown while the content loads; makes the frame non-blocking.
    pub fallback: Option<Node>,
}

pub struct Frame;

impl Component for Frame {
    type Props = FrameProps;

    fn setup(_: &FrameProps, _: &Handle<Self>) -> Self {
        Frame
    }

    fn render(&self, props: &FrameProps, _: &Handle<Self>) -> Node {
        Node::Frame(Rc::new(FrameNode { src: props.src.clone(), name: props.name.clone(), fallback: props.fallback.clone() }))
    }
}

/// Frame lifecycle events, for [`crate::Handle::on_frame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameEvent {
    /// A reload (or navigation into the frame) started.
    ReloadStart,
    /// The new content is in the page.
    ReloadComplete,
}

impl FrameEvent {
    pub fn name(self) -> &'static str {
        match self {
            FrameEvent::ReloadStart => "reloadStart",
            FrameEvent::ReloadComplete => "reloadComplete",
        }
    }
}

/// Options for [`FrameHandle::reload_with`].
#[derive(Clone, Debug, Default)]
pub struct ReloadOptions {
    /// Load this URL instead (it becomes the frame's `src`).
    pub src: Option<String>,
    /// `"get"` (default) or `"post"`.
    pub method: Option<String>,
    /// Form fields to send. For GET they replace the URL query.
    pub body: Option<Vec<(String, String)>>,
}

/// A request for frame content, handed to a custom resolver (see `RunOptions::resolve_frame`).
#[derive(Clone, Debug)]
pub struct FrameRequest {
    /// Absolute URL of the content.
    pub url: String,
    /// `GET`, or the method of the form being submitted.
    pub method: String,
    /// Submitted form fields (URL-encoded on the wire).
    pub form: Option<Vec<(String, String)>>,
    /// The name of the frame asking, if it has one.
    pub target: Option<String>,
}

/// Content a resolver produced: server-style HTML, or a node tree to render directly.
pub enum FrameContent {
    Html(String),
    Node(Node),
}

/// A resolved frame request. `url` is the final URL when the resolver followed redirects.
pub struct FrameResponse {
    pub content: FrameContent,
    pub url: Option<String>,
}

/// Future returned by a frame resolver.
pub type ResolveFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<FrameResponse, String>>>>;

/// Resolves frame content in the browser instead of fetching it from the server (Remix's
/// `resolveFrame`). A newer load of the same frame drops the pending future.
pub type ResolveFrame = Rc<dyn Fn(FrameRequest) -> ResolveFuture>;

/// A frame in the page: the top-level document or a `<Frame>`.
#[derive(Clone)]
pub struct FrameHandle {
    #[cfg(target_arch = "wasm32")]
    pub(crate) frame: Option<Rc<crate::dom::frames::FrameState>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) src: String,
}

impl FrameHandle {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn server(src: String) -> FrameHandle {
        FrameHandle { src }
    }

    /// The frame's current source URL.
    pub fn src(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            self.frame.as_ref().map(|f| f.src()).unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.src.clone()
        }
    }

    /// The frame's name, if it has one.
    pub fn name(&self) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            self.frame.as_ref().and_then(|f| f.name())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// Re-fetch the frame's content and reconcile it into the page. Client entries inside
    /// keep their state and receive new props. No-op on the server.
    pub fn reload(&self) {
        self.reload_with(ReloadOptions::default());
    }

    pub fn reload_with(&self, options: ReloadOptions) {
        #[cfg(target_arch = "wasm32")]
        if let Some(f) = &self.frame {
            let f = f.clone();
            wasm_bindgen_futures::spawn_local(async move {
                f.reload(options).await;
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = options;
    }

    /// Show `node` as the frame's content now, without a request (Remix's `frame.replace`).
    pub fn replace(&self, node: Node) {
        #[cfg(target_arch = "wasm32")]
        if let Some(f) = &self.frame {
            f.apply_node(node);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = node;
    }

    /// Like [`FrameHandle::reload_with`], resolving once the new content is in the page.
    pub async fn reload_async(&self, options: ReloadOptions) {
        #[cfg(target_arch = "wasm32")]
        if let Some(f) = &self.frame {
            f.clone().reload(options).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = options;
    }
}

/// Lookup of frames in the page (`handle.frames` in Remix).
pub struct Frames;

impl Frames {
    /// The top frame (the document).
    pub fn top(&self) -> FrameHandle {
        #[cfg(target_arch = "wasm32")]
        {
            FrameHandle { frame: Some(crate::dom::frames::top()) }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            FrameHandle::server(crate::component::server_frame_src().1)
        }
    }

    /// A mounted frame by name (the most recently mounted one wins).
    pub fn get(&self, name: &str) -> Option<FrameHandle> {
        #[cfg(target_arch = "wasm32")]
        {
            crate::dom::frames::by_name(name).map(|f| FrameHandle { frame: Some(f) })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = name;
            None
        }
    }
}

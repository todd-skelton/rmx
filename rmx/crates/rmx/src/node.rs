// Parts of the instance and node types are only read by the browser runtime.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::borrow::Cow;
use std::rc::Rc;

use crate::component::ErasedComponent;

/// Text that is usually a `&'static str` from the template and only allocates when dynamic.
pub type Str = Cow<'static, str>;

/// A UI description produced by `rsx!` and returned from `Component::render`.
#[derive(Clone)]
pub enum Node {
    Element(Box<Element>),
    Text(Str),
    Fragment(Vec<Node>),
    Component(ComponentNode),
    /// A subtree shared by reference, used for `children` passed down through props.
    Shared(Rc<Node>),
    /// Trusted HTML written as-is by the server renderer (doctype, inline CSS).
    Raw(Str),
    Empty,
}

#[derive(Clone)]
pub struct Element {
    pub tag: &'static str,
    pub key: Option<Key>,
    pub attrs: Vec<(&'static str, AttrValue)>,
    pub events: Vec<(&'static str, EventHandler)>,
    pub children: Vec<Node>,
}

#[derive(Clone)]
pub struct ComponentNode {
    pub(crate) key: Option<Key>,
    pub(crate) inner: Box<dyn ErasedComponent>,
}

impl Node {
    #[doc(hidden)]
    pub fn element(
        tag: &'static str,
        key: Option<Key>,
        attrs: Vec<(&'static str, AttrValue)>,
        events: Vec<(&'static str, EventHandler)>,
        children: Vec<Node>,
    ) -> Node {
        Node::Element(Box::new(Element { tag, key, attrs, events, children }))
    }

    /// Trusted, unescaped HTML. Only the server renderer writes it.
    pub fn raw(html: impl Into<Str>) -> Node {
        Node::Raw(html.into())
    }

    pub(crate) fn key(&self) -> Option<&Key> {
        match self {
            Node::Element(el) => el.key.as_ref(),
            Node::Component(c) => c.key.as_ref(),
            _ => None,
        }
    }
}

/// Identity for list items so reordering moves DOM nodes instead of rewriting them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Int(i64),
    Str(Str),
}

macro_rules! int_key {
    ($($t:ty),*) => {$(
        impl From<$t> for Key {
            fn from(v: $t) -> Key { Key::Int(v as i64) }
        }
    )*};
}
int_key!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl From<&'static str> for Key {
    fn from(v: &'static str) -> Key {
        Key::Str(Cow::Borrowed(v))
    }
}

impl From<String> for Key {
    fn from(v: String) -> Key {
        Key::Str(Cow::Owned(v))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue {
    Str(Str),
    /// `true` renders a bare boolean attribute; `false` omits it.
    Bool(bool),
    /// Omit the attribute (for example `class={None}`).
    Absent,
}

/// Values usable as attribute values in `rsx!`.
pub trait IntoAttr {
    fn into_attr(self) -> AttrValue;
}

impl IntoAttr for AttrValue {
    fn into_attr(self) -> AttrValue {
        self
    }
}

impl IntoAttr for &'static str {
    fn into_attr(self) -> AttrValue {
        AttrValue::Str(Cow::Borrowed(self))
    }
}

impl IntoAttr for String {
    fn into_attr(self) -> AttrValue {
        AttrValue::Str(Cow::Owned(self))
    }
}

impl IntoAttr for Str {
    fn into_attr(self) -> AttrValue {
        AttrValue::Str(self)
    }
}

impl IntoAttr for bool {
    fn into_attr(self) -> AttrValue {
        AttrValue::Bool(self)
    }
}

impl<T: IntoAttr> IntoAttr for Option<T> {
    fn into_attr(self) -> AttrValue {
        self.map_or(AttrValue::Absent, IntoAttr::into_attr)
    }
}

macro_rules! display_attr_and_node {
    ($($t:ty),*) => {$(
        impl IntoAttr for $t {
            fn into_attr(self) -> AttrValue { AttrValue::Str(Cow::Owned(self.to_string())) }
        }
        impl IntoNode for $t {
            fn into_node(self) -> Node { Node::Text(Cow::Owned(self.to_string())) }
        }
    )*};
}
display_attr_and_node!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64, char);

/// Values usable as children in `rsx!` (`{expr}` blocks).
pub trait IntoNode {
    fn into_node(self) -> Node;
}

impl IntoNode for Node {
    fn into_node(self) -> Node {
        self
    }
}

impl IntoNode for &'static str {
    fn into_node(self) -> Node {
        Node::Text(Cow::Borrowed(self))
    }
}

impl IntoNode for String {
    fn into_node(self) -> Node {
        Node::Text(Cow::Owned(self))
    }
}

impl IntoNode for &String {
    fn into_node(self) -> Node {
        Node::Text(Cow::Owned(self.clone()))
    }
}

impl IntoNode for Str {
    fn into_node(self) -> Node {
        Node::Text(self)
    }
}

impl IntoNode for () {
    fn into_node(self) -> Node {
        Node::Empty
    }
}

impl<T: IntoNode> IntoNode for Option<T> {
    fn into_node(self) -> Node {
        self.map_or(Node::Empty, IntoNode::into_node)
    }
}

impl<T: IntoNode> IntoNode for Vec<T> {
    fn into_node(self) -> Node {
        Node::Fragment(self.into_iter().map(IntoNode::into_node).collect())
    }
}

/// `children` passed to a component. Cloning is a reference-count bump, so a component can
/// place `{&props.children}` in its output without copying the subtree.
#[derive(Clone)]
pub struct Children(Rc<Node>);

impl Default for Children {
    fn default() -> Self {
        Children(Rc::new(Node::Empty))
    }
}

impl From<Node> for Children {
    fn from(node: Node) -> Self {
        Children(Rc::new(node))
    }
}

impl IntoNode for &Children {
    fn into_node(self) -> Node {
        Node::Shared(self.0.clone())
    }
}

impl IntoNode for Children {
    fn into_node(self) -> Node {
        Node::Shared(self.0)
    }
}

/// A DOM event delivered to a handler. Server builds never construct one.
pub struct Event {
    #[cfg(target_arch = "wasm32")]
    pub(crate) raw: web_sys::Event,
}

impl Event {
    /// The underlying browser event.
    #[cfg(target_arch = "wasm32")]
    pub fn raw(&self) -> &web_sys::Event {
        &self.raw
    }
}

/// An event listener created with [`crate::Handle::on`].
#[derive(Clone)]
pub struct EventHandler(pub(crate) Rc<dyn Fn(&Event)>);

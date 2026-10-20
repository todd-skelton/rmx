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
    /// Server content loaded from a URL (see [`crate::Frame`]).
    Frame(Rc<FrameNode>),
    Empty,
}

#[derive(Clone)]
pub struct Element {
    pub tag: &'static str,
    pub key: Option<Key>,
    pub attrs: Vec<(&'static str, AttrValue)>,
    pub events: Vec<(&'static str, EventHandler)>,
    pub children: Vec<Node>,
    /// Mixin side effects that cannot be expressed as attributes.
    pub(crate) extra: Option<Box<ElementExtra>>,
}

/// Generated styles to insert and node callbacks to run after mount.
#[derive(Clone, Default)]
pub(crate) struct ElementExtra {
    pub(crate) styles: Vec<Rc<StyleRule>>,
    pub(crate) refs: Vec<crate::mixin::NodeRef>,
}

/// A `css()` rule: a generated class name and the CSS text scoped under it.
#[derive(Debug, PartialEq, Eq)]
pub struct StyleRule {
    pub class: String,
    pub css: Str,
}

/// A `<Frame>` placeholder in the tree.
#[derive(Clone)]
pub struct FrameNode {
    pub src: String,
    pub name: Option<String>,
    pub fallback: Option<Node>,
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
        Node::Element(Box::new(Element { tag, key, attrs, events, children, extra: None }))
    }

    /// An element with mixins applied (`mix={...}` in `rsx!`). Mixins run in order, after
    /// the element's own attributes.
    #[doc(hidden)]
    pub fn element_with_mixins(
        tag: &'static str,
        key: Option<Key>,
        attrs: Vec<(&'static str, AttrValue)>,
        events: Vec<(&'static str, EventHandler)>,
        children: Vec<Node>,
        mixins: Vec<crate::Mixin>,
    ) -> Node {
        let mut el = Element { tag, key, attrs, events, children, extra: None };
        for m in mixins {
            m.apply(&mut el);
        }
        Node::Element(Box::new(el))
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

impl Children {
    /// The children as a node.
    pub fn node(&self) -> &Node {
        &self.0
    }
}

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
    /// The element whose handler is running (events are delegated, so the browser's
    /// `currentTarget` is the document).
    #[cfg(target_arch = "wasm32")]
    pub(crate) current: Option<web_sys::Node>,
}

impl Event {
    /// The underlying browser event.
    #[cfg(target_arch = "wasm32")]
    pub fn raw(&self) -> &web_sys::Event {
        &self.raw
    }

    /// The element whose handler is running.
    #[cfg(target_arch = "wasm32")]
    pub fn current_element(&self) -> Option<web_sys::Element> {
        use wasm_bindgen::JsCast;
        self.current.clone()?.dyn_into().ok()
    }

    pub fn prevent_default(&self) {
        #[cfg(target_arch = "wasm32")]
        self.raw.prevent_default();
    }

    pub fn stop_propagation(&self) {
        #[cfg(target_arch = "wasm32")]
        self.raw.stop_propagation();
    }

    /// `event.target.value` for inputs, selects and textareas (empty otherwise).
    pub fn target_value(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            self.raw
                .target()
                .and_then(|t| js_sys::Reflect::get(&t, &"value".into()).ok())
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }

    /// `event.target.checked` for checkboxes and radios.
    pub fn target_checked(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.raw
                .target()
                .and_then(|t| js_sys::Reflect::get(&t, &"checked".into()).ok())
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// `event.key` for keyboard events.
    pub fn key(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Reflect::get(&self.raw, &"key".into()).ok().and_then(|v| v.as_string()).unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }

    /// The text fields of the form this event belongs to (the target, or the target's
    /// form), as `new FormData(form)` would collect them.
    pub fn form_fields(&self) -> Vec<(String, String)> {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let Some(target) = self.raw.target() else { return Vec::new() };
            let form = if js_sys::Reflect::get(&target, &"elements".into()).is_ok_and(|v| !v.is_undefined()) {
                target.unchecked_into::<web_sys::HtmlFormElement>()
            } else {
                match js_sys::Reflect::get(&target, &"form".into()).ok().and_then(|f| f.dyn_into().ok()) {
                    Some(f) => f,
                    None => return Vec::new(),
                }
            };
            let Ok(data) = web_sys::FormData::new_with_form(&form) else { return Vec::new() };
            let mut out = Vec::new();
            if let Ok(iter) = js_sys::try_iter(&data) {
                for pair in iter.into_iter().flatten().flatten() {
                    let pair: js_sys::Array = pair.unchecked_into();
                    if let (Some(k), Some(v)) = (pair.get(0).as_string(), pair.get(1).as_string()) {
                        out.push((k, v));
                    }
                }
            }
            out
        }
        #[cfg(not(target_arch = "wasm32"))]
        Vec::new()
    }

    /// A boolean property of the event (`metaKey`, `repeat`, ...).
    pub fn prop_bool(&self, name: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Reflect::get(&self.raw, &name.into()).ok().and_then(|v| v.as_bool()).unwrap_or(false)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = name;
            false
        }
    }

    /// A numeric property of the event (`button`, `clientX`, ...).
    pub fn prop_f64(&self, name: &str) -> Option<f64> {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Reflect::get(&self.raw, &name.into()).ok().and_then(|v| v.as_f64())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = name;
            None
        }
    }

    /// An attribute of the element the handler is attached to (`currentTarget`).
    pub fn current_attr(&self, name: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            self.current.clone()?.dyn_into::<web_sys::Element>().ok()?.get_attribute(name)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = name;
            None
        }
    }

    /// Submit the form the event target belongs to, as if its submit button was pressed
    /// (`form.requestSubmit()`), so the navigation runtime handles it.
    pub fn request_submit(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let Some(target) = self.raw.target() else { return };
            let form = if js_sys::Reflect::get(&target, &"elements".into()).is_ok_and(|v| !v.is_undefined()) {
                Some(target.unchecked_into::<web_sys::HtmlFormElement>())
            } else {
                js_sys::Reflect::get(&target, &"form".into()).ok().and_then(|f| f.dyn_into().ok())
            };
            if let Some(form) = form {
                let _ = form.request_submit();
            }
        }
    }

    /// The first form field with this name.
    pub fn form_value(&self, name: &str) -> Option<String> {
        self.form_fields().into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// An event listener created with [`crate::Handle::on`] or [`EventHandler::new`].
#[derive(Clone)]
pub struct EventHandler(pub(crate) Rc<dyn Fn(&Event)>);

impl EventHandler {
    /// A handler that does not touch component state (use [`crate::Handle::on`] for that).
    pub fn new(f: impl Fn(&Event) + 'static) -> EventHandler {
        #[cfg(target_arch = "wasm32")]
        {
            EventHandler(Rc::new(f))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            drop(f);
            EventHandler(Rc::new(|_| {}))
        }
    }
}

/// Conversions applied to component prop values in `rsx!`: identity, `T` into `Option<T>`,
/// and string literals into `String`.
pub trait IntoProp<T> {
    fn into_prop(self) -> T;
}

impl<T> IntoProp<T> for T {
    fn into_prop(self) -> T {
        self
    }
}

impl<T> IntoProp<Option<T>> for T {
    fn into_prop(self) -> Option<T> {
        Some(self)
    }
}

impl IntoProp<String> for &str {
    fn into_prop(self) -> String {
        self.to_string()
    }
}

impl IntoProp<Option<String>> for &str {
    fn into_prop(self) -> Option<String> {
        Some(self.to_string())
    }
}

impl IntoProp<Str> for &'static str {
    fn into_prop(self) -> Str {
        Cow::Borrowed(self)
    }
}

impl IntoProp<Children> for Node {
    fn into_prop(self) -> Children {
        Children::from(self)
    }
}

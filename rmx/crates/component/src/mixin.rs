//! Mixins: reusable behavior and props applied to a host element with `mix={...}`.
//!
//! Built-ins mirror Remix 3:
//!
//! - [`css`] generates a scoped class from CSS text (nesting like `&:hover { .. }` and
//!   `@media` blocks work, since rules use native CSS nesting).
//! - [`on`] attaches an event handler.
//! - [`node_ref`] hands the DOM element to a callback after it is inserted (browser only).
//! - [`attrs`] supplies attribute defaults.
//! - [`Mixin::new`] builds a custom mixin from a function over the host element.
//!
//! ```ignore
//! rsx! { <button mix={[css("color: white; &:hover { color: gold }"), on("click", handler)]}>"Go"</button> }
//! ```

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use crate::node::{AttrValue, Element, ElementExtra, EventHandler, IntoAttr, StyleRule};

/// A host-element transformation applied by `mix={...}`.
#[derive(Clone)]
pub struct Mixin(MixinKind);

#[derive(Clone)]
enum MixinKind {
    Style(Rc<StyleRule>),
    Event(&'static str, EventHandler),
    Ref(NodeRef),
    Attrs(Vec<(&'static str, AttrValue)>),
    Custom(Rc<dyn Fn(&mut Element)>),
    Many(Vec<Mixin>),
}

/// A callback that receives the host element once it is in the document.
#[derive(Clone)]
pub struct NodeRef(#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] pub(crate) Rc<dyn Fn(&NodeHandle)>);

/// The DOM element a [`node_ref`] callback receives. On the server it is never created.
pub struct NodeHandle {
    #[cfg(target_arch = "wasm32")]
    pub(crate) el: web_sys::Element,
}

impl NodeHandle {
    /// The underlying DOM element.
    #[cfg(target_arch = "wasm32")]
    pub fn element(&self) -> &web_sys::Element {
        &self.el
    }
}

impl Mixin {
    /// A custom mixin: a function that adjusts the host element (attributes, events, other
    /// mixins). Runs during both server and browser rendering.
    pub fn new(f: impl Fn(&mut Element) + 'static) -> Mixin {
        Mixin(MixinKind::Custom(Rc::new(f)))
    }

    pub(crate) fn apply(self, el: &mut Element) {
        match self.0 {
            MixinKind::Style(rule) => {
                add_class(el, &rule.class);
                el.extra.get_or_insert_with(Default::default).styles.push(rule);
            }
            MixinKind::Event(name, handler) => el.events.push((name, handler)),
            MixinKind::Ref(r) => el.extra.get_or_insert_with(Box::<ElementExtra>::default).refs.push(r),
            MixinKind::Attrs(defaults) => {
                for (name, value) in defaults {
                    if !el.attrs.iter().any(|(n, _)| *n == name) {
                        el.attrs.push((name, value));
                    }
                }
            }
            MixinKind::Custom(f) => f(el),
            MixinKind::Many(list) => list.into_iter().for_each(|m| m.apply(el)),
        }
    }
}

impl Element {
    /// Set (or replace) an attribute. For use in custom mixins.
    pub fn set_attr(&mut self, name: &'static str, value: impl IntoAttr) {
        let value = value.into_attr();
        match self.attrs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => *v = value,
            None => self.attrs.push((name, value)),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&AttrValue> {
        self.attrs.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// Add an event handler. For use in custom mixins.
    pub fn add_event(&mut self, name: &'static str, handler: EventHandler) {
        self.events.push((name, handler));
    }

    /// Apply another mixin (composition).
    pub fn mix(&mut self, mixin: impl IntoMixins) {
        for m in mixin.into_mixins() {
            m.apply(self);
        }
    }
}

fn add_class(el: &mut Element, class: &str) {
    match el.attrs.iter_mut().find(|(n, _)| *n == "class") {
        Some((_, AttrValue::Str(existing))) if !existing.is_empty() => {
            *existing = Cow::Owned(format!("{existing} {class}"));
        }
        Some((_, v)) => *v = AttrValue::Str(Cow::Owned(class.to_string())),
        None => el.attrs.push(("class", AttrValue::Str(Cow::Owned(class.to_string())))),
    }
}

/// Values accepted by `mix={...}`: a mixin, an array or `Vec` of them, or an `Option`.
pub trait IntoMixins {
    fn into_mixins(self) -> Vec<Mixin>;
}

impl IntoMixins for Mixin {
    fn into_mixins(self) -> Vec<Mixin> {
        vec![self]
    }
}

impl IntoMixins for &Mixin {
    fn into_mixins(self) -> Vec<Mixin> {
        vec![self.clone()]
    }
}

impl<T: IntoMixins> IntoMixins for Option<T> {
    fn into_mixins(self) -> Vec<Mixin> {
        self.map_or_else(Vec::new, IntoMixins::into_mixins)
    }
}

impl<T: IntoMixins> IntoMixins for Vec<T> {
    fn into_mixins(self) -> Vec<Mixin> {
        self.into_iter().flat_map(IntoMixins::into_mixins).collect()
    }
}

impl<T: IntoMixins, const N: usize> IntoMixins for [T; N] {
    fn into_mixins(self) -> Vec<Mixin> {
        self.into_iter().flat_map(IntoMixins::into_mixins).collect()
    }
}

impl From<Vec<Mixin>> for Mixin {
    fn from(list: Vec<Mixin>) -> Mixin {
        Mixin(MixinKind::Many(list))
    }
}

/// Scoped styles from CSS declarations. The same text always yields the same class, and
/// rules are cached per thread, so calling `css` in `render` is cheap.
///
/// ```ignore
/// css("padding: 12px 24px; border-radius: 4px; &:hover { background: navy } @media (width < 600px) { padding: 8px }")
/// ```
pub fn css(text: &str) -> Mixin {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, Rc<StyleRule>>> = std::cell::RefCell::new(HashMap::new());
    }
    let rule = CACHE.with(|c| {
        if let Some(rule) = c.borrow().get(text) {
            return rule.clone();
        }
        let normalized = normalize_css(text);
        let class = format!("rmxc-{}", hash36(&normalized));
        let rule = Rc::new(StyleRule { class, css: Cow::Owned(normalized) });
        c.borrow_mut().insert(text.to_string(), rule.clone());
        rule
    });
    Mixin(MixinKind::Style(rule))
}

/// Attach an event listener (`on('click', ...)` in Remix).
pub fn on(event: &'static str, handler: EventHandler) -> Mixin {
    Mixin(MixinKind::Event(event, handler))
}

/// Run `f` with the DOM element after it is inserted (browser only).
pub fn node_ref(f: impl Fn(&NodeHandle) + 'static) -> Mixin {
    Mixin(MixinKind::Ref(NodeRef(Rc::new(f))))
}

/// Attribute defaults, used only where the element does not set the attribute itself.
pub fn attrs(defaults: Vec<(&'static str, AttrValue)>) -> Mixin {
    Mixin(MixinKind::Attrs(defaults))
}

/// Collapse whitespace so formatting differences do not create new classes.
fn normalize_css(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// FNV-1a, printed in base 36: short, stable class names.
fn hash36(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let mut n = h;
    let mut out = Vec::new();
    while n > 0 && out.len() < 8 {
        let d = (n % 36) as u8;
        out.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
        n /= 36;
    }
    String::from_utf8(out).expect("ascii")
}

/// The full CSS text for a rule, wrapped in its cascade layer:
/// `@layer rmx.rmxc-x { .rmxc-x { ... } }`.
pub(crate) fn rule_css(rule: &StyleRule) -> String {
    format!("@layer rmx.{0} {{ .{0} {{ {1} }} }}", rule.class, rule.css)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    #[test]
    fn css_is_stable_and_merges_classes() {
        let a = css("color:  red;\n padding: 4px");
        let b = css("color: red; padding: 4px");
        let (MixinKind::Style(ra), MixinKind::Style(rb)) = (&a.0, &b.0) else { panic!() };
        assert_eq!(ra.class, rb.class);
        let ra = ra.clone();
        let node = Node::element_with_mixins("p", None, vec![("class", AttrValue::Str("lead".into()))], vec![], vec![], vec![a, attrs(vec![("title", AttrValue::Str("t".into()))])]);
        let Node::Element(el) = node else { panic!() };
        assert_eq!(el.attr("class"), Some(&AttrValue::Str(format!("lead {}", ra.class).into())));
        assert_eq!(el.attr("title"), Some(&AttrValue::Str("t".into())));
        assert_eq!(rule_css(&ra), format!("@layer rmx.{0} {{ .{0} {{ color: red; padding: 4px }} }}", ra.class));
    }
}

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

use crate::host_mixin::{HostMixin, MixinHandle};
use crate::node::{AttrValue, Element, EventHandler, IntoAttr, StyleRule};
use crate::signal::Signal;

/// A host-element transformation applied by `mix={...}`.
#[derive(Clone)]
pub struct Mixin(MixinKind);

#[derive(Clone)]
enum MixinKind {
    Style(Rc<StyleRule>),
    Event(&'static str, EventHandler),
    Attrs(Vec<(&'static str, AttrValue)>),
    Custom(Rc<dyn Fn(&mut Element)>),
    Many(Vec<Mixin>),
    Stateful(Rc<dyn crate::host_mixin::ErasedMixin>),
}

/// A callback that receives the host element once it is in the document, and a signal
/// that aborts when the element is removed.
#[derive(Clone)]
pub struct NodeRef(#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] pub(crate) Rc<RefCallback>);

type RefCallback = dyn Fn(&NodeHandle, &Signal);

/// `ref` as a browser-only stateful mixin: the callback runs once per insertion, not on
/// every render, and its signal aborts on removal.
struct RefMixin {
    callback: NodeRef,
}

impl HostMixin for RefMixin {
    type Args = NodeRef;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|this, h, event| {
            #[cfg(target_arch = "wasm32")]
            {
                let signal = Signal::from_raw(h.signal());
                (this.callback.0)(&event.node, &signal);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, h, event);
        });
        RefMixin { callback: NodeRef(Rc::new(|_, _| {})) }
    }

    fn render(&mut self, callback: &NodeRef, _: &mut Element, _: &MixinHandle<Self>) {
        self.callback = callback.clone();
    }
}

/// The DOM element a [`node_ref`] callback receives. On the server it is never created.
#[derive(Clone)]
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

    #[cfg(target_arch = "wasm32")]
    fn call(&self, method: &str) {
        use wasm_bindgen::JsCast;
        if let Ok(f) = js_sys::Reflect::get(&self.el, &method.into()).and_then(|f| f.dyn_into::<js_sys::Function>()) {
            let _ = f.call0(&self.el);
        }
    }

    /// The lowercase tag name (`"button"`); empty on the server.
    pub fn tag_name(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            self.el.tag_name().to_ascii_lowercase()
        }
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }

    pub fn focus(&self) {
        #[cfg(target_arch = "wasm32")]
        self.call("focus");
    }

    /// `scrollIntoView({ behavior: 'smooth', block: 'start' })`
    pub fn scroll_into_view(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            let options = web_sys::ScrollIntoViewOptions::new();
            options.set_behavior(web_sys::ScrollBehavior::Smooth);
            options.set_block(web_sys::ScrollLogicalPosition::Start);
            self.el.scroll_into_view_with_scroll_into_view_options(&options);
        }
    }

    /// Select the text of an input or textarea.
    pub fn select(&self) {
        #[cfg(target_arch = "wasm32")]
        self.call("select");
    }

    /// The `value` property of a form control.
    pub fn value(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Reflect::get(&self.el, &"value".into()).ok().and_then(|v| v.as_string()).unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }

    pub fn set_value(&self, value: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = js_sys::Reflect::set(&self.el, &"value".into(), &value.into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = value;
    }
}

impl Mixin {
    /// A custom mixin: a function that adjusts the host element (attributes, events, other
    /// mixins). Runs during both server and browser rendering.
    pub fn new(f: impl Fn(&mut Element) + 'static) -> Mixin {
        Mixin(MixinKind::Custom(Rc::new(f)))
    }

    pub(crate) fn stateful(descriptor: Rc<dyn crate::host_mixin::ErasedMixin>) -> Mixin {
        Mixin(MixinKind::Stateful(descriptor))
    }

    pub(crate) fn apply(self, el: &mut Element) {
        match self.0 {
            MixinKind::Style(rule) => {
                add_class(el, &rule.class);
                el.extra.get_or_insert_with(Default::default).styles.push(rule);
            }
            MixinKind::Event(name, handler) => el.events.push((name, handler)),
            MixinKind::Attrs(defaults) => {
                for (name, value) in defaults {
                    if !el.attrs.iter().any(|(n, _)| *n == name) {
                        el.attrs.push((name, value));
                    }
                }
            }
            MixinKind::Custom(f) => f(el),
            MixinKind::Many(list) => list.into_iter().for_each(|m| m.apply(el)),
            MixinKind::Stateful(d) => el.extra.get_or_insert_with(Default::default).mixins.push(d),
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

/// What a host keeps from its mixins: its children and raw HTML props. Remix runs mixins
/// without them and drops any a mixin tries to add, so a mixin cannot replace content.
pub(crate) struct Withheld {
    children: Vec<crate::Node>,
    raw: Vec<(&'static str, AttrValue)>,
}

/// Take the children and raw HTML props out of `el` before its mixins run.
pub(crate) fn withhold(el: &mut Element) -> Withheld {
    let children = std::mem::take(&mut el.children);
    let raw = if el.attrs.iter().any(|(n, _)| crate::attrs::is_html_prop(n)) {
        el.attrs.extract_if(.., |(n, _)| crate::attrs::is_html_prop(n)).collect()
    } else {
        Vec::new()
    };
    Withheld { children, raw }
}

/// Put back what [`withhold`] took, dropping children and raw HTML props the mixins added.
pub(crate) fn restore(el: &mut Element, held: Withheld) {
    if !el.children.is_empty() {
        el.children.clear();
        report_mixin_error("mixin elements must not receive children");
    }
    if el.attrs.iter().any(|(n, _)| crate::attrs::is_html_prop(n) || *n == "outerHTML") {
        el.attrs.retain(|(n, _)| !crate::attrs::is_html_prop(n) && *n != "outerHTML");
        report_mixin_error("mixins must not return children or raw HTML props");
    }
    el.children = held.children;
    el.attrs.extend(held.raw);
}

#[cold]
fn report_mixin_error(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(&js_sys::Error::new(message));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("rmx: {message}");
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

/// A component's `mix` prop (`mix: Vec<Mixin>` in its props) takes one mixin or an array,
/// like a host element's `mix`, so the component can pass it on to the element it renders.
impl crate::IntoProp<Vec<Mixin>> for Mixin {
    fn into_prop(self) -> Vec<Mixin> {
        vec![self]
    }
}

impl<const N: usize> crate::IntoProp<Vec<Mixin>> for [Mixin; N] {
    fn into_prop(self) -> Vec<Mixin> {
        self.into()
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

/// Attach a capture-phase listener (`on('keydown', ..., true)` in Remix): it runs on the
/// way down, before listeners on the target and its descendants.
pub fn on_capture(event: &'static str, handler: EventHandler) -> Mixin {
    Mixin(MixinKind::Event(event, EventHandler(handler.0, true)))
}

/// Run `f` with the DOM element after it is inserted (Remix's `ref`, browser only). The
/// signal aborts when the element is removed: clean up observers and listeners with it.
pub fn node_ref(f: impl Fn(&NodeHandle, &Signal) + 'static) -> Mixin {
    crate::host_mixin::mixin::<RefMixin>(NodeRef(Rc::new(f)))
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
    format!("@layer rmx.{} {{ {} }}", rule.class, rule_body(rule))
}

/// The rule without its layer: `.rmxc-x { ... }`. Top-level at-rules that cannot be
/// nested in a style rule (`@keyframes`, `@font-face`, `@property`, `@function`,
/// `@counter-style`) are moved in front of it, as Remix does for its style objects.
pub(crate) fn rule_body(rule: &StyleRule) -> String {
    let (prelude, rest) = split_prelude(&rule.css);
    let body = format!(".{} {{ {} }}", rule.class, rest.trim());
    if prelude.is_empty() {
        body
    } else {
        format!("{} {body}", prelude.join(" "))
    }
}

fn hoisted_at_rule(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let name = name.trim_start_matches('@');
    let name = ["-webkit-", "-moz-", "-o-"].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name);
    matches!(name, "keyframes" | "font-face" | "property" | "function" | "counter-style")
}

/// Split CSS text into the hoisted top-level at-rule blocks and everything else.
fn split_prelude(css: &str) -> (Vec<&str>, String) {
    let bytes = css.as_bytes();
    let (mut prelude, mut rest) = (Vec::new(), String::with_capacity(css.len()));
    let (mut i, mut kept) = (0, 0);
    let mut depth = 0u32;
    let mut quote: Option<u8> = None;
    let mut statement_start = true;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 1;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                statement_start = depth == 0;
                i += 1;
                continue;
            }
            b';' if depth == 0 => statement_start = true,
            b'@' if depth == 0 && statement_start => {
                let name_end = css[i..].find(|c: char| c.is_whitespace() || c == '{').map_or(css.len(), |n| i + n);
                if hoisted_at_rule(&css[i..name_end]) {
                    if let Some(end) = block_end(bytes, i) {
                        rest.push_str(&css[kept..i]);
                        prelude.push(css[i..end].trim());
                        kept = end;
                        i = end;
                        continue;
                    }
                }
                statement_start = false;
            }
            c if !c.is_ascii_whitespace() => statement_start = false,
            _ => {}
        }
        i += 1;
    }
    rest.push_str(&css[kept..]);
    (prelude, rest)
}

/// The index just past the `}` closing the first block that starts at or after `from`.
fn block_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut depth = 0u32;
    let mut quote: Option<u8> = None;
    let mut i = from;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 1;
            } else if b == q {
                quote = None;
            }
        } else {
            match b {
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' if depth == 1 => return Some(i + 1),
                b'}' => depth = depth.saturating_sub(1),
                b';' if depth == 0 => return None,
                _ => {}
            }
        }
        i += 1;
    }
    None
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

    #[test]
    fn hoists_keyframes_out_of_the_class_rule() {
        let rule = StyleRule {
            class: "c".into(),
            css: "animation: spin 1s; @keyframes spin { from { opacity: 0 } to { opacity: 1 } } content: \"@keyframes x {}\"; @media (width > 1px) { color: red }".into(),
        };
        assert_eq!(
            rule_body(&rule),
            ".c { animation: spin 1s;  content: \"@keyframes x {}\"; @media (width > 1px) { color: red } }"
                .replace(".c {", "@keyframes spin { from { opacity: 0 } to { opacity: 1 } } .c {")
        );
        let plain = StyleRule { class: "d".into(), css: "&:hover { color: red } @font-face { font-family: X }".into() };
        assert_eq!(rule_body(&plain), "@font-face { font-family: X } .d { &:hover { color: red } }");
    }
}

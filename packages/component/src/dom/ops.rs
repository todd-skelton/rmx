//! The hot DOM calls of mounting and patching, made through one small JS object.
//!
//! Going through web-sys costs a call into JS per DOM operation and a `TextDecoder` run per
//! string. Building rows of a list is mostly such calls, so the operations here combine
//! the common pairs (create a node and append it to its parent) into one call, pass
//! template strings (tags, attribute names, static text) as indexes into a JS-side table
//! filled once per string, and decode short ASCII strings in JS straight from WASM memory.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen]
extern "C" {
    /// The object built by [`OPS_JS`].
    type Ops;

    #[wasm_bindgen(method, js_name = s)]
    fn string(this: &Ops, ptr: u32, len: u32) -> u32;
    #[wasm_bindgen(method, js_name = e)]
    fn element(this: &Ops, parent: Option<&web_sys::Node>, tag: u32) -> web_sys::Element;
    #[wasm_bindgen(method, js_name = n)]
    fn svg_element(this: &Ops, parent: Option<&web_sys::Node>, tag: u32) -> web_sys::Element;
    #[wasm_bindgen(method, js_name = t)]
    fn text(this: &Ops, parent: Option<&web_sys::Node>, ptr: u32, len: u32) -> web_sys::Text;
    #[wasm_bindgen(method, js_name = ts)]
    fn static_text(this: &Ops, parent: Option<&web_sys::Node>, id: u32) -> web_sys::Text;
    #[wasm_bindgen(method, js_name = d)]
    fn set_data(this: &Ops, node: &web_sys::Text, ptr: u32, len: u32);
    #[wasm_bindgen(method, js_name = a)]
    fn set_attr(this: &Ops, el: &web_sys::Element, name: u32, ptr: u32, len: u32);
    #[wasm_bindgen(method, js_name = v)]
    fn set_static_attr(this: &Ops, el: &web_sys::Element, name: u32, value: u32);
    #[wasm_bindgen(method, js_name = r)]
    fn remove_attr(this: &Ops, el: &web_sys::Element, name: u32);
    #[wasm_bindgen(method, js_name = k)]
    fn event_kind(this: &Ops, listener: &JsValue, key_ptr: u32, key_len: u32, name: u32, capture: bool) -> u32;
    #[wasm_bindgen(method, js_name = b)]
    fn bind(this: &Ops, el: &web_sys::Element, kind: u32, value: f64);
    #[wasm_bindgen(method, js_name = u)]
    fn unbind(this: &Ops, el: &web_sys::Element, kind: u32);
}

/// Strings shorter than 32 bytes that are ASCII are built with `String.fromCharCode`,
/// which is several times faster than a `TextDecoder` call for them. The memory view is
/// renewed when WASM memory grows (the old buffer is then detached).
const OPS_JS: &str = r#"
const d = document, S = [], K = [], dec = new TextDecoder();
let u8 = new Uint8Array(0);
const str = (p, l) => {
  if (u8.byteLength === 0) u8 = new Uint8Array(mem.buffer);
  if (l < 32) {
    let s = '';
    for (let i = p, e = p + l; i < e; i++) {
      const c = u8[i];
      if (c > 127) return dec.decode(u8.subarray(p, e));
      s += String.fromCharCode(c);
    }
    return s;
  }
  return dec.decode(u8.subarray(p, p + l));
};
return {
  s(p, l) { S.push(str(p, l)); return S.length - 1; },
  e(p, t) { const e = d.createElement(S[t]); if (p) p.appendChild(e); return e; },
  n(p, t) { const e = d.createElementNS('http://www.w3.org/2000/svg', S[t]); if (p) p.appendChild(e); return e; },
  t(p, a, l) { const t = d.createTextNode(str(a, l)); if (p) p.appendChild(t); return t; },
  ts(p, i) { const t = d.createTextNode(S[i]); if (p) p.appendChild(t); return t; },
  d(t, p, l) { t.data = str(p, l); },
  a(e, n, p, l) { try { e.setAttribute(S[n], str(p, l)); } catch {} },
  v(e, n, i) { try { e.setAttribute(S[n], S[i]); } catch {} },
  r(e, n) { e.removeAttribute(S[n]); },
  k(f, p, l, n, c) { K.push([f, str(p, l), S[n], c]); return K.length - 1; },
  b(e, k, id) { const t = K[k]; e[t[1]] = id; e.addEventListener(t[2], t[0], t[3]); },
  u(e, k) { const t = K[k]; delete e[t[1]]; e.removeEventListener(t[2], t[0], t[3]); },
};
"#;

/// FxHash: string ids are looked up by pointer for every element and attribute, where
/// SipHash showed up in profiles.
#[derive(Default)]
pub(crate) struct FastHasher(u64);

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u8(&mut self, n: u8) {
        self.write_u64(u64::from(n));
    }

    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }

    fn write_isize(&mut self, n: isize) {
        self.write_u64(n as u64);
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

thread_local! {
    static OPS: Ops = js_sys::Function::new_with_args("mem", OPS_JS)
        .call1(&JsValue::NULL, &wasm_bindgen::memory())
        .expect("rmx DOM ops")
        .unchecked_into();
    /// Ids of `&'static str`s in the JS string table, by address and length.
    static STRINGS: RefCell<FastMap<(usize, usize), u32>> = RefCell::default();
}

fn ptr(s: &str) -> (u32, u32) {
    (s.as_ptr() as u32, s.len() as u32)
}

/// The JS string table index of a template string, adding it on first use.
pub(crate) fn sid(s: &'static str) -> u32 {
    let key = (s.as_ptr() as usize, s.len());
    if let Some(id) = STRINGS.with(|m| m.borrow().get(&key).copied()) {
        return id;
    }
    let (p, l) = ptr(s);
    let id = OPS.with(|o| o.string(p, l));
    STRINGS.with(|m| m.borrow_mut().insert(key, id));
    id
}

/// Create an element (in the SVG namespace when `svg`), appended to `parent` when given.
pub(crate) fn element(parent: Option<&web_sys::Node>, tag: &'static str, svg: bool) -> web_sys::Element {
    let tag = sid(tag);
    OPS.with(|o| if svg { o.svg_element(parent, tag) } else { o.element(parent, tag) })
}

/// Create a text node, appended to `parent` when given. Static text is passed by id.
pub(crate) fn text(parent: Option<&web_sys::Node>, text: &crate::Str) -> web_sys::Text {
    match text {
        std::borrow::Cow::Borrowed(s) => {
            let id = sid(s);
            OPS.with(|o| o.static_text(parent, id))
        }
        std::borrow::Cow::Owned(s) => {
            let (p, l) = ptr(s);
            OPS.with(|o| o.text(parent, p, l))
        }
    }
}

pub(crate) fn set_data(node: &web_sys::Text, text: &str) {
    let (p, l) = ptr(text);
    OPS.with(|o| o.set_data(node, p, l));
}

/// `setAttribute`, ignoring invalid names as web-sys callers here did.
pub(crate) fn set_attr(el: &web_sys::Element, name: &'static str, value: &crate::Str) {
    let name = sid(name);
    match value {
        std::borrow::Cow::Borrowed(v) => {
            let v = sid(v);
            OPS.with(|o| o.set_static_attr(el, name, v));
        }
        std::borrow::Cow::Owned(v) => {
            let (p, l) = ptr(v);
            OPS.with(|o| o.set_attr(el, name, p, l));
        }
    }
}

pub(crate) fn set_attr_string(el: &web_sys::Element, name: &'static str, value: &str) {
    let name = sid(name);
    let (p, l) = ptr(value);
    OPS.with(|o| o.set_attr(el, name, p, l));
}

pub(crate) fn remove_attr(el: &web_sys::Element, name: &'static str) {
    let name = sid(name);
    OPS.with(|o| o.remove_attr(el, name));
}

/// Register the shared listener for one event type and phase. `bind` stores the handler
/// id in the element's `key` expando and adds the listener; `unbind` undoes both.
pub(crate) fn event_kind(listener: &JsValue, key: &str, name: &'static str, capture: bool) -> u32 {
    let name = sid(name);
    let (p, l) = ptr(key);
    OPS.with(|o| o.event_kind(listener, p, l, name, capture))
}

pub(crate) fn bind(el: &web_sys::Element, kind: u32, value: f64) {
    OPS.with(|o| o.bind(el, kind, value));
}

pub(crate) fn unbind(el: &web_sys::Element, kind: u32) {
    OPS.with(|o| o.unbind(el, kind));
}

//! Server rendering.
//!
//! [`render`] turns a node tree into [`Rendered`]: HTML segments with `<Frame>` placeholders,
//! plus hydration data and generated styles. All of it is plain strings, so it can cross
//! threads and await frame content after the (non-`Send`) node tree is gone. The server
//! render middleware assembles the final, streamed document from it.
//!
//! [`render_to_string`] is the synchronous shortcut for trees without frames.
//!
//! Output conventions, shared with the browser runtime:
//!
//! - Client entries are wrapped in `<!--rmx:h:ID-->` ... `<!--/rmx:h-->`.
//! - Frames are wrapped in `<!--rmx:f:ID-->` ... `<!--/rmx:f-->`.
//! - `<script type="application/json" id="rmx-data">` holds
//!   `{"h":{ID:{"n":name,"p":props}},"f":{ID:{"src":..,"name":..,"status":..}}}`.
//! - `css()` rules are emitted as `<style data-rmx-style="CLASS">`.
//! - A pending frame's content arrives later as `<template id="ID">HTML<!--rmx:te--></template>`.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::mixin::rule_css;
use crate::node::{AttrValue, Element, FrameNode, Node, StyleRule};

/// Marks the end of a streamed frame template.
pub const TEMPLATE_END: &str = "rmx:te";

/// Written after the first part of a streamed response, before any frame templates:
/// `<!-- rmx:flush fragment -->` or `<!-- rmx:flush document -->`. A client reading the
/// response can apply everything before it without waiting for the slow frames.
pub fn flush_marker(document: bool) -> &'static str {
    if document {
        "<!-- rmx:flush document -->"
    } else {
        "<!-- rmx:flush fragment -->"
    }
}

/// Where the flush marker starts and ends in `html`.
pub fn find_flush_marker(html: &str) -> Option<(usize, usize)> {
    let start = html.find("<!-- rmx:flush ")?;
    let end = html[start..].find("-->")? + start + 3;
    Some((start, end))
}

/// A rendered tree, ready for frame resolution and assembly.
#[derive(Clone, Debug, Default)]
pub struct Rendered {
    pub segments: Vec<Segment>,
    pub data: HydrationData,
    /// `(class, css)` for every `css()` rule used, in first-use order.
    pub styles: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub enum Segment {
    Html(String),
    Frame(FrameSegment),
}

/// A `<Frame>` waiting for its content.
#[derive(Clone, Debug)]
pub struct FrameSegment {
    pub id: String,
    pub src: String,
    pub name: Option<String>,
    /// Rendered fallback HTML; `Some` means the frame is non-blocking.
    pub fallback: Option<String>,
}

/// Client entries and frames found while rendering.
#[derive(Clone, Debug, Default)]
pub struct HydrationData {
    /// `(id, entry name, props JSON)`
    pub entries: Vec<(String, String, String)>,
    /// `(id, src, name, status)` where status is `"pending"` or `"resolved"`.
    pub frames: Vec<(String, String, Option<String>, &'static str)>,
}

impl HydrationData {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.frames.is_empty()
    }

    pub fn merge(&mut self, other: HydrationData) {
        self.entries.extend(other.entries);
        self.frames.extend(other.frames);
    }

    /// Mark a frame resolved (its content was inlined).
    pub fn set_resolved(&mut self, id: &str) {
        if let Some(f) = self.frames.iter_mut().find(|f| f.0 == id) {
            f.3 = "resolved";
        }
    }

    pub fn to_json(&self) -> String {
        let mut s = String::from("{");
        if !self.entries.is_empty() {
            s.push_str("\"h\":{");
            for (i, (id, name, props)) in self.entries.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let _ = write!(s, "{}:{{\"n\":{},\"p\":{}}}", json_str(id), json_str(name), if props.is_empty() { "null" } else { props });
            }
            s.push('}');
        }
        if !self.frames.is_empty() {
            if !self.entries.is_empty() {
                s.push(',');
            }
            s.push_str("\"f\":{");
            for (i, (id, src, name, status)) in self.frames.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let name = name.as_deref().map_or("null".to_string(), json_str);
                let _ = write!(s, "{}:{{\"src\":{},\"name\":{name},\"status\":{}}}", json_str(id), json_str(src), json_str(status));
            }
            s.push('}');
        }
        s.push('}');
        s
    }

    /// The `<script id="rmx-data">` tag, or an empty string when there is nothing to hydrate.
    pub fn to_script(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        format!(r#"<script type="application/json" id="rmx-data">{}</script>"#, escape_script_json(&self.to_json()))
    }

    /// Parse the JSON written by [`HydrationData::to_json`].
    pub fn from_json(json: &str) -> Option<HydrationData> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let mut data = HydrationData::default();
        if let Some(h) = v.get("h").and_then(|h| h.as_object()) {
            for (id, e) in h {
                let name = e.get("n")?.as_str()?.to_string();
                let props = e.get("p").map(|p| p.to_string()).unwrap_or_default();
                data.entries.push((id.clone(), name, props));
            }
        }
        if let Some(f) = v.get("f").and_then(|f| f.as_object()) {
            for (id, e) in f {
                let src = e.get("src").and_then(|s| s.as_str()).unwrap_or_default().to_string();
                let name = e.get("name").and_then(|s| s.as_str()).map(str::to_string);
                let status = if e.get("status").and_then(|s| s.as_str()) == Some("pending") { "pending" } else { "resolved" };
                data.frames.push((id.clone(), src, name, status));
            }
        }
        Some(data)
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

/// Make JSON safe inside a `<script>` element.
pub fn escape_script_json(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// `<style data-rmx-style>` tags for the given rules.
pub fn style_tags(styles: &[(String, String)]) -> String {
    let mut out = String::new();
    for (class, css) in styles {
        let _ = write!(out, r#"<style data-rmx-style="{class}">{}</style>"#, css.replace("</style", "\\3C/style"));
    }
    out
}

/// Unique ids for markers. Seeded from the clock so ids from different server processes
/// rarely collide inside one long-lived page.
pub fn next_id(prefix: char) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let seed = *SEED.get_or_init(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        (t ^ (t >> 29)) % 1_000_000
    });
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{seed:x}{n:x}")
}

/// Render a tree into segments, hydration data and styles.
pub fn render(node: &Node) -> Rendered {
    let mut ssr = Ssr::default();
    ssr.write(node);
    ssr.finish()
}

/// Render a tree to one HTML string. Frames show their fallback (or nothing); use the render
/// middleware to resolve frame content. Styles go at the end of `<head>` (or the start),
/// hydration data before `</body>` (or the end).
pub fn render_to_string(node: &Node) -> String {
    let rendered = render(node);
    let mut html = String::new();
    for seg in &rendered.segments {
        match seg {
            Segment::Html(s) => html.push_str(s),
            Segment::Frame(f) => {
                let _ = write!(html, "<!--rmx:f:{}-->", f.id);
                html.push_str(f.fallback.as_deref().unwrap_or(""));
                html.push_str("<!--/rmx:f-->");
            }
        }
    }
    finalize_html(html, &rendered.styles, &rendered.data)
}

/// Insert style tags and the hydration data script into assembled HTML.
pub fn finalize_html(mut html: String, styles: &[(String, String)], data: &HydrationData) -> String {
    if !styles.is_empty() {
        let tags = style_tags(styles);
        match find_ci(&html, "</head>") {
            Some(pos) => html.insert_str(pos, &tags),
            None => {
                // Keep a doctype first.
                let at = if html.trim_start().to_ascii_lowercase().starts_with("<!doctype") { html.find('>').map_or(0, |i| i + 1) } else { 0 };
                html.insert_str(at, &tags);
            }
        }
    }
    let script = data.to_script();
    if !script.is_empty() {
        match find_ci(&html, "</body>") {
            Some(pos) => html.insert_str(pos, &script),
            None => html.push_str(&script),
        }
    }
    html
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().rfind(needle)
}

#[derive(Default)]
pub(crate) struct Ssr {
    out: String,
    segments: Vec<Segment>,
    data: HydrationData,
    styles: Vec<(String, String)>,
    seen_styles: HashSet<String>,
    in_entry: bool,
}

impl Ssr {
    fn finish(mut self) -> Rendered {
        if !self.out.is_empty() {
            self.segments.push(Segment::Html(std::mem::take(&mut self.out)));
        }
        Rendered { segments: self.segments, data: self.data, styles: self.styles }
    }

    pub(crate) fn write(&mut self, node: &Node) {
        match node {
            Node::Element(el) => self.element(el),
            Node::Text(text) => escape_into(&mut self.out, text, false),
            Node::Fragment(nodes) => nodes.iter().for_each(|n| self.write(n)),
            Node::Component(c) => c.inner.ssr(self),
            Node::Shared(n) => self.write(n),
            Node::Raw(html) => self.out.push_str(html),
            Node::Frame(f) => self.frame(f),
            Node::Empty => {}
        }
    }

    fn frame(&mut self, f: &Rc<FrameNode>) {
        let id = next_id('f');
        let fallback = f.fallback.as_ref().map(|fb| {
            let mut sub = Ssr { in_entry: self.in_entry, ..Ssr::default() };
            sub.write(fb);
            let r = sub.finish();
            self.data.merge(r.data);
            for (class, css) in r.styles {
                self.add_style(class, css);
            }
            r.segments
                .into_iter()
                .map(|s| match s {
                    Segment::Html(h) => h,
                    Segment::Frame(_) => String::new(),
                })
                .collect::<String>()
        });
        let status = if fallback.is_some() { "pending" } else { "resolved" };
        self.data.frames.push((id.clone(), f.src.clone(), f.name.clone(), status));
        if !self.out.is_empty() {
            self.segments.push(Segment::Html(std::mem::take(&mut self.out)));
        }
        self.segments.push(Segment::Frame(FrameSegment { id, src: f.src.clone(), name: f.name.clone(), fallback }));
    }

    fn add_style(&mut self, class: String, css: String) {
        if self.seen_styles.insert(class.clone()) {
            self.styles.push((class, css));
        }
    }

    fn add_rule(&mut self, rule: &StyleRule) {
        if !self.seen_styles.contains(&rule.class) {
            self.add_style(rule.class.clone(), rule_css(rule));
        }
    }

    /// Entries nested inside another entry hydrate as ordinary children of the outer one.
    pub(crate) fn client_entry(&mut self, name: &'static str, props: impl FnOnce() -> String, node: &Node) {
        if self.in_entry {
            return self.write(node);
        }
        let id = next_id('h');
        let _ = write!(self.out, "<!--rmx:h:{id}-->");
        self.in_entry = true;
        self.write(node);
        self.in_entry = false;
        self.out.push_str("<!--/rmx:h-->");
        self.data.entries.push((id, name.to_string(), props()));
    }

    fn element(&mut self, el: &Element) {
        if let Some(extra) = &el.extra {
            for rule in &extra.styles {
                self.add_rule(rule);
            }
        }
        let out = &mut self.out;
        out.push('<');
        out.push_str(el.tag);
        for (name, value) in &el.attrs {
            match value {
                AttrValue::Str(v) => {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    escape_into(out, v, true);
                    out.push('"');
                }
                AttrValue::Bool(true) => {
                    out.push(' ');
                    out.push_str(name);
                }
                AttrValue::Bool(false) | AttrValue::Absent => {}
            }
        }
        out.push('>');
        if is_void(el.tag) {
            return;
        }
        if el.tag == "script" || el.tag == "style" {
            // Raw text elements: children are written unescaped.
            for child in &el.children {
                if let Node::Text(t) | Node::Raw(t) = child {
                    let t = if el.tag == "script" { t.replace("</script", "<\\/script") } else { t.replace("</style", "\\3C/style") };
                    self.out.push_str(&t);
                }
            }
        } else {
            for child in &el.children {
                self.write(child);
            }
        }
        self.out.push_str("</");
        self.out.push_str(el.tag);
        self.out.push('>');
    }
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr"
    )
}

pub(crate) fn escape_into(out: &mut String, s: &str, attr: bool) {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' if attr => "&quot;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

/// Escape text for HTML.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&mut out, s, true);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_and_attributes() {
        let node = Node::element(
            "a",
            None,
            vec![("title", AttrValue::Str("say \"hi\" & <go>".into())), ("hidden", AttrValue::Bool(true))],
            vec![],
            vec![Node::Text("1 < 2 & 3".into())],
        );
        assert_eq!(
            render_to_string(&node),
            r#"<a title="say &quot;hi&quot; &amp; &lt;go&gt;" hidden>1 &lt; 2 &amp; 3</a>"#
        );
    }

    #[test]
    fn hydration_data_round_trips() {
        let mut d = HydrationData::default();
        d.entries.push(("h1".into(), "Counter".into(), r#"{"initial":1}"#.into()));
        d.frames.push(("f1".into(), "/x".into(), Some("side".into()), "pending"));
        let back = HydrationData::from_json(&d.to_json()).unwrap();
        assert_eq!(back.entries, d.entries);
        assert_eq!(back.frames, d.frames);
        assert!(d.to_script().contains(r#""f":{"f1":{"src":"/x","name":"side","status":"pending"}}"#));
    }
}

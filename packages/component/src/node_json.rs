//! Nodes as client-entry props.
//!
//! A client entry's props are JSON. Like Remix 3, element-valued props (`children`, a
//! `fallback`) serialize to plain descriptors and revive in the browser:
//!
//! - text is a string, a fragment is an array, nothing is `null`;
//! - an element is `{"$rmx":true,"type":tag,"key":k,"props":{attrs..,"children":[..]},"css":[[class,css]..]}`;
//! - a `<Frame>` is `{"$rmxFrame":true,"props":{"src":..,"name":..,"fallback":..}}`;
//! - a `<Fragment>` is `{"$rmxFragment":true,"key":k,"props":{"children":[..]}}`, so a keyed
//!   group keeps its key (Remix 3.1);
//! - a component renders on the server and serializes as its output.
//!
//! Event handlers and `node_ref` callbacks cannot cross the boundary and are dropped.
//! `css()` rules travel with their element, so a revived subtree keeps its styles.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::node::{AttrValue, Children, Element, ElementExtra, FrameNode, Key, Node, StyleRule};

impl Node {
    /// The JSON descriptor for this node (see the module docs).
    pub fn to_json(&self) -> Value {
        match self {
            Node::Element(el) => element_json(el),
            Node::Text(t) => Value::String(t.to_string()),
            Node::Fragment(nodes) => Value::Array(nodes.iter().map(Node::to_json).collect()),
            Node::Component(c) if c.inner.component_type() == std::any::TypeId::of::<crate::Fragment>() => {
                let mut out = Map::new();
                out.insert("$rmxFragment".into(), Value::Bool(true));
                if let Some(key) = key_json(c.key.as_ref()) {
                    out.insert("key".into(), key);
                }
                let children = match c.inner.render_static().to_json() {
                    Value::Array(items) => Value::Array(items),
                    one => Value::Array(vec![one]),
                };
                out.insert("props".into(), serde_json::json!({ "children": children }));
                Value::Object(out)
            }
            Node::Component(c) => c.inner.render_static().to_json(),
            Node::Shared(n) => n.to_json(),
            Node::Raw(html) => serde_json::json!({ "$rmxRaw": html.as_ref() }),
            Node::Frame(f) => {
                let mut props = Map::new();
                props.insert("src".into(), Value::String(f.src.clone()));
                if let Some(name) = &f.name {
                    props.insert("name".into(), Value::String(name.clone()));
                }
                if let Some(fallback) = &f.fallback {
                    props.insert("fallback".into(), fallback.to_json());
                }
                serde_json::json!({ "$rmxFrame": true, "props": props })
            }
            Node::Empty => Value::Null,
        }
    }

    /// Revive a node from [`Node::to_json`] output. Unknown shapes become empty nodes.
    pub fn from_json(value: &Value) -> Node {
        match value {
            Value::Null | Value::Bool(_) => Node::Empty,
            Value::String(s) => Node::Text(Cow::Owned(s.clone())),
            Value::Number(n) => Node::Text(Cow::Owned(n.to_string())),
            Value::Array(items) => Node::Fragment(items.iter().map(Node::from_json).collect()),
            Value::Object(obj) => {
                if obj.get("$rmx").and_then(Value::as_bool) == Some(true) {
                    element_from_json(obj)
                } else if obj.get("$rmxFrame").and_then(Value::as_bool) == Some(true) {
                    let props = obj.get("props").and_then(Value::as_object);
                    let field = |k: &str| props.and_then(|p| p.get(k));
                    Node::Frame(Rc::new(FrameNode {
                        src: field("src").and_then(Value::as_str).unwrap_or_default().to_string(),
                        name: field("name").and_then(Value::as_str).map(str::to_string),
                        fallback: field("fallback").map(Node::from_json),
                    }))
                } else if obj.get("$rmxFragment").and_then(Value::as_bool) == Some(true) {
                    let children = obj.get("props").and_then(|p| p.get("children")).map_or(Node::Empty, Node::from_json);
                    Node::component::<crate::Fragment>(key_from_json(obj.get("key")), crate::FragmentProps { children: children.into() })
                } else if let Some(html) = obj.get("$rmxRaw").and_then(Value::as_str) {
                    Node::Raw(Cow::Owned(html.to_string()))
                } else {
                    Node::Empty
                }
            }
        }
    }
}

fn element_json(el: &Element) -> Value {
    if crate::host_mixin::has_mixins(el) {
        // Stateful mixins cannot cross the wire: serialize what they render on the server.
        return element_json(&crate::host_mixin::resolve_server(el.clone()));
    }
    let mut props = Map::new();
    for (name, value) in &el.attrs {
        match value {
            AttrValue::Str(s) => {
                props.insert((*name).to_string(), Value::String(s.to_string()));
            }
            AttrValue::Bool(true) => {
                props.insert((*name).to_string(), Value::Bool(true));
            }
            AttrValue::Html(html) => {
                props.insert((*name).to_string(), serde_json::json!({ "$rmxHtml": html.as_ref() }));
            }
            AttrValue::Bool(false) | AttrValue::Absent => {}
        }
    }
    if !el.children.is_empty() {
        props.insert("children".into(), Value::Array(el.children.iter().map(Node::to_json).collect()));
    }
    let mut out = Map::new();
    out.insert("$rmx".into(), Value::Bool(true));
    out.insert("type".into(), Value::String(el.tag.to_string()));
    if let Some(key) = key_json(el.key.as_ref()) {
        out.insert("key".into(), key);
    }
    out.insert("props".into(), Value::Object(props));
    if let Some(extra) = &el.extra {
        if !extra.styles.is_empty() {
            let rules = extra.styles.iter().map(|r| serde_json::json!([r.class, r.css.as_ref()])).collect();
            out.insert("css".into(), Value::Array(rules));
        }
    }
    Value::Object(out)
}

fn key_json(key: Option<&Key>) -> Option<Value> {
    match key? {
        Key::Int(i) => Some(Value::from(*i)),
        Key::Str(s) => Some(Value::String(s.to_string())),
    }
}

fn key_from_json(value: Option<&Value>) -> Option<Key> {
    match value? {
        Value::Number(n) => n.as_i64().map(Key::Int),
        Value::String(s) => Some(Key::Str(Cow::Owned(s.clone()))),
        _ => None,
    }
}

fn element_from_json(obj: &Map<String, Value>) -> Node {
    let tag = intern(obj.get("type").and_then(Value::as_str).unwrap_or("div"));
    let key = key_from_json(obj.get("key"));
    let mut attrs = Vec::new();
    let mut children = Vec::new();
    if let Some(props) = obj.get("props").and_then(Value::as_object) {
        for (name, value) in props {
            if name == "children" {
                match value {
                    Value::Array(items) => children.extend(items.iter().map(Node::from_json)),
                    other => children.push(Node::from_json(other)),
                }
                continue;
            }
            let value = match value {
                Value::String(s) => AttrValue::Str(Cow::Owned(s.clone())),
                Value::Bool(b) => AttrValue::Bool(*b),
                Value::Number(n) => AttrValue::Str(Cow::Owned(n.to_string())),
                Value::Object(o) => match o.get("$rmxHtml").and_then(Value::as_str) {
                    Some(html) => AttrValue::Html(Cow::Owned(html.to_string())),
                    None if crate::attrs::is_html_prop(name) => crate::attrs::invalid_html_prop(name),
                    None => continue,
                },
                _ => continue,
            };
            attrs.push((intern(name), value));
        }
    }
    let styles: Vec<Rc<StyleRule>> = obj
        .get("css")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .filter_map(|r| {
                    let class = r.get(0)?.as_str()?.to_string();
                    let css = r.get(1)?.as_str()?.to_string();
                    Some(Rc::new(StyleRule { class, css: Cow::Owned(css) }))
                })
                .collect()
        })
        .unwrap_or_default();
    let extra = (!styles.is_empty()).then(|| Box::new(ElementExtra { styles, ..Default::default() }));
    crate::attrs::check_html_props(&attrs);
    Node::Element(Box::new(Element { tag, key, attrs, events: Vec::new(), children, extra }))
}

/// Tag and attribute names are `&'static str` in the node tree. Names that arrive as JSON
/// are leaked once each; the set of distinct names in a document is small.
fn intern(name: &str) -> &'static str {
    thread_local! {
        static NAMES: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
    }
    NAMES.with(|names| {
        if let Some(n) = names.borrow().get(name) {
            return *n;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        names.borrow_mut().insert(leaked);
        leaked
    })
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Node, D::Error> {
        Ok(Node::from_json(&Value::deserialize(d)?))
    }
}

impl Serialize for Children {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.node().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Children {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Children, D::Error> {
        Node::deserialize(d).map(Children::from)
    }
}

#[cfg(test)]
mod tests {
    use crate::{css, rsx, Component, Frame, Handle, Node};

    #[derive(Clone, Default)]
    struct BadgeProps {
        label: String,
    }

    struct Badge;

    impl Component for Badge {
        type Props = BadgeProps;
        fn setup(_: &BadgeProps, _: &Handle<Self>) -> Self {
            Badge
        }
        fn render(&self, props: &BadgeProps, _: &Handle<Self>) -> Node {
            rsx! { <b>{props.label.clone()}</b> }
        }
    }

    struct Failing;

    impl Component for Failing {
        type Props = ();
        fn setup(_: &(), _: &Handle<Self>) -> Self {
            Failing
        }
        fn render(&self, _: &(), h: &Handle<Self>) -> Node {
            rsx! { <p>{h.fail("bad")}</p> }
        }
    }

    #[test]
    fn a_failed_component_render_serializes_as_nothing() {
        let node = rsx! { <div><Failing/>"after"</div> };
        assert_eq!(crate::render_to_string(&Node::from_json(&node.to_json())), "<div>after</div>");
    }

    #[test]
    fn unsafe_html_sets_content_and_survives_json() {
        let node = rsx! { <div class="md" innerHTML={crate::unsafe_html("<b>bold</b> &amp;")}>"ignored"</div> };
        assert_eq!(crate::render_to_string(&node), r#"<div class="md"><b>bold</b> &amp;</div>"#);
        let revived = Node::from_json(&node.to_json());
        assert_eq!(crate::render_to_string(&revived), r#"<div class="md"><b>bold</b> &amp;</div>"#);
    }

    #[test]
    #[should_panic(expected = "Invalid innerHTML prop: expected a value created with unsafe_html()")]
    fn inner_html_rejects_plain_strings() {
        Node::from_json(&serde_json::json!({ "$rmx": true, "type": "div", "props": { "innerHTML": "<img src=x onerror=alert(1)>" } }));
    }

    #[test]
    #[should_panic(expected = "Invalid srcdoc prop: expected a value created with unsafe_html()")]
    fn srcdoc_rejects_json_shaped_values() {
        Node::from_json(&serde_json::json!({ "$rmx": true, "type": "iframe", "props": { "srcdoc": { "value": "<p>HTML</p>" } } }));
    }

    #[test]
    fn fragments_and_keys_round_trip() {
        let node = rsx! {
            <dl>
                <crate::Fragment key="a"><dt>"A"</dt><dd>"first"</dd></crate::Fragment>
                <crate::Fragment key={2}><dt>"B"</dt></crate::Fragment>
                <li key="k">"keyed"</li>
            </dl>
        };
        let json = node.to_json();
        let text = json.to_string();
        assert!(text.contains(r#"{"$rmxFragment":true,"key":"a","props":{"children":[{"$rmx":true,"props":{"children":["A"]},"type":"dt"}"#), "{text}");
        assert!(text.contains(r#""$rmxFragment":true,"key":2"#), "{text}");
        assert!(text.contains(r#"{"$rmx":true,"key":"k","props":{"children":["keyed"]},"type":"li"}"#), "{text}");
        let revived = Node::from_json(&json);
        assert_eq!(revived.to_json(), json, "keys survive the round trip");
        let Node::Element(dl) = &revived else { panic!("an element") };
        assert!(matches!(&dl.children[0], Node::Component(c) if c.key == Some(crate::node::Key::Str("a".into()))));
        assert_eq!(crate::render_to_string(&revived), "<dl><dt>A</dt><dd>first</dd><dt>B</dt><li>keyed</li></dl>");
    }

    #[test]
    fn elements_frames_and_components_round_trip() {
        let node = rsx! {
            <p class="status" hidden mix={css("color: red")}>
                "Waiting " <Badge label="soon"/>
                <Frame src="/frames/a" fallback={rsx! { <i>"Loading"</i> }}/>
            </p>
        };
        let json = node.to_json();
        let text = json.to_string();
        assert!(text.contains(r#""type":"p""#), "{text}");
        assert!(text.contains(r#""hidden":true"#), "{text}");
        assert!(text.contains(r#""$rmxFrame":true"#), "{text}");
        assert!(text.contains(r#""type":"b""#), "the component renders into its output: {text}");
        assert!(text.contains("color: red"), "css rules travel with the element: {text}");

        let revived = Node::from_json(&json);
        assert_eq!(revived.to_json(), json);
        let html = crate::render_to_string(&revived);
        assert!(html.contains(r#"<p class="status rmxc-"#), "{html}");
        assert!(html.contains("<b>soon</b>"), "{html}");
        assert!(html.contains("color: red"), "revived rules are emitted: {html}");
    }
}

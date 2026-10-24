//! Ports of the hydration section of Remix's `stream.test.tsx`. rsmix ships one wasm bundle,
//! so hydration data names the registered entry (`{"n":name,"p":props}`) instead of a
//! module URL and export; the `resolveClientEntry` and import map cases do not apply.
//!
//! Divergence: Remix marks an entry rendered inside another entry as its own hydration
//! entry, because each entry is a separately loaded module. rsmix's outer entry hydrates its
//! whole subtree (every component is in the bundle), so a nested entry is part of the outer
//! one: one marker, one data record.

extern crate rsmix_component as rsmix;

use std::cell::Cell;

use rsmix::__private::serde_json::{self, json, Value};
use rsmix::{render_to_string, rsx, Children, Component, Frame, Handle, Node};
use serde::{Deserialize, Serialize, Serializer};

/// The `rmx-data` script's JSON.
fn data(html: &str) -> Value {
    let open = r#"<script type="application/json" id="rmx-data">"#;
    let start = html.find(open).expect("rmx-data script") + open.len();
    let end = start + html[start..].find("</script>").expect("script end");
    serde_json::from_str(&html[start..end]).expect("rmx-data parses")
}

/// The one hydration entry, with its id.
fn single_entry(html: &str) -> (String, Value) {
    let h = data(html)["h"].as_object().expect("entries").clone();
    assert_eq!(h.len(), 1, "{html}");
    h.into_iter().next().unwrap()
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct CounterProps {
    #[serde(rename = "initialCount")]
    initial_count: i32,
}

struct Counter;

impl Component for Counter {
    type Props = CounterProps;
    rsmix::client_entry!("Counter");
    fn setup(_: &CounterProps, _: &Handle<Self>) -> Self {
        Counter
    }
    fn render(&self, p: &CounterProps, _: &Handle<Self>) -> Node {
        rsx! { <div>"Count: " {p.initial_count}</div> }
    }
}

#[test]
fn renders_a_hydrated_component_with_its_hydration_data() {
    let html = render_to_string(&rsx! { <Counter initial_count={42}/> });
    let (id, entry) = single_entry(&html);
    assert_eq!(entry, json!({ "n": "Counter", "p": { "initialCount": 42 } }));
    let start = html.find(&format!("<!--rmx:h:{id}-->")).expect("start marker");
    let content = html.find("<div>Count: 42</div>").expect("content");
    let end = html.find("<!--/rmx:h-->").expect("end marker");
    assert!(start < content && content < end, "{html}");
    assert!(html[end..].starts_with(r#"<!--/rmx:h--><script type="application/json" id="rmx-data">"#), "{html}");
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct LabelProps {
    label: String,
}

struct Label;

impl Component for Label {
    type Props = LabelProps;
    rsmix::client_entry!("Label");
    fn setup(_: &LabelProps, _: &Handle<Self>) -> Self {
        Label
    }
    fn render(&self, p: &LabelProps, _: &Handle<Self>) -> Node {
        rsx! { <button>{p.label.clone()}</button> }
    }
}

#[test]
fn escapes_rmx_data_payloads_that_contain_script_end_tags() {
    let label = r#"</script><script>alert("xss")</script>"#;
    let html = render_to_string(&rsx! { <Label label={label}/> });
    let script = &html[html.find(r#"id="rmx-data">"#).unwrap()..];
    let script = &script[..script.find("</script>").unwrap()];
    assert!(!script.contains("</script"), "{html}");
    let (_, entry) = single_entry(&html);
    assert_eq!(entry["p"]["label"], label);
}

#[test]
fn renders_multiple_hydrated_components_with_unique_ids() {
    let html = render_to_string(&rsx! { <div><Label label="First"/><Label label="Second"/></div> });
    assert!(html.contains("<button>First</button>") && html.contains("<button>Second</button>"), "{html}");
    let h = data(&html)["h"].as_object().unwrap().clone();
    assert_eq!(h.len(), 2);
    assert!(h.values().all(|e| e["n"] == "Label"));
    assert_eq!(html.matches("<!--rmx:h:").count(), 2);
}

thread_local! {
    static SERIALIZED: Cell<u32> = const { Cell::new(0) };
}

/// Counts its serializations, like Remix's `toJSON` spy.
#[derive(Clone, Default, Deserialize)]
struct Counted {
    count: u32,
}

impl Serialize for Counted {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SERIALIZED.with(|c| c.set(c.get() + 1));
        json!({ "count": self.count, "label": "</script>日本語🙂" }).serialize(s)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct SnapshotProps {
    value: Counted,
}

struct Snapshot;

impl Component for Snapshot {
    type Props = SnapshotProps;
    rsmix::client_entry!("Snapshot");
    fn setup(_: &SnapshotProps, _: &Handle<Self>) -> Self {
        Snapshot
    }
    fn render(&self, _: &SnapshotProps, _: &Handle<Self>) -> Node {
        rsx! { <p>"Content"</p> }
    }
}

#[test]
fn serializes_entry_props_once() {
    SERIALIZED.with(|c| c.set(0));
    let html = render_to_string(&rsx! { <Snapshot value={Counted { count: 1 }}/> });
    let (_, entry) = single_entry(&html);
    assert_eq!(entry["p"]["value"], json!({ "count": 1, "label": "</script>日本語🙂" }));
    assert_eq!(SERIALIZED.with(Cell::get), 1);
}

/// Props whose serialization fails (Remix: a `toJSON` that returns `undefined`).
#[derive(Clone, Default, Deserialize)]
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("no JSON form"))
    }
}

struct Broken;

impl Component for Broken {
    type Props = Unserializable;
    rsmix::client_entry!("Broken");
    fn setup(_: &Unserializable, _: &Handle<Self>) -> Self {
        Broken
    }
    fn render(&self, _: &Unserializable, _: &Handle<Self>) -> Node {
        rsx! { <p>"Content"</p> }
    }
}

#[test]
#[should_panic(expected = "client entry props must serialize to JSON")]
fn rejects_entry_props_that_do_not_serialize() {
    render_to_string(&Node::component::<Broken>(None, Unserializable));
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Nested {
    value: i32,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct CardProps {
    title: String,
    count: i32,
    enabled: bool,
    items: Vec<String>,
    nested: Nested,
}

struct Card;

impl Component for Card {
    type Props = CardProps;
    rsmix::client_entry!("Card");
    fn setup(_: &CardProps, _: &Handle<Self>) -> Self {
        Card
    }
    fn render(&self, p: &CardProps, _: &Handle<Self>) -> Node {
        rsx! {
            <div>
                <h2>{p.title.clone()}</h2>
                <p>"Count: " {p.count}</p>
                <section>"Enabled: " {p.enabled.to_string()}</section>
                <ul>{p.items.iter().enumerate().map(|(i, item)| rsx! { <li key={i}>{item.clone()}</li> }).collect::<Vec<_>>()}</ul>
                <main>"Nested value: " {p.nested.value}</main>
            </div>
        }
    }
}

#[test]
fn renders_a_hydrated_component_with_complex_props() {
    let items = vec!["one".to_string(), "two".to_string(), "three".to_string()];
    let html = render_to_string(&rsx! {
        <Card title="Test Card" count={10} enabled={true} items={items} nested={Nested { value: 99 }}/>
    });
    assert!(html.starts_with("<!--rmx:h:"), "{html}");
    assert!(
        html.contains(concat!(
            "<div><h2>Test Card</h2><p>Count: 10</p><section>Enabled: true</section>",
            "<ul><li>one</li><li>two</li><li>three</li></ul><main>Nested value: 99</main></div>",
        )),
        "{html}"
    );
    let (_, entry) = single_entry(&html);
    assert_eq!(
        entry["p"],
        json!({ "title": "Test Card", "count": 10, "enabled": true, "items": ["one", "two", "three"], "nested": { "value": 99 } })
    );
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct BoxProps {
    children: Children,
}

struct Panel;

impl Component for Panel {
    type Props = BoxProps;
    rsmix::client_entry!("Panel");
    fn setup(_: &BoxProps, _: &Handle<Self>) -> Self {
        Panel
    }
    fn render(&self, p: &BoxProps, _: &Handle<Self>) -> Node {
        rsx! { <div><h1>"Test Card"</h1>{&p.children}</div> }
    }
}

#[test]
fn serializes_host_elements_in_entry_props() {
    let html = render_to_string(&rsx! { <Panel><p>"Hello, world!"</p></Panel> });
    let (_, entry) = single_entry(&html);
    assert_eq!(entry["p"]["children"], json!([{ "$rmx": true, "type": "p", "props": { "children": ["Hello, world!"] } }]));
}

struct DeepChild;

impl Component for DeepChild {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        DeepChild
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <span>"Hello, world!"</span> }
    }
}

struct UnwrappedChild;

impl Component for UnwrappedChild {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        UnwrappedChild
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <p><DeepChild/></p> }
    }
}

#[test]
fn serializes_component_elements_in_entry_props_as_their_output() {
    let html = render_to_string(&rsx! { <Panel><UnwrappedChild/></Panel> });
    let (_, entry) = single_entry(&html);
    assert_eq!(
        entry,
        json!({
            "n": "Panel",
            "p": { "children": [{
                "$rmx": true,
                "type": "p",
                "props": { "children": [{ "$rmx": true, "type": "span", "props": { "children": ["Hello, world!"] } }] },
            }] },
        })
    );
}

#[test]
fn serializes_frames_in_entry_props_as_frame_descriptors() {
    let html = render_to_string(&rsx! {
        <Panel><Frame src="/child-frame" fallback={rsx! { <p>"Loading child frame…"</p> }}/></Panel>
    });
    let (_, entry) = single_entry(&html);
    assert_eq!(
        entry["p"]["children"],
        json!([{
            "$rmxFrame": true,
            "props": {
                "src": "/child-frame",
                "fallback": { "$rmx": true, "type": "p", "props": { "children": ["Loading child frame…"] } },
            },
        }])
    );
}

#[test]
fn nested_entries_hydrate_as_part_of_the_outer_entry() {
    let html = render_to_string(&rsx! { <Panel><Label label="Inner"/></Panel> });
    assert_eq!(html.matches("<!--rmx:h:").count(), 1, "{html}");
    let (_, entry) = single_entry(&html);
    assert_eq!(entry["n"], "Panel");
    assert_eq!(entry["p"]["children"], json!([{ "$rmx": true, "type": "button", "props": { "children": ["Inner"] } }]));
    assert!(html.contains("<div><h1>Test Card</h1><button>Inner</button></div><!--/rmx:h-->"), "{html}");
}

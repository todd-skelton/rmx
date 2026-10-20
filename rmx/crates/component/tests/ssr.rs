use rmx::{render_to_string, rsx, Children, Component, Handle, Node};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default)]
struct LayoutProps {
    title: &'static str,
    children: Children,
}

struct Layout;

impl Component for Layout {
    type Props = LayoutProps;
    fn setup(_: &LayoutProps, _: &Handle<Self>) -> Self {
        Layout
    }
    fn render(&self, p: &LayoutProps, _: &Handle<Self>) -> Node {
        rsx! { <html><body><h1>{p.title}</h1>{&p.children}</body></html> }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct CounterProps {
    initial: i32,
    label: String,
}

struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rmx::client_entry!("Counter");

    fn setup(p: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: p.initial }
    }
    fn render(&self, p: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <button disabled={false} on:click={h.on(|this, h| { this.count += 1; h.update(); })}>
                {p.label.clone()} ": " {self.count}
            </button>
        }
    }
}

#[test]
fn renders_components_children_and_client_entries() {
    let items = ["a", "<b>"];
    let page = rsx! {
        <Layout title="Hi & bye">
            <ul>{items.iter().map(|i| rsx! { <li key={*i}>{*i}</li> }).collect::<Vec<_>>()}</ul>
            <Counter initial={3} label="</script>"/>
            <input required/>
        </Layout>
    };
    let html = render_to_string(&page);
    // Marker ids are unique per render; normalize the one entry id.
    let id_start = html.find("<!--rmx:h:").expect("entry marker") + "<!--rmx:h:".len();
    let id = &html[id_start..html[id_start..].find("-->").unwrap() + id_start];
    let html = html.replace(id, "ID");
    assert_eq!(
        html,
        concat!(
            "<html><body><h1>Hi &amp; bye</h1>",
            "<ul><li>a</li><li>&lt;b&gt;</li></ul>",
            "<!--rmx:h:ID--><button>&lt;/script&gt;: 3</button><!--/rmx:h-->",
            "<input required>",
            r#"<script type="application/json" id="rmx-data">{"h":{"ID":{"n":"Counter","p":{"initial":3,"label":"\u003c/script>"}}}}</script>"#,
            "</body></html>",
        )
    );
}

#[test]
fn fragments_doctype_and_optional_attributes() {
    let selected: Option<&'static str> = None;
    let node = rsx! {
        <!DOCTYPE html>
        <p class={selected} data-x={7}>"x"</p>
    };
    assert_eq!(render_to_string(&node), r#"<!DOCTYPE html><p data-x="7">x</p>"#);
}

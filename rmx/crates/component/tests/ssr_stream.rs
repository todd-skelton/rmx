//! Ports of Remix's `stream.test.tsx` server rendering cases (special props, svg,
//! doctype, head-like elements, styles). rsx takes HTML attribute names (`class`, `for`,
//! `stroke-linecap`), so the JSX camelCase mapping cases become their HTML spelling, and
//! void elements are written `<input>` rather than `<input />`.

extern crate rmx_component as rmx;

use std::cell::RefCell;

use rmx::{css, render_to_string, rsx, unsafe_html, AttrValue, Children, Component, Handle, Node};

fn html(node: Node) -> String {
    render_to_string(&node)
}

// basic and component nodes

#[test]
fn streams_basic_html() {
    assert_eq!(html(rsx! { <div>"Hello, world!"</div> }), "<div>Hello, world!</div>");
}

#[test]
fn renders_zero() {
    let n = 0;
    assert_eq!(html(rsx! { <div>{n}</div> }), "<div>0</div>");
}

#[test]
fn encodes_large_multi_byte_output() {
    let content = "日本語".repeat(20_000) + "🙂";
    assert_eq!(html(rsx! { <p>{content.clone()}</p> }), format!("<p>{content}</p>"));
}

#[test]
fn escapes_text_node_content() {
    let text = r#"<img src=x onerror="alert(1)">&</img>"#;
    assert_eq!(html(rsx! { <>{text}</> }), r#"&lt;img src=x onerror="alert(1)"&gt;&amp;&lt;/img&gt;"#);
}

#[test]
fn escapes_text_children_in_elements() {
    assert_eq!(html(rsx! { <div>{"<script>alert(1)</script>"}</div> }), "<div>&lt;script&gt;alert(1)&lt;/script&gt;</div>");
}

#[test]
fn preserves_unicode_and_entity_text_while_escaping_text_and_attribute_delimiters() {
    let value = "caf\u{e9} \u{1f600} \"'<&> &amp; \"'<&>";
    assert_eq!(
        html(rsx! { <p title={value}>{value}</p> }),
        "<p title=\"caf\u{e9} \u{1f600} &quot;&#39;&lt;&amp;&gt; &amp;amp; &quot;&#39;&lt;&amp;&gt;\">caf\u{e9} \u{1f600} \"'&lt;&amp;&gt; &amp;amp; \"'&lt;&amp;&gt;</p>"
    );
}

#[test]
fn renders_script_string_children_as_raw_text() {
    let script = r#"if (value < 10 && value > 0) console.log("in range")"#;
    assert_eq!(html(rsx! { <script>{script}</script> }), format!("<script>{script}</script>"));
}

#[test]
fn renders_empty_script_elements() {
    assert_eq!(html(rsx! { <script></script> }), "<script></script>");
}

#[test]
fn prevents_script_string_children_from_terminating_the_element() {
    let json = r#"{"imports":{"example":"/example.js?before=a&after=</ScRiPt><script>alert(1)</script>"}}"#;
    assert_eq!(
        html(rsx! { <script type="importmap">{json}</script> }),
        r#"<script type="importmap">{"imports":{"example":"/example.js?before=a&after=</\u0053cRiPt><\u0073cript>alert(1)</\u0073cript>"}}</script>"#
    );
}

#[test]
fn escapes_script_prefixes_conservatively_while_preserving_non_matching_text() {
    let script = "prescription pre<scription pre<Scription pre</scRipTion pre</ScripTion </ script> </script><script><!-- <script> -->";
    assert_eq!(
        html(rsx! { <script>{script}</script> }),
        concat!(
            "<script>",
            r"prescription pre<\u0073cription pre<\u0053cription pre</\u0073cRipTion pre</\u0053cripTion ",
            r"</ script> </\u0073cript><\u0073cript><!-- <\u0073cript> -->",
            "</script>"
        )
    );
}

#[test]
fn renders_svg_script_string_children_as_raw_text() {
    let script = r#"if (value < 10 && value > 0) console.log("in range")"#;
    assert_eq!(html(rsx! { <svg><script>{script}</script></svg> }), format!("<svg><script>{script}</script></svg>"));
}

#[test]
fn renders_textarea_value_as_escaped_text_content() {
    assert_eq!(html(rsx! { <textarea value="Hello <Ryan> & friends"></textarea> }), "<textarea>Hello &lt;Ryan&gt; &amp; friends</textarea>");
    assert_eq!(html(rsx! { <textarea defaultValue="Hello <Ryan> & friends"></textarea> }), "<textarea>Hello &lt;Ryan&gt; &amp; friends</textarea>");
}

#[test]
fn renders_fragments_and_lists() {
    let items = vec![rsx! { <li>"a"</li> }, rsx! { <li>"b"</li> }];
    assert_eq!(html(rsx! { <ul><>{"x"}{items}</></ul> }), "<ul>x<li>a</li><li>b</li></ul>");
}

// component nodes

#[derive(Clone, Default)]
struct GreetingProps {
    name: &'static str,
}

struct Greeting;

impl Component for Greeting {
    type Props = GreetingProps;
    fn setup(_: &GreetingProps, _: &Handle<Self>) -> Self {
        Greeting
    }
    fn render(&self, props: &GreetingProps, _: &Handle<Self>) -> Node {
        rsx! { <div>"Hello, " {props.name} "!"</div> }
    }
}

#[test]
fn renders_component_nodes() {
    assert_eq!(html(rsx! { <Greeting name="World"/> }), "<div>Hello, World!</div>");
}

/// Setup-scope state read by render, like a Remix component's closure variables.
struct Zero {
    n: i32,
}

impl Component for Zero {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Zero { n: 0 }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <span>{self.n}</span> }
    }
}

#[test]
fn renders_zero_from_component_state() {
    assert_eq!(html(rsx! { <Zero/> }), "<span>0</span>");
}

#[derive(Clone, Default)]
struct ChildProps {
    children: Children,
}

struct Theme {
    color: &'static str,
    size: u32,
}

struct User {
    name: &'static str,
}

/// Provides a value from setup and renders its children.
macro_rules! provider {
    ($name:ident, $value:expr) => {
        struct $name;
        impl Component for $name {
            type Props = ChildProps;
            fn setup(_: &ChildProps, h: &Handle<Self>) -> Self {
                h.set_context($value);
                $name
            }
            fn render(&self, props: &ChildProps, _: &Handle<Self>) -> Node {
                rsx! { <>{&props.children}</> }
            }
        }
    };
}

provider!(ThemeProvider, Theme { color: "blue", size: 16 });
provider!(RedThemeProvider, Theme { color: "red", size: 0 });
provider!(UserProvider, User { name: "John" });
provider!(CountProvider, 42_i32);

struct ThemedText {
    theme: std::rc::Rc<Theme>,
}

impl Component for ThemedText {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        ThemedText { theme: h.context::<Theme>().expect("theme") }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let style = format!("color: {}; font-size: {}px", self.theme.color, self.theme.size);
        rsx! { <p style={style}>"Themed!"</p> }
    }
}

#[test]
fn provides_and_reads_context() {
    assert_eq!(
        html(rsx! { <ThemeProvider><div><ThemedText/></div></ThemeProvider> }),
        r#"<div><p style="color: blue; font-size: 16px">Themed!</p></div>"#
    );
}

struct Display;

impl Component for Display {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Display
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let (theme, user) = (h.context::<Theme>().expect("theme"), h.context::<User>().expect("user"));
        rsx! { <p style={format!("color: {}", theme.color)}>"Hello, " {user.name} "!"</p> }
    }
}

#[test]
fn provides_and_reads_nested_context() {
    assert_eq!(
        html(rsx! { <RedThemeProvider><UserProvider><div><Display/></div></UserProvider></RedThemeProvider> }),
        r#"<div><p style="color: red">Hello, John!</p></div>"#
    );
}

#[test]
fn reads_the_nearest_provider_of_a_context_type() {
    assert_eq!(
        html(rsx! { <ThemeProvider><RedThemeProvider><ThemedText/></RedThemeProvider><ThemedText/></ThemeProvider> }),
        r#"<p style="color: red; font-size: 0px">Themed!</p><p style="color: blue; font-size: 16px">Themed!</p>"#
    );
}

struct CountDisplay;

impl Component for CountDisplay {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        CountDisplay
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! { <span>"Count: " {*h.context::<i32>().expect("count")}</span> }
    }
}

struct DoubleDisplay {
    count: i32,
}

impl Component for DoubleDisplay {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        DoubleDisplay { count: *h.context::<i32>().expect("count") }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <span>"Double: " {self.count * 2}</span> }
    }
}

#[test]
fn provides_context_to_multiple_consumers() {
    assert_eq!(
        html(rsx! { <CountProvider><div><CountDisplay/><br/><DoubleDisplay/></div></CountProvider> }),
        "<div><span>Count: 42</span><br><span>Double: 84</span></div>"
    );
}

struct Leaf;

impl Component for Leaf {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Leaf
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! { <i id={h.dom_id()}></i> }
    }
}

struct Mid;

impl Component for Mid {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Mid
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <Leaf/> }
    }
}

#[test]
fn gives_sibling_components_unique_ids_after_nested_components() {
    let out = html(rsx! { <div><Mid/><Mid/><Leaf/></div> });
    let ids: Vec<&str> = out.split("id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    assert_eq!(ids.len(), 3, "{out}");
    assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), 3, "{out}");
}

thread_local! {
    static SEEN: RefCell<Option<(String, String, bool)>> = const { RefCell::new(None) };
}

struct Inspect;

impl Component for Inspect {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let (frame, top) = (h.frame(), h.frames().top());
        SEEN.with(|s| *s.borrow_mut() = Some((frame.src(), top.src(), frame == top)));
        Inspect
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! { <div>{h.frames().top().src()}</div> }
    }
}

#[test]
fn exposes_the_current_and_top_frame_src_during_ssr() {
    rmx::set_server_frame_src("https://example.com/dashboard", "https://example.com/dashboard");
    let out = html(rsx! { <Inspect/> });
    let seen = SEEN.with(|s| s.borrow_mut().take());
    assert_eq!(seen, Some(("https://example.com/dashboard".to_string(), "https://example.com/dashboard".to_string(), true)));
    assert_eq!(out, "<div>https://example.com/dashboard</div>");

    rmx::set_server_frame_src("https://example.com/sidebar", "https://example.com/dashboard");
    html(rsx! { <Inspect/> });
    let seen = SEEN.with(|s| s.borrow_mut().take());
    assert_eq!(seen, Some(("https://example.com/sidebar".to_string(), "https://example.com/dashboard".to_string(), false)));
}

/// rmx has no throw: a render that fails returns `h.fail(error)`. Remix rejects the whole
/// stream; rmx reports the error (stderr and the app's error listeners) and leaves the
/// component out, so the rest of the page still renders.
struct Failing;

impl Component for Failing {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Failing
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! { <p>"partial" {h.fail("Render error!")}</p> }
    }
}

#[test]
fn leaves_out_a_component_whose_render_fails() {
    assert_eq!(html(rsx! { <main><h1>"Title"</h1><Failing/><footer>"Footer"</footer></main> }), "<main><h1>Title</h1><footer>Footer</footer></main>");
}

struct Seo;

impl Component for Seo {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        Seo
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <><title>"Component Title"</title><meta name="description" content="Component Description"/></> }
    }
}

// special props

#[test]
fn renders_inner_html_on_elements() {
    assert_eq!(html(rsx! { <div innerHTML={unsafe_html("<b>Bold</b>")}></div> }), "<div><b>Bold</b></div>");
}

#[test]
fn renders_branded_iframe_srcdoc_as_escaped_attribute_markup() {
    assert_eq!(
        html(rsx! { <iframe srcdoc={unsafe_html("<p class=\"x\">Hi & bye</p>")}></iframe> }),
        r#"<iframe srcdoc="&lt;p class=&quot;x&quot;&gt;Hi &amp; bye&lt;/p&gt;"></iframe>"#
    );
}

#[test]
fn omits_invalid_and_reserved_host_prop_names() {
    let s = |v: &'static str| AttrValue::Str(v.into());
    let el = Node::element("div", None, vec![("onclick", s("alert(1)")), ("data ok", s("x")), ("title", s("kept"))], vec![], vec![]);
    assert_eq!(html(el), r#"<div title="kept"></div>"#);
}

#[test]
fn blocks_javascript_urls_in_executable_url_attributes() {
    let sneaky = "\u{0} \tJ\na\rv\ta\ns\rc\tr\ni\tp\tt:alert(1)";
    let out = html(rsx! {
        <>
            <a id="link" href="javascript:alert(1)"></a>
            <img id="image" alt="" src={sneaky}/>
            <form id="form" action="javascript:alert(1)"></form>
            <button id="button" formaction="javascript:alert(1)"></button>
            <svg><use id="use" xlink:href="javascript:alert(1)"></use></svg>
            <object id="object" data="javascript:alert(1)"></object>
        </>
    });
    assert!(!out.contains("alert(1)"), "{out}");
    assert_eq!(out.matches("has blocked a javascript: URL as a security precaution").count(), 6, "{out}");
}

#[test]
fn preserves_allowed_urls_and_non_executable_url_attributes() {
    assert_eq!(
        html(rsx! {
            <>
                <a href="/docs"></a>
                <a href="mailto:test@example.com"></a>
                <a href="blob:https://example.com/id"></a>
                <img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP"/>
                <img alt="" srcset="javascript:alert(1) 1x"/>
                <video poster="javascript:alert(1)"></video>
                <div data="javascript:alert(1)"></div>
            </>
        }),
        concat!(
            r#"<a href="/docs"></a><a href="mailto:test@example.com"></a><a href="blob:https://example.com/id"></a>"#,
            r#"<img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP"><img alt="" srcset="javascript:alert(1) 1x">"#,
            r#"<video poster="javascript:alert(1)"></video><div data="javascript:alert(1)"></div>"#
        )
    );
}

#[test]
fn renders_booleanish_string_attributes_as_explicit_false_values() {
    assert_eq!(
        html(rsx! { <div contenteditable={false} draggable={false} spellcheck={false}><input value={false}/></div> }),
        r#"<div contenteditable="false" draggable="false" spellcheck="false"><input value="false"></div>"#
    );
}

#[test]
fn preserves_boolean_and_escaped_values_in_html_and_svg_attributes() {
    let title: Option<&'static str> = None;
    assert_eq!(
        html(rsx! {
            <>
                <input disabled={true} required={false} value={0} title="A & \"B\""/>
                <div contenteditable={true} draggable={false} title={title}></div>
                <svg preserveAlpha={false} focusable={true}><use xlink:href="#safe"></use></svg>
            </>
        }),
        concat!(
            r#"<input disabled value="0" title="A &amp; &quot;B&quot;">"#,
            r#"<div contenteditable="true" draggable="false"></div>"#,
            r##"<svg preserveAlpha="false" focusable="true"><use xlink:href="#safe"></use></svg>"##
        )
    );
}

#[test]
fn renders_translate_with_yes_no_attribute_values() {
    assert_eq!(html(rsx! { <div translate="no"></div> }), r#"<div translate="no"></div>"#);
}

#[test]
fn renders_input_default_value_as_a_value_attribute() {
    assert_eq!(html(rsx! { <input defaultValue="Hello \"Ryan\" & friends"/> }), r#"<input value="Hello &quot;Ryan&quot; &amp; friends">"#);
}

#[test]
fn renders_input_default_checked_as_a_checked_attribute() {
    assert_eq!(html(rsx! { <input type="checkbox" defaultChecked={true}/> }), r#"<input type="checkbox" checked>"#);
}

#[test]
fn handles_table_attributes_colspan_and_rowspan() {
    assert_eq!(
        html(rsx! { <table><tr><td colspan={2} rowspan={3}>"Cell"</td></tr></table> }),
        r#"<table><tr><td colspan="2" rowspan="3">Cell</td></tr></table>"#
    );
}

#[test]
fn serializes_css_mixin_styles_into_style_tags_and_class_names() {
    let out = html(rsx! { <html><head></head><body><div mix={css("color: red")}>"x"</div></body></html> });
    let class = out.split("class=\"").nth(1).and_then(|s| s.split('"').next()).expect("class").to_string();
    assert!(class.starts_with("rmxc-"), "{out}");
    assert!(out.contains(&format!(r#"<style data-rmx-style="{class}">@layer rmx.{class} {{ .{class} {{ color: red }} }}</style></head>"#)), "{out}");
}

#[test]
fn prevents_css_mixin_styles_from_breaking_out_of_style_tags() {
    let out = html(rsx! { <html><head></head><body><div mix={css("content: '</style><script>alert(1)</script>'")}></div></body></html> });
    assert!(!out.contains("</style><script>"), "{out}");
}

/// The generated class names in `out` that `tag` elements carry, in order.
fn classes_of(out: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag} class=\"");
    out.match_indices(&open).map(|(i, _)| out[i + open.len()..].split('"').next().unwrap().to_string()).collect()
}

#[test]
fn handles_css_mixin_styles() {
    let out = html(rsx! { <div mix={css("color: red; font-size: 16px")}>"Styled text"</div> });
    assert!(out.contains("<style data-rmx-style="), "{out}");
    assert!(out.contains(".rmxc-"), "{out}");
    assert!(out.contains("color: red; font-size: 16px"), "{out}");
    let class = &classes_of(&out, "div")[0];
    assert!(class.starts_with("rmxc-") && class[5..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "{out}");
}

#[test]
fn passes_string_style_attributes_through() {
    assert_eq!(
        html(rsx! { <div style="color: blue; font-weight: bold;">"String styled"</div> }),
        r#"<div style="color: blue; font-weight: bold;">String styled</div>"#
    );
}

#[test]
fn combines_css_mixin_with_an_existing_class() {
    let out = html(rsx! { <div class="existing-class" mix={css("background: yellow")}>"Combined classes"</div> });
    assert!(out.contains(r#"<div class="existing-class rmxc-"#), "{out}");
    assert!(out.contains("background: yellow"), "{out}");
}

#[test]
fn deduplicates_styles_across_multiple_elements() {
    let out = html(rsx! {
        <div>
            <span mix={css("color: red; font-size: 14px")}>"First"</span>
            <span mix={css("color: red; font-size: 14px")}>"Second"</span>
            <span mix={css("color: blue")}>"Third"</span>
        </div>
    });
    assert_eq!(out.matches("color: red").count(), 1, "{out}");
    assert!(out.contains("color: blue"), "{out}");
    let spans = classes_of(&out, "span");
    assert_eq!(spans.len(), 3, "{out}");
    assert_eq!(spans[0], spans[1]);
    assert_ne!(spans[0], spans[2]);
}

#[test]
fn places_styles_in_head_when_an_html_root_exists() {
    let out = html(rsx! { <html><body><div mix={css("color: purple")}>"Content"</div></body></html> });
    assert!(out.contains("<html><head><style data-rmx-style="), "{out}");
    assert!(out.contains("color: purple"), "{out}");
    assert!(out.contains("</style></head><body>"), "{out}");
}

#[test]
fn places_styles_in_a_head_when_there_is_no_html_root() {
    let out = html(rsx! { <div mix={css("color: orange")}>"No HTML root"</div> });
    assert!(out.starts_with("<head><style data-rmx-style="), "{out}");
    assert!(out.contains("color: orange"), "{out}");
    let class = &classes_of(&out, "div")[0];
    assert!(out.ends_with(&format!(r#"</style></head><div class="{class}">No HTML root</div>"#)), "{out}");
}

#[test]
fn handles_css_mixin_in_components() {
    struct StyledButton;
    impl Component for StyledButton {
        type Props = &'static str;
        fn setup(_: &&'static str, _: &Handle<Self>) -> Self {
            StyledButton
        }
        fn render(&self, label: &&'static str, _: &Handle<Self>) -> Node {
            rsx! { <button mix={css("background: blue; color: white; padding: 10px")}>{*label}</button> }
        }
    }
    let out = html(rsx! { <div>{Node::component::<StyledButton>(None, "Click me")}{Node::component::<StyledButton>(None, "And me")}</div> });
    assert_eq!(out.matches("background: blue").count(), 1, "{out}");
    let buttons = classes_of(&out, "button");
    assert_eq!(buttons.len(), 2, "{out}");
    assert_eq!(buttons[0], buttons[1]);
}

#[test]
fn adds_no_class_or_style_for_an_empty_css_mixin() {
    assert_eq!(html(rsx! { <div mix={css("")}>"Empty css mixin"</div> }), "<div>Empty css mixin</div>");
    assert_eq!(html(rsx! { <div mix={css(" \n\t ")}>"Blank css mixin"</div> }), "<div>Blank css mixin</div>");
}

#[test]
fn handles_pseudo_selectors_in_css_mixin() {
    let out = html(rsx! { <button mix={css("color: black; &:hover { color: red } &:focus { outline: 2px solid blue }")}>"Hover me"</button> });
    assert!(out.contains(":hover"), "{out}");
    assert!(out.contains("color: red"), "{out}");
    assert!(out.contains(":focus"), "{out}");
    assert!(out.contains("outline: 2px solid blue"), "{out}");
}

#[test]
fn handles_media_queries_in_css_mixin() {
    let out = html(rsx! { <div mix={css("font-size: 14px; @media (min-width: 768px) { font-size: 16px }")}>"Responsive text"</div> });
    assert!(out.contains("@media (min-width: 768px)"), "{out}");
    assert!(out.contains("font-size: 16px"), "{out}");
}

#[test]
fn preserves_range_media_query_operators_in_css_mixin() {
    let out = html(rsx! { <div mix={css("@media (width < 900px) { display: none }")}></div> });
    assert!(out.contains("@media (width < 900px)"), "{out}");
}

#[test]
fn merges_styles_with_existing_head_content() {
    let out = html(rsx! {
        <html>
            <head><title>"Page Title"</title><meta charset="utf-8"/></head>
            <body><div mix={css("font-weight: bold")}>"Bold text"</div></body>
        </html>
    });
    let head = &out[out.find("<head>").expect("head") + 6..out.find("</head>").expect("head end")];
    assert!(head.starts_with(r#"<title>Page Title</title><meta charset="utf-8"><style data-rmx-style="#), "{out}");
    assert!(head.contains("font-weight: bold"), "{out}");
    assert_eq!(out.matches("<head>").count(), 1, "{out}");
}

#[test]
fn places_styles_after_the_html_start_tag_when_a_document_has_no_head() {
    let out = html(rsx! { <html lang="en"><body><main mix={css("color: teal")}>"Main"</main></body></html> });
    assert!(out.starts_with(r#"<html lang="en"><head><style data-rmx-style="#), "{out}");
    assert!(out.contains("</style></head><body><main class=\"rmxc-"), "{out}");
}

// svg

#[test]
fn renders_svg_with_preserved_view_box_and_kebab_attributes() {
    assert_eq!(
        html(rsx! {
            <svg viewBox="0 0 24 24" fill="none" class="icon">
                <path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5"></path>
            </svg>
        }),
        r#"<svg viewBox="0 0 24 24" fill="none" class="icon"><path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5"></path></svg>"#
    );
}

#[test]
fn renders_foreign_object_subtree_as_html() {
    assert_eq!(
        html(rsx! { <svg><foreignObject><div id="x" class="a">"Hello"</div></foreignObject></svg> }),
        r#"<svg><foreignObject><div id="x" class="a">Hello</div></foreignObject></svg>"#
    );
}

#[test]
fn renders_xml_lang_and_xml_space() {
    assert_eq!(
        html(rsx! { <svg><text xml:lang="en" xml:space="preserve">"Hi"</text></svg> }),
        r#"<svg><text xml:lang="en" xml:space="preserve">Hi</text></svg>"#
    );
}

#[test]
fn renders_canonical_camel_case_unit_attributes_for_svg_filters_and_gradients() {
    let out = html(rsx! {
        <svg><defs>
            <filter id="f" filterUnits="userSpaceOnUse" primitiveUnits="objectBoundingBox"><feGaussianBlur stdDeviation="2.5"></feGaussianBlur></filter>
            <linearGradient id="g" gradientUnits="userSpaceOnUse"></linearGradient>
        </defs></svg>
    });
    for kept in [r#"filterUnits="userSpaceOnUse""#, r#"primitiveUnits="objectBoundingBox""#, r#"gradientUnits="userSpaceOnUse""#, r#"stdDeviation="2.5""#] {
        assert!(out.contains(kept), "{kept} in {out}");
    }
}

#[test]
fn renders_svg_booleanish_string_attributes_as_explicit_values() {
    let out = html(rsx! {
        <svg>
            <animate autoReverse={false}></animate>
            <rect externalResourcesRequired={false} focusable={false}></rect>
            <feColorMatrix preserveAlpha={false}></feColorMatrix>
        </svg>
    });
    for kept in [r#"autoReverse="false""#, r#"externalResourcesRequired="false""#, r#"focusable="false""#, r#"preserveAlpha="false""#] {
        assert!(out.contains(kept), "{kept} in {out}");
    }
}

// doctype and head-like elements

#[test]
fn handles_whitespace_before_html_element() {
    assert_eq!(html(rsx! { <>"\n  "<html><body>"Content"</body></html></> }), "\n  <html><body>Content</body></html>");
}

#[test]
fn renders_title_elements_in_place() {
    assert_eq!(
        html(rsx! { <html><body><title>"Page Title"</title><div>"Content"</div></body></html> }),
        "<html><body><title>Page Title</title><div>Content</div></body></html>"
    );
}

#[test]
fn renders_meta_and_link_elements_in_place() {
    assert_eq!(
        html(rsx! { <div><meta name="description" content="Test page"/><h1>"Hello"</h1><link rel="stylesheet" href="/styles.css"/></div> }),
        r#"<div><meta name="description" content="Test page"><h1>Hello</h1><link rel="stylesheet" href="/styles.css"></div>"#
    );
}

#[test]
fn renders_head_like_elements_from_components_in_place() {
    assert_eq!(
        html(rsx! { <div><Seo/><main>"Content"</main></div> }),
        r#"<div><title>Component Title</title><meta name="description" content="Component Description"><main>Content</main></div>"#
    );
}

#[test]
fn keeps_bare_head_like_elements_out_of_an_explicit_head_tag() {
    assert_eq!(
        html(rsx! {
            <html>
                <head><meta charset="utf-8"/></head>
                <body><title>"Body Title"</title><link rel="stylesheet" href="/app.css"/><div>"Content"</div></body>
            </html>
        }),
        r#"<html><head><meta charset="utf-8"></head><body><title>Body Title</title><link rel="stylesheet" href="/app.css"><div>Content</div></body></html>"#
    );
}

#[test]
fn renders_structured_data_and_regular_scripts_in_place() {
    assert_eq!(
        html(rsx! {
            <div>
                <script type="text/javascript" innerHTML={unsafe_html("console.log('Regular script')")}></script>
                <script type="application/ld+json" innerHTML={unsafe_html(r#"{"@context":"https://schema.org","@type":"WebPage"}"#)}></script>
                <h1>"Mixed Scripts Page"</h1>
            </div>
        }),
        r#"<div><script type="text/javascript">console.log('Regular script')</script><script type="application/ld+json">{"@context":"https://schema.org","@type":"WebPage"}</script><h1>Mixed Scripts Page</h1></div>"#
    );
}

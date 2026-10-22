//! Ports of Remix's `stream.test.tsx` server rendering cases (special props, svg,
//! doctype, head-like elements, styles). rsx takes HTML attribute names (`class`, `for`,
//! `stroke-linecap`), so the JSX camelCase mapping cases become their HTML spelling, and
//! void elements are written `<input>` rather than `<input />`.

extern crate rmx_component as rmx;

use rmx::{css, render_to_string, rsx, unsafe_html, AttrValue, Component, Handle, Node};

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

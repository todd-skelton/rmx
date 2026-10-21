//! Ported from `safe-html.test.ts`, one test per Remix case. Remix's "throws when not used
//! as a template tag" cases are compile errors here (see the `compile_fail` examples on the
//! crate docs).

use super::*;

// --- html ----------------------------------------------------------------------------------

#[test]
fn html_returns_a_safe_html_value() {
    let value: SafeHtml = html!("<h1>Hello</h1>");
    assert_eq!(value.to_string(), "<h1>Hello</h1>");
}

#[test]
fn html_escapes_special_characters() {
    assert_eq!(html!("<h1>{}</h1>", "<script>alert(1)</script>"), "<h1>&lt;script&gt;alert(1)&lt;/script&gt;</h1>");
    assert_eq!(html!("<p title=\"{}\">{}</p>", "a \"quote\" & 'apostrophe'", 1.5), "<p title=\"a &quot;quote&quot; &amp; &#39;apostrophe&#39;\">1.5</p>");
}

#[test]
fn html_preserves_nested_safe_html_fragments() {
    assert_eq!(html!("<h1>{} World</h1>", html_raw!("<b>Hello</b>")), "<h1><b>Hello</b> World</h1>");
}

#[test]
fn html_flattens_arrays_and_preserves_nested_escapes() {
    let items: Vec<Box<dyn ToHtml>> = vec![Box::new("<li>"), Box::new(html_raw!("<b>Hello</b>")), Box::new("</li>")];
    assert_eq!(html!("<ul>{items}</ul>"), "<ul>&lt;li&gt;<b>Hello</b>&lt;/li&gt;</ul>");
}

#[test]
fn html_handles_numbers_and_booleans() {
    assert_eq!(html!("<div>{} {} {}</div>", 42, true, false), "<div>42 true false</div>");
}

#[test]
fn html_handles_none() {
    let nothing: Option<&str> = None;
    assert_eq!(html!("<div>{nothing}{nothing}</div>"), "<div></div>");
}

#[test]
fn placeholders_resolve_names_positions_and_captures() {
    let who = "<you>";
    assert_eq!(html!("{0}{1}{0} {name} {who}", "a", "b", name = "<n>"), "aba &lt;n&gt; &lt;you&gt;");
    assert_eq!(html!("{{literal}} {}", 1), "{literal} 1");
    let mut calls = 0;
    let mut next = || {
        calls += 1;
        calls
    };
    assert_eq!(html!("{n}{n}", n = next()), "11", "named arguments are evaluated once");
}

// --- html.raw ------------------------------------------------------------------------------

#[test]
fn raw_does_not_escape_interpolated_strings() {
    let raw_html = "<b>Bold</b>";
    assert_eq!(html_raw!("<div>{raw_html}</div>"), "<div><b>Bold</b></div>");
}

#[test]
fn raw_does_not_escape_numbers() {
    let num = 42;
    assert_eq!(html_raw!("<div>{num}</div>"), "<div>42</div>");
}

#[test]
fn raw_does_not_escape_boolean_values() {
    let flag = true;
    assert_eq!(html_raw!("<div>{flag}</div>"), "<div>true</div>");
}

#[test]
fn raw_handles_none() {
    let none: Option<String> = None;
    assert_eq!(html_raw!("<div>{none}{none}</div>"), "<div></div>");
}

#[test]
fn raw_handles_false() {
    assert_eq!(html_raw!("<div>{}</div>", false), "<div>false</div>");
}

#[test]
fn raw_preserves_safe_html_fragments() {
    let icon = html_raw!("<i>icon</i>");
    assert_eq!(html_raw!("<div>{icon}</div>"), "<div><i>icon</i></div>");
}

#[test]
fn raw_does_not_escape_dangerous_html_characters() {
    let dangerous = "<script>alert(\"XSS\")</script>";
    assert_eq!(html_raw!("<div>{dangerous}</div>"), "<div><script>alert(\"XSS\")</script></div>");
}

#[test]
fn raw_flattens_arrays_without_escaping() {
    let items = ["<li>A</li>", "<li>B</li>", "<li>C</li>"];
    assert_eq!(html_raw!("<ul>{items}</ul>"), "<ul><li>A</li><li>B</li><li>C</li></ul>");
}

#[test]
fn raw_handles_nested_arrays() {
    let nested = [["<a>", "<b>"], ["</b>", "</a>"]];
    assert_eq!(html_raw!("<div>{nested}</div>"), "<div><a><b></b></a></div>");
}

#[test]
fn raw_can_be_used_to_build_pre_escaped_html_snippets() {
    let escaped = "<script>alert(1)</script>".replace('<', "&lt;").replace('>', "&gt;");
    assert_eq!(html_raw!("<div class=\"content\">{escaped}</div>"), "<div class=\"content\">&lt;script&gt;alert(1)&lt;/script&gt;</div>");
}

#[test]
fn raw_works_with_multiple_interpolations() {
    let (title, body, footer) = ("<h1>Title</h1>", "<p>Content</p>", "<footer>Footer</footer>");
    assert_eq!(html_raw!("<div>{title}{body}{footer}</div>"), "<div><h1>Title</h1><p>Content</p><footer>Footer</footer></div>");
}

#[test]
fn escape_html_borrows_when_nothing_changes() {
    assert!(matches!(escape_html("plain"), Cow::Borrowed("plain")));
    assert_eq!(escape_html("a<b"), "a&lt;b");
}

//! Ported from safe-html.test.ts.

use super::*;

#[test]
fn html_escapes_interpolations() {
    assert_eq!(html!("<h1>Hello</h1>"), "<h1>Hello</h1>");
    assert_eq!(html!("<h1>{}</h1>", "<script>alert(1)</script>"), "<h1>&lt;script&gt;alert(1)&lt;/script&gt;</h1>");
    assert_eq!(html!("<h1>{} World</h1>", html_raw!("<b>Hello</b>")), "<h1><b>Hello</b> World</h1>");
}

#[test]
fn html_flattens_sequences_and_keeps_nested_escapes() {
    let items: Vec<Box<dyn ToHtml>> = vec![Box::new("<li>"), Box::new(html_raw!("<b>Hello</b>")), Box::new("</li>")];
    assert_eq!(html!("<ul>{items}</ul>"), "<ul>&lt;li&gt;<b>Hello</b>&lt;/li&gt;</ul>");
}

#[test]
fn html_handles_numbers_booleans_and_none() {
    assert_eq!(html!("<div>{} {} {}</div>", 42, true, false), "<div>42 true false</div>");
    let nothing: Option<&str> = None;
    assert_eq!(html!("<div>{nothing}{nothing}</div>"), "<div></div>");
    assert_eq!(html!("<p title=\"{}\">{}</p>", "a \"quote\" & 'apostrophe'", 1.5), "<p title=\"a &quot;quote&quot; &amp; &#39;apostrophe&#39;\">1.5</p>");
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

#[test]
fn html_raw_does_not_escape() {
    let raw = "<b>Bold</b>";
    assert_eq!(html_raw!("<div>{raw}</div>"), "<div><b>Bold</b></div>");
    assert_eq!(html_raw!("<div>{}</div>", 42), "<div>42</div>");
    assert_eq!(html_raw!("<div>{}</div>", true), "<div>true</div>");
    assert_eq!(html_raw!("<div>{}</div>", false), "<div>false</div>");
    let none: Option<String> = None;
    assert_eq!(html_raw!("<div>{none}{none}</div>"), "<div></div>");
    let icon = html_raw!("<i>icon</i>");
    assert_eq!(html_raw!("<div>{icon}</div>"), "<div><i>icon</i></div>");
    assert_eq!(html_raw!("<div>{}</div>", "<script>alert(\"XSS\")</script>"), "<div><script>alert(\"XSS\")</script></div>");
    let items = ["<li>A</li>", "<li>B</li>", "<li>C</li>"];
    assert_eq!(html_raw!("<ul>{items}</ul>"), "<ul><li>A</li><li>B</li><li>C</li></ul>");
    let nested = [["<a>", "<b>"], ["</b>", "</a>"]];
    assert_eq!(html_raw!("<div>{nested}</div>"), "<div><a><b></b></a></div>");
    let escaped = "<script>alert(1)</script>".replace('<', "&lt;").replace('>', "&gt;");
    assert_eq!(html_raw!("<div class=\"content\">{escaped}</div>"), "<div class=\"content\">&lt;script&gt;alert(1)&lt;/script&gt;</div>");
    let (title, body, footer) = ("<h1>Title</h1>", "<p>Content</p>", "<footer>Footer</footer>");
    assert_eq!(html_raw!("<div>{title}{body}{footer}</div>"), "<div><h1>Title</h1><p>Content</p><footer>Footer</footer></div>");
}

#[test]
fn escape_html_borrows_when_nothing_changes() {
    assert!(matches!(escape_html("plain"), Cow::Borrowed("plain")));
    assert_eq!(escape_html("a<b"), "a&lt;b");
}

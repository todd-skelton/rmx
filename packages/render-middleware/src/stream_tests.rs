//! Ports of the frames section of Remix's `stream.test.tsx`. Remix renders with a
//! `resolveFrame` callback; rsmix resolves frames through the router, so each case is a
//! small app whose frame routes stand in for the callback. A frame route that should stay
//! pending waits on a [`Gate`] the test opens.
//!
//! Not ported: import map and module preload cases (rsmix ships one wasm bundle), and
//! `onError` for frame failures (a failed frame response is used as the frame's content).

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Waker};

use rsmix::__private::serde_json;

use rsmix::{css, rsx, Component, Frame, Handle, Node};
use rsmix_fetch::{block_on, Body, BodySender};
use rsmix_response as response;
use rsmix_fetch_router::{Context, Router};

use super::Render;

/// A frame route that waits until the test sends its HTML.
#[derive(Clone)]
struct Gate(Arc<Mutex<Option<Body>>>);

fn gate() -> (BodySender, Gate) {
    let (tx, body) = Body::channel();
    (tx, Gate(Arc::new(Mutex::new(Some(body)))))
}

impl Gate {
    async fn html(&self) -> String {
        let body = self.0.lock().unwrap().take().expect("gate opened once");
        body.text().await.unwrap()
    }
}

fn open(tx: BodySender, html: &str) {
    tx.send(html.to_string());
    drop(tx);
}

fn body(router: &Router, url: &str) -> Body {
    block_on(router.fetch_url(url)).into_body()
}

fn next(body: &mut Body) -> Option<String> {
    block_on(body.chunk()).map(|c| String::from_utf8(c.unwrap().to_vec()).unwrap())
}

/// Whether the body has no chunk ready (polled without waiting).
fn waiting(body: &mut Body) -> bool {
    let mut cx = TaskContext::from_waker(Waker::noop());
    let mut fut = std::pin::pin!(body.chunk());
    (0..3).all(|_| fut.as_mut().poll(&mut cx).is_pending())
}

fn drain(router: &Router, url: &str) -> String {
    block_on(body(router, url).text()).unwrap()
}

/// `<!--rmx:f:ID-->` becomes `<!--rmx:f-->`, so tests can compare markup.
fn without_ids(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find("<!--rmx:f:") {
        out.push_str(&rest[..i]);
        out.push_str("<!--rmx:f-->");
        rest = &rest[i + rest[i..].find("-->").unwrap() + 3..];
    }
    out.push_str(rest);
    out
}

fn frame_data(html: &str) -> serde_json::Value {
    let start = html.find(r#"id="rmx-data">"#).expect("rmx-data") + r#"id="rmx-data">"#.len();
    let end = start + html[start..].find("</script>").unwrap();
    let data: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
    data["f"].clone()
}

fn statuses(html: &str) -> Vec<String> {
    let f = frame_data(html);
    f.as_object().unwrap().values().map(|v| v["status"].as_str().unwrap().to_string()).collect()
}

#[test]
fn non_blocking_frames_send_the_fallback_then_stream_a_template() {
    let (tx, product) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! {
            <div>
                <h1>"Page Title"</h1>
                <Frame src="/fragments/product" fallback={rsx! { <div>"Loading product..."</div> }}/>
                <p>"Footer content"</p>
            </div>
        })
    });
    router.get("/fragments/product", move |_ctx: Context| {
        let product = product.clone();
        async move { response::html(product.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(
        without_ids(&first).starts_with("<div><h1>Page Title</h1><!--rmx:f--><div>Loading product...</div><!--/rmx:f--><p>Footer content</p></div>"),
        "{first}"
    );
    let f = frame_data(&first);
    let (id, entry) = f.as_object().unwrap().iter().next().unwrap();
    assert_eq!(entry, &serde_json::json!({ "src": "/fragments/product", "name": null, "status": "pending" }));
    assert!(first.contains(&format!("<!--rmx:f:{id}-->")), "{first}");
    assert!(first.ends_with("<!-- rmx:flush fragment -->"), "{first}");
    assert!(waiting(&mut res));

    open(tx, "<div>Async Product Content</div>");
    let second = next(&mut res).unwrap();
    assert_eq!(second, format!("<template id=\"{id}\"><div>Async Product Content</div><!--rmx:te--></template>"));
    assert_eq!(next(&mut res), None);
}

#[test]
fn blocking_frames_render_in_one_chunk_with_resolved_data() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/fragments/product" name="test"/> }) });
    router.get("/fragments/product", |_ctx: Context| async move { response::html("<section>Resolved</section>") });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    let f = frame_data(&first);
    let (id, entry) = f.as_object().unwrap().iter().next().unwrap();
    assert_eq!(entry, &serde_json::json!({ "src": "/fragments/product", "name": "test", "status": "resolved" }));
    assert!(first.starts_with(&format!("<!--rmx:f:{id}--><section>Resolved</section><!--/rmx:f-->")), "{first}");
    assert!(!first.contains("rmx:flush"), "{first}");
    assert_eq!(next(&mut res), None);
}

#[test]
fn strips_doctype_markup_from_frame_templates() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <Frame src="/x" fallback={rsx! { <div>"Loading..."</div> }}/> })
    });
    router.get("/x", |ctx: Context| async move {
        ctx.render(rsx! { <html><head></head><body><div mix={css("color: rebeccapurple")}>"Resolved"</div></body></html> })
    });
    let html = drain(&router, "http://localhost/");
    assert!(!html.to_ascii_lowercase().contains("<!doctype"), "{html}");
    let template = &html[html.find("<template").expect("template")..];
    assert!(template.contains("<head><style data-rmx-style="), "{html}");
    assert!(template.contains("<div class=\"rmxc-"), "{html}");
    assert!(template.contains(">Resolved</div>"), "{html}");
}

#[test]
fn keeps_frame_content_in_the_template_when_the_doctype_is_its_own_chunk() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <Frame src="/x" fallback={rsx! { <div>"Loading..."</div> }}/> })
    });
    router.get("/x", |_ctx: Context| async move {
        let (tx, body) = Body::channel();
        tx.send("<!DOCTYPE html>");
        tx.send("<div id=\"resolved\">Resolved</div>");
        drop(tx);
        response::html_stream(body)
    });
    let html = drain(&router, "http://localhost/");
    assert!(!html.contains("<!DOCTYPE"), "{html}");
    assert!(html.contains("\"><div id=\"resolved\">Resolved</div><!--rmx:te--></template>"), "{html}");
}

#[test]
fn keeps_sibling_order_and_frames_inside_fallbacks_across_the_first_chunk() {
    let (tx, pending) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        let nested = rsx! { <><i>"Loading"</i><Frame src="/nested"/></> };
        ctx.render(rsx! {
            <main>
                "before"
                <Frame src="/blocking"/>
                <span>"between"</span>
                <Frame src="/pending" fallback={nested}/>
                "after"
            </main>
        })
    });
    router.get("/blocking", |_ctx: Context| async move { response::html("<b>Blocking</b>") });
    router.get("/nested", |_ctx: Context| async move { response::html("<em>Nested</em>") });
    router.get("/pending", move |_ctx: Context| {
        let pending = pending.clone();
        async move { response::html(pending.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(
        without_ids(&first).starts_with(concat!(
            "<main>before<!--rmx:f--><b>Blocking</b><!--/rmx:f--><span>between</span>",
            "<!--rmx:f--><i>Loading</i><!--rmx:f--><em>Nested</em><!--/rmx:f-->",
            "<!--/rmx:f-->after</main>",
        )),
        "{first}"
    );
    let mut s = statuses(&first);
    s.sort();
    assert_eq!(s, ["pending", "resolved", "resolved"]);
    assert!(waiting(&mut res));
    open(tx, "<strong>Done</strong>");
    assert!(next(&mut res).unwrap().contains("<strong>Done</strong>"));
    assert_eq!(next(&mut res), None);
}

#[test]
fn assigns_ids_to_nested_non_blocking_frames() {
    let (tx, outer) = gate();
    let (tx_inner, inner) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        let fallback = rsx! {
            <div>"Outer loading"<Frame src="/inner" fallback={rsx! { <span>"Inner loading"</span> }}/></div>
        };
        ctx.render(rsx! { <div><Frame src="/outer" fallback={fallback}/></div> })
    });
    router.get("/outer", move |_ctx: Context| {
        let outer = outer.clone();
        async move { response::html(outer.html().await) }
    });
    router.get("/inner", move |_ctx: Context| {
        let inner = inner.clone();
        async move { response::html(inner.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert_eq!(statuses(&first), ["pending", "pending"]);
    let f = frame_data(&first);
    let ids: Vec<&String> = f.as_object().unwrap().keys().collect();
    assert_ne!(ids[0], ids[1]);
    assert!(first.contains("Outer loading") && first.contains("Inner loading"), "{first}");
    open(tx_inner, "<div>Inner</div>");
    open(tx, "<div>Outer</div>");
    let rest = block_on(res.text()).unwrap();
    assert_eq!(rest.matches("<template id=").count(), 2, "{rest}");
}

#[test]
fn awaits_nested_blocking_frames_before_the_first_chunk() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/outer"/> }) });
    router.get("/outer", |ctx: Context| async move { ctx.render(rsx! { <div>"Outer"<Frame src="/inner"/></div> }) });
    router.get("/inner", |_ctx: Context| async move { response::html("<div>Inner</div>") });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(
        without_ids(&first).starts_with("<!--rmx:f--><div>Outer<!--rmx:f--><div>Inner</div><!--/rmx:f--></div><!--/rmx:f-->"),
        "{first}"
    );
    // The inner frame's data travels with the outer content into the page's one script.
    assert_eq!(first.matches("id=\"rmx-data\"").count(), 1, "{first}");
    assert_eq!(statuses(&first), ["resolved", "resolved"]);
    assert_eq!(next(&mut res), None);
}

#[test]
fn streams_a_non_blocking_frame_nested_in_a_blocking_one_later() {
    let (tx, inner) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/outer"/> }) });
    router.get("/outer", |ctx: Context| async move {
        ctx.render(rsx! { <div>"Outer"<Frame src="/inner" fallback={rsx! { <span>"Inner loading"</span> }}/></div> })
    });
    router.get("/inner", move |_ctx: Context| {
        let inner = inner.clone();
        async move { response::html(inner.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(first.contains("Outer") && first.contains("<span>Inner loading</span>"), "{first}");
    let mut s = statuses(&first);
    s.sort();
    assert_eq!(s, ["pending", "resolved"]);
    assert!(waiting(&mut res));
    open(tx, "<div>Inner</div>");
    let second = next(&mut res).unwrap();
    assert!(second.starts_with("<template id=\"f") && second.contains("<div>Inner</div>"), "{second}");
    assert_eq!(next(&mut res), None);
}

#[test]
fn escapes_frame_template_content_to_prevent_template_breakouts() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <Frame src="/danger" fallback={rsx! { <div>"Loading..."</div> }}/> })
    });
    router.get("/danger", |_ctx: Context| async move {
        response::html(r#"</template><script>alert("xss")</script><TEMPLATE><p>safe</p></Template>"#)
    });
    let html = drain(&router, "http://localhost/");
    let template = &html[html.find("<template id=").unwrap()..];
    assert!(template.contains(r#"<\/template><script>alert("xss")</script><TEMPLATE><p>safe</p><\/Template>"#), "{html}");
    assert!(!template.contains("</template><script>"), "{html}");
    assert!(template.ends_with("<!--rmx:te--></template>"), "{html}");
}

#[test]
fn streams_multiple_non_blocking_frames_in_order_of_resolution() {
    let (tx1, frame1) = gate();
    let (tx2, frame2) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! {
            <div>
                <Frame src="/frame1" fallback={rsx! { <div>"Loading frame 1..."</div> }}/>
                <Frame src="/frame2" fallback={rsx! { <div>"Loading frame 2..."</div> }}/>
            </div>
        })
    });
    router.get("/frame1", move |_ctx: Context| {
        let g = frame1.clone();
        async move { response::html(g.html().await) }
    });
    router.get("/frame2", move |_ctx: Context| {
        let g = frame2.clone();
        async move { response::html(g.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(first.contains("Loading frame 1...") && first.contains("Loading frame 2..."), "{first}");
    assert_eq!(statuses(&first).len(), 2);
    open(tx2, "<div>Second Frame Content</div>");
    let second = next(&mut res).unwrap();
    assert!(second.starts_with("<template id=\"f") && second.contains("Second Frame Content"), "{second}");
    open(tx1, "<div>First Frame Content</div>");
    let third = next(&mut res).unwrap();
    assert!(third.starts_with("<template id=\"f") && third.contains("First Frame Content"), "{third}");
    assert_eq!(next(&mut res), None);
}

#[test]
fn renders_descendant_frames_in_blocking_and_non_blocking_mixes() {
    let (tx, slow) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/parent"/> }) });
    router.get("/parent", |ctx: Context| async move {
        ctx.render(rsx! {
            <main>
                <h2>"Parent Header"</h2>
                <Frame src="/blocking-child"/>
                <Frame src="/non-blocking-child" fallback={rsx! { <span>"Loading..."</span> }}/>
                <p>"Parent Footer"</p>
            </main>
        })
    });
    router.get("/blocking-child", |_ctx: Context| async move { response::html("<div>Blocking Child</div>") });
    router.get("/non-blocking-child", move |_ctx: Context| {
        let g = slow.clone();
        async move { response::html(g.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(
        without_ids(&first).starts_with(concat!(
            "<!--rmx:f--><main><h2>Parent Header</h2><!--rmx:f--><div>Blocking Child</div><!--/rmx:f-->",
            "<!--rmx:f--><span>Loading...</span><!--/rmx:f--><p>Parent Footer</p></main><!--/rmx:f-->",
        )),
        "{first}"
    );
    open(tx, "<div>Non-blocking Child</div>");
    let second = next(&mut res).unwrap();
    assert!(second.starts_with("<template id=\"f") && second.contains("<div>Non-blocking Child</div>"), "{second}");
    assert_eq!(next(&mut res), None);
}

/// Remix repeats a rule in every frame template that uses it, because its client adopts
/// rules per frame. rsmix pins adopted server rules for the page, so the inlined outer frame's
/// rule moves into the page head and the streamed inner template carries its own copy.
#[test]
fn repeats_selector_addressed_styles_in_each_frame_template_that_uses_them() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move { ctx.render(rsx! { <Frame src="/outer"/> }) });
    router.get("/outer", |ctx: Context| async move {
        ctx.render(rsx! {
            <section mix={css("color: red")}>
                <Frame src="/inner" fallback={rsx! { <span>"Loading inner..."</span> }}/>
            </section>
        })
    });
    router.get("/inner", |ctx: Context| async move { ctx.render(rsx! { <span mix={css("color: red")}>"Inner"</span> }) });
    let html = drain(&router, "http://localhost/");
    let template = html.find("<template").expect("template");
    let tags: Vec<&str> = html.match_indices("data-rmx-style=\"").map(|(i, _)| html[i..].split('>').next().unwrap()).collect();
    assert_eq!(tags.len(), 2, "{html}");
    assert_eq!(tags[0], tags[1], "{html}");
    assert!(html.starts_with("<head><style data-rmx-style="), "{html}");
    assert!(html[template..].contains("<style data-rmx-style="), "{html}");
    assert!(!html[..template].contains("<head><head>"), "{html}");
}

#[test]
fn delays_a_non_blocking_parent_until_its_blocking_child_resolves() {
    let (tx, child) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <Frame src="/parent" fallback={rsx! { <div>"Loading parent..."</div> }}/> })
    });
    router.get("/parent", |ctx: Context| async move {
        ctx.render(rsx! { <section><h2>"Parent Content"</h2><Frame src="/child"/><p>"Parent footer"</p></section> })
    });
    router.get("/child", move |_ctx: Context| {
        let g = child.clone();
        async move { response::html(g.html().await) }
    });
    let mut res = body(&router, "http://localhost/");
    let first = next(&mut res).unwrap();
    assert!(first.contains("<div>Loading parent...</div>"), "{first}");
    assert_eq!(statuses(&first), ["pending"]);
    assert!(waiting(&mut res), "the parent template waits for its blocking child");
    open(tx, "<article>Child Content</article>");
    let second = next(&mut res).unwrap();
    assert!(second.starts_with("<template id=\"f"), "{second}");
    assert!(second.contains("<h2>Parent Content</h2>") && second.contains("<article>Child Content</article>"), "{second}");
    assert_eq!(next(&mut res), None);
}

#[test]
fn dropping_the_response_drops_pending_frame_work() {
    struct Dropped(Arc<Mutex<u32>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            *self.0.lock().unwrap() += 1;
        }
    }
    let dropped = Arc::new(Mutex::new(0));
    let (_tx, slow) = gate();
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <Frame src="/product" fallback={rsx! { <div>"Loading..."</div> }}/> })
    });
    let counter = dropped.clone();
    router.get("/product", move |_ctx: Context| {
        let g = slow.clone();
        let guard = Dropped(counter.clone());
        async move {
            let html = g.html().await;
            drop(guard);
            response::html(html)
        }
    });
    let mut res = body(&router, "http://localhost/");
    assert!(next(&mut res).unwrap().contains("Loading..."));
    assert!(waiting(&mut res));
    assert_eq!(*dropped.lock().unwrap(), 0);
    drop(res);
    assert_eq!(*dropped.lock().unwrap(), 1, "the pending frame request was dropped with the response");
}

#[derive(Clone, Default)]
struct InspectProps {
    label: &'static str,
}

static SEEN: Mutex<Vec<(&'static str, String, String)>> = Mutex::new(Vec::new());

struct Inspect;

impl Component for Inspect {
    type Props = InspectProps;
    fn setup(_: &InspectProps, _: &Handle<Self>) -> Self {
        Inspect
    }
    fn render(&self, p: &InspectProps, h: &Handle<Self>) -> Node {
        let src = h.frame().src();
        SEEN.lock().unwrap().push((p.label, src.clone(), h.frames().top().src()));
        rsx! { <div data-label={p.label}>{src}</div> }
    }
}

#[test]
fn preserves_the_top_frame_src_across_nested_frame_renders() {
    let mut router = Router::new();
    router.get("/app", |ctx: Context| async move { ctx.render(rsx! { <main><Inspect label="root"/><Frame src="/settings"/></main> }) });
    router.get("/settings", |ctx: Context| async move {
        ctx.render(rsx! { <section><Inspect label="settings"/><Frame src="/settings/profile"/></section> })
    });
    router.get("/settings/profile", |ctx: Context| async move { ctx.render(rsx! { <Inspect label="profile"/> }) });
    let html = drain(&router, "https://example.com/app");
    for src in ["https://example.com/app", "https://example.com/settings", "https://example.com/settings/profile"] {
        assert!(html.contains(&format!(">{src}</div>")), "{html}");
    }
    let seen = SEEN.lock().unwrap().clone();
    let top = "https://example.com/app".to_string();
    assert_eq!(
        seen,
        [
            ("root", top.clone(), top.clone()),
            ("settings", "https://example.com/settings".to_string(), top.clone()),
            ("profile", "https://example.com/settings/profile".to_string(), top.clone()),
        ]
    );
}

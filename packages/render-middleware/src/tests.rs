use rmx::{rsx, Frame};
use rmx_fetch::block_on;
use rmx_response as response;
use rmx_fetch_router::{Context, Router};

use super::Render;

fn app() -> Router {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! {
            <html>
                <body>
                    <h1>"Home"</h1>
                    <Frame src="/sidebar"/>
                    <Frame src="/slow" name="slow" fallback={rsx! { <p>"Loading"</p> }}/>
                </body>
            </html>
        })
    });
    router.get("/sidebar", |ctx: Context| async move {
        let frame = if ctx.is_frame_request() { "frame" } else { "page" };
        ctx.render(rsx! { <nav>"Sidebar " {frame}</nav> })
    });
    router.get("/slow", |ctx: Context| async move { ctx.render(rsx! { <section>"Slow content"</section> }) });
    router.get("/moved", |_ctx: Context| async move { response::redirect("/sidebar") });
    router.get("/frame-redirect", |ctx: Context| async move { ctx.render(rsx! { <div><Frame src="/moved"/></div> }) });
    router
}

fn get(router: &Router, url: &str) -> String {
    let res = block_on(router.fetch_url(url));
    assert_eq!(res.headers()["content-type"], "text/html; charset=UTF-8");
    block_on(res.into_body().text()).unwrap()
}

#[test]
fn inlines_blocking_frames_and_streams_the_rest() {
    let html = get(&app(), "http://localhost/");
    assert!(html.starts_with("<!DOCTYPE html><html><body><h1>Home</h1><!--rmx:f:"), "{html}");
    assert!(html.contains("<nav>Sidebar frame</nav><!--/rmx:f-->"), "{html}");
    assert!(html.contains("<p>Loading</p><!--/rmx:f-->"), "{html}");
    assert_eq!(html.matches("id=\"rmx-data\"").count(), 1, "{html}");
    let template = html.find("<template id=\"f").expect("streamed template");
    assert!(html[template..].contains("<section>Slow content</section><!--rmx:te--></template></body></html>"), "{html}");
    assert!(html[..template].ends_with("<!-- rmx:flush document -->"), "{html}");
    // Hydration data comes before the templates and records which frame is pending.
    let data = html.find("id=\"rmx-data\"").expect("rmx-data");
    assert!(data < template);
    assert!(html.contains(r#""src":"/sidebar","name":null,"status":"resolved""#), "{html}");
    assert!(html.contains(r#""src":"/slow","name":"slow","status":"pending""#), "{html}");
}

#[test]
fn nested_streamed_frames_stream_through_the_parent() {
    let mut router = Router::new();
    router.get("/", |ctx: Context| async move {
        ctx.render(rsx! { <html><body><Frame src="/outer" fallback={rsx! { <p>"outer..."</p> }}/></body></html> })
    });
    router.get("/outer", |ctx: Context| async move {
        ctx.render(rsx! { <div>"Outer"<Frame src="/inner" fallback={rsx! { <p>"inner..."</p> }}/></div> })
    });
    router.get("/inner", |ctx: Context| async move { ctx.render(rsx! { <b>"Inner"</b> }) });
    let html = get(&router, "http://localhost/");
    let outer = html.find("<template id=\"").expect("outer template");
    let body = &html[outer..];
    // The outer template carries the inner fallback; the inner content follows as its own template.
    assert!(body.contains("<div>Outer<!--rmx:f:"), "{html}");
    assert!(body.contains("<p>inner...</p><!--/rmx:f--></div>"), "{html}");
    assert!(!body[..body.find("rmx:te").unwrap()].contains("rmx:flush"), "{html}");
    assert_eq!(body.matches("<template id=").count(), 2, "{html}");
    assert!(body.contains("<b>Inner</b><!--rmx:te--></template>"), "{html}");
    assert!(html.ends_with("</template></body></html>"), "{html}");
}

#[test]
fn named_frames_send_their_target() {
    let mut router = Router::new();
    // The same URL renders the whole page or, for its own frame, just the content.
    router.get("/courses", |ctx: Context| async move {
        if ctx.frame_target() == Some("courses") {
            return ctx.render(rsx! { <ul><li>"Course"</li></ul> });
        }
        let src = ctx.url().href();
        ctx.render(rsx! { <html><body><Frame name="courses" src={src}/></body></html> })
    });
    let html = get(&router, "http://localhost/courses");
    assert!(html.contains("<ul><li>Course</li></ul><!--/rmx:f-->"), "{html}");
}

#[test]
fn frame_requests_get_fragments_and_redirects_are_followed() {
    let router = app();
    let html = get(&router, "http://localhost/sidebar");
    assert_eq!(html, "<nav>Sidebar page</nav>");
    let html = get(&router, "http://localhost/frame-redirect");
    assert!(html.starts_with("<div><!--rmx:f:"), "{html}");
    assert!(html.contains("<nav>Sidebar frame</nav><!--/rmx:f--></div>"), "{html}");
}

use rmx::{rsx, Frame};
use rmx_fetch::{block_on, response};
use rmx_router::{Context, Router};

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
    let template = html.find("<template id=\"f").expect("streamed template");
    assert!(html[template..].contains("<section>Slow content</section><!--rmx:te--></template></body></html>"), "{html}");
    // Hydration data comes before the templates and records which frame is pending.
    let data = html.find("id=\"rmx-data\"").expect("rmx-data");
    assert!(data < template);
    assert!(html.contains(r#""src":"/sidebar","name":null,"status":"resolved""#), "{html}");
    assert!(html.contains(r#""src":"/slow","name":"slow","status":"pending""#), "{html}");
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

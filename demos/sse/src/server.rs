use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::{header, HeaderValue};
use rmx::{css, rsx, Children, Component, Handle, Mixin, Node};
use rmx::fetch::{Body, Response};
use rmx::middleware::compression::compression;
use rmx::middleware::logger::logger;
use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
use rmx::middleware::render::Render;
use rmx::router::routes::{get, Route, RouteMap};
use rmx::router::{action, Context, Router};
use rmx::data_schema::{self as s, coerce, form_data as f};
use rmx::url::Url;
use sse_demo::*;

pub fn main() {
    rmx::router::routes::routes_command(&*ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44102".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "demos/sse/public".into());
    let addr = format!("{host}:{port}");
    println!("Server-Sent Events demo on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::node_fetch_server::listen(&addr, router(&public, Duration::from_secs(1)))).expect("serve");
}

// app/routes.ts
#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub messages: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { home: get("/"), messages: get("/messages") });

/// `interval` is the time between messages.
pub fn router(public: &str, interval: Duration) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(compression());
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, index: vec![], ..Default::default() },
    ));


    router.map(
        &*ROUTES,
        RoutesActions {
            home: action(|ctx: Context| async move {
                let limit = message_limit(ctx.url());
                ctx.render(rsx! { <HomePage limit={limit}/> })
            }),
            messages: action(move |ctx: Context| async move {
                let limit = message_limit(ctx.url());
                // The task stops when the client goes away: the server drops the body, and with it
                // this future.
                let body = Body::from_task(move |tx| async move {
                    let mut count = 0;
                    loop {
                        tokio::time::sleep(interval).await;
                        count += 1;
                        let message = Message { count, message: format!("Message #{count} at {}", local_time()) };
                        let data = serde_json::to_string(&message).expect("message serializes");
                        if !tx.send(format!("event: message\ndata: {data}\n\n")) {
                            return;
                        }
                        if limit.is_some_and(|limit| count >= limit) {
                            return;
                        }
                    }
                });
                let mut res = Response::new(body);
                let headers = res.headers_mut();
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
                headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
                headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
                res
            }),
        },
    );
    router
}

/// `utils/message-limit.ts`: a positive `?limit=`, or none.
pub fn message_limit(url: &Url) -> Option<u32> {
    let schema = f::object(|o| o.field("limit", s::optional(coerce::number())));
    let limit = s::parse(&schema, url.search_params()).ok().flatten()?;
    // A limit counts messages: a positive whole number.
    (limit >= 1.0 && limit.fract() == 0.0 && limit <= f64::from(u32::MAX)).then_some(limit as u32)
}

/// `toLocaleTimeString()` for the current UTC time.
fn local_time() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{h12}:{m:02}:{s:02} {} UTC", if h < 12 { "AM" } else { "PM" })
}

// ---------------------------------------------------------------------------------------
// ui/layout.tsx
// ---------------------------------------------------------------------------------------

const GLOBAL_CSS: &str = "
@layer reset {
  * { box-sizing: border-box; margin: 0; padding: 0; }
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
@keyframes slideIn {
  from { opacity: 0; transform: translateX(-20px); }
  to { opacity: 1; transform: translateX(0); }
}
";

#[derive(Clone, Default)]
pub struct LayoutProps {
    pub children: Children,
}

pub struct Layout;

impl Component for Layout {
    type Props = LayoutProps;

    fn setup(_: &LayoutProps, _: &Handle<Self>) -> Self {
        Layout
    }

    fn render(&self, props: &LayoutProps, _: &Handle<Self>) -> Node {
        rsx! {
            <html lang="en">
                <head>
                    <meta charset="UTF-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
                    <title>"Server-Sent Events Demo"</title>
                    <script type="module" async src="/boot.js"></script>
                    <style>{GLOBAL_CSS}</style>
                </head>
                <body mix={css("font-family: system-ui, -apple-system, sans-serif; line-height: 1.5; padding: 2rem; max-width: 800px; margin: 0 auto; background: #f5f5f5;")}>
                    {&props.children}
                </body>
            </html>
        }
    }
}

// ---------------------------------------------------------------------------------------
// actions/home.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct HomePageProps {
    pub limit: Option<u32>,
}

pub struct HomePage;

impl Component for HomePage {
    type Props = HomePageProps;

    fn setup(_: &HomePageProps, _: &Handle<Self>) -> Self {
        HomePage
    }

    fn render(&self, props: &HomePageProps, _: &Handle<Self>) -> Node {
        let limit_info = match props.limit {
            Some(limit) => rsx! {
                <>
                    "Stream will close after " <strong>{limit}</strong> {format!(" message{}.", if limit == 1 { "" } else { "s" })}
                </>
            },
            None => rsx! {
                <>
                    "No limit set. "
                    <a href="?limit=10" mix={css("color: #007bff; text-decoration: underline;")}>"Add " <code mix={code_style()}>"?limit=10"</code> " to the URL"</a>
                    " to limit messages."
                </>
            },
        };
        rsx! {
            <Layout>
                <h1 mix={css("color: #333; margin-bottom: 0.5rem;")}>"Server-Sent Events Demo"</h1>
                <p mix={css("color: #666; margin-bottom: 2rem;")}>"Real-time updates with compression middleware"</p>
                <div mix={card_style()}>
                    <label mix={label_style()}>"Compression:"</label>
                    <div mix={note_style()}>
                        "Encoding is negotiated automatically via " <code mix={code_style()}>"Accept-Encoding"</code> " header."
                        <br/>
                        "Open DevTools Network tab to see " <code mix={code_style()}>"Content-Encoding"</code> " response header."
                    </div>
                </div>
                <div mix={card_style()}>
                    <label mix={label_style()}>"Message Limit:"</label>
                    <div id="limit-info" mix={note_style()}>{limit_info}</div>
                </div>
                <MessageStream limit={props.limit}/>
            </Layout>
        }
    }
}

fn label_style() -> Mixin {
    css("display: block; font-weight: 600; margin-bottom: 0.5rem; color: #333;")
}

fn note_style() -> Mixin {
    css("padding: 0.5rem; background: #f8f9fa; border-radius: 4px; color: #666;")
}

fn code_style() -> Mixin {
    css("background: #f5f5f5; padding: 0.2rem 0.4rem; border-radius: 3px; font-family: 'Courier New', monospace; font-size: 0.9em;")
}

#[cfg(test)]
mod tests;

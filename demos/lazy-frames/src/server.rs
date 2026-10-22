use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::{header, HeaderValue, StatusCode};
use lazy_frames_demo::*;
use rmx::{css, rsx, Children, Component, Frame, Handle, Mixin, Node};
use rmx::cookie::{Cookie, CookieProperties, SameSite};
use rmx::response::{self, FileResponseOptions};
use rmx::fetch::Response;
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::logger::logger;
use rmx::fs::{open_lazy_file, OpenLazyFileOptions};
use rmx::middleware::static_files::{static_files, StaticOptions};
use rmx::middleware::render::{Render, ResponseInit};
use rmx::router::routes::{get, post, prefix, Route, RouteMap};
use rmx::router::{action, Context, Next, Router};

pub fn main() {
    rmx::router::routes::routes_command(&*ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44100".into());
    let root = std::env::var("DEMO_DIR").unwrap_or_else(|_| "demos/lazy-frames".into());
    let addr = format!("{host}:{port}");
    println!("lazy-frames demo on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::node_fetch_server::listen(&addr, router(Config::from_env(root)))).expect("serve");
}

// ---------------------------------------------------------------------------------------
// Routes (app/routes.ts)
// ---------------------------------------------------------------------------------------

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub latency: Route,
    pub frames: FrameRoutes,
}

#[derive(RouteMap)]
pub struct FrameRoutes {
    pub html: Route,
    pub ui: Route,
    pub interactive: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| {
    let frames = prefix("frames");
    Routes {
        home: get("/"),
        latency: post("/demo-latency"),
        frames: FrameRoutes { html: frames.get("html/:id"), ui: frames.get("ui/:id"), interactive: frames.get("interactive/:id") },
    }
});

// ---------------------------------------------------------------------------------------
// Router (app/router.ts)
// ---------------------------------------------------------------------------------------

pub struct Config {
    /// The demo directory: `public/` and `frames-html/` live here.
    pub root: PathBuf,
    /// `RMX_ENV=production` makes frame responses publicly cacheable.
    pub production: bool,
    /// Delay applied to every request while the latency cookie is set.
    pub latency: Duration,
}

impl Config {
    fn from_env(root: String) -> Config {
        let ms = std::env::var("DEMO_LATENCY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(520);
        Config {
            root: root.into(),
            production: std::env::var("RMX_ENV").as_deref() == Ok("production"),
            latency: Duration::from_millis(ms),
        }
    }
}

pub fn router(config: Config) -> Router {
    let config = Arc::new(config);
    let mut router = Router::new();
    if !config.production && std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(load_theme);
    let latency = config.latency;
    router.use_middleware(move |ctx: Context, next: Next| demo_latency(ctx, next, latency));
    router.use_middleware(static_files(
        config.root.join("public"),
        StaticOptions { cache_control: Some("no-store".into()), index: vec![], ..Default::default() },
    ));
    router.use_middleware(form_data());


    router.map(
        &*ROUTES,
        RoutesActions {
            home: action(|ctx: Context| async move {
                let latency = *ctx.get::<DemoLatency>().expect("demo latency middleware");
                let theme = *ctx.get::<Theme>().expect("theme middleware");
                ctx.render(rsx! { <HomePage latency={latency} theme={theme}/> })
            }),
            latency: action(|ctx: Context| async move {
                let enabled = match ctx.form_data().get("enabled") {
                    Some("true") => true,
                    Some("false") => false,
                    _ => return response::status(StatusCode::BAD_REQUEST, "Expected enabled to be true or false"),
                };
                let cookie = if enabled { LATENCY_COOKIE.serialize("1") } else { LATENCY_COOKIE.serialize_with("", &CookieProperties { max_age: Some(0), ..Default::default() }) };
                let mut res = response::redirect_with(&ROUTES.home.href(()), StatusCode::SEE_OTHER);
                if let Ok(v) = HeaderValue::from_str(&cookie) {
                    res.headers_mut().append(header::SET_COOKIE, v);
                }
                res
            }),
        },
    );

    // actions/frames/controller.tsx: three response shapes behind one route map.
    let (html_config, ui_config, interactive_config) = (config.clone(), config.clone(), config.clone());
    router.map(
        &ROUTES.frames,
        FrameRoutesActions {
            html: action(move |ctx: Context| {
                let config = html_config.clone();
                async move {
                    let Some(file) = ctx.param("id").and_then(html_frame_file) else { return not_found() };
                    let path = config.root.join("frames-html").join(file);
                    let Ok(file) = open_lazy_file(&path, OpenLazyFileOptions::default()) else { return not_found() };
                    let options = FileResponseOptions { cache_control: Some(frame_cache_control(&config).into()), ..Default::default() };
                    let response = response::file(&file, ctx.request(), &options);
                    response.await.unwrap_or_else(|_| not_found())
                }
            }),
            ui: action(move |ctx: Context| {
                let config = ui_config.clone();
                async move {
                    let Some(Exhibit { kind: ExhibitKind::Ui(ui), .. }) = ctx.param("id").and_then(exhibit) else { return not_found() };
                    let init = ResponseInit::default().header("cache-control", frame_cache_control(&config));
                    ctx.render_with(rsx! { <MetricFrame exhibit={ui} served_at={format_time(SystemTime::now())}/> }, init)
                }
            }),
            interactive: action(move |ctx: Context| {
                let config = interactive_config.clone();
                async move {
                    let Some(Exhibit { kind: ExhibitKind::Interactive(ix), .. }) = ctx.param("id").and_then(exhibit) else {
                        return not_found();
                    };
                    let init = ResponseInit::default().header("cache-control", frame_cache_control(&config));
                    ctx.render_with(rsx! { <InteractiveFrame exhibit={ix} served_at={format_time(SystemTime::now())}/> }, init)
                }
            }),
        },
    );
    router
}

fn frame_cache_control(config: &Config) -> &'static str {
    if config.production {
        "public, max-age=300, stale-while-revalidate=3600"
    } else {
        "no-store"
    }
}

fn not_found() -> Response {
    let mut res = response::status(StatusCode::NOT_FOUND, "Frame example not found");
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=UTF-8"));
    res
}

fn html_frame_file(id: &str) -> Option<&'static str> {
    match id {
        MOTION_ARTIFACT_ID => Some("edition-orbit.html"),
        "field-notes" => Some("field-notes.html"),
        "packing-list" => Some("packing-list.html"),
        "reading-list" => Some("reading-list.html"),
        _ => None,
    }
}

/// `toLocaleTimeString('en-US')` for the current UTC time: `3:04:05 PM`.
fn format_time(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{h12}:{m:02}:{s:02} {} UTC", if h < 12 { "AM" } else { "PM" })
}

// ---------------------------------------------------------------------------------------
// Middleware (middleware/theme.ts, middleware/demo-latency.ts)
// ---------------------------------------------------------------------------------------

static THEME: LazyLock<Cookie> =
    LazyLock::new(|| {
        Cookie::new(THEME_COOKIE)
            .decode(|value| Some(value.to_string()))
            .encode(str::to_string)
            .max_age(THEME_COOKIE_MAX_AGE)
            .path("/")
            .same_site(SameSite::Lax)
    });

static LATENCY_COOKIE: LazyLock<Cookie> = LazyLock::new(|| {
    Cookie::new("lazy-frames-latency").http_only(true).max_age(60 * 60 * 24 * 365).path("/").same_site(SameSite::Lax)
});

/// The saved theme, for server rendering.
async fn load_theme(mut ctx: Context, next: Next) -> Response {
    let theme = Theme::parse(THEME.parse(ctx.header("cookie")).as_deref());
    ctx.set(theme);
    next.run(ctx).await
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DemoLatency {
    pub enabled: bool,
    pub duration: Duration,
}

/// Reads the latency cookie, exposes it to handlers, and delays every request while it is on.
async fn demo_latency(mut ctx: Context, next: Next, duration: Duration) -> Response {
    let enabled = LATENCY_COOKIE.parse(ctx.header("cookie")).as_deref() == Some("1");
    ctx.set(DemoLatency { enabled, duration });
    if enabled {
        tokio::time::sleep(duration).await;
    }
    let mut res = next.run(ctx).await;
    res.headers_mut().append(header::VARY, HeaderValue::from_static("Cookie"));
    if enabled {
        if let Ok(v) = HeaderValue::from_str(&format!("demo-latency;dur={}", duration.as_millis())) {
            res.headers_mut().append(HeaderName::from_static("server-timing"), v);
        }
    }
    res
}

use http::HeaderName;

// ---------------------------------------------------------------------------------------
// Data (data/exhibits.ts)
// ---------------------------------------------------------------------------------------

pub const MOTION_ARTIFACT_ID: &str = "edition-orbit";

#[derive(Clone)]
pub struct Exhibit {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: ExhibitKind,
}

#[derive(Clone)]
pub enum ExhibitKind {
    Html,
    Ui(UiExhibit),
    Interactive(InteractiveExhibit),
}

#[derive(Clone, Default)]
pub struct UiExhibit {
    pub metric: &'static str,
    pub metric_label: &'static str,
    pub trend: &'static str,
    pub details: [&'static str; 3],
}

#[derive(Clone, Default)]
pub struct InteractiveExhibit {
    pub initial_count: i32,
    pub action_label: &'static str,
    pub prompt: &'static str,
    pub accent: Accent,
}

pub static EXHIBITS: LazyLock<Vec<Exhibit>> = LazyLock::new(|| {
    let interactive = |initial_count, prompt, accent| {
        ExhibitKind::Interactive(InteractiveExhibit { initial_count, action_label: "Increment local count", prompt, accent })
    };
    vec![
        Exhibit {
            id: "field-notes",
            title: "Stream a checked-in fragment",
            description: "A file response supplies Frame content without invoking the rmx component renderer.",
            kind: ExhibitKind::Html,
        },
        Exhibit {
            id: "signal-board",
            title: "Three kinds of Frame response",
            description: "A server-rendered component returns component output, generated CSS, and request-time data.",
            kind: ExhibitKind::Ui(UiExhibit {
                metric: "3",
                metric_label: "Response types in this demo",
                trend: "All three use the same LazyFrame boundary.",
                details: ["HTML streamed from disk", "Server-rendered components", "Components + client entry"],
            }),
        },
        Exhibit {
            id: "idea-counter",
            title: "Hydrate a client component",
            description: "The Frame response contains a client entry that hydrates after insertion.",
            kind: interactive(12, "This counter is a client component inside the Frame response.", Accent::Violet),
        },
        Exhibit {
            id: "packing-list",
            title: "Keep the file route explicit",
            description: "Known route IDs map to checked-in files; unknown IDs return a 404 response.",
            kind: ExhibitKind::Html,
        },
        Exhibit {
            id: "studio-schedule",
            title: "Load before the viewport arrives",
            description: "The preload distance is configurable without changing the Frame response.",
            kind: ExhibitKind::Ui(UiExhibit {
                metric: "320 px",
                metric_label: "Default vertical preload margin",
                trend: "Applied above and below the viewport.",
                details: ["One shared load observer", "Configurable rootMargin", "No request before intersection"],
            }),
        },
        Exhibit {
            id: "theme-state",
            title: "Keep state through a theme change",
            description: "Local client state arrives only after this Frame approaches the viewport.",
            kind: interactive(31, "Document theme changes do not remount this client component.", Accent::Coral),
        },
        Exhibit {
            id: "reading-list",
            title: "Frame content joins the document",
            description: "Static HTML can inherit parent styles and theme tokens without a component runtime.",
            kind: ExhibitKind::Html,
        },
        Exhibit {
            id: "release-health",
            title: "Load once, keep the Frame",
            description: "The Frame response contains server-rendered components and their generated styles.",
            kind: ExhibitKind::Ui(UiExhibit {
                metric: "1\u{d7}",
                metric_label: "Frame mount per section",
                trend: "Leaving the viewport does not unmount it.",
                details: ["Loaded DOM stays in place", "Scrolling back makes no request", "Local client state stays intact"],
            }),
        },
        Exhibit {
            id: "retained-counter",
            title: "Scroll away and come back",
            description: "The last Frame inserts and hydrates a client component near the end of the page.",
            kind: interactive(24, "Leaving the viewport does not unmount this client component.", Accent::Teal),
        },
    ]
});

pub fn exhibit(id: &str) -> Option<Exhibit> {
    EXHIBITS.iter().find(|e| e.id == id).cloned()
}

fn frame_href(exhibit: &Exhibit) -> String {
    let params = [("id", exhibit.id)];
    match exhibit.kind {
        ExhibitKind::Html => ROUTES.frames.html.href(params),
        ExhibitKind::Ui(_) => ROUTES.frames.ui.href(params),
        ExhibitKind::Interactive(_) => ROUTES.frames.interactive.href(params),
    }
}

fn category_label(kind: &ExhibitKind) -> &'static str {
    match kind {
        ExhibitKind::Html => "Static HTML",
        ExhibitKind::Ui(_) => "Server-rendered components",
        ExhibitKind::Interactive(_) => "Components + client entry",
    }
}

// ---------------------------------------------------------------------------------------
// Frame responses (actions/frames/controller.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct MetricFrameProps {
    pub exhibit: UiExhibit,
    pub served_at: String,
}

pub struct MetricFrame;

impl Component for MetricFrame {
    type Props = MetricFrameProps;

    fn setup(_: &MetricFrameProps, _: &Handle<Self>) -> Self {
        MetricFrame
    }

    fn render(&self, props: &MetricFrameProps, _: &Handle<Self>) -> Node {
        let ex = &props.exhibit;
        rsx! {
            <article mix={ui_frame_style()}>
                <div mix={ui_frame_header_style()}>
                    <h3 mix={ui_heading_style()}>{ex.metric_label}</h3>
                    <p mix={served_time_style()}>"Delivered at " <time>{props.served_at.clone()}</time></p>
                </div>
                <div mix={metric_grid_style()}>
                    <p mix={metric_style()}>{ex.metric}</p>
                    <p mix={trend_style()}>{ex.trend}</p>
                </div>
                <ul mix={detail_list_style()}>
                    {ex.details.iter().map(|d| rsx! {
                        <li key={*d} mix={detail_item_style()}><span aria-hidden="true" mix={detail_mark_style()}></span>{*d}</li>
                    }).collect::<Vec<_>>()}
                </ul>
            </article>
        }
    }
}

#[derive(Clone, Default)]
pub struct InteractiveFrameProps {
    pub exhibit: InteractiveExhibit,
    pub served_at: String,
}

pub struct InteractiveFrame;

impl Component for InteractiveFrame {
    type Props = InteractiveFrameProps;

    fn setup(_: &InteractiveFrameProps, _: &Handle<Self>) -> Self {
        InteractiveFrame
    }

    fn render(&self, props: &InteractiveFrameProps, _: &Handle<Self>) -> Node {
        let ex = &props.exhibit;
        rsx! {
            <article mix={interactive_frame_style()}>
                <div mix={ui_frame_header_style()}>
                    <h3 mix={interactive_heading_style()}>{ex.prompt}</h3>
                </div>
                <FramePlayground initial_count={ex.initial_count} action_label={ex.action_label}
                    accent={ex.accent} served_at={props.served_at.clone()}/>
            </article>
        }
    }
}

fn ui_frame_style() -> Mixin {
    css("border-radius: 6px; padding: clamp(24px, 4vw, 42px); background-color: var(--server-frame-bg); color: var(--server-frame-text); transition: background-color 180ms ease, color 180ms ease;")
}
fn ui_frame_header_style() -> Mixin {
    css("display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; @media (max-width: 640px) { flex-direction: column; }")
}
fn ui_heading_style() -> Mixin {
    css("max-width: 520px; margin: 0; font-size: clamp(22px, 3vw, 34px); line-height: 1.08; letter-spacing: -0.03em;")
}
fn served_time_style() -> Mixin {
    css("margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; color: var(--server-frame-muted); white-space: nowrap;")
}
fn metric_grid_style() -> Mixin {
    css("display: grid; gap: 16px; margin-top: 42px; padding-bottom: 32px; border-bottom: 1px solid var(--server-frame-rule);")
}
fn metric_style() -> Mixin {
    css("margin: 0; font-size: clamp(48px, 8vw, 86px); font-weight: 800; line-height: 0.9; letter-spacing: -0.07em; color: var(--server-frame-highlight);")
}
fn trend_style() -> Mixin {
    css("margin: 0; padding-bottom: 5px; font-size: 15px; line-height: 1.5; color: var(--server-frame-copy);")
}
fn detail_list_style() -> Mixin {
    css("display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin: 28px 0 0; padding: 0; list-style: none; @media (max-width: 720px) { grid-template-columns: 1fr; }")
}
fn detail_item_style() -> Mixin {
    css("display: flex; align-items: center; gap: 10px; font-size: 13px; line-height: 1.4; color: var(--server-frame-detail);")
}
fn detail_mark_style() -> Mixin {
    css("flex: 0 0 auto; width: 7px; height: 7px; border-radius: 50%; background-color: var(--server-frame-accent);")
}
fn interactive_frame_style() -> Mixin {
    css("border: 1px solid var(--interactive-border); border-radius: 6px; padding: clamp(24px, 4vw, 42px); background-color: var(--interactive-bg); color: var(--interactive-text); transition: background-color 180ms ease, border-color 180ms ease, color 180ms ease;")
}
fn interactive_heading_style() -> Mixin {
    css("max-width: 520px; margin: 0; font-size: clamp(22px, 3vw, 34px); line-height: 1.08; letter-spacing: -0.03em;")
}

// ---------------------------------------------------------------------------------------
// Document (ui/document.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: String,
    pub theme: Theme,
    pub children: Children,
}

pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        rsx! {
            <html lang="en" data-theme={props.theme.as_str()}>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <meta name="description" content="A long-page demo that mounts rmx Frames as they approach the viewport."/>
                    <title>{props.title.clone()}</title>
                    <script type="module" async src="/boot.js"></script>
                </head>
                <body id="top" data-theme={props.theme.as_str()} mix={body_style()}>{&props.children}</body>
            </html>
        }
    }
}

fn body_style() -> Mixin {
    css("--page-bg: #f3eee4; --page-text: #25241e; --page-heading: #282720; --page-muted: #686257; --page-subtle: #81786b; --page-rule: #c8c0b2; --surface-bg: rgba(255, 253, 249, 0.74); --surface-border: #d7d0c4; --frame-shell-bg: rgba(255, 255, 255, 0.46); --placeholder-bg: rgba(247, 243, 235, 0.72); --placeholder-border: #bbb1a1; --placeholder-text: #6e665b; --placeholder-heading: #3e3a32; --placeholder-dot: #aaa091; --plain-bg: #fffdf8; --plain-border: #d5cec2; --plain-text: #2d2b25; --plain-list: #555044; --plain-muted: #81786b; --plain-rule: #e2dcd2; --control-bg: rgba(255, 253, 249, 0.92); --control-text: #14201e; --control-muted: #68736e; --control-border: #b8c0bb; --control-border-hover: #66746d; --control-button-bg: #ffffff; --control-button-hover: #f4f7f4; --control-shadow: rgba(20, 32, 30, 0.14); --hero-bg: #14201e; --hero-text: #f3f0e7; --hero-muted: #c7d4ce; --hero-rule: rgba(243, 240, 231, 0.16); --hero-copy: #dfe7e2; --focus-ring: #f4cf71; --footer-bg: #d9e5db; --footer-text: #183029; --footer-muted: #557066; --server-frame-bg: #15201e; --server-frame-text: #eff8ee; --server-frame-muted: #a6b6b1; --server-frame-copy: #c7d4d0; --server-frame-detail: #dfe9e5; --server-frame-rule: rgba(239, 248, 238, 0.16); --server-frame-accent: #9ed89b; --server-frame-highlight: #f4cf71; --interactive-bg: #fffdf9; --interactive-text: #1f1930; --interactive-muted: #655f78; --interactive-subtle: #756d85; --interactive-accent: #7557c8; --interactive-pending: #b8afc9; --interactive-border: #d9d0e4; --interactive-button-border: #cbc3d9; --interactive-button-text: #453d59; --interactive-button-hover: #f6f2fa; --theme-motion-coral: #ff785a; --theme-motion-cream: #fff7e8; --theme-motion-lime: #c9f27b; --theme-motion-bg-start: #1c3942; --theme-motion-bg-end: #182c35; --theme-motion-glow: rgba(201, 242, 123, 0.2); --theme-motion-border: rgba(255, 247, 232, 0.22); --theme-motion-ring: rgba(255, 247, 232, 0.34); --theme-motion-ring-soft: rgba(255, 247, 232, 0.2); --theme-motion-caption: rgba(255, 247, 232, 0.7); margin: 0; background-color: var(--page-bg); color: var(--page-text); color-scheme: light; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif; text-rendering: optimizeLegibility; &[data-theme=\"dark\"] { --page-bg: #0d1614; --page-text: #e9eee8; --page-heading: #f2f5ef; --page-muted: #adb8b1; --page-subtle: #929f97; --page-rule: #394740; --surface-bg: rgba(24, 35, 32, 0.82); --surface-border: #36443f; --frame-shell-bg: rgba(28, 42, 38, 0.78); --placeholder-bg: rgba(19, 30, 27, 0.78); --placeholder-border: #506059; --placeholder-text: #a8b5ae; --placeholder-heading: #edf2ec; --placeholder-dot: #718078; --plain-bg: #17221f; --plain-border: #3a4943; --plain-text: #edf2ec; --plain-list: #bdc8c1; --plain-muted: #96a49c; --plain-rule: #34423d; --control-bg: rgba(18, 29, 26, 0.94); --control-text: #f0f4ef; --control-muted: #a9b6af; --control-border: #52615b; --control-border-hover: #8b9b93; --control-button-bg: #24332f; --control-button-hover: #30423c; --control-shadow: rgba(0, 0, 0, 0.34); --hero-bg: #080f0e; --hero-muted: #b9c9c2; --hero-rule: rgba(243, 240, 231, 0.13); --footer-bg: #13241f; --footer-text: #e5efe7; --footer-muted: #96b1a6; --server-frame-bg: #20342e; --server-frame-muted: #b3c5bd; --server-frame-copy: #d1ddd7; --server-frame-detail: #edf4ef; --server-frame-rule: rgba(239, 248, 238, 0.2); --server-frame-accent: #a7e1a3; --server-frame-highlight: #f4d581; --interactive-bg: #211d2a; --interactive-text: #f4effa; --interactive-muted: #c7badd; --interactive-subtle: #b7abc8; --interactive-accent: #bda7ff; --interactive-pending: #766d85; --interactive-border: #554a63; --interactive-button-border: #6d607d; --interactive-button-text: #e4dcef; --interactive-button-hover: #31293d; --theme-motion-coral: #c94f3b; --theme-motion-cream: #17302e; --theme-motion-lime: #4f6f31; --theme-motion-bg-start: #edf1df; --theme-motion-bg-end: #cbded6; --theme-motion-glow: rgba(102, 144, 65, 0.24); --theme-motion-border: rgba(23, 48, 46, 0.2); --theme-motion-ring: rgba(23, 48, 46, 0.3); --theme-motion-ring-soft: rgba(23, 48, 46, 0.17); --theme-motion-caption: rgba(23, 48, 46, 0.72); color-scheme: dark; } & * { box-sizing: border-box; } & ::selection { background-color: var(--focus-ring); color: #14201e; }")
}

// ---------------------------------------------------------------------------------------
// Home (actions/home.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct HomePageProps {
    pub latency: DemoLatency,
    pub theme: Theme,
}

pub struct HomePage;

impl Component for HomePage {
    type Props = HomePageProps;

    fn setup(_: &HomePageProps, _: &Handle<Self>) -> Self {
        HomePage
    }

    fn render(&self, props: &HomePageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Document title="Lazy-loaded Frames" theme={props.theme}>
                <div mix={demo_controls_style()}>
                    <ThemeToggle theme={props.theme}/>
                    <LatencyToggle latency={props.latency}/>
                </div>
                <header mix={hero_style()}>
                    <div mix={hero_inner_style()}>
                        <h1 mix={hero_heading_style()}>"Load Frames as" <br/> "they approach."</h1>
                        <p mix={hero_copy_style()}>
                            "This demo\u{2019}s " <code>"LazyFrame"</code> " renders a placeholder with the page, then mounts an rmx "
                            <code>"Frame"</code> " within 320 pixels of the viewport. Loaded Frames stay mounted."
                        </p>
                        <ol mix={flow_style()} aria-label="Lazy frame lifecycle">
                            <li mix={flow_item_style()}><span mix={flow_number_style()}>"1"</span><span>"The initial document contains the placeholder."</span></li>
                            <li mix={flow_item_style()}><span mix={flow_number_style()}>"2"</span><span>"An observer detects the host 320 pixels from the viewport."</span></li>
                            <li mix={flow_item_style()}><span mix={flow_number_style()}>"3"</span><span>"The Frame requests its route once and remains mounted."</span></li>
                        </ol>
                        <a href="#delivery-comparison" mix={scroll_cue_style()}>"Compare delivery " <span aria-hidden="true">"\u{2193}"</span></a>
                    </div>
                </header>
                <main mix={main_style()}>
                    <DeliveryComparison/>
                    <section mix={intro_style()} aria-labelledby="response-shapes-heading">
                        <h2 id="response-shapes-heading" mix={intro_heading_style()}>"LazyFrame works with every response type"</h2>
                        <p mix={intro_copy_style()}>
                            "The examples alternate between checked-in HTML, server-rendered components, and components with a client entry. The loading behavior does not change."
                        </p>
                    </section>
                    {EXHIBITS.iter().enumerate().map(|(index, ex)| rsx! {
                        <ExhibitSection key={ex.id} exhibit={ex.clone()} index={index}/>
                    }).collect::<Vec<_>>()}
                </main>
                <footer mix={footer_style()}>
                    <h2 mix={footer_heading_style()}>"Ten Frames loaded near the viewport. The first shipped with the document."</h2>
                    <a href="#top" mix={footer_link_style()}>"Return to the beginning"</a>
                </footer>
            </Document>
        }
    }
}

#[derive(Clone, Default)]
pub struct LatencyToggleProps {
    pub latency: DemoLatency,
}

pub struct LatencyToggle;

impl Component for LatencyToggle {
    type Props = LatencyToggleProps;

    fn setup(_: &LatencyToggleProps, _: &Handle<Self>) -> Self {
        LatencyToggle
    }

    fn render(&self, props: &LatencyToggleProps, _: &Handle<Self>) -> Node {
        let DemoLatency { enabled, duration } = props.latency;
        rsx! {
            <form action={ROUTES.latency.href(())} method="post" mix={latency_toggle_style()}>
                <input type="hidden" name="enabled" value={if enabled { "false" } else { "true" }}/>
                <div mix={latency_status_style()}>
                    <span aria-hidden="true" mix={[latency_dot_style(), if enabled { latency_dot_enabled_style() } else { latency_dot_disabled_style() }]}></span>
                    <span>
                        <strong mix={latency_label_style()}>{format!("Global latency {}", if enabled { "on" } else { "off" })}</strong>
                        <small mix={latency_detail_style()}>
                            {if enabled { format!("{} ms on every request", duration.as_millis()) } else { "Responses run at full speed".to_string() }}
                        </small>
                    </span>
                </div>
                <button type="submit" aria-pressed={if enabled { "true" } else { "false" }} mix={latency_button_style()}>
                    {if enabled { "Disable" } else { "Enable" }}
                </button>
            </form>
        }
    }
}

#[derive(Clone, Default)]
pub struct NoProps {}

pub struct DeliveryComparison;

impl Component for DeliveryComparison {
    type Props = NoProps;

    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        DeliveryComparison
    }

    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        let href = ROUTES.frames.html.href([("id", MOTION_ARTIFACT_ID)]);
        rsx! {
            <section id="delivery-comparison" mix={comparison_section_style()}>
                <header mix={comparison_header_style()}>
                    <h2 mix={comparison_heading_style()}>"The same response, eager and lazy"</h2>
                    <div mix={comparison_description_style()}>
                        <p>
                            "Both Frames resolve the route below. The regular Frame is resolved during server rendering; LazyFrame waits to mount it in the browser. The checked-in fragment carries scoped CSS, keyframes, and a reduced-motion fallback."
                        </p>
                        <code mix={route_style()}>{href.clone()}</code>
                    </div>
                </header>
                <div mix={comparison_grid_style()}>
                    <article data-delivery="eager" mix={delivery_card_style()}>
                        <header mix={delivery_header_style()}>
                            <h3 mix={delivery_heading_style()}>"Regular Frame"</h3>
                            <p mix={delivery_detail_style()}><strong>"Initial document."</strong> " Resolved before the response is sent."</p>
                        </header>
                        <div mix={frame_shell_style()}><Frame src={href.clone()}/></div>
                    </article>
                    <article data-delivery="lazy" mix={delivery_card_style()}>
                        <header mix={delivery_header_style()}>
                            <h3 mix={delivery_heading_style()}>"LazyFrame"</h3>
                            <p mix={delivery_detail_style()}><strong>"Near the viewport."</strong> " Requested once, retained, and paused offscreen."</p>
                        </header>
                        <div mix={frame_shell_style()}>
                            <LazyFrame src={href} pause_animations_when_inactive fallback={rsx! { <FrameStatus fetching={true}/> }}>
                                <FrameStatus fetching={false}/>
                            </LazyFrame>
                        </div>
                    </article>
                </div>
            </section>
        }
    }
}

#[derive(Clone)]
pub struct ExhibitSectionProps {
    pub exhibit: Exhibit,
    pub index: usize,
}

impl Default for ExhibitSectionProps {
    fn default() -> Self {
        ExhibitSectionProps { exhibit: EXHIBITS[0].clone(), index: 0 }
    }
}

pub struct ExhibitSection;

impl Component for ExhibitSection {
    type Props = ExhibitSectionProps;

    fn setup(_: &ExhibitSectionProps, _: &Handle<Self>) -> Self {
        ExhibitSection
    }

    fn render(&self, props: &ExhibitSectionProps, _: &Handle<Self>) -> Node {
        let ex = &props.exhibit;
        let href = frame_href(ex);
        rsx! {
            <section id={ex.id} mix={exhibit_section_style()}>
                <div mix={section_grid_style()}>
                    <header mix={section_header_style()}>
                        <p mix={section_index_style()}>
                            <span>{format!("{:02}", props.index + 1)}</span>
                            <span>{category_label(&ex.kind)}</span>
                        </p>
                        <h2 mix={section_heading_style()}>{ex.title}</h2>
                        <p mix={section_description_style()}>{ex.description}</p>
                        <code mix={route_style()}>{href.clone()}</code>
                    </header>
                    <div mix={frame_shell_style()}>
                        <LazyFrame src={href} fallback={rsx! { <FrameStatus fetching={true}/> }}>
                            <FrameStatus fetching={false}/>
                        </LazyFrame>
                    </div>
                </div>
            </section>
        }
    }
}

#[derive(Clone, Default)]
pub struct FrameStatusProps {
    pub fetching: bool,
}

pub struct FrameStatus;

impl Component for FrameStatus {
    type Props = FrameStatusProps;

    fn setup(_: &FrameStatusProps, _: &Handle<Self>) -> Self {
        FrameStatus
    }

    fn render(&self, props: &FrameStatusProps, _: &Handle<Self>) -> Node {
        let (title, detail) = if props.fetching {
            ("Request in progress", "The Frame response has not arrived yet.")
        } else {
            ("Not requested", "The browser will request this Frame inside the preload margin.")
        };
        rsx! {
            <p data-phase={if props.fetching { "fetching" } else { "waiting" }}>
                <span aria-hidden="true"></span>
                <span><strong>{title}</strong><small>{detail}</small></span>
            </p>
        }
    }
}

fn demo_controls_style() -> Mixin {
    css("position: fixed; z-index: 20; top: 16px; right: 16px; display: flex; align-items: center; gap: 8px; border: 1px solid var(--control-border); border-radius: 10px; padding: 8px; background-color: var(--control-bg); box-shadow: 0 6px 20px var(--control-shadow); color: var(--control-text); transition: background-color 160ms ease, border-color 160ms ease, color 160ms ease; @media (max-width: 560px) { top: 8px; right: 8px; left: 8px; justify-content: space-between; }")
}
fn latency_toggle_style() -> Mixin {
    css("display: flex; align-items: center; gap: 14px; @media (max-width: 560px) { gap: 10px; & small { display: none; } }")
}
fn latency_status_style() -> Mixin {
    css("display: flex; align-items: center; gap: 10px;")
}
fn latency_dot_style() -> Mixin {
    css("flex: 0 0 auto; width: 9px; height: 9px; border-radius: 50%;")
}
fn latency_dot_enabled_style() -> Mixin {
    css("background-color: #d4543f; box-shadow: 0 0 0 5px rgba(212, 84, 63, 0.13);")
}
fn latency_dot_disabled_style() -> Mixin {
    css("background-color: var(--control-muted);")
}
fn latency_label_style() -> Mixin {
    css("display: block; font-size: 12px;")
}
fn latency_detail_style() -> Mixin {
    css("display: block; margin-top: 2px; font-size: 10px; color: var(--control-muted);")
}
fn latency_button_style() -> Mixin {
    css("min-height: 36px; border: 1px solid var(--control-border); border-radius: 6px; padding: 7px 11px; background-color: var(--control-button-bg); color: var(--control-text); font: inherit; font-size: 11px; font-weight: 750; cursor: pointer; &:hover { border-color: var(--control-border-hover); background-color: var(--control-button-hover); } &:focus-visible { outline: 3px solid var(--focus-ring); outline-offset: 2px; }")
}
fn hero_style() -> Mixin {
    css("min-height: max(100svh, 760px); display: grid; align-items: center; padding: clamp(96px, 10vw, 132px) clamp(24px, 6vw, 76px) 72px; background-color: var(--hero-bg); color: var(--hero-text); transition: background-color 180ms ease;")
}
fn hero_inner_style() -> Mixin {
    css("width: min(1120px, 100%); margin: 0 auto;")
}
fn hero_heading_style() -> Mixin {
    css("max-width: 980px; margin: 0 0 32px; font-size: clamp(52px, 9vw, 112px); font-weight: 760; line-height: 0.92; letter-spacing: -0.065em;")
}
fn hero_copy_style() -> Mixin {
    css("max-width: 680px; margin: 0; font-size: clamp(18px, 2vw, 24px); line-height: 1.45; color: var(--hero-muted); & code { color: var(--focus-ring); }")
}
fn flow_style() -> Mixin {
    css("display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); margin: clamp(56px, 9vh, 88px) 0 0; padding: 0; border-top: 1px solid var(--hero-rule); border-bottom: 1px solid var(--hero-rule); list-style: none; & li + li { border-left: 1px solid var(--hero-rule); } @media (max-width: 760px) { grid-template-columns: 1fr; & li + li { border-top: 1px solid var(--hero-rule); border-left: 0; } }")
}
fn flow_item_style() -> Mixin {
    css("display: grid; grid-template-columns: 28px 1fr; gap: 14px; min-height: 112px; align-items: start; padding: 22px 24px; font-size: 14px; line-height: 1.5; color: var(--hero-copy);")
}
fn flow_number_style() -> Mixin {
    css("font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; font-weight: 700; color: var(--focus-ring);")
}
fn scroll_cue_style() -> Mixin {
    css("display: inline-flex; align-items: center; gap: 14px; margin-top: 52px; border-bottom: 1px solid color-mix(in srgb, var(--hero-text) 40%, transparent); padding-bottom: 6px; color: var(--hero-text); font-size: 13px; font-weight: 700; text-decoration: none; &:hover { border-color: var(--focus-ring); color: var(--focus-ring); }")
}
fn main_style() -> Mixin {
    css("width: min(1120px, calc(100% - 40px)); margin: 0 auto;")
}
fn intro_style() -> Mixin {
    css("display: grid; grid-template-columns: minmax(280px, 0.8fr) minmax(280px, 1fr); gap: clamp(32px, 7vw, 96px); align-items: start; padding: 120px 0 72px; @media (max-width: 720px) { grid-template-columns: 1fr; }")
}
fn intro_heading_style() -> Mixin {
    css("max-width: 620px; margin: 0; font-size: clamp(34px, 4.5vw, 58px); font-weight: 700; line-height: 1; letter-spacing: -0.045em; color: var(--page-heading);")
}
fn intro_copy_style() -> Mixin {
    css("max-width: 660px; margin: 0; font-size: 17px; line-height: 1.65; color: var(--page-muted);")
}
fn comparison_section_style() -> Mixin {
    css("min-height: 105svh; display: grid; align-content: center; gap: 52px; padding: 120px 0; scroll-margin-top: 84px;")
}
fn comparison_header_style() -> Mixin {
    css("display: grid; grid-template-columns: minmax(280px, 0.9fr) minmax(280px, 0.65fr); align-items: end; gap: clamp(32px, 8vw, 112px); @media (max-width: 760px) { grid-template-columns: 1fr; }")
}
fn comparison_heading_style() -> Mixin {
    css("max-width: 700px; margin: 0; font-size: clamp(40px, 5.5vw, 68px); line-height: 1; letter-spacing: -0.05em; color: var(--page-text);")
}
fn comparison_description_style() -> Mixin {
    css("& p { margin: 0 0 18px; font-size: 15px; line-height: 1.65; color: var(--page-muted); }")
}
fn comparison_grid_style() -> Mixin {
    css("display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; @media (max-width: 900px) { grid-template-columns: 1fr; }")
}
fn delivery_card_style() -> Mixin {
    css("min-width: 0; border: 1px solid var(--surface-border); border-radius: 10px; padding: clamp(12px, 2vw, 18px); background-color: var(--surface-bg); transition: background-color 180ms ease, border-color 180ms ease;")
}
fn delivery_header_style() -> Mixin {
    css("min-height: 104px; display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; padding: 8px 8px 20px; @media (max-width: 520px) { flex-direction: column; gap: 12px; }")
}
fn delivery_heading_style() -> Mixin {
    css("margin: 0; font-size: 22px; line-height: 1.1; letter-spacing: -0.035em; color: var(--page-heading);")
}
fn delivery_detail_style() -> Mixin {
    css("max-width: 230px; margin: 0; font-size: 12px; line-height: 1.55; color: var(--page-muted); & strong { color: var(--page-heading); }")
}
fn exhibit_section_style() -> Mixin {
    css("min-height: 100svh; display: grid; align-items: center; border-top: 1px solid var(--page-rule); padding: 12vh 0; scroll-margin-top: 84px;")
}
fn section_grid_style() -> Mixin {
    css("display: grid; grid-template-columns: minmax(250px, 0.62fr) minmax(0, 1fr); align-items: center; gap: clamp(42px, 8vw, 116px); @media (max-width: 820px) { grid-template-columns: 1fr; }")
}
fn section_header_style() -> Mixin {
    css("align-self: start;")
}
fn section_index_style() -> Mixin {
    css("display: flex; gap: 14px; margin: 0 0 22px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; line-height: 1.4; color: var(--page-subtle); & span:first-child { font-weight: 700; color: var(--page-heading); }")
}
fn section_heading_style() -> Mixin {
    css("margin: 0; font-size: clamp(34px, 4.6vw, 62px); line-height: 0.98; letter-spacing: -0.055em; color: var(--page-text);")
}
fn section_description_style() -> Mixin {
    css("margin: 22px 0 20px; font-size: 16px; line-height: 1.65; color: var(--page-muted);")
}
fn route_style() -> Mixin {
    css("display: inline-block; max-width: 100%; overflow: hidden; font-size: 11px; text-overflow: ellipsis; white-space: nowrap; color: var(--page-subtle);")
}
fn frame_shell_style() -> Mixin {
    css("min-width: 0; min-height: 320px; border: 1px solid var(--surface-border); border-radius: 10px; padding: 6px; background-color: var(--frame-shell-bg); & > div > p { min-height: 304px; display: flex; align-items: center; justify-content: center; gap: 14px; margin: 0; border: 1px dashed var(--placeholder-border); border-radius: 6px; padding: 24px; background-color: var(--placeholder-bg); color: var(--placeholder-text); } & > div > p > span:first-child { flex: 0 0 auto; width: 10px; height: 10px; border-radius: 50%; background-color: var(--placeholder-dot); } & > div > p strong { display: block; font-size: 13px; color: var(--placeholder-heading); } & > div > p small { display: block; margin-top: 4px; font-size: 11px; line-height: 1.5; } & .plain-frame { min-height: 304px; border: 1px solid var(--plain-border); border-radius: 6px; padding: clamp(24px, 4vw, 42px); background-color: var(--plain-bg); color: var(--plain-text); } & .plain-frame > header { display: flex; justify-content: space-between; gap: 24px; margin-bottom: 28px; } & .plain-frame > header p { margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; color: var(--plain-muted); } & .plain-frame h3 { max-width: 620px; margin: 0; font-family: Georgia, Times New Roman, serif; font-size: clamp(25px, 3vw, 38px); font-weight: 500; line-height: 1.12; letter-spacing: -0.025em; } & .plain-frame ul { display: grid; gap: 9px; margin: 28px 0; padding-left: 21px; font-size: 14px; line-height: 1.5; color: var(--plain-list); } & .plain-frame > p:last-child { margin: 0; border-top: 1px solid var(--plain-rule); padding-top: 16px; font-size: 11px; line-height: 1.5; color: var(--plain-muted); }")
}
fn footer_style() -> Mixin {
    css("min-height: 55svh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 80px 24px; background-color: var(--footer-bg); text-align: center; color: var(--footer-text); transition: background-color 180ms ease, color 180ms ease;")
}
fn footer_heading_style() -> Mixin {
    css("max-width: 860px; margin: 0; font-size: clamp(38px, 6vw, 76px); line-height: 0.98; letter-spacing: -0.055em;")
}
fn footer_link_style() -> Mixin {
    css("margin-top: 42px; border-bottom: 1px solid var(--footer-muted); padding-bottom: 5px; color: var(--footer-text); font-size: 13px; font-weight: 700; text-decoration: none; &:hover { border-color: var(--footer-text); }")
}

#[cfg(test)]
mod tests;

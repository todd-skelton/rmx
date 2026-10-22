//! The spa demo: a client-only app whose router runs in the browser.
//!
//! `routes`, `router` and the pages are ordinary rmx code; the same router could run on a
//! server with `rmx-render-middleware`. In the page, [`start`] hands it to `rmx::spa::run`.

use std::sync::LazyLock;

use http::StatusCode;
use rmx::{css, on, rsx, Children, Component, FrameEvent, Handle, Mixin, Node};
use rmx::router::routes::{get, post, Route, RouteMap};
use rmx::router::{action, Context, Next, Router};
use rmx::spa::{Render, ResponseInit};

// app/routes.ts
#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub about: Route,
    pub greet: Route,
    pub submit_greet: Route,
}

pub static ROUTES: LazyLock<Routes> =
    LazyLock::new(|| Routes { home: get("/"), about: get("/about"), greet: get("/greet"), submit_greet: post("/greet") });

/// Simulated route latency (`actions/sleep.ts`). Dropped when a newer navigation
/// supersedes the request, which cancels the wait.
async fn sleep(ms: i32) {
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ms;
}

fn log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&message.into());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = message;
}

/// `[SPA] → GET /about` / `[SPA] ← 200 GET /about (701 ms)`
async fn log_spa_requests(ctx: Context, next: Next) -> rmx::fetch::Response {
    let (method, url) = (ctx.method().clone(), ctx.url().clone());
    let path = format!("{}{}", url.pathname(), url.search());
    let start = rmx::browser::now();
    log(&format!("[SPA] \u{2192} {method} {path}"));
    let res = next.run(ctx).await;
    let ms = (rmx::browser::now() - start).round();
    log(&format!("[SPA] \u{2190} {} {method} {} ({ms} ms)", res.status().as_u16(), url.pathname()));
    res
}

// app/router.tsx and app/actions/controller.tsx
pub fn router() -> Router {
    let mut router = Router::new();
    router.use_middleware(log_spa_requests);
    router.map(
        &*ROUTES,
        RoutesActions {
            home: action(|ctx: Context| async move {
                sleep(700).await;
                ctx.render(rsx! { <HomePage url={ctx.url().pathname().to_string()}/> })
            }),
            about: action(|ctx: Context| async move {
                sleep(700).await;
                ctx.render(rsx! { <AboutPage url={ctx.url().pathname().to_string()}/> })
            }),
            greet: action(|ctx: Context| async move {
                ctx.render(rsx! { <GreetingPage name="friend" url={ctx.url().pathname().to_string()}/> })
            }),
            submit_greet: action(|mut ctx: Context| async move {
                let form = ctx.take_body().form().await.unwrap_or_default();
                let name = form.get("name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("friend").to_string();
                sleep(700).await;
                ctx.render(rsx! { <GreetingPage is_submission name={name} url={ctx.url().pathname().to_string()}/> })
            }),
        },
    );
    router.not_found(|ctx: Context| async move {
        ctx.render_with(rsx! { <NotFoundPage url={ctx.url().pathname().to_string()}/> }, ResponseInit::status(StatusCode::NOT_FOUND))
    });
    router
}

/// Browser entry: run the router in the page.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::spa::run(router(), rmx::spa::RunOptions { fallback: Some(rsx! { <LoadingIndicator/> }) });
}

// ---------------------------------------------------------------------------------------
// Pages (app/actions/pages.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct PageProps {
    /// The route's pathname.
    pub url: String,
}

pub struct HomePage;

impl Component for HomePage {
    type Props = PageProps;

    fn setup(_: &PageProps, _: &Handle<Self>) -> Self {
        HomePage
    }

    fn render(&self, props: &PageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout url={props.url.clone()}>
                <article>
                    <p mix={eyebrow_style()}>"Home"</p>
                    <h1 mix={title_style()}>"A client-only rmx app"</h1>
                    <p mix={body_style()}>
                        "Route handlers use " <code>"ctx.render()"</code> " while the SPA runtime preserves the router's "
                        <code>"Request"</code> "-to-" <code>"Response"</code> " contract."
                    </p>
                    <form method="POST" action={ROUTES.submit_greet.href(())} mix={form_style()}>
                        <label for="name" mix={label_style()}>"What should we call you?"</label>
                        <div mix={form_controls_style()}>
                            <input id="name" name="name" autocomplete="name" required mix={input_style()}/>
                            <button type="submit" mix={button_style()}>"Submit"</button>
                        </div>
                    </form>
                </article>
            </Layout>
        }
    }
}

pub struct AboutPage;

impl Component for AboutPage {
    type Props = PageProps;

    fn setup(_: &PageProps, _: &Handle<Self>) -> Self {
        AboutPage
    }

    fn render(&self, props: &PageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout url={props.url.clone()}>
                <article>
                    <p mix={eyebrow_style()}>"About"</p>
                    <h1 mix={title_style()}>"URLs in, rendered UI out"</h1>
                    <p mix={body_style()}>
                        "Each route waits briefly before returning a SPA response, making top-frame loading and cancellation visible without introducing a second navigation system."
                    </p>
                </article>
            </Layout>
        }
    }
}

pub struct NotFoundPage;

impl Component for NotFoundPage {
    type Props = PageProps;

    fn setup(_: &PageProps, _: &Handle<Self>) -> Self {
        NotFoundPage
    }

    fn render(&self, props: &PageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout url={props.url.clone()}>
                <article>
                    <p mix={eyebrow_style()}>"404"</p>
                    <h1 mix={title_style()}>"Page not found"</h1>
                    <p mix={body_style()}>"Try going back to the " <a href={ROUTES.home.href(())}>"home page"</a> "."</p>
                </article>
            </Layout>
        }
    }
}

#[derive(Clone, Default)]
pub struct GreetingPageProps {
    pub url: String,
    pub is_submission: bool,
    pub name: String,
}

/// Shows its own pending state while a submission to the current URL is in flight.
pub struct GreetingPage {
    is_pending: bool,
}

impl Component for GreetingPage {
    type Props = GreetingPageProps;

    fn setup(_: &GreetingPageProps, h: &Handle<Self>) -> Self {
        h.on_frame(&h.frame(), FrameEvent::ReloadComplete, |this, h| {
            if this.is_pending {
                this.is_pending = false;
                h.update();
            }
        });
        GreetingPage { is_pending: false }
    }

    fn render(&self, props: &GreetingPageProps, h: &Handle<Self>) -> Node {
        rsx! {
            <Layout url={props.url.clone()}>
                <article>
                    {props.is_submission.then(|| rsx! { <p mix={eyebrow_style()}>"Form submitted"</p> })}
                    <h1 mix={title_style()}>{format!("Hello, {}!", props.name)}</h1>
                    <p mix={body_style()}>
                        "The frame navigation listener turns the native form navigation into a routed request. Back and forward traversals revisit this URL with GET because history entries do not retain form data."
                    </p>
                    <form method="POST" action={ROUTES.submit_greet.href(())} mix={[form_style(), on("submit", h.on(|this, h| {
                        this.is_pending = true;
                        h.update();
                    }))]}>
                        <label for="next-name" mix={label_style()}>"Try another name"</label>
                        <div mix={form_controls_style()}>
                            <input id="next-name" name="name" autocomplete="name" required mix={input_style()}/>
                            <button type="submit" disabled={self.is_pending} aria-busy={if self.is_pending { "true" } else { "false" }} mix={button_style()}>
                                {if self.is_pending { "Submitting\u{2026}" } else { "Submit again" }}
                            </button>
                        </div>
                    </form>
                    <p mix={body_style()}>
                        "This form targets the current URL, so it replaces the active history entry. The first submission from "
                        <a href={ROUTES.home.href(())}>"home"</a> " pushes a new entry."
                    </p>
                </article>
            </Layout>
        }
    }
}

fn eyebrow_style() -> Mixin {
    css("margin: 0 0 0.5rem; color: #6a48d7; font-size: 0.75rem; font-weight: 700; letter-spacing: 0.12em; text-transform: uppercase;")
}
fn title_style() -> Mixin {
    css("margin: 0; font-size: clamp(2rem, 7vw, 3.5rem); line-height: 1.05;")
}
fn body_style() -> Mixin {
    css("max-width: 38rem; margin: 1.5rem 0 0; color: #5c5965; font-size: 1.125rem; line-height: 1.7;")
}
fn form_style() -> Mixin {
    css("display: grid; gap: 0.75rem; max-width: 30rem; margin-top: 2rem;")
}
fn label_style() -> Mixin {
    css("font-weight: 700;")
}
fn form_controls_style() -> Mixin {
    css("display: flex; gap: 0.75rem;")
}
fn input_style() -> Mixin {
    css("min-width: 0; flex: 1; border: 1px solid #bcb4d4; border-radius: 0.6rem; padding: 0.7rem 0.8rem; font: inherit;")
}
fn button_style() -> Mixin {
    css("border: 0; border-radius: 0.6rem; padding: 0.7rem 1rem; color: white; background-color: #5b36d6; font: inherit; font-weight: 700; cursor: pointer; &:disabled { cursor: wait; opacity: 0.7; }")
}

// ---------------------------------------------------------------------------------------
// App shell (app/ui/app-shell.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct LayoutProps {
    pub url: String,
    pub children: Children,
}

/// Shows a loading indicator while the top frame loads a different URL.
pub struct Layout {
    show_loading: bool,
}

impl Component for Layout {
    type Props = LayoutProps;

    fn setup(_: &LayoutProps, h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| {
            let frame = h.frame();
            h.on_frame(&frame, FrameEvent::ReloadStart, |this, h| {
                let loading = rmx::url::Url::parse(&h.frame().src()).map(|u| u.pathname().to_string()).unwrap_or_default();
                let current = h.props().map(|p| p.url).unwrap_or_default();
                this.show_loading = loading != current;
                h.update();
            });
            h.on_frame(&frame, FrameEvent::ReloadComplete, |this, h| {
                if this.show_loading {
                    this.show_loading = false;
                    h.update();
                }
            });
        });
        Layout { show_loading: false }
    }

    fn render(&self, props: &LayoutProps, _: &Handle<Self>) -> Node {
        let nav = [(&ROUTES.home, "Home"), (&ROUTES.about, "About"), (&ROUTES.greet, "Greet")].map(|(route, label)| {
            let href = route.href(());
            rsx! { <NavLink current={props.url == href} href={href}>{label}</NavLink> }
        });
        rsx! {
            <div mix={app_shell_style()}>
                <div mix={content_style()}>
                    <header mix={header_style()}>
                        <a href={ROUTES.home.href(())} mix={brand_style()}>"rmx SPA"</a>
                        <nav aria-label="Main navigation" mix={nav_style()}>{nav.to_vec()}</nav>
                    </header>
                    <main aria-busy={if self.show_loading { "true" } else { "false" }} mix={main_style()}>
                        {if self.show_loading { rsx! { <LoadingIndicator/> } } else { rmx::IntoNode::into_node(&props.children) }}
                    </main>
                </div>
            </div>
        }
    }
}

#[derive(Clone, Default)]
pub struct NavLinkProps {
    pub href: String,
    pub current: bool,
    pub children: Children,
}

pub struct NavLink;

impl Component for NavLink {
    type Props = NavLinkProps;

    fn setup(_: &NavLinkProps, _: &Handle<Self>) -> Self {
        NavLink
    }

    fn render(&self, props: &NavLinkProps, _: &Handle<Self>) -> Node {
        rsx! {
            <a href={props.href.clone()} aria-current={props.current.then_some("page")} mix={nav_link_style()}>{&props.children}</a>
        }
    }
}

#[derive(Clone, Default)]
pub struct NoProps {}

pub struct LoadingIndicator;

impl Component for LoadingIndicator {
    type Props = NoProps;

    fn setup(_: &NoProps, _: &Handle<Self>) -> Self {
        LoadingIndicator
    }

    fn render(&self, _: &NoProps, _: &Handle<Self>) -> Node {
        rsx! { <div role="status" mix={loading_style()}>"Loading\u{2026}"</div> }
    }
}

fn app_shell_style() -> Mixin {
    css("position: fixed; inset: 0; min-width: 320px; overflow: auto; color: #202124; background-color: #f7f5ff; font-family: Inter, ui-sans-serif, system-ui, sans-serif; font-synthesis: none; & * { box-sizing: border-box; }")
}
fn content_style() -> Mixin {
    css("width: min(100% - 2rem, 48rem); margin: 0 auto;")
}
fn header_style() -> Mixin {
    css("display: flex; align-items: center; justify-content: space-between; padding: 1.5rem 0;")
}
fn brand_style() -> Mixin {
    css("color: inherit; font-size: 1.125rem; font-weight: 700; text-decoration: none;")
}
fn nav_style() -> Mixin {
    css("display: flex; gap: 0.5rem;")
}
fn nav_link_style() -> Mixin {
    css("border-radius: 999px; padding: 0.5rem 0.75rem; color: #5b36d6; text-decoration: none; &:hover, &[aria-current=\"page\"] { background-color: #e7e0ff; }")
}
fn main_style() -> Mixin {
    css("min-height: 18rem; border: 1px solid #ded8ef; border-radius: 1rem; background-color: white; box-shadow: 0 1rem 3rem rgb(64 44 120 / 10%); padding: clamp(2rem, 8vw, 5rem);")
}
fn loading_style() -> Mixin {
    css("width: 100%; padding: 2rem; text-align: center; color: #6a48d7; font-size: 1.125rem;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The routes need no reactor on the host (`sleep` is a no-op there).
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context as TaskContext, Poll, Waker};
        let mut cx = TaskContext::from_waker(Waker::noop());
        let mut f = std::pin::pin!(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    fn render(url: &str, method: &str, form: Option<Vec<(String, String)>>) -> (Option<String>, String) {
        let request = rmx::FrameRequest { url: url.into(), method: method.into(), form, enc_type: None, target: None, signal: rmx::Signal::never() };
        let res = block_on(rmx::spa::resolve(&router(), request)).expect("route renders");
        let html = match res.content {
            rmx::FrameContent::Node(node) => rmx::render_to_string(&node),
            rmx::FrameContent::Html(html) => html,
        };
        (res.url, html)
    }

    #[test]
    fn routes_render_pages() {
        let (_, home) = render("http://localhost/", "GET", None);
        assert!(home.contains("A client-only rmx app"));
        assert!(home.contains(r#"aria-current="page""#));
        let (_, about) = render("http://localhost/about", "GET", None);
        assert!(about.contains("URLs in, rendered UI out"));
        let (_, missing) = render("http://localhost/nope", "GET", None);
        assert!(missing.contains("Page not found"));
    }

    #[test]
    fn form_posts_reach_the_route() {
        let (_, greet) = render("http://localhost/greet", "POST", Some(vec![("name".into(), " Ada ".into())]));
        assert!(greet.contains("Hello, Ada!"));
        assert!(greet.contains("Form submitted"));
        let (_, blank) = render("http://localhost/greet", "POST", Some(vec![("name".into(), "  ".into())]));
        assert!(blank.contains("Hello, friend!"));
    }
}

//! The server half: `router.ts`, `middleware/i18n.ts`, `actions/controller.tsx`,
//! `actions/home-page.tsx` and `ui/{document,i18n}.tsx`.

mod styles;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use http::{header, HeaderValue, StatusCode};
use rmx::{rsx, Children, Component, Handle, Node};
use rmx::cookie::{Cookie, CookieProperties, SameSite};
use rmx::fetch::{response, Response};
use rmx::headers::{AcceptLanguage, Vary};
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
use rmx::render::Render;
use rmx::router::routes::{get, post, Route, RouteMap};
use rmx::router::{action, Context, Controller, Middleware, Next, Router};
use serde_json::Value;

use crate::i18n::{DetectionSource, Language, Translator};
use crate::intl::{format_currency, format_date_time, format_number, format_relative_days};
use crate::{CartPreview, NumberPreview};
use styles::*;

pub fn main() {
    rmx::router::routes::routes_command(&*ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44107".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "demos/i18n/public".into());
    let addr = format!("{host}:{port}");
    println!("i18n demo running at http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router(&public))).expect("serve");
}

// ---------------------------------------------------------------------------------------
// routes.ts and router.ts
// ---------------------------------------------------------------------------------------

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub language: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { home: get("/(:locale)"), language: post("/language") });

/// `routes.home.href({ locale })`
pub fn home_href(locale: Option<Language>) -> String {
    match locale {
        Some(l) => ROUTES.home.href([("locale", l.code())]),
        None => ROUTES.home.href(()),
    }
}

pub fn router(public: &str) -> Router {
    let mut router = Router::new();
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, index: vec![], ..Default::default() },
    ));
    router.use_middleware(form_data());

    // actions/controller.tsx: the controller's middleware runs after a route matches, so
    // it sees `params.locale`.
    router.map(
        &*ROUTES,
        Controller::new(RoutesActions {
            home: action(|ctx: Context| async move {
                let i18n = *i18n(&ctx);
                ctx.render(rsx! { <I18nProvider value={i18n}><HomePage/></I18nProvider> })
            }),
            language: action(language),
        })
        .with(i18n_middleware()),
    );
    router
}

async fn language(ctx: Context) -> Response {
    let form = ctx.form_data();
    let set_cookie = |res: &mut Response, value: String| {
        res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&value).expect("cookie header"));
    };
    match form.get("intent") {
        Some("clear") => {
            let mut res = response::redirect_with(&home_href(None), StatusCode::SEE_OTHER);
            set_cookie(&mut res, LOCALE_COOKIE.serialize_with("", &CookieProperties { expires: Some(UNIX_EPOCH), max_age: Some(0), ..Default::default() }));
            res
        }
        Some("save") => {
            let Some(locale) = form.get("locale").and_then(Language::parse) else {
                return response::status(StatusCode::BAD_REQUEST, "Invalid locale");
            };
            let mut res = response::redirect_with(&home_href(Some(locale)), StatusCode::SEE_OTHER);
            set_cookie(&mut res, LOCALE_COOKIE.serialize(locale.code()));
            res
        }
        _ => response::status(StatusCode::BAD_REQUEST, "Invalid intent"),
    }
}

// ---------------------------------------------------------------------------------------
// i18n/config.ts
// ---------------------------------------------------------------------------------------

/// The locale is a non-sensitive preference, so this cookie does not need a signature.
pub static LOCALE_COOKIE: LazyLock<Cookie> = LazyLock::new(|| {
    Cookie::new("locale")
        .http_only(true)
        .max_age(60 * 60 * 24 * 365)
        .path("/")
        .same_site(SameSite::Lax)
        .secure(std::env::var("RMX_ENV").is_ok_and(|v| v == "production"))
});

/// `resources`: the translation tables in `locales/*.json`, generated from the Remix
/// demo's `i18n/locales/*.ts`.
static RESOURCES: LazyLock<[Value; 5]> = LazyLock::new(|| {
    [
        include_str!("../locales/en.json"),
        include_str!("../locales/es.json"),
        include_str!("../locales/fr.json"),
        include_str!("../locales/ja.json"),
        include_str!("../locales/ar.json"),
    ]
    .map(|json| serde_json::from_str(json).expect("valid locale JSON"))
});

pub fn resources(language: Language) -> &'static Value {
    &RESOURCES[Language::ALL.iter().position(|l| *l == language).expect("listed language")]
}

/// `I18nState`: what the middleware resolves for each request.
#[derive(Clone, Copy, Debug, Default)]
pub struct I18nState {
    pub locale: Language,
    pub detection_source: DetectionSource,
}

impl I18nState {
    pub fn direction(&self) -> &'static str {
        self.locale.direction()
    }

    /// `createTranslator(instance, locale)`, falling back to English.
    pub fn t(&self) -> Translator<'static> {
        Translator::new(self.locale, resources(self.locale), Some(resources(Language::FALLBACK)))
    }
}

// ---------------------------------------------------------------------------------------
// middleware/i18n.ts
// ---------------------------------------------------------------------------------------

/// Resolve a locale from the URL path, preference cookie, or browser preference, falling
/// back to the configured default language.
pub fn detect_language(headers: &http::HeaderMap, path_locale: Option<&str>) -> (Language, DetectionSource) {
    if let Some(locale) = path_locale.and_then(Language::parse) {
        return (locale, DetectionSource::Path);
    }
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(locale) = LOCALE_COOKIE.parse(header(header::COOKIE)).as_deref().and_then(Language::parse) {
        return (locale, DetectionSource::Cookie);
    }
    let codes = Language::ALL.map(Language::code);
    let preferred = AcceptLanguage::parse(header(header::ACCEPT_LANGUAGE)).preferred(&codes).and_then(Language::parse);
    (preferred.unwrap_or(Language::FALLBACK), DetectionSource::Browser)
}

/// Each request gets its own state, so concurrent requests cannot change each other's
/// language.
pub fn i18n_middleware() -> impl Middleware {
    |mut ctx: Context, next: Next| async move {
        let path_locale = ctx.param("locale").map(str::to_string);
        if path_locale.as_deref().is_some_and(|p| Language::parse(p).is_none()) {
            return response::not_found();
        }
        let (locale, detection_source) = detect_language(ctx.headers(), path_locale.as_deref());
        ctx.set(I18nState { locale, detection_source });

        let mut res = next.run(ctx).await;
        let is_html = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html"));
        if is_html {
            let headers = res.headers_mut();
            headers.insert(header::CONTENT_LANGUAGE, HeaderValue::from_static(locale.code()));
            if detection_source != DetectionSource::Path {
                let mut vary = Vary::from_headers(headers.get_all(header::VARY).iter().filter_map(|v| v.to_str().ok()));
                vary.add("Accept-Language");
                vary.add("Cookie");
                headers.insert(header::VARY, HeaderValue::from_str(&vary.to_string()).expect("vary header"));
            }
        }
        res
    }
}

/// `context.i18n`
pub fn i18n(ctx: &Context) -> &I18nState {
    ctx.get::<I18nState>().expect("i18n_middleware runs before the controller")
}

// ---------------------------------------------------------------------------------------
// ui/i18n.tsx and ui/document.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct I18nProviderProps {
    pub value: I18nState,
    pub children: Children,
}

/// Provides the request-scoped state to the server-rendered component tree.
pub struct I18nProvider;

impl Component for I18nProvider {
    type Props = I18nProviderProps;

    fn setup(props: &I18nProviderProps, h: &Handle<Self>) -> Self {
        h.set_context(props.value);
        I18nProvider
    }

    fn render(&self, props: &I18nProviderProps, _: &Handle<Self>) -> Node {
        rsx! { <>{&props.children}</> }
    }
}

/// `getI18n(handle)`
pub fn get_i18n<C: Component>(h: &Handle<C>) -> I18nState {
    *h.context::<I18nState>().expect("rendered inside I18nProvider")
}

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub lang: &'static str,
    pub dir: &'static str,
    pub title: String,
    pub children: Children,
}

/// HTML document shell with explicit localized metadata.
pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        rsx! {
            <html lang={props.lang} dir={props.dir}>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>{props.title.clone()}</title>
                    <link rel="icon" href="/favicon.svg" type="image/svg+xml" sizes="any"/>
                    <script type="module" async src="/boot.js"></script>
                </head>
                <body mix={body()}>{&props.children}</body>
            </html>
        }
    }
}

// ---------------------------------------------------------------------------------------
// actions/home-page.tsx
// ---------------------------------------------------------------------------------------

const TASK_COUNTS: [u64; 7] = [0, 1, 2, 5, 11, 100, 1_000_000];
/// `new Date(Date.UTC(2026, 3, 15, 14, 30))`
const DEMO_DATE: ((i32, u8, u8), (u8, u8)) = ((2026, 4, 15), (14, 30));
const DEMO_CURRENCY: &str = "USD";

pub struct HomePage {
    i18n: I18nState,
}

impl Component for HomePage {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        HomePage { i18n: get_i18n(h) }
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let I18nState { locale, detection_source } = self.i18n;
        let t = self.i18n.t();
        let code = locale.code();
        let formatted_date = format_date_time(code, DEMO_DATE.0, DEMO_DATE.1);
        let formatted_number = format_number(code, 1_250_000);
        let formatted_currency = format_currency(code, DEMO_CURRENCY, "49.99");
        let formatted_relative_time = format_relative_days(code, -3);

        let options = Language::ALL.map(|l| {
            rsx! { <option key={l.code()} value={l.code()} selected={l == locale} lang={l.code()}>{l.name()}</option> }
        });
        let quick_links = Language::ALL.map(|l| {
            rsx! {
                <a key={l.code()} href={home_href(Some(l))} hreflang={l.code()} lang={l.code()} data-rmx-document
                    mix={quick_switch_pill()} aria-current={(l == locale).then_some("page")}>{l.name()}</a>
            }
        });
        let steps = DetectionSource::ALL.map(|source| {
            let active = source == detection_source;
            rsx! {
                <li key={source.as_str()} mix={step_item()} data-active={active.then_some("true")}>
                    <span>{t.t(&format!("detection.step_{}", source.as_str()))}</span>
                    {active.then(|| rsx! { <span mix={active_badge()}>{t.t("detection.active_badge")}</span> })}
                </li>
            }
        });
        let task_rows = TASK_COUNTS.map(|count| {
            rsx! {
                <div key={count} mix={demo_row()}>
                    <span mix={demo_label()} dir="ltr">{format!("count = {count}")}</span>
                    <span mix={demo_value()}>{t.t_count("pluralization.tasks", count)}</span>
                </div>
            }
        });
        let value_row = |label: &str, value: String| {
            rsx! {
                <div mix={demo_row()}>
                    <span mix={demo_label()}>{t.t(label)}</span>
                    <span mix={demo_value()}>{value}</span>
                </div>
            }
        };

        rsx! {
            <Document lang={code} dir={locale.direction()} title={t.t("common.title")}>
                <div mix={page_wrapper()}>
                    <header mix={header()}>
                        <a href={home_href(None)} data-rmx-document mix={brand_group()}>
                            <span mix={logo()} aria-hidden="true">"🌐"</span>
                            <span mix={brand_title()}>{t.t("common.brand")}</span>
                        </a>

                        <form method="POST" action={ROUTES.language.href(())} data-rmx-document mix={switcher_form()}>
                            <label for="locale-select" mix={sr_only()}>{t.t("switcher.label")}</label>
                            <select id="locale-select" name="locale" mix={select()}>{options}</select>
                            <button type="submit" name="intent" value="save" mix={button()}>{t.t("switcher.button")}</button>
                            <button type="submit" name="intent" value="clear" mix={clear_preference_button()}>
                                {t.t("switcher.clear_preference")}
                            </button>
                        </form>
                    </header>

                    <main>
                        <section mix={hero()}>
                            <div mix={eyebrow_badge()}>{t.t("hero.tagline")}</div>
                            <h1 mix={hero_heading()}>{t.t("hero.heading")}</h1>
                            <p mix={hero_description()}>{t.t("hero.description")}</p>
                            <div mix={hero_welcome()}>
                                <span aria-hidden="true">"👋"</span>
                                <span>{t.t_with("hero.welcome_user", &[("name", "Ada Lovelace")])}</span>
                            </div>
                        </section>

                        <nav aria-label={t.t("switcher.quick_switch")} mix={quick_switch_container()}>
                            <span mix={quick_switch_label()}>{format!("{}:", t.t("switcher.quick_switch"))}</span>
                            {quick_links}
                        </nav>

                        <div mix={grid()}>
                            <section mix={card()}>
                                <div mix={card_header()}>
                                    <span mix={card_icon()} aria-hidden="true">"🔍"</span>
                                    <h2 mix={card_title()}>{t.t("detection.title")}</h2>
                                </div>
                                <p mix={card_description()}>{t.t("detection.description")}</p>
                                <ol mix={step_list()}>{steps}</ol>
                                <div mix={demo_row()}>
                                    <span mix={demo_label()}>{format!("{}:", t.t("detection.active_source"))}</span>
                                    <span mix={demo_value()}>{t.t(&format!("detection.source_{}", detection_source.as_str()))}</span>
                                </div>
                            </section>

                            <section mix={card()}>
                                <div mix={card_header()}>
                                    <span mix={card_icon()} aria-hidden="true">"🔢"</span>
                                    <h2 mix={card_title()}>{t.t("pluralization.title")}</h2>
                                </div>
                                <p mix={card_description()}>{t.t("pluralization.description")}</p>
                                <div mix={demo_rows()}>{task_rows}</div>
                                <CartPreview locale={locale} translations={t.get("pluralization").cloned().unwrap_or_default()}/>
                            </section>

                            <section mix={card()}>
                                <div mix={card_header()}>
                                    <span mix={card_icon()} aria-hidden="true">"📅"</span>
                                    <h2 mix={card_title()}>{t.t("formatting.title")}</h2>
                                </div>
                                <p mix={card_description()}>{t.t_with("formatting.description", &[("language", locale.name())])}</p>
                                <div mix={demo_rows()}>
                                    {value_row("formatting.date_label", formatted_date)}
                                    {value_row("formatting.number_label", formatted_number)}
                                    {value_row("formatting.currency_label", formatted_currency)}
                                    {value_row("formatting.relative_time_label", formatted_relative_time)}
                                </div>
                            </section>

                            <section mix={card()}>
                                <div mix={card_header()}>
                                    <h2 mix={card_title()}>{t.t("formatting.preview_value")}</h2>
                                </div>
                                <p mix={card_description()}>{t.t("formatting.preview_description")}</p>
                                <NumberPreview locale={locale} button_label={t.t("formatting.preview_button")}
                                    value_label={t.t("formatting.preview_value")}/>
                            </section>
                        </div>
                    </main>

                    <footer mix={footer()}>
                        <p>{t.t("footer.note")}</p>
                    </footer>
                </div>
            </Document>
        }
    }
}

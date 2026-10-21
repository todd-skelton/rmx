use std::sync::{LazyLock, Mutex};

use frame_navigation_demo::*;
use http::{header, HeaderValue, StatusCode};
use rmx::{css, rsx, Component, Frame, Handle, Mixin, Node};
use rmx::cookie::{Cookie, CookieProperties, SameSite};
use rmx::fetch::{response, Response};
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::logger::logger;
use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
use rmx::render::{Render, ResponseInit};
use rmx::middleware::auth::{auth, require_auth as require_auth_for, scheme, scheme_fn, AuthExt, SchemeResult};
use frame_navigation_demo::routes::{AccountActions, AuthActions, MainActions, SettingsActions, ROUTES};
use rmx::router::routes::FormActions;
use rmx::router::{action, Action, Context, Controller, Middleware, Router};

pub fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44111".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "examples/frame-navigation/public".into());
    let addr = format!("{host}:{port}");
    println!("frame-navigation demo on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router(&public))).expect("serve");
}

// ---------------------------------------------------------------------------------------
// Auth (middleware/auth.ts): a cookie auth scheme, and route middleware that sends
// visitors to login
// ---------------------------------------------------------------------------------------

static AUTH: LazyLock<Cookie> =
    LazyLock::new(|| Cookie::new("frame-navigation-auth").http_only(true).same_site(SameSite::Lax).path("/"));

/// The identity the auth cookie stands for.
#[derive(Clone, Copy, Debug)]
struct FrameAuthIdentity;

fn load_auth() -> impl Middleware {
    auth(vec![scheme(scheme_fn("auth-cookie", |ctx: &Context| {
        let ok = AUTH.parse(ctx.header("cookie")).as_deref() == Some("1");
        async move { ok.then_some(SchemeResult::Success(FrameAuthIdentity)) }
    }))])
}

fn require_auth() -> impl Middleware {
    require_auth_for::<FrameAuthIdentity>().on_failure(|_, _| response::redirect("/auth/login"))
}

fn with_cookie(mut res: Response, set_cookie: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(set_cookie) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// A browser request for the named frame (soft navigation or reload).
fn is_frame_request(ctx: &Context, frame: &str) -> bool {
    ctx.is_frame_request() && ctx.frame_target() == Some(frame)
}

// ---------------------------------------------------------------------------------------
// Data (data/account.ts): kept in memory
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Account {
    display_name: String,
    program: String,
    expected_graduation: String,
}

static ACCOUNT: LazyLock<Mutex<Account>> = LazyLock::new(|| {
    Mutex::new(Account {
        display_name: "Riley Student".into(),
        program: "Human Computer Interaction".into(),
        expected_graduation: "2027-05".into(),
    })
});

fn account() -> Account {
    ACCOUNT.lock().expect("account lock").clone()
}

#[derive(Default)]
struct AccountErrors {
    display_name: Option<&'static str>,
    program: Option<&'static str>,
    expected_graduation: Option<&'static str>,
}

impl AccountErrors {
    fn any(&self) -> bool {
        self.display_name.is_some() || self.program.is_some() || self.expected_graduation.is_some()
    }
}

fn validate(a: &Account) -> AccountErrors {
    let month_ok = {
        let b = a.expected_graduation.as_bytes();
        b.len() == 7
            && b[..4].iter().all(u8::is_ascii_digit)
            && b[4] == b'-'
            && matches!(a.expected_graduation[5..].parse::<u8>(), Ok(1..=12))
            && b[5..].iter().all(u8::is_ascii_digit)
    };
    AccountErrors {
        display_name: if a.display_name.is_empty() {
            Some("Enter a display name.")
        } else if a.display_name.chars().count() > 80 {
            Some("Display name is too long.")
        } else {
            None
        },
        program: if a.program.is_empty() {
            Some("Enter a program.")
        } else if a.program.chars().count() > 120 {
            Some("Program is too long.")
        } else {
            None
        },
        expected_graduation: (!month_ok).then_some("Choose an expected graduation month."),
    }
}

// ---------------------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------------------

pub fn router(public: &str) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, index: vec![], ..Default::default() },
    ));
    router.use_middleware(form_data());
    router.use_middleware(load_auth());

    let r = &*ROUTES;
    router.map(
        &r.auth,
        AuthActions {
            logout: action(|_ctx: Context| async move {
                with_cookie(response::redirect(&ROUTES.auth.login.index.href(())), &AUTH.serialize_with("", &CookieProperties { max_age: Some(0), ..Default::default() }))
            }),
        },
    );
    router.map(
        &r.auth.login,
        FormActions {
            index: action(|ctx: Context| async move {
                if ctx.auth::<FrameAuthIdentity>().is_ok() {
                    return response::redirect(&ROUTES.main.index.href(()));
                }
                ctx.render(login_page())
            }),
            action: action(|_ctx: Context| async move {
                with_cookie(response::redirect(&ROUTES.main.index.href(())), &AUTH.serialize("1"))
            }),
        },
    );

    router.map(
        &r.main,
        Controller::new(MainActions {
            index: action(|ctx: Context| async move {
                ctx.render(rsx! { <Layout title="Dashboard" active_nav={MainNav::Dashboard}>{dashboard()}</Layout> })
            }),
            calendar: action(|ctx: Context| async move {
                ctx.render(rsx! { <Layout title="Calendar" active_nav={MainNav::Calendar}>{calendar()}</Layout> })
            }),
            courses: action(|ctx: Context| async move {
                let query = ctx.url().search_params().get("q").unwrap_or_default().trim().to_string();
                if is_frame_request(&ctx, COURSES_FRAME) {
                    return ctx.render(courses(&query));
                }
                let src = ctx.url().href();
                ctx.render(rsx! {
                    <Layout title="Courses" active_nav={MainNav::Courses}><Frame name={COURSES_FRAME} src={src}/></Layout>
                })
            }),
        })
        .with(require_auth()),
    );
    router.map(
        &r.main.account,
        Controller::new(AccountActions {
            index: action(|ctx: Context| async move {
                let saved = ctx.url().search_params().has("saved");
                if is_frame_request(&ctx, ACCOUNT_FRAME) {
                    return ctx.render(account_page(&account(), saved));
                }
                ctx.render(account_document("Account", ctx.url().href()))
            }),
        })
        .with(require_auth()),
    );
    router.map(
        &r.main.account.edit,
        Controller::new(FormActions { index: action(edit_account_page), action: action(update_account) }).with(require_auth()),
    );

    router.map(
        &r.settings,
        Controller::new(SettingsActions {
            index: action(|_ctx: Context| async move {
                response::redirect_with(&ROUTES.settings.overview.href(()), StatusCode::SEE_OTHER)
            }),
            overview: settings_action("overview"),
            profile: settings_action("profile"),
            notifications: settings_action("notifications"),
            privacy: settings_action("privacy"),
            grading: settings_action("grading"),
            integrations: settings_action("integrations"),
        })
        .with(require_auth()),
    );
    router
}

async fn edit_account_page(ctx: Context) -> Response {
    if is_frame_request(&ctx, ACCOUNT_FRAME) {
        return ctx.render(account_editor(&account(), &AccountErrors::default()));
    }
    ctx.render(account_document("Edit account", ctx.url().href()))
}

async fn update_account(ctx: Context) -> Response {
    let form = ctx.form_data();
    let field = |n: &str| form.get(n).unwrap_or_default().trim().to_string();
    let values = Account {
        display_name: field("displayName"),
        program: field("program"),
        expected_graduation: field("expectedGraduation"),
    };
    let errors = validate(&values);
    if errors.any() {
        let editor = account_editor(&values, &errors);
        let init = ResponseInit::status(StatusCode::UNPROCESSABLE_ENTITY);
        if is_frame_request(&ctx, ACCOUNT_FRAME) {
            return ctx.render_with(editor, init);
        }
        return ctx.render_with(rsx! { <Layout title="Account" active_nav={MainNav::Account}>{editor}</Layout> }, init);
    }
    *ACCOUNT.lock().expect("account lock") = values;
    response::redirect_with(&format!("{}?saved", ROUTES.main.account.index.href(())), StatusCode::SEE_OTHER)
}

/// `actions/settings/controller.tsx`: every section renders the same way.
fn settings_action(slug: &'static str) -> Action {
    action(move |ctx: Context| async move {
        let page = settings_section(slug);
        let init = if slug == "privacy" {
            // The demo's privacy page fails on purpose, to show error handling.
            ResponseInit::status(StatusCode::INTERNAL_SERVER_ERROR)
        } else {
            ResponseInit::default()
        };
        if is_frame_request(&ctx, SETTINGS_FRAME) {
            return ctx.render_with(settings_layout(slug, page), init);
        }
        let src = ctx.url().href();
        ctx.render_with(
            rsx! { <Layout title="Settings" active_nav={MainNav::Settings}><Frame name={SETTINGS_FRAME} src={src}/></Layout> },
            init,
        )
    })
}

// ---------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------

fn dashboard() -> Node {
    let card = |label: &str, value: &str, href: &str| StatCard { label: label.into(), value: value.into(), href: href.into() };
    let cards = vec![
        card("In progress", "4 courses", "/courses"),
        card("Due this week", "7 tasks", "/calendar"),
        card("Average grade", "92%", "/account"),
    ];
    rsx! {
        <section>
            <h1 mix={title_style()}>"Learning dashboard"</h1>
            <p mix={description_style()}>"Keep up with coursework, deadlines, and instructor updates from one central place."</p>
            <DashboardStatGrid cards={cards}/>
            <p mix={css("margin-top: 0.85rem; color: #64748b; font-size: 0.9rem;")}>"Try tabbing to a card, pressing Enter, or Cmd/Ctrl-clicking one."</p>
        </section>
    }
}

fn calendar() -> Node {
    rsx! {
        <section>
            <h1 mix={title_style()}>"Calendar"</h1>
            <p mix={description_style()}>"Track class sessions, assignment due dates, and office hours."</p>
            <div mix={card_style()}>
                <p mix={css("margin: 0; font-weight: 600;")}>"Today"</p>
                <p mix={css("margin: 0.6rem 0 0;")}>"11:00 AM - UX Research Workshop"</p>
                <p mix={css("margin: 0.4rem 0 0;")}>"3:30 PM - Intro to Accessibility Quiz"</p>
            </div>
        </section>
    }
}

const COURSES: [&str; 4] = [
    "Introduction to Product Design",
    "Applied Statistics for Engineers",
    "Web Accessibility Foundations",
    "Data Visualization for the Web",
];

fn courses(query: &str) -> Node {
    let q = query.to_lowercase();
    let filtered: Vec<&str> = COURSES.iter().copied().filter(|c| c.to_lowercase().contains(&q)).collect();
    let n = filtered.len();
    let summary = if query.is_empty() {
        format!("{n} courses")
    } else {
        format!("{n} {} matching \u{201c}{query}\u{201d}", if n == 1 { "course" } else { "courses" })
    };
    let list = if filtered.is_empty() {
        rsx! { <p mix={css("margin-top: 0.75rem; border: 1px dashed #cbd5e1; border-radius: 12px; padding: 1rem; color: #64748b;")}>"Try a different course name."</p> }
    } else {
        let items: Vec<Node> = filtered
            .iter()
            .map(|c| rsx! { <li mix={css("border: 1px solid #e2e8f0; border-radius: 12px; padding: 0.9rem 1rem;")}>{*c}</li> })
            .collect();
        rsx! { <ul id="course-list" aria-labelledby="course-results" mix={css("list-style: none; margin: 0.75rem 0 0; padding: 0; display: grid; gap: 0.75rem;")}>{items}</ul> }
    };
    rsx! {
        <section aria-labelledby="courses-heading">
            <h1 id="courses-heading" mix={title_style()}>"Courses"</h1>
            <p mix={description_style()}>"Continue where you left off in each course track."</p>
            <form method="GET" action="/courses" data-rmx-target={COURSES_FRAME} data-rmx-history="replace" data-rmx-reset-focus="manual"
                mix={css("margin-top: 1.5rem; max-width: 42rem;")}>
                <label for="course-filter" mix={css("display: block; margin-bottom: 0.5rem; color: #334155; font-size: 0.9rem; font-weight: 600;")}>"Filter courses"</label>
                <div mix={css("display: flex; align-items: center; flex-wrap: wrap; gap: 0.75rem;")}>
                    <CourseFilterInput query={query.to_string()}/>
                    <button type="submit" mix={primary_button()}>"Filter"</button>
                    {(!query.is_empty()).then(|| rsx! {
                        <a href="/courses" id="clear-filter" data-rmx-target={COURSES_FRAME} mix={css("color: #1d4ed8; font-weight: 600;")}>"Clear"</a>
                    })}
                </div>
            </form>
            <p id="course-results" role="status" mix={css("margin: 1.25rem 0 0; color: #475569; font-size: 0.9rem;")}>{summary}</p>
            {list}
        </section>
    }
}

fn card_style() -> Mixin {
    css("margin-top: 1rem; border: 1px solid #e2e8f0; border-radius: 14px; background-color: #ffffff; padding: 1rem;")
}

fn primary_button() -> Mixin {
    css("border: 1px solid #1d4ed8; border-radius: 8px; padding: 0.65rem 1rem; font: inherit; font-weight: 600; cursor: pointer; color: #ffffff; background-color: #2563eb; &:hover { background-color: #1d4ed8; }")
}

// Account -----------------------------------------------------------------------------

fn account_document(title: &str, frame_src: String) -> Node {
    rsx! { <Layout title={title.to_string()} active_nav={MainNav::Account}><Frame name={ACCOUNT_FRAME} src={frame_src}/></Layout> }
}

fn format_month(value: &str) -> String {
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    match value.split_once('-') {
        Some((year, month)) => match month.parse::<usize>() {
            Ok(m @ 1..=12) => format!("{} {year}", MONTHS[m - 1]),
            _ => value.to_string(),
        },
        None => value.to_string(),
    }
}

fn account_page(a: &Account, saved: bool) -> Node {
    rsx! {
        <section aria-labelledby="account-heading">
            <div mix={css("display: flex; align-items: start; justify-content: space-between; gap: 1rem;")}>
                <div>
                    <h1 id="account-heading" mix={title_style()}>"Account"</h1>
                    <p mix={description_style()}>"Manage your profile, enrollment details, and contact preferences."</p>
                </div>
                <a href="/account/edit" id="edit-account" data-rmx-target={ACCOUNT_FRAME} mix={primary_button()}>"Edit"</a>
            </div>
            {saved.then(|| rsx! {
                <p role="status" id="saved" mix={css("border: 1px solid #bbf7d0; background: #f0fdf4; color: #166534; border-radius: 10px; padding: 0.75rem 1rem;")}>"Account saved"</p>
            })}
            <dl id="account-details" mix={[card_style(), css("display: grid; grid-template-columns: 180px 1fr; row-gap: 0.6rem;")]}>
                <dt>"Name"</dt><dd mix={css("margin: 0;")}>{a.display_name.clone()}</dd>
                <dt>"Program"</dt><dd mix={css("margin: 0;")}>{a.program.clone()}</dd>
                <dt>"Expected graduation"</dt><dd mix={css("margin: 0;")}>{format_month(&a.expected_graduation)}</dd>
            </dl>
        </section>
    }
}

#[derive(Clone, Default)]
struct FormFieldProps {
    label: &'static str,
    name: &'static str,
    input_type: &'static str,
    value: String,
    error: Option<&'static str>,
    max_length: u32,
}

struct FormField;

impl Component for FormField {
    type Props = FormFieldProps;

    fn setup(_: &FormFieldProps, _: &Handle<Self>) -> Self {
        FormField
    }

    fn render(&self, p: &FormFieldProps, _: &Handle<Self>) -> Node {
        let error_id = format!("{}-error", p.name);
        rsx! {
            <div mix={css("display: grid; gap: 0.4rem; margin-bottom: 1rem;")}>
                <label for={p.name} mix={css("font-weight: 600; font-size: 0.9rem; color: #334155;")}>{p.label}</label>
                <input id={p.name} name={p.name} type={p.input_type} value={p.value.clone()} required maxlength={p.max_length}
                    aria-invalid={p.error.map(|_| "true")} aria-describedby={p.error.map(|_| error_id.clone())}
                    mix={css("border: 1px solid #cbd5e1; border-radius: 8px; padding: 0.65rem 0.75rem; font: inherit; &[aria-invalid=\"true\"] { border-color: #dc2626; }")}/>
                {p.error.map(|e| rsx! { <p id={error_id.clone()} class="field-error" mix={css("margin: 0; color: #b91c1c; font-size: 0.85rem;")}>{e}</p> })}
            </div>
        }
    }
}

fn account_editor(a: &Account, errors: &AccountErrors) -> Node {
    rsx! {
        <section aria-labelledby="account-heading" mix={css("max-width: 48rem;")}>
            <h1 id="account-heading" mix={title_style()}>"Edit account"</h1>
            <p mix={description_style()}>"Update the information shown in your student profile. This data is kept in memory and resets when the demo server restarts."</p>
            <form method="POST" action="/account/edit" id="account-form" mix={css("margin-top: 1.25rem;")}>
                <FormField label="Display name" name="displayName" input_type="text" value={a.display_name.clone()} error={errors.display_name} max_length={80}/>
                <FormField label="Program" name="program" input_type="text" value={a.program.clone()} error={errors.program} max_length={120}/>
                <FormField label="Expected graduation" name="expectedGraduation" input_type="month" value={a.expected_graduation.clone()} error={errors.expected_graduation} max_length={7}/>
                <button type="submit" mix={primary_button()}>"Save account"</button>
            </form>
        </section>
    }
}

// Settings ----------------------------------------------------------------------------

const SETTINGS_ITEMS: [(&str, &str); 6] = [
    ("overview", "Overview"),
    ("profile", "Profile"),
    ("notifications", "Notifications"),
    ("privacy", "Privacy"),
    ("grading", "Grading"),
    ("integrations", "Integrations"),
];

fn settings_layout(active: &str, content: Node) -> Node {
    let nav: Vec<Node> = SETTINGS_ITEMS
        .iter()
        .map(|(slug, label)| {
            let current = (*slug == active).then_some("page");
            rsx! { <a href={format!("/settings/{slug}")} data-rmx-target={SETTINGS_FRAME} aria-current={current}>{*label}</a> }
        })
        .collect();
    rsx! {
        <section mix={css("display: grid; grid-template-columns: 240px minmax(0, 1fr); gap: 1.25rem; align-items: start;")}>
            <aside mix={css("position: sticky; top: 1.5rem; border: 1px solid #e2e8f0; border-radius: 14px; background-color: #ffffff; padding: 0.9rem;")}>
                <p mix={css("margin: 0; color: #64748b; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.04em; font-weight: 600;")}>"Settings"</p>
                <nav id="settings-nav" mix={[css("margin-top: 0.7rem; display: grid; gap: 0.35rem; & a { text-decoration: none; border-radius: 10px; padding: 0.55rem 0.65rem; font-size: 0.9rem; color: #334155; font-weight: 500; } & a[aria-current=\"page\"] { color: #0f172a; background-color: #e2e8f0; font-weight: 600; }")]}>{nav}</nav>
            </aside>
            <section id="settings-content" mix={css("min-width: 0;")}>{content}</section>
        </section>
    }
}

fn settings_section(slug: &str) -> Node {
    let (title, description, rows): (&str, &str, Vec<(&str, &str)>) = match slug {
        "profile" => ("Profile", "Update personal details shown to instructors and classmates.", vec![("Display name", "Riley Student"), ("Timezone", "America/Los_Angeles")]),
        "notifications" => ("Notifications", "Choose how you hear about deadlines, grades, and announcements.", vec![("Assignment reminders", "Email + push"), ("Grade posted", "Email"), ("Course announcements", "Push")]),
        "privacy" => ("Privacy", "Control what activity and profile details are visible to classmates and collaborators.", vec![("Show course progress to classmates", "Off"), ("Show profile photo", "On")]),
        "grading" => ("Grading", "Preferences for how grades and feedback are displayed.", vec![("Grade format", "Percentage"), ("Show rubric feedback", "On")]),
        "integrations" => ("Integrations", "Connect calendars and tools to your workspace.", vec![("Google Calendar", "Connected"), ("GitHub Classroom", "Not connected")]),
        _ => ("Overview", "Manage your workspace preferences across profile, notifications, privacy, grading, and integrations.", vec![("Plan", "Student"), ("Language", "English (US)")]),
    };
    let rows: Vec<Node> = rows
        .into_iter()
        .map(|(k, v)| rsx! { <p mix={css("margin: 0; display: flex; justify-content: space-between; gap: 1rem;")}><span>{k}</span><strong>{v}</strong></p> })
        .collect();
    rsx! {
        <section>
            <h2 id="settings-title" mix={css("margin: 0; font-size: 1.5rem;")}>{title}</h2>
            <p mix={css("margin-top: 0.6rem; color: #475569;")}>{description}</p>
            <div mix={[card_style(), css("display: grid; gap: 0.6rem;")]}>{rows}</div>
        </section>
    }
}

// Login -------------------------------------------------------------------------------

fn login_page() -> Node {
    rsx! {
        <html lang="en">
            {head("Sign in | LMS")}
            <body mix={css("margin: 0; min-height: 100vh; font-family: Inter, ui-sans-serif, system-ui, sans-serif; color: #0f172a; background: radial-gradient(circle at top, rgba(99, 102, 241, 0.16), transparent 28%), #f8fafc;")}>
                <main mix={css("min-height: 100vh; display: grid; place-items: center; padding: 2rem;")}>
                    <section mix={css("max-width: 38rem; border: 1px solid #e2e8f0; border-radius: 18px; background-color: #ffffff; padding: 1.5rem; box-shadow: 0 12px 30px rgba(15, 23, 42, 0.06);")}>
                        <p mix={css("margin: 0; font-size: 0.8rem; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; color: #6366f1;")}>"Demo auth"</p>
                        <h2 id="login-title" mix={css("margin-top: 0.6rem; margin-bottom: 0.75rem; font-size: 1.7rem;")}>"Fake login page"</h2>
                        <p mix={css("margin: 0; color: #475569; line-height: 1.7;")}>"This route renders outside the app shell so it is obvious the protected frame navigation fell back to a top-level login page."</p>
                        <form method="POST" action="/auth/login" mix={css("margin-top: 1.5rem;")}>
                            <button type="submit" id="login" mix={css("border: none; border-radius: 999px; padding: 0.8rem 1.1rem; font-weight: 600; cursor: pointer; background-color: #0f172a; color: #ffffff;")}>"Set auth cookie"</button>
                        </form>
                    </section>
                </main>
            </body>
        </html>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::fetch::{block_on, Body};

    fn fetch(router: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> (u16, Option<String>, String) {
        let mut req = http::Request::builder().method(method).uri(format!("http://localhost{path}"));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = block_on(router.fetch(req.body(Body::empty()).unwrap()));
        let status = res.status().as_u16();
        let location = res.headers().get("location").map(|v| v.to_str().unwrap().to_string());
        (status, location, block_on(res.into_body().text()).unwrap())
    }

    /// The auth cookie as the login action sets it (`1`, base64-encoded like Remix).
    const SIGNED_IN: (&str, &str) = ("cookie", "frame-navigation-auth=MQ==");

    #[test]
    fn signed_in_cookie_is_what_login_sets() {
        assert_eq!(AUTH.serialize("1").split(';').next(), Some(SIGNED_IN.1));
    }

    #[test]
    fn controllers_require_auth_except_the_auth_routes() {
        let router = router("examples/frame-navigation/public");
        for path in ["/", "/courses", "/calendar", "/account", "/account/edit", "/settings", "/settings/profile"] {
            let (status, location, _) = fetch(&router, "GET", path, &[]);
            assert_eq!((status, location.as_deref()), (302, Some("/auth/login")), "{path}");
        }
        let (status, _, body) = fetch(&router, "GET", "/auth/login", &[]);
        assert_eq!(status, 200);
        assert!(body.contains("<form"), "{body}");
        let (status, location, _) = fetch(&router, "GET", "/auth/login", &[SIGNED_IN]);
        assert_eq!((status, location.as_deref()), (302, Some("/")));
        let (status, location, _) = fetch(&router, "POST", "/auth/logout", &[SIGNED_IN]);
        assert_eq!((status, location.as_deref()), (302, Some("/auth/login")));
    }

    #[test]
    fn settings_index_redirects_and_sections_render_in_their_frame() {
        let router = router("examples/frame-navigation/public");
        let (status, location, _) = fetch(&router, "GET", "/settings", &[SIGNED_IN]);
        assert_eq!((status, location.as_deref()), (303, Some("/settings/overview")));
        let (status, _, page) = fetch(&router, "GET", "/settings/profile", &[SIGNED_IN]);
        assert_eq!(status, 200);
        assert!(page.contains("<html"), "full document");
        let frame = [SIGNED_IN, ("x-remix-frame", "true"), ("x-remix-target", SETTINGS_FRAME)];
        let (status, _, partial) = fetch(&router, "GET", "/settings/profile", &frame);
        assert_eq!(status, 200);
        assert!(!partial.contains("<html") && partial.contains("Profile"), "{partial}");
        assert_eq!(fetch(&router, "GET", "/settings/privacy", &frame).0, 500);
    }

    #[test]
    fn account_edit_validates_in_the_account_frame() {
        let router = router("examples/frame-navigation/public");
        let (status, _, partial) =
            fetch(&router, "GET", "/account/edit", &[SIGNED_IN, ("x-remix-frame", "true"), ("x-remix-target", ACCOUNT_FRAME)]);
        assert_eq!(status, 200);
        assert!(!partial.contains("<html") && partial.contains("displayName"), "{partial}");
        let (status, _, _) = fetch(&router, "POST", "/account/edit", &[SIGNED_IN, ("x-remix-frame", "true"), ("x-remix-target", ACCOUNT_FRAME)]);
        assert_eq!(status, 422);
    }
}

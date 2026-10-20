//! The Remix social-auth demo in rmx: credentials login, Google, GitHub and X login through
//! rmx-auth, signup, forgot and reset password, flash messages, and session-backed route
//! protection.
//!
//! What maps to what:
//!
//! | Remix | rmx |
//! | --- | --- |
//! | `session(cookie, createFsSessionStorage(dir))` | `session(cookie, FsStorage::new(dir))` |
//! | `auth({ schemes: [createSessionAuthScheme(..)] })` | `auth(vec![scheme(session_scheme(..))])` |
//! | `requireAuth({ onFailure })` | `require_auth().on_failure(..)` |
//! | `completeAuth(context)` | `complete_auth(&ctx)` |
//! | `startExternalAuth` / `finishExternalAuth` | `rmx::auth::start_external_auth` / `finish_external_auth` |
//! | `createExternalProviderRegistry({ env, origin })` | `ExternalProviders::from_env(origin, env, fetch)` |
//! | `session.flash('error', ..)` / `session.get('error')` | the same, on `ctx.session()` |
//!
//! A provider's button is enabled when its `<NAME>_CLIENT_ID` and `<NAME>_CLIENT_SECRET` are
//! set, as in Remix. Not ported: SQLite (data lives in memory, see [`data`]).

pub mod data;
pub mod external;
pub mod ui;

use http::StatusCode;
use rmx::{css, rsx, Node};
use rmx::cookie::Cookie;
use rmx::fetch::{response, Response};
use rmx::middleware::auth::{auth, complete_auth, require_auth, scheme, session_scheme, Auth, AuthExt};
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::session::{session, SessionExt};
use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
use rmx::render::{Render, ResponseInit};
use rmx::router::{middleware, Context, IntoResponse, Middleware, Next, Router};
use rmx::session::{Session, SessionStorage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use data::{hash_password, normalize_email, normalize_optional_text, verify_password, AuthAccount, Db, User};
use external::{provider_label, ExternalProviders, ProviderLink};
use ui::*;

// ---------------------------------------------------------------------------------------
// Router (app/router.ts)
// ---------------------------------------------------------------------------------------

pub fn router<S: SessionStorage>(cookie: Cookie, storage: S, db: Db, public: &str, providers: ExternalProviders) -> Router {
    let mut router = Router::new();
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store, must-revalidate".into()), etag: ETag::Off, last_modified: false, ..Default::default() },
    ));
    router.use_middleware(form_data());
    router.use_middleware(session(cookie, storage));
    router.use_middleware(load_database(db.clone()));
    router.use_middleware(load_auth(db));

    // actions/controller.tsx
    let registry = providers.clone();
    router.get("/", move |ctx: Context| {
        let providers = registry.clone();
        async move { home(ctx, &providers).await }
    });
    router.group(vec![middleware(require_login())], |r| {
        r.get("/account", account);
    });

    // actions/auth/controller.tsx
    router.post("/auth/login", login);
    router.post("/auth/logout", |ctx: Context| async move {
        ctx.session().unset("auth");
        ctx.session().regenerate_id(true);
        response::redirect("/")
    });

    router.get("/auth/signup", |ctx: Context| async move { ctx.render(signup_page(&ctx, None, None, None)) });
    router.post("/auth/signup", signup);
    router.get("/auth/forgot-password", |ctx: Context| async move { ctx.render(forgot_password_page(&ctx, None, None)) });
    router.post("/auth/forgot-password", forgot_password);
    router.get("/auth/reset-password/:token", reset_password_form);
    router.post("/auth/reset-password/:token", reset_password);

    // actions/auth/{google,github,x}/controller.ts
    external::routes(&mut router, &providers);
    router
}

// ---------------------------------------------------------------------------------------
// Database and auth middleware (middleware/database.ts, middleware/auth.ts)
// ---------------------------------------------------------------------------------------

fn load_database(db: Db) -> impl Middleware {
    move |mut ctx: Context, next: Next| {
        ctx.set(db.clone());
        next.run(ctx)
    }
}

fn db(ctx: &Context) -> &Db {
    ctx.get::<Db>().expect("load_database() runs before handlers")
}

/// What login stores in the session under `auth` (utils/auth-session.ts).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub user_id: u64,
    pub login_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_account_id: Option<u64>,
}

fn parse_auth_session(session: &Session) -> Option<AuthSession> {
    let value: AuthSession = session.get("auth")?;
    matches!(value.login_method.as_str(), "credentials" | "google" | "github" | "x").then_some(value)
}

/// The identity resolved for each request.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthIdentity {
    pub login_method: String,
    pub user: User,
    pub auth_account: Option<AuthAccount>,
    pub provider_profile: Option<Value>,
}

fn load_auth(db: Db) -> impl Middleware {
    auth(vec![scheme(
        session_scheme(parse_auth_session, move |value: AuthSession| {
            let user = db.find_user(value.user_id);
            let auth_account = value.auth_account_id.and_then(|id| db.find_auth_account(id));
            // parseProviderProfile
            let provider_profile = auth_account.as_ref().and_then(|a| serde_json::from_str::<Value>(&a.profile_json).ok());
            async move { Some(AuthIdentity { user: user?, login_method: value.login_method, auth_account, provider_profile }) }
        })
        .invalidate(|session| session.unset("auth")),
    )])
}

fn require_login() -> impl Middleware {
    require_auth::<AuthIdentity>().on_failure(|_, _| response::redirect("/"))
}

/// `returnTo` from the query when it is a local path (not `//host`).
fn safe_return_to(ctx: &Context) -> Option<String> {
    let value = ctx.url().search_params().get("returnTo")?.to_string();
    (value.starts_with('/') && !value.starts_with("//")).then_some(value)
}

fn post_auth_redirect(ctx: &Context) -> String {
    safe_return_to(ctx).unwrap_or_else(|| "/account".into())
}

/// `path` with the request's safe `returnTo` carried along.
fn with_return_to(path: &str, ctx: &Context) -> String {
    match safe_return_to(ctx) {
        Some(r) => format!("{path}?{}", [("returnTo", r)].into_iter().collect::<rmx::url::SearchParams>()),
        None => path.to_string(),
    }
}

// ---------------------------------------------------------------------------------------
// Home, login and account (actions/controller.tsx, ui/home/*, ui/account-page.tsx)
// ---------------------------------------------------------------------------------------

async fn home(ctx: Context, providers: &ExternalProviders) -> Response {
    if ctx.auth::<AuthIdentity>().is_ok() {
        return response::redirect("/account");
    }
    let error = ctx.session().get::<String>("error");
    let success = ctx.session().get::<String>("success");
    let links = providers.links(&ctx);
    ctx.render(login_page(&ctx, error.as_deref(), success.as_deref(), links))
}

/// ui/provider-presentation.tsx
fn provider_icon(name: &str) -> Node {
    match name {
        "google" => google_icon(social_icon()),
        "github" => github_icon(social_icon()),
        _ => x_icon(social_icon()),
    }
}

/// ui/home/social-provider-button.tsx
fn social_provider_button(link: ProviderLink) -> Node {
    match link.href {
        Some(href) => rsx! {
            <a href={href} data-provider={link.name} mix={social_button()}>
                {provider_icon(link.name)}
                <span mix={social_button_label()}>{link.label}</span>
            </a>
        },
        None => rsx! {
            <button type="button" disabled aria-disabled="true" title={link.disabled_reason.unwrap_or_default()} data-provider={link.name}
                mix={[social_button(), social_button_disabled()]}>
                {provider_icon(link.name)}
                <span mix={social_button_label()}>{link.label}</span>
            </button>
        },
    }
}

fn login_page(ctx: &Context, error: Option<&str>, success: Option<&str>, links: Vec<ProviderLink>) -> Node {
    let providers: Vec<Node> = links.into_iter().map(social_provider_button).collect();
    rsx! {
        <Document title="Social Auth Demo">
            <AuthCard title="Welcome Back" subtitle="Sign in to your account"
                footer={rsx! { <p mix={footer_text()}>"Don't have an account? "<a href={with_return_to("/auth/signup", ctx)} mix={helper_link()}>"Sign up"</a></p> }}>
                {notice(Tone::Error, error)}
                {notice(Tone::Success, success)}
                <form method="POST" action={with_return_to("/auth/login", ctx)} mix={form()}>
                    <TextField id="email" name="email" input_type="email" label="Email" placeholder="Enter your email" auto_complete="email" required={true} icon={email_icon(field_icon())}/>
                    <TextField id="password" name="password" input_type="password" label="Password" placeholder="Enter your password" auto_complete="current-password" required={true} icon={password_icon(field_icon())}/>
                    <div mix={form_options()}>
                        <label mix={remember_me()}>
                            <input type="checkbox" name="remember" value="yes" mix={remember_checkbox()}/>
                            <span>"Remember me"</span>
                        </label>
                        <a href={with_return_to("/auth/forgot-password", ctx)} mix={helper_link()}>"Forgot password?"</a>
                    </div>
                    <button type="submit" mix={submit_button()}>"Sign In"</button>
                </form>
                <div mix={divider()}>
                    <div mix={divider_line()}></div>
                    <span mix={divider_text()}>"or continue with"</span>
                    <div mix={divider_line()}></div>
                </div>
                <div mix={social_buttons()}>{providers}</div>
            </AuthCard>
        </Document>
    }
}

async fn login(ctx: Context) -> Response {
    let email = normalize_email(ctx.form_data().get("email").unwrap_or_default());
    let password = ctx.form_data().get("password").unwrap_or_default().to_string();
    let user = db(&ctx).find_user_by_email(&email).filter(|u| u.password_hash.as_deref().is_some_and(|h| verify_password(&password, h)));
    let Some(user) = user else {
        ctx.session().flash("error", "Invalid email or password. Please try again.");
        return response::redirect(&with_return_to("/", &ctx));
    };
    complete_auth(&ctx).set("auth", AuthSession { user_id: user.id, login_method: "credentials".into(), auth_account_id: None });
    response::redirect(&post_auth_redirect(&ctx))
}

async fn account(ctx: Context) -> Response {
    let Auth::Authenticated { identity, .. } = ctx.auth::<AuthIdentity>() else {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };
    let user = &identity.user;
    let display_name = user.name.clone().or_else(|| user.email.clone()).unwrap_or_else(|| "Authenticated User".into());
    let provider = provider_label(&identity.login_method);
    let details = serde_json::to_string_pretty(&json!({
        "loginMethod": identity.login_method,
        "user": user,
        "authAccount": identity.auth_account,
        "providerProfile": identity.provider_profile,
    }))
    .unwrap_or_default();
    let avatar = match &user.avatar_url {
        Some(url) => rsx! { <img src={url.clone()} alt={display_name.clone()} mix={profile_avatar()}/> },
        None => rsx! { <div mix={profile_fallback_avatar()}>{user_icon(css("width: 2rem; height: 2rem;"))}</div> },
    };
    ctx.render(rsx! {
        <Document title="Your Account">
            <AuthCard title="Signed In" subtitle={format!("Authenticated with {provider}")}>
                <div mix={profile_header()}>
                    {avatar}
                    <div>
                        <p mix={profile_name()}>{display_name.clone()}</p>
                        <p mix={profile_meta()}>{user.email.clone().unwrap_or_else(|| "No email on file".into())}</p>
                        <p mix={profile_meta()}>"Provider: "{provider}</p>
                    </div>
                </div>
                <div mix={info_panel()}>
                    <p>"This page shows the local user record together with any linked provider account data saved in memory."</p>
                </div>
                <pre mix={data_dump()}>{details}</pre>
                <form method="POST" action="/auth/logout" mix={css("margin-top: 1rem;")}>
                    <button type="submit" mix={submit_button()}>"Logout"</button>
                </form>
            </AuthCard>
        </Document>
    })
}

// ---------------------------------------------------------------------------------------
// Signup (actions/auth/signup/*)
// ---------------------------------------------------------------------------------------

/// The first problem with a field, worded like remix/data-schema's checks.
fn check_min_length(value: Option<&str>, min: usize) -> Option<String> {
    match value {
        None => Some("Expected string".into()),
        Some(v) if v.chars().count() < min => Some(format!("Expected at least {min} characters")),
        Some(_) => None,
    }
}

fn check_email(value: Option<&str>) -> Option<String> {
    let v = value?;
    // /^[^\s@]+@[^\s@]+\.[^\s@]+$/: one @, no whitespace, and a dot inside the domain.
    let valid = !v.chars().any(char::is_whitespace)
        && v.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.contains('@') && domain.char_indices().any(|(i, c)| c == '.' && i > 0 && i + 1 < domain.len())
        });
    (!valid).then(|| "Expected valid email".into())
}

fn signup_page(ctx: &Context, error: Option<&str>, name: Option<String>, email: Option<String>) -> Node {
    rsx! {
        <Document title="Create Account">
            <AuthCard title="Create Account" subtitle="Sign up with email and password"
                footer={footer("Already have an account?", &with_return_to("/", ctx), "Sign in")}>
                {notice(Tone::Error, error)}
                <form method="POST" action={with_return_to("/auth/signup", ctx)} mix={form()}>
                    <TextField id="name" name="name" input_type="text" label="Name" placeholder="Enter your name" auto_complete="name" default_value={name} required={true} icon={user_icon(field_icon())}/>
                    <TextField id="email" name="email" input_type="email" label="Email" placeholder="Enter your email" auto_complete="email" default_value={email} required={true} icon={email_icon(field_icon())}/>
                    <TextField id="password" name="password" input_type="password" label="Password" placeholder="Create a password" auto_complete="new-password" required={true} icon={password_icon(field_icon())}/>
                    <button type="submit" mix={submit_button()}>"Create Account"</button>
                </form>
            </AuthCard>
        </Document>
    }
}

async fn signup(ctx: Context) -> Response {
    let form = ctx.form_data();
    let (name, email, password) = (form.get("name"), form.get("email"), form.get("password"));
    let issue = check_min_length(name, 1).or_else(|| check_min_length(email, 0)).or_else(|| check_email(email)).or_else(|| check_min_length(password, 8));
    let bad_request = ResponseInit::status(StatusCode::BAD_REQUEST);
    if let Some(issue) = issue {
        let page = signup_page(&ctx, Some(&issue), name.and_then(normalize_optional_text), email.and_then(normalize_optional_text));
        return ctx.render_with(page, bad_request);
    }
    let (name, email, password) = (name.unwrap_or_default().trim().to_string(), normalize_email(email.unwrap_or_default()), password.unwrap_or_default());
    if db(&ctx).find_user_by_email(&email).is_some() {
        let page = signup_page(&ctx, Some("An account with that email already exists."), Some(name), Some(email));
        return ctx.render_with(page, bad_request);
    }
    let user = db(&ctx).create_user(&email, hash_password(password), &name);
    ctx.session().regenerate_id(true);
    ctx.session().set("auth", AuthSession { user_id: user.id, login_method: "credentials".into(), auth_account_id: None });
    response::redirect(&post_auth_redirect(&ctx))
}

// ---------------------------------------------------------------------------------------
// Forgot and reset password (actions/auth/forgot-password/*, actions/auth/reset-password/*)
// ---------------------------------------------------------------------------------------

fn forgot_password_page(ctx: &Context, error: Option<&str>, email: Option<String>) -> Node {
    rsx! {
        <Document title="Forgot Password">
            <AuthCard title="Forgot Password" subtitle="Enter your email and we will generate a reset link"
                footer={footer("Remembered it?", &with_return_to("/", ctx), "Back to sign in")}>
                {notice(Tone::Error, error)}
                <form method="POST" action={with_return_to("/auth/forgot-password", ctx)} mix={form()}>
                    <TextField id="email" name="email" input_type="email" label="Email" placeholder="Enter your email" auto_complete="email" default_value={email} required={true} icon={email_icon(field_icon())}/>
                    <button type="submit" mix={submit_button()}>"Send Reset Link"</button>
                </form>
            </AuthCard>
        </Document>
    }
}

async fn forgot_password(ctx: Context) -> Response {
    let email = ctx.form_data().get("email");
    if let Some(issue) = check_min_length(email, 0).or_else(|| check_email(email)) {
        let page = forgot_password_page(&ctx, Some(&issue), email.map(str::to_string));
        return ctx.render_with(page, ResponseInit::status(StatusCode::BAD_REQUEST));
    }
    let email = normalize_email(email.unwrap_or_default());
    let reset_href = db(&ctx).find_user_by_email(&email).map(|user| {
        let token = db(&ctx).create_reset_token(user.id);
        format!("{}/auth/reset-password/{token}", ctx.url().origin())
    });
    let login_href = with_return_to("/", &ctx);
    let link = match &reset_href {
        Some(href) => rsx! { <p><a href={href.clone()} class="reset-link" mix={helper_link()}>{href.clone()}</a></p> },
        None => rsx! { <p>"No matching local account was found."</p> },
    };
    ctx.render(rsx! {
        <Document title="Reset Link Ready">
            <AuthCard title="Check Your Email" subtitle={format!("If {email} matches a local account, the demo generated a reset link.")}>
                {notice(Tone::Success, Some("Password reset instructions are ready."))}
                <div mix={info_panel()}>
                    <p mix={css("margin-bottom: 0.625rem;")}>"This demo does not send email. If a matching account exists, the reset URL is shown below."</p>
                    {link}
                </div>
                <div mix={button_row()}>
                    {reset_href.map(|href| rsx! { <a href={href} mix={submit_button()}>"Open Reset Form"</a> })}
                    <a href={login_href} mix={secondary_button()}>"Back to Sign In"</a>
                </div>
            </AuthCard>
        </Document>
    })
}

fn error_page(ctx: &Context, title: &str, message: &str) -> Response {
    let page = rsx! {
        <Document title={title.to_string()}>
            <AuthCard title={title.to_string()} subtitle="This request could not be completed.">
                {notice(Tone::Error, Some(message))}
                <a href={with_return_to("/", ctx)} mix={secondary_button()}>"Back to Sign In"</a>
            </AuthCard>
        </Document>
    };
    ctx.render_with(page, ResponseInit::status(StatusCode::BAD_REQUEST))
}

fn reset_password_page(ctx: &Context, error: Option<&str>) -> Node {
    let action = with_return_to(&format!("/auth/reset-password/{}", ctx.param("token").unwrap_or_default()), ctx);
    rsx! {
        <Document title="Reset Password">
            <AuthCard title="Reset Password" subtitle="Create a new password for your account"
                footer={footer("Changed your mind?", &with_return_to("/", ctx), "Back to sign in")}>
                {notice(Tone::Error, error)}
                <form method="POST" action={action} mix={form()}>
                    <TextField id="password" name="password" input_type="password" label="New Password" placeholder="Enter a new password" auto_complete="new-password" required={true} icon={password_icon(field_icon())}/>
                    <TextField id="confirmPassword" name="confirmPassword" input_type="password" label="Confirm Password" placeholder="Confirm your password" auto_complete="new-password" required={true} icon={password_icon(field_icon())}/>
                    <button type="submit" mix={submit_button()}>"Update Password"</button>
                </form>
            </AuthCard>
        </Document>
    }
}

const EXPIRED: (&str, &str) = ("Reset Link Expired", "That password reset link is missing or has expired.");

async fn reset_password_form(ctx: Context) -> Response {
    if db(&ctx).load_reset_token(ctx.param("token").unwrap_or_default()).is_none() {
        return error_page(&ctx, EXPIRED.0, EXPIRED.1);
    }
    ctx.render(reset_password_page(&ctx, None))
}

async fn reset_password(ctx: Context) -> Response {
    let Some(token) = db(&ctx).load_reset_token(ctx.param("token").unwrap_or_default()) else {
        return error_page(&ctx, EXPIRED.0, EXPIRED.1);
    };
    let (password, confirm) = (ctx.form_data().get("password"), ctx.form_data().get("confirmPassword"));
    let issue = check_min_length(password, 8).or_else(|| check_min_length(confirm, 8));
    let issue = issue.or_else(|| (password != confirm).then(|| "Passwords must match.".to_string()));
    if let Some(issue) = issue {
        return ctx.render_with(reset_password_page(&ctx, Some(&issue)), ResponseInit::status(StatusCode::BAD_REQUEST));
    }
    let Some(user) = db(&ctx).find_user(token.user_id) else {
        db(&ctx).delete_reset_token(&token.token);
        return error_page(&ctx, "Account Not Found", "The account for that reset link no longer exists.");
    };
    db(&ctx).update_password(user.id, hash_password(password.unwrap_or_default()));
    db(&ctx).delete_reset_token(&token.token);
    ctx.session().flash("success", "Password updated. You can sign in now.");
    let login_href = with_return_to("/", &ctx);
    ctx.render(rsx! {
        <Document title="Password Updated">
            <AuthCard title="Password Updated" subtitle="Your password has been changed successfully.">
                {notice(Tone::Success, Some("You can sign in with your new password now."))}
                <div mix={button_row()}>
                    <a href={login_href} mix={submit_button()}>"Back to Sign In"</a>
                </div>
            </AuthCard>
        </Document>
    })
}

#[cfg(test)]
mod tests;

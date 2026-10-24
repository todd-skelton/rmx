//! `actions/auth`: the account page, login, signup and logout, plus `pages.tsx`.

use http::StatusCode;
use rsmix::{css, rsx, Children, Component, Handle, Mixin, Node};
use rsmix::fetch::Response;
use rsmix::response;
use rsmix::middleware::auth::{complete_auth, credentials_provider, verify_credentials};
use rsmix::middleware::form_data::FormDataExt;
use rsmix::middleware::session::SessionExt;
use rsmix::middleware::render::ResponseInit;
use rsmix::router::routes::FormActions;
use rsmix::router::{action, Context, Router};
use rsmix::data_schema::checks::{max_length, min_length};
use rsmix::data_schema::{self as s, form_data as f, Issue, PathSegment, Schema};

use super::passwords::{hash_password, verify_password};
use super::ui::Layout;
use super::{current_user, db, render, render_with, see_other, token, AuthSession};
use crate::routes::{AuthActions, ROUTES};
use crate::theme::{self, border, font_size, space, surface, text, Tone, DANGER, FOCUS_RING, RADIUS};

pub fn routes(router: &mut Router) {
    let r = &ROUTES.auth;
    router.map(r, AuthActions { index: action(index), logout: action(logout) });
    router.map(&r.login, FormActions { index: action(login_page), action: action(login) });
    router.map(&r.signup, FormActions { index: action(signup_page), action: action(signup) });
}

// --- actions/auth/controller.tsx ---------------------------------------------------------

async fn index(ctx: Context) -> Response {
    match current_user(&ctx) {
        Some(user) => {
            let page = rsx! { <AuthStatusPage csrf_token={token(&ctx)} username={user.username.clone()}/> };
            render(&ctx, page)
        }
        None => response::redirect(&ROUTES.auth.login.index.href(())),
    }
}

async fn logout(ctx: Context) -> Response {
    let session = ctx.session();
    session.unset("auth");
    session.regenerate_id(true);
    see_other(&ROUTES.auth.login.index.href(()))
}

// --- actions/auth/login/controller.tsx ---------------------------------------------------

async fn login_page(ctx: Context) -> Response {
    if current_user(&ctx).is_some() {
        return response::redirect(&ROUTES.home.href(()));
    }
    let error = ctx.session().get::<String>("auth:error");
    render(&ctx, rsx! { <LoginPage csrf_token={token(&ctx)} error={error}/> })
}

/// `credentialsSchema`: a trimmed username and a password, both defaulting to "".
fn credentials(ctx: &Context) -> (String, String) {
    let schema = f::object(|o| {
        let username = o.field("username", s::defaulted(s::string(), String::new()).transform(|v| v.trim().to_string()));
        (username, o.field("password", s::defaulted(s::string(), String::new())))
    });
    s::parse(&schema, ctx.form_data()).unwrap_or_default()
}

async fn login(ctx: Context) -> Response {
    // middleware/auth.ts passwordProvider
    let db = db(&ctx).clone();
    let password = credentials_provider(credentials, move |(username, password): (String, String), _: &Context| {
        let user = db
            .find_user_by_username(&username)
            .filter(|user| db.password_for(user.id).is_some_and(|record| verify_password(&password, &record.password_hash)));
        async move { user }
    })
    .named("password");
    let Some(user) = verify_credentials(&password, &ctx).await else {
        ctx.session().flash("auth:error", "Invalid username or password.");
        return see_other(&ROUTES.auth.login.index.href(()));
    };
    complete_auth(&ctx).set("auth", AuthSession { user_id: user.id });
    see_other(&ROUTES.home.href(()))
}

// --- actions/auth/signup/controller.tsx --------------------------------------------------

async fn signup_page(ctx: Context) -> Response {
    if current_user(&ctx).is_some() {
        return response::redirect(&ROUTES.home.href(()));
    }
    render(&ctx, rsx! { <SignupPage csrf_token={token(&ctx)}/> })
}

/// `signupSchema`
fn parse_signup(ctx: &Context) -> Result<(String, String), AuthFormErrors> {
    let username = s::defaulted(s::string(), String::new())
        .transform(|v| v.trim().to_string())
        .pipe(min_length(3))
        .pipe(max_length(32))
        .refine_with(|v| v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'), "Use only letters, numbers, underscores, and dashes.");
    let password = s::defaulted(s::string(), String::new()).pipe(min_length(8)).pipe(max_length(128));
    let schema = f::object(|o| (o.field("username", &username), o.field("password", &password)));
    s::parse(&schema, ctx.form_data()).map_err(|e| issues_to_errors(&e.issues))
}

/// `issuesToErrors`: the last issue for each field, others for the form.
fn issues_to_errors(issues: &[Issue]) -> AuthFormErrors {
    let mut errors = AuthFormErrors::default();
    for issue in issues {
        let slot = match issue.path.first() {
            Some(PathSegment::Key(k)) if k == "username" => &mut errors.username,
            Some(PathSegment::Key(k)) if k == "password" => &mut errors.password,
            _ => &mut errors.form,
        };
        *slot = Some(issue.message.clone());
    }
    errors
}

async fn signup(ctx: Context) -> Response {
    let (username, password) = match parse_signup(&ctx) {
        Ok(values) => values,
        Err(errors) => {
            let page = rsx! { <SignupPage csrf_token={token(&ctx)} errors={errors}/> };
            return render_with(&ctx, page, ResponseInit::status(StatusCode::BAD_REQUEST));
        }
    };
    let taken = || {
        let errors = AuthFormErrors { username: Some("That username is already taken.".into()), ..Default::default() };
        render_with(&ctx, rsx! { <SignupPage csrf_token={token(&ctx)} errors={errors}/> }, ResponseInit::status(StatusCode::CONFLICT))
    };
    if db(&ctx).find_user_by_username(&username).is_some() {
        return taken();
    }
    let password_hash = hash_password(&password);
    let Some(user) = db(&ctx).create_user(&username, &password_hash) else { return taken() };
    complete_auth(&ctx).set("auth", AuthSession { user_id: user.id });
    see_other(&ROUTES.home.href(()))
}

// --- actions/auth/pages.tsx --------------------------------------------------------------

/// `AuthFormErrors`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthFormErrors {
    pub form: Option<String>,
    pub password: Option<String>,
    pub username: Option<String>,
}

fn page_style() -> Mixin {
    css(&format!("display: grid; min-height: calc(100vh - 80px); place-items: center; padding: {};", space::XL))
}

fn card_style() -> Mixin {
    css(&format!(
        "width: min(100%, 420px); border: 1px solid {}; border-radius: 16px; background-color: {}; box-shadow: 0 16px 34px rgb(0 0 0 / 0.10); padding: {};",
        border::SUBTLE,
        surface::LVL1,
        space::XL
    ))
}

fn eyebrow_style() -> Mixin {
    css(&format!(
        "color: {}; font-size: {}; font-weight: 600; letter-spacing: {}; margin: 0 0 0.5rem; text-transform: uppercase;",
        text::MUTED,
        font_size::XS,
        theme::LETTER_SPACING_WIDE
    ))
}

fn title_style() -> Mixin {
    css(&format!(
        "color: {}; font-size: {}; letter-spacing: {}; line-height: {}; margin: 0;",
        text::PRIMARY,
        font_size::XXL,
        theme::LETTER_SPACING_TIGHT,
        theme::LINE_HEIGHT_TIGHT
    ))
}

fn body_style() -> Mixin {
    css(&format!("color: {}; line-height: {}; margin: {} 0 {};", text::SECONDARY, theme::LINE_HEIGHT_RELAXED, space::SM, space::LG))
}

fn footer_text_style() -> Mixin {
    css(&format!("color: {}; margin: {} 0 0;", text::SECONDARY, space::LG))
}

#[derive(Clone, Default)]
pub struct AuthStatusPageProps {
    pub csrf_token: String,
    pub username: String,
}

pub struct AuthStatusPage;

impl Component for AuthStatusPage {
    type Props = AuthStatusPageProps;

    fn setup(_: &AuthStatusPageProps, _: &Handle<Self>) -> Self {
        AuthStatusPage
    }

    fn render(&self, props: &AuthStatusPageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout title="Account">
                <section mix={page_style()}>
                    <div mix={card_style()}>
                        <p mix={eyebrow_style()}>"Signed in"</p>
                        <h1 mix={title_style()}>{format!("Welcome back, {}.", props.username)}</h1>
                        <p mix={body_style()}>"Your session is active and backed by the local user record."</p>
                        <form action={ROUTES.auth.logout.href(())} method="post">
                            <input type="hidden" name="_csrf" value={props.csrf_token.clone()}/>
                            <button type="submit" mix={theme::button(Tone::Neutral)}>"Sign out"</button>
                        </form>
                    </div>
                </section>
            </Layout>
        }
    }
}

#[derive(Clone, Default)]
pub struct LoginPageProps {
    pub csrf_token: String,
    pub error: Option<String>,
    pub errors: Option<AuthFormErrors>,
}

pub struct LoginPage;

impl Component for LoginPage {
    type Props = LoginPageProps;

    fn setup(_: &LoginPageProps, _: &Handle<Self>) -> Self {
        LoginPage
    }

    fn render(&self, props: &LoginPageProps, _: &Handle<Self>) -> Node {
        let errors = props.errors.clone().unwrap_or_default();
        let error = props.error.clone().or_else(|| errors.form.clone());
        rsx! {
            <Layout title="Sign in">
                <AuthShell eyebrow="Welcome back" title="Sign in to Timeboxer" description="Use your username and password to continue.">
                    <AuthForm action={ROUTES.auth.login.action.href(())} csrf_token={props.csrf_token.clone()} submit_label="Sign in" error={error} errors={errors}/>
                    <p mix={footer_text_style()}>
                        "Need an account? " <a href={ROUTES.auth.signup.index.href(())}>"Create one"</a> "."
                    </p>
                </AuthShell>
            </Layout>
        }
    }
}

#[derive(Clone, Default)]
pub struct SignupPageProps {
    pub csrf_token: String,
    pub errors: Option<AuthFormErrors>,
}

pub struct SignupPage;

impl Component for SignupPage {
    type Props = SignupPageProps;

    fn setup(_: &SignupPageProps, _: &Handle<Self>) -> Self {
        SignupPage
    }

    fn render(&self, props: &SignupPageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout title="Create account">
                <AuthShell eyebrow="Get started" title="Create your account" description="Pick a username and password. Passwords are stored separately from user profiles.">
                    <AuthForm action={ROUTES.auth.signup.action.href(())} csrf_token={props.csrf_token.clone()} submit_label="Create account" errors={props.errors.clone().unwrap_or_default()}/>
                    <p mix={footer_text_style()}>
                        "Already have an account? " <a href={ROUTES.auth.login.index.href(())}>"Sign in"</a> "."
                    </p>
                </AuthShell>
            </Layout>
        }
    }
}

#[derive(Clone, Default)]
struct AuthShellProps {
    children: Children,
    description: String,
    eyebrow: String,
    title: String,
}

struct AuthShell;

impl Component for AuthShell {
    type Props = AuthShellProps;

    fn setup(_: &AuthShellProps, _: &Handle<Self>) -> Self {
        AuthShell
    }

    fn render(&self, props: &AuthShellProps, _: &Handle<Self>) -> Node {
        rsx! {
            <section mix={page_style()}>
                <div mix={card_style()}>
                    <p mix={eyebrow_style()}>{props.eyebrow.clone()}</p>
                    <h1 mix={title_style()}>{props.title.clone()}</h1>
                    <p mix={body_style()}>{props.description.clone()}</p>
                    {&props.children}
                </div>
            </section>
        }
    }
}

#[derive(Clone, Default)]
struct AuthFormProps {
    action: String,
    csrf_token: String,
    error: Option<String>,
    errors: AuthFormErrors,
    submit_label: String,
}

struct AuthForm;

impl Component for AuthForm {
    type Props = AuthFormProps;

    fn setup(_: &AuthFormProps, _: &Handle<Self>) -> Self {
        AuthForm
    }

    fn render(&self, props: &AuthFormProps, _: &Handle<Self>) -> Node {
        let input = css(&format!(
            "border: 1px solid {}; border-radius: {RADIUS}; box-sizing: border-box; color: {}; font: inherit; min-height: 32px; padding: {} {}; width: 100%; &:focus {{ border-color: {FOCUS_RING}; outline: 2px solid {FOCUS_RING}; outline-offset: 2px; }}",
            border::DEFAULT,
            text::PRIMARY,
            space::SM,
            space::MD
        ));
        let field = css(&format!("color: {}; display: grid; font-size: {}; gap: {};", text::SECONDARY, font_size::SM, space::XS));
        let field_error = css(&format!("color: {DANGER};"));
        let alert = props.error.clone().map(|error| {
            let style = css(&format!(
                "background-color: {DANGER}; border: 1px solid {DANGER}; border-radius: {RADIUS}; color: rgb(255 255 255 / 0.92); margin: 0; padding: {};",
                space::MD
            ));
            rsx! { <p role="alert" mix={style}>{error}</p> }
        });
        let username_error = props.errors.username.clone();
        let password_error = props.errors.password.clone();
        rsx! {
            <form action={props.action.clone()} method="post" mix={css(&format!("display: grid; gap: {};", space::MD))}>
                <input type="hidden" name="_csrf" value={props.csrf_token.clone()}/>
                {alert}
                <label mix={field.clone()}>
                    <span>"Username"</span>
                    <input
                        aria-describedby={username_error.as_ref().map(|_| "username-error")}
                        aria-invalid={username_error.as_ref().map(|_| true)}
                        autocomplete="username" maxlength="32" minlength="3" name="username" required type="text" mix={input.clone()}/>
                    {username_error.map(|e| rsx! { <small id="username-error" mix={field_error.clone()}>{e}</small> })}
                </label>
                <label mix={field}>
                    <span>"Password"</span>
                    <input
                        aria-describedby={password_error.as_ref().map(|_| "password-error")}
                        aria-invalid={password_error.as_ref().map(|_| true)}
                        autocomplete="current-password" maxlength="128" minlength="8" name="password" required type="password" mix={input}/>
                    {password_error.map(|e| rsx! { <small id="password-error" mix={field_error}>{e}</small> })}
                </label>
                <button type="submit" mix={theme::button(Tone::Primary)}>{props.submit_label.clone()}</button>
            </form>
        }
    }
}

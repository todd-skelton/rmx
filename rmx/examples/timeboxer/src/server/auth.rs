//! `actions/auth`: the account page, login, signup and logout, plus `pages.tsx`.

use http::StatusCode;
use rmx::{css, rsx, Children, Component, Handle, Mixin, Node};
use rmx::fetch::{response, Response};
use rmx::middleware::auth::{complete_auth, credentials_provider, verify_credentials};
use rmx::middleware::form_data::FormDataExt;
use rmx::middleware::session::SessionExt;
use rmx::render::ResponseInit;
use rmx::router::{Context, Router};

use super::passwords::{hash_password, verify_password};
use super::ui::Layout;
use super::{current_user, db, render, render_with, see_other, token, AuthSession};
use crate::routes::ROUTES;
use crate::theme::{self, border, font_size, space, surface, text, Tone, DANGER, FOCUS_RING, RADIUS};

pub fn routes(router: &mut Router) {
    let r = &ROUTES.auth;
    router.route(&r.index, index);
    router.route(&r.logout, logout);
    router.route(&r.login.index, login_page);
    router.route(&r.login.action, login);
    router.route(&r.signup.index, signup_page);
    router.route(&r.signup.action, signup);
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
    let form = ctx.form_data();
    (form.get("username").unwrap_or_default().trim().to_string(), form.get("password").unwrap_or_default().to_string())
}

async fn login(ctx: Context) -> Response {
    // middleware/auth.ts passwordProvider
    let db = db(&ctx).clone();
    let password = credentials_provider(credentials, move |(username, password): (String, String)| {
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

/// `signupSchema`, with `remix/data-schema`'s messages.
fn parse_signup(ctx: &Context) -> Result<(String, String), AuthFormErrors> {
    let form = ctx.form_data();
    let username = form.get("username").unwrap_or_default().trim().to_string();
    let password = form.get("password").unwrap_or_default().to_string();
    let errors = AuthFormErrors {
        form: None,
        username: length_issue(&username, 3, 32).or_else(|| {
            (!username.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
                .then(|| "Use only letters, numbers, underscores, and dashes.".to_string())
        }),
        password: length_issue(&password, 8, 128),
    };
    if errors.username.is_some() || errors.password.is_some() {
        return Err(errors);
    }
    Ok((username, password))
}

/// `minLength(min)` and `maxLength(max)`, counting UTF-16 code units as JavaScript does.
pub fn length_issue(value: &str, min: usize, max: usize) -> Option<String> {
    let length = value.encode_utf16().count();
    if length < min {
        Some(format!("Expected at least {min} characters"))
    } else if length > max {
        Some(format!("Expected at most {max} characters"))
    } else {
        None
    }
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

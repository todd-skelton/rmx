//! `actions/auth`: login, register, logout, forgot and reset password.

use rmx::{css, rsx, Mixin, Node};
use rmx::fetch::{response, Response};
use rmx::middleware::auth::{complete_auth, credentials_provider, verify_credentials};
use rmx::middleware::session::SessionExt;
use rmx::render::ResponseInit;
use rmx::router::routes::FormActions;
use rmx::router::{action, Context, Router};
use rmx::data_schema::checks::min_length;
use rmx::data_schema::{self as s, form_data as f, Schema};

use super::data::{hash_password, normalize_email, now_ms, verify_password, PasswordResetToken, UserInput};
use super::ui::Document;
use super::{bad_request, db, login_redirect_url, post_auth_redirect, render, render_with, AuthSession};
use crate::routes::{AuthActions, ROUTES};

pub fn routes(router: &mut Router) {
    let r = &ROUTES.auth;
    router.map(
        r,
        AuthActions {
            logout: action(|ctx: Context| async move {
                ctx.session().unset("auth");
                ctx.session().regenerate_id(true);
                response::redirect(&ROUTES.home.href(()))
            }),
        },
    );
    router.map(&r.login, FormActions { index: action(login_page), action: action(login) });
    router.map(
        &r.register,
        FormActions { index: action(|ctx: Context| async move { render(&ctx, register_page()) }), action: action(register) },
    );
    router.map(
        &r.forgot_password,
        FormActions {
            index: action(|ctx: Context| async move { render(&ctx, forgot_password_page()) }),
            action: action(forgot_password),
        },
    );
    router.map(&r.reset_password, FormActions { index: action(reset_password_page), action: action(reset_password) });
}

/// `authCardStyle`
fn auth_card() -> Mixin {
    css("max-width: 500px; margin: 2rem auto;")
}

fn error_alert(error: Option<String>) -> Option<Node> {
    error.map(|e| rsx! { <div class="alert alert-error" mix={css("margin-bottom: 1.5rem;")}>{e}</div> })
}

// --- schemas.ts -------------------------------------------------------------------------

/// `s.string().pipe(minLength(8))`
fn password_field() -> impl Schema<Output = String> {
    s::string().pipe(min_length(8))
}

struct Registration {
    name: String,
    email: String,
    password: String,
}

/// `registrationSchema`
fn registration_schema() -> impl Schema<Output = Registration> {
    f::object(|o| Registration {
        name: o.field("name", text_field()),
        email: o.field("email", text_field()),
        password: o.field("password", password_field()),
    })
}

/// `resetPasswordSchema`: `(password, confirmPassword)`
fn reset_password_schema() -> impl Schema<Output = (String, String)> {
    f::object(|o| (o.field("password", password_field()), o.field("confirmPassword", password_field())))
}

use super::schemas::{parse_form, text as field, text_field};

// --- login ------------------------------------------------------------------------------

async fn login_page(ctx: Context) -> Response {
    let error = ctx.session().get::<String>("error");
    let form_action = login_redirect_url(&ctx, &ROUTES.auth.login.action);
    let page = rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <h1>"Login"</h1>
                {error_alert(error)}
                <form method="POST" action={form_action}>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" required autocomplete="email"/>
                    </div>
                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input type="password" id="password" name="password" required autocomplete="current-password"/>
                    </div>
                    <button type="submit" class="btn">"Login"</button>
                </form>
                <p mix={css("margin-top: 1.5rem;")}>
                    "Don't have an account? " <a href={ROUTES.auth.register.index.href(())}>"Register here"</a>
                </p>
                <p><a href={ROUTES.auth.forgot_password.index.href(())}>"Forgot password?"</a></p>
                <div mix={css("margin-top: 2rem; padding: 1rem; background: #f8f9fa; border-radius: 4px;")}>
                    <p mix={css("font-size: 0.9rem;")}><strong>"Demo Accounts:"</strong></p>
                    <p mix={css("font-size: 0.9rem;")}>"Admin: admin@bookstore.com / admin123"</p>
                    <p mix={css("font-size: 0.9rem;")}>"Customer: customer@example.com / password123"</p>
                </div>
            </div>
        </Document>
    };
    render(&ctx, page)
}

async fn login(ctx: Context) -> Response {
    // middleware/auth.ts passwordProvider
    let db = db(&ctx).clone();
    let password = credentials_provider(
        |ctx: &Context| (normalize_email(&field(ctx, "email")), field(ctx, "password")),
        move |(email, password): (String, String)| {
            let user = db.find_user_by_email(&email).filter(|u| verify_password(&password, &u.password_hash));
            async move { user }
        },
    );
    let Some(user) = verify_credentials(&password, &ctx).await else {
        ctx.session().flash("error", "Invalid email or password. Please try again.");
        return response::redirect(&login_redirect_url(&ctx, &ROUTES.auth.login.index));
    };
    complete_auth(&ctx).set("auth", AuthSession { user_id: user.id });
    response::redirect(&post_auth_redirect(&ctx))
}

// --- register ---------------------------------------------------------------------------

fn register_page() -> Node {
    rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <h1>"Register"</h1>
                <form method="POST" action={ROUTES.auth.register.action.href(())}>
                    <div class="form-group">
                        <label for="name">"Name"</label>
                        <input type="text" id="name" name="name" required autocomplete="name"/>
                    </div>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" required autocomplete="email"/>
                    </div>
                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input type="password" id="password" name="password" required autocomplete="new-password"/>
                    </div>
                    <button type="submit" class="btn">"Register"</button>
                </form>
                <p mix={css("margin-top: 1.5rem;")}>
                    "Already have an account? " <a href={ROUTES.auth.login.index.href(())}>"Login here"</a>
                </p>
            </div>
        </Document>
    }
}

async fn register(ctx: Context) -> Response {
    let Registration { name, email, password } = match parse_form(&ctx, registration_schema()) {
        Ok(input) => input,
        Err(issues) => return bad_request(issues),
    };
    let email = normalize_email(&email);
    if db(&ctx).find_user_by_email(&email).is_some() {
        let page = rsx! {
            <Document>
                <div class="card" mix={auth_card()}>
                    <div class="alert alert-error">"An account with this email already exists."</div>
                    <p>
                        <a href={ROUTES.auth.register.index.href(())} class="btn">"Back to Register"</a>
                        <a href={ROUTES.auth.login.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Login"</a>
                    </p>
                </div>
            </Document>
        };
        return render_with(&ctx, page, ResponseInit::status(http::StatusCode::BAD_REQUEST));
    }
    let created = db(&ctx).create_user(UserInput {
        email: Some(email),
        password_hash: Some(hash_password(&password)),
        name: Some(name),
        role: None,
    });
    let user = match created {
        Ok(user) => user,
        Err(e) => return bad_request(e.to_string()),
    };
    ctx.session().regenerate_id(true);
    ctx.session().set("auth", AuthSession { user_id: user.id });
    response::redirect(&ROUTES.account.index.href(()))
}

// --- forgot and reset password ----------------------------------------------------------

fn forgot_password_page() -> Node {
    rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <h1>"Forgot Password"</h1>
                <p>"Enter your email address and we'll send you a link to reset your password."</p>
                <form method="POST" action={ROUTES.auth.forgot_password.action.href(())}>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" required autocomplete="email"/>
                    </div>
                    <button type="submit" class="btn">"Send Reset Link"</button>
                </form>
                <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.auth.login.index.href(())}>"Back to Login"</a></p>
            </div>
        </Document>
    }
}

async fn forgot_password(ctx: Context) -> Response {
    // `forgotPasswordSchema`
    let token = db(&ctx).find_user_by_email(&field(&ctx, "email")).map(|user| {
        let token = rmx::crypto::random_hex(13);
        db(&ctx).create_reset_token(PasswordResetToken { token: token.clone(), user_id: user.id, expires_at: now_ms() + 3_600_000 });
        token
    });
    let demo_link = token.map(|token| {
        rsx! {
            <div mix={css("margin-top: 1rem; padding: 1rem; background: #f8f9fa; border-radius: 4px;")}>
                <p mix={css("font-size: 0.9rem;")}><strong>"Demo Mode:"</strong>" Click the link below to reset your password"</p>
                <p mix={css("margin-top: 0.5rem;")}>
                    <a href={ROUTES.auth.reset_password.index.href([("token", token.as_str())])} class="btn btn-secondary">"Reset Password"</a>
                </p>
            </div>
        }
    });
    let page = rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <div class="alert alert-success">"Password reset link sent! Check your email."</div>
                {demo_link}
                <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.auth.login.index.href(())} class="btn">"Back to Login"</a></p>
            </div>
        </Document>
    };
    render(&ctx, page)
}

async fn reset_password_page(ctx: Context) -> Response {
    let token = ctx.param("token").unwrap_or_default().to_string();
    let error = ctx.session().get::<String>("error");
    let page = rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <h1>"Reset Password"</h1>
                <p>"Enter your new password below."</p>
                {error_alert(error)}
                <form method="POST" action={ROUTES.auth.reset_password.action.href([("token", token.as_str())])}>
                    <div class="form-group">
                        <label for="password">"New Password"</label>
                        <input type="password" id="password" name="password" required autocomplete="new-password"/>
                    </div>
                    <div class="form-group">
                        <label for="confirmPassword">"Confirm Password"</label>
                        <input type="password" id="confirmPassword" name="confirmPassword" required autocomplete="new-password"/>
                    </div>
                    <button type="submit" class="btn">"Reset Password"</button>
                </form>
            </div>
        </Document>
    };
    render(&ctx, page)
}

async fn reset_password(ctx: Context) -> Response {
    let (password, confirm) = match parse_form(&ctx, reset_password_schema()) {
        Ok(input) => input,
        Err(issues) => return bad_request(issues),
    };
    let token = ctx.param("token").unwrap_or_default().to_string();
    let back = |message: &str| {
        ctx.session().flash("error", message);
        response::redirect(&ROUTES.auth.reset_password.index.href([("token", token.as_str())]))
    };
    if password != confirm {
        return back("Passwords do not match.");
    }
    let user = db(&ctx).find_reset_token(&token).filter(|t| t.expires_at >= now_ms()).and_then(|t| db(&ctx).find_user(t.user_id));
    let Some(user) = user else { return back("Invalid or expired reset token.") };
    let update = UserInput { password_hash: Some(hash_password(&password)), ..Default::default() };
    if let Err(e) = db(&ctx).update_user(user.id, update) {
        return bad_request(e.to_string());
    }
    db(&ctx).delete_reset_token(&token);
    let page = rsx! {
        <Document>
            <div class="card" mix={auth_card()}>
                <div class="alert alert-success">"Password reset successfully! You can now login with your new password."</div>
                <p><a href={ROUTES.auth.login.index.href(())} class="btn">"Login"</a></p>
            </div>
        </Document>
    };
    render(&ctx, page)
}

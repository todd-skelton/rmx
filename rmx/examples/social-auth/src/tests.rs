//! app/router.test.ts, with memory session storage like test/helpers.ts.

use rmx_cookie::{Cookie, SameSite};
use rmx_fetch::{block_on, Body, Response};
use rmx_router::Router;
use rmx_session::MemoryStorage;

use super::*;

const ORIGIN: &str = "https://social-auth.test";

struct TestApp {
    router: Router,
    db: Db,
}

fn create_test_router() -> TestApp {
    let cookie = Cookie::new("social-auth-session")
        .secrets(vec!["test-social-auth-secret".into()])
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(60 * 60 * 24 * 30)
        .path("/");
    let db = Db::seeded();
    TestApp { router: router(cookie, MemoryStorage::new(), db.clone(), "public"), db }
}

fn session_cookie(res: &Response) -> Option<String> {
    res.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("social-auth-session="))
        .map(|v| v.split(';').next().unwrap().to_string())
}

impl TestApp {
    fn get(&self, path: &str, cookie: Option<&str>) -> Response {
        let mut req = http::Request::get(format!("{ORIGIN}{path}"));
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        block_on(self.router.fetch(req.body(Body::empty()).unwrap()))
    }

    fn post(&self, path: &str, form: &[(&str, &str)], cookie: Option<&str>) -> Response {
        let body = form.iter().copied().collect::<rmx_url::SearchParams>().to_string();
        let mut req = http::Request::post(format!("{ORIGIN}{path}")).header("content-type", "application/x-www-form-urlencoded");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        block_on(self.router.fetch(req.body(Body::from(body)).unwrap()))
    }
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn location(res: &Response) -> &str {
    res.headers()["location"].to_str().unwrap()
}

fn assert_contains(html: &str, needle: &str) {
    assert!(html.contains(needle), "expected HTML to contain {needle:?}");
}

#[test]
fn seed_hashes_from_web_crypto_verify() {
    let db = Db::seeded();
    let hash = db.find_user(2).unwrap().password_hash.unwrap();
    assert!(verify_password("password123", &hash));
    assert!(!verify_password("password124", &hash));
    let ours = hash_password("s3cret-pass");
    assert!(ours.starts_with("pbkdf2_sha256$100000$"));
    assert!(verify_password("s3cret-pass", &ours));
    assert!(!verify_password("x", "bcrypt$1$aa$bb"));
}

#[test]
fn renders_the_login_page_at_the_home_route() {
    let app = create_test_router();
    let res = app.get("/", None);
    assert_eq!(res.status(), 200);
    let html = text(res);
    assert_contains(&html, "Welcome Back");
    assert_contains(&html, "Sign in to your account");
}

#[test]
fn renders_disabled_social_buttons_when_provider_env_vars_are_missing() {
    let html = text(create_test_router().get("/", None));
    assert_contains(&html, "GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET");
    assert_contains(&html, "GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET");
    assert_contains(&html, "X_CLIENT_ID and X_CLIENT_SECRET");
}

#[test]
fn logs_in_with_credentials_and_shows_the_protected_account_page() {
    let app = create_test_router();
    let login = app.post("/auth/login", &[("email", "user@example.com"), ("password", "password123")], None);
    assert_eq!(login.status(), 302);
    assert_eq!(location(&login), "/account");
    let cookie = session_cookie(&login).expect("session cookie");

    let account = app.get("/account", Some(&cookie));
    assert_eq!(account.status(), 200);
    let html = text(account);
    assert_contains(&html, "Signed In");
    assert_contains(&html, "Demo User");
    assert_contains(&html, "Credentials");

    let home = app.get("/", Some(&cookie));
    assert_eq!((home.status().as_u16(), location(&home)), (302, "/account"), "signed-in visitors skip the login page");
}

#[test]
fn shows_an_error_after_invalid_credentials() {
    let app = create_test_router();
    let login = app.post("/auth/login", &[("email", "user@example.com"), ("password", "wrong-password")], None);
    assert_eq!(login.status(), 302);
    assert_eq!(location(&login), "/");
    let cookie = session_cookie(&login).expect("session cookie");

    let html = text(app.get("/", Some(&cookie)));
    assert_contains(&html, "Invalid email or password. Please try again.");
    let again = text(app.get("/", Some(&cookie)));
    assert!(!again.contains("Invalid email or password"), "flash messages show once");
}

#[test]
fn creates_a_new_user_during_signup_and_signs_them_in() {
    let app = create_test_router();
    let res = app.post("/auth/signup", &[("name", "New Demo User"), ("email", "new-user@example.com"), ("password", "password123")], None);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/account");
    let cookie = session_cookie(&res).expect("session cookie");
    let html = text(app.get("/account", Some(&cookie)));
    assert_contains(&html, "New Demo User");
    assert_contains(&html, "new-user@example.com");
}

#[test]
fn signup_validation_keeps_values() {
    let app = create_test_router();
    let res = app.post("/auth/signup", &[("name", "Ada"), ("email", "not-an-email"), ("password", "password123")], None);
    assert_eq!(res.status(), 400);
    let html = text(res);
    assert_contains(&html, "Expected valid email");
    assert_contains(&html, r#"value="Ada""#);

    let res = app.post("/auth/signup", &[("name", "Ada"), ("email", "ada@example.com"), ("password", "short")], None);
    assert_contains(&text(res), "Expected at least 8 characters");

    let res = app.post("/auth/signup", &[("name", "Again"), ("email", "USER@example.com"), ("password", "password123")], None);
    assert_eq!(res.status(), 400);
    assert_contains(&text(res), "An account with that email already exists.");
}

#[test]
fn creates_a_reset_token_and_allows_resetting_the_password() {
    let app = create_test_router();
    let forgot = app.post("/auth/forgot-password", &[("email", "user@example.com")], None);
    assert_eq!(forgot.status(), 200);
    let html = text(forgot);
    assert_contains(&html, "Password reset instructions are ready.");

    let token = app.db.reset_token_for_user(2).expect("token");
    assert_contains(&html, &format!("{ORIGIN}/auth/reset-password/{}", token.token));
    assert_eq!(app.get(&format!("/auth/reset-password/{}", token.token), None).status(), 200);

    let mismatch = app.post(&format!("/auth/reset-password/{}", token.token), &[("password", "newpassword123"), ("confirmPassword", "other-password")], None);
    assert_eq!(mismatch.status(), 400);
    assert_contains(&text(mismatch), "Passwords must match.");

    let reset = app.post(&format!("/auth/reset-password/{}", token.token), &[("password", "newpassword123"), ("confirmPassword", "newpassword123")], None);
    assert_eq!(reset.status(), 200);
    let cookie = session_cookie(&reset).expect("flash cookie");
    assert_contains(&text(reset), "Password Updated");
    assert_contains(&text(app.get("/", Some(&cookie))), "Password updated. You can sign in now.");

    let used = app.get(&format!("/auth/reset-password/{}", token.token), None);
    assert_eq!(used.status(), 400);
    assert_contains(&text(used), "Reset Link Expired");

    let login = app.post("/auth/login", &[("email", "user@example.com"), ("password", "newpassword123")], None);
    assert_eq!(login.status(), 302);
    assert_eq!(location(&login), "/account");
}

#[test]
fn logs_out_and_redirects_subsequent_protected_requests_back_to_home() {
    let app = create_test_router();
    let login = app.post("/auth/login", &[("email", "user@example.com"), ("password", "password123")], None);
    let cookie = session_cookie(&login).expect("session cookie");

    let logout = app.post("/auth/logout", &[], Some(&cookie));
    assert_eq!(logout.status(), 302);
    assert_eq!(location(&logout), "/");

    let account = app.get("/account", Some(&cookie));
    assert_eq!(account.status(), 302);
    assert_eq!(location(&account), "/");
}

#[test]
fn return_to_is_kept_and_must_be_local() {
    let app = create_test_router();
    let html = text(app.get("/?returnTo=/account", None));
    assert_contains(&html, r#"action="/auth/login?returnTo=%2Faccount""#);
    let login = app.post("/auth/login?returnTo=/account%3Ftab%3D1", &[("email", "admin@example.com"), ("password", "password123")], None);
    assert_eq!(location(&login), "/account?tab=1");
    let login = app.post("/auth/login?returnTo=//evil.test", &[("email", "admin@example.com"), ("password", "password123")], None);
    assert_eq!(location(&login), "/account");
}

#[test]
fn unconfigured_providers_flash_an_error() {
    let app = create_test_router();
    let res = app.get("/auth/github/login", None);
    assert_eq!((res.status().as_u16(), location(&res)), (302, "/"));
    let cookie = session_cookie(&res).expect("flash cookie");
    assert_contains(&text(app.get("/", Some(&cookie))), "GitHub login is not configured.");
}

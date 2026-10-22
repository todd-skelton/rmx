//! Remix's `session.test.ts` and `session-expiration.test.ts`. `Date.now` mocks become
//! [`MOCK_NOW`](super::MOCK_NOW); thrown errors become panics.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::header::SET_COOKIE;
use rmx_cookie::Cookie;
use rmx_fetch::{Body, Response};
use rmx_headers::SetCookie;
use rmx_router::{Context, Router};
use rmx_session::{CookieStorage, Error, FsStorage, MemoryStorage, Session, SessionStorage};

use super::{session, SessionExt, MOCK_NOW};

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

fn set_now(ms: i64) {
    MOCK_NOW.with(|now| now.set(Some(ms)));
}

fn at_ms(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms as u64)
}

/// A response with its body read.
struct Res {
    body: String,
    set_cookies: Vec<String>,
}

impl Res {
    fn new(res: Response) -> Res {
        let set_cookies = res.headers().get_all(SET_COOKIE).iter().map(|v| v.to_str().unwrap().to_string()).collect();
        Res { body: run(res.into_body().text()).unwrap(), set_cookies }
    }

    fn set_cookie(&self) -> SetCookie {
        SetCookie::parse(&self.set_cookies[0]).unwrap()
    }
}

fn fetch(router: &Router, url: &str, from: Option<&Res>) -> Res {
    let mut req = http::Request::builder().uri(url);
    if let Some(from) = from.filter(|r| !r.set_cookies.is_empty()) {
        let cookie = from.set_cookie();
        req = req.header("cookie", format!("{}={}", cookie.name, cookie.value));
    }
    Res::new(run(router.fetch(req.body(Body::empty()).unwrap())))
}

fn sess_cookie() -> Cookie {
    Cookie::new("__sess").secrets(vec!["secret1".into()])
}

fn set_user_router(cookie: Cookie) -> Router {
    let mut router = Router::new().with(session(cookie, CookieStorage::new()));
    router.get("/", |ctx: Context| async move {
        ctx.session().set("userId", "123");
        "ok"
    });
    router
}

// --- session.test.ts ---------------------------------------------------------------------

#[test]
fn defaults_secure_to_the_request_protocol() {
    let router = set_user_router(sess_cookie());
    assert!(fetch(&router, "https://remix.run/", None).set_cookie().secure);
    assert!(!fetch(&router, "http://localhost/", None).set_cookie().secure);
}

#[test]
fn preserves_explicitly_disabling_secure_on_https() {
    let router = set_user_router(sess_cookie().secure(false));
    assert!(!fetch(&router, "https://remix.run/", None).set_cookie().secure);
}

#[test]
fn preserves_explicitly_enabling_secure_on_http() {
    let router = set_user_router(sess_cookie().secure(true));
    assert!(fetch(&router, "http://localhost/", None).set_cookie().secure);
}

#[test]
fn defaults_session_cookies_to_http_only() {
    let router = set_user_router(sess_cookie());
    assert!(fetch(&router, "https://remix.run/", None).set_cookie().http_only);
}

#[test]
fn respects_and_warns_about_explicitly_disabling_http_only() {
    let cookie = sess_cookie().http_only(false);
    assert_eq!(
        super::http_only_warning(&cookie).as_deref(),
        Some("Session cookie \"__sess\" is configured with httpOnly: false and may be accessible to client-side JavaScript.")
    );
    assert_eq!(super::http_only_warning(&sess_cookie()), None);
    assert_eq!(super::http_only_warning(&sess_cookie().http_only(true)), None);
    let router = set_user_router(cookie);
    assert!(!fetch(&router, "https://remix.run/", None).set_cookie().http_only);
}

#[test]
fn persists_session_data_across_requests() {
    let mut router = Router::new().with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |ctx: Context| async move {
        let count = ctx.session().get::<i64>("count").unwrap_or(0) + 1;
        ctx.session().set("count", count);
        format!("Count: {count}")
    });
    let res1 = fetch(&router, "https://remix.run/", None);
    assert_eq!(res1.body, "Count: 1");
    let res2 = fetch(&router, "https://remix.run/", Some(&res1));
    assert_eq!(res2.body, "Count: 2");
    let res3 = fetch(&router, "https://remix.run/", Some(&res2));
    assert_eq!(res3.body, "Count: 3");
}

#[test]
fn allows_for_direct_return_of_a_fetched_response() {
    // Remix returns `fetch(...)`, whose headers are immutable; rmx responses are always owned.
    let mut router = Router::new().with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |ctx: Context| async move {
        ctx.session().set("count", 1);
        Response::new(Body::from("ok"))
    });
    assert_eq!(fetch(&router, "https://remix.run/", None).set_cookies.len(), 1);
}

#[test]
#[should_panic(expected = "Session cookie must be signed")]
fn throws_if_the_session_cookie_is_not_signed() {
    let _ = session(Cookie::new("__sess").secrets(vec![]), CookieStorage::new());
}

#[test]
#[should_panic(expected = "Existing session found, refusing to overwrite")]
fn throws_at_request_time_if_the_session_is_already_started() {
    let mut router = Router::new()
        .with(session(sess_cookie(), CookieStorage::new()))
        .with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |_ctx: Context| async { "Home" });
    fetch(&router, "https://remix.run/", None);
}

#[test]
#[should_panic(expected = "Cannot save session that was initialized by another middleware/handler")]
fn throws_at_request_time_if_the_session_is_modified_by_another_handler() {
    let mut router = Router::new().with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |mut ctx: Context| async move {
        ctx.set(Session::new());
        "Home"
    });
    fetch(&router, "https://remix.run/", None);
}

#[test]
#[should_panic(expected = "Cannot save session that was initialized by another middleware/handler")]
fn throws_if_the_session_is_replaced_in_place() {
    let mut router = Router::new().with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |mut ctx: Context| async move {
        if let Some(s) = ctx.get_mut::<Session>() {
            *s = Session::new();
        }
        "Home"
    });
    fetch(&router, "https://remix.run/", None);
}

#[test]
fn does_not_overwrite_cookies_set_by_other_handlers() {
    let mut router = Router::new().with(session(sess_cookie(), CookieStorage::new()));
    router.get("/", |ctx: Context| async move {
        let count = ctx.session().get::<i64>("count").unwrap_or(0) + 1;
        ctx.session().set("count", count);
        let mut res = Response::new(Body::from(format!("Count: {count}")));
        res.headers_mut().insert(SET_COOKIE, format!("count={count}").parse().unwrap());
        res
    });
    let res = fetch(&router, "https://remix.run/", None);
    assert_eq!(res.set_cookies.len(), 2);
    assert_eq!(res.set_cookies[0], "count=1");
}

// --- session-expiration.test.ts ----------------------------------------------------------

/// Remix's `createRequests`: a router whose `/start` signs in, `/update` writes, `/destroy`
/// ends the session and `/` prints the user id.
fn create_requests(cookie: Cookie, storage: impl SessionStorage) -> impl Fn(&str, Option<&Res>) -> Res {
    let mut router = Router::new().with(session(cookie.secrets(vec!["test-secret".into()]), storage));
    router.get("/start", |ctx: Context| async move {
        ctx.session().set("userId", "123");
        "ok"
    });
    router.get("/update", |ctx: Context| async move {
        ctx.session().set("updated", true);
        "ok"
    });
    router.get("/destroy", |ctx: Context| async move {
        ctx.session().destroy();
        "ok"
    });
    router.get("/", |ctx: Context| async move { ctx.session().get::<String>("userId").unwrap_or_else(|| "none".into()) });
    move |path, from| fetch(&router, &format!("https://app.example{path}"), from)
}

fn session_cookie() -> Cookie {
    Cookie::new("__session")
}

#[test]
fn enforces_max_age_with_cookie_storage_at_the_expiration_boundary() {
    let mut now = 1_000_000;
    set_now(now);
    let request = create_requests(session_cookie().max_age(60), CookieStorage::new());
    let response = request("/start", None);
    now += 59_999;
    set_now(now);
    let before_expiry = request("/", Some(&response));
    assert_eq!(before_expiry.body, "123");
    assert_eq!(before_expiry.set_cookies.len(), 0);
    set_now(now + 1);
    assert_eq!(request("/", Some(&response)).body, "none");
}

#[test]
fn enforces_max_age_with_memory_storage() {
    set_now(1_000_000);
    let request = create_requests(session_cookie().max_age(60), MemoryStorage::new());
    let response = request("/start", None);
    assert_eq!(request("/", Some(&response)).body, "123");
    set_now(1_060_000);
    assert_eq!(request("/", Some(&response)).body, "none");
}

#[test]
fn enforces_max_age_with_filesystem_storage() {
    let dir = std::env::temp_dir().join(format!("session-expiration-{}", rmx_crypto::random_hex(8)));
    let result = std::panic::catch_unwind(|| {
        set_now(1_000_000);
        let request = create_requests(session_cookie().max_age(60), FsStorage::new(&dir).unwrap());
        let response = request("/start", None);
        assert_eq!(request("/", Some(&response)).body, "123");
        set_now(1_060_000);
        assert_eq!(request("/", Some(&response)).body, "none");
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn enforces_an_absolute_expires_date_without_renewing_it_on_writes() {
    let mut now = 1_000_000;
    set_now(now);
    let request = create_requests(session_cookie().expires(at_ms(now + 60_000)), CookieStorage::new());
    let response = request("/start", None);
    now += 30_000;
    set_now(now);
    let updated = request("/update", Some(&response));
    assert_eq!(request("/", Some(&updated)).body, "123");
    set_now(now + 30_000);
    assert_eq!(request("/", Some(&updated)).body, "none");
}

#[test]
fn gives_max_age_precedence_over_expires_and_renews_it_only_on_writes() {
    let mut now = 1_000_000;
    set_now(now);
    let request = create_requests(session_cookie().max_age(60).expires(at_ms(now - 1_000)), CookieStorage::new());
    let response = request("/start", None);
    now += 30_000;
    set_now(now);
    let updated = request("/update", Some(&response));
    now += 30_000;
    set_now(now);
    assert_eq!(request("/", Some(&response)).body, "none");
    assert_eq!(request("/", Some(&updated)).body, "123");
    set_now(now + 30_000);
    assert_eq!(request("/", Some(&updated)).body, "none");
}

#[test]
fn matches_the_whole_second_precision_of_the_expires_header() {
    set_now(1_000_000);
    let request = create_requests(session_cookie().expires(at_ms(1_060_500)), CookieStorage::new());
    let response = request("/start", None);
    set_now(1_060_000);
    assert_eq!(request("/", Some(&response)).body, "none");
}

#[test]
fn starts_a_new_session_for_malformed_or_incomplete_expiration_metadata() {
    let request = create_requests(session_cookie().max_age(60), CookieStorage::new());
    let cookie = Cookie::new("__session").secrets(vec!["test-secret".into()]);
    for value in ["{", "null", r#"{"value":"id"}"#, r#"{"value":"id","expires":1e400}"#] {
        let response = Res { body: String::new(), set_cookies: vec![cookie.serialize(value)] };
        assert_eq!(request("/", Some(&response)).body, "none", "{value}");
    }
}

#[test]
fn expires_immediately_for_zero_and_negative_max_age() {
    let zero = create_requests(session_cookie().max_age(0), CookieStorage::new());
    assert_eq!(zero("/", Some(&zero("/start", None))).body, "none");
    let negative = create_requests(session_cookie().max_age(-1), CookieStorage::new());
    assert_eq!(negative("/", Some(&negative("/start", None))).body, "none");
}

#[test]
fn preserves_sessions_without_a_configured_lifetime() {
    set_now(1_000_000);
    let request = create_requests(session_cookie(), CookieStorage::new());
    let response = request("/start", None);
    set_now(1_000_000 + 365 * 24 * 60 * 60 * 1000);
    assert_eq!(request("/", Some(&response)).body, "123");
}

#[test]
fn requires_timestamped_cookies_when_a_lifetime_is_configured() {
    let storage = MemoryStorage::new();
    let previous = create_requests(session_cookie(), storage.clone());
    let response = previous("/start", None);
    let current = create_requests(session_cookie().max_age(60), storage);
    assert_eq!(current("/", Some(&response)).body, "none");
}

#[test]
fn preserves_the_empty_cookie_value_when_a_session_is_destroyed() {
    let request = create_requests(session_cookie().max_age(60), CookieStorage::new());
    let response = request("/destroy", Some(&request("/start", None)));
    assert_eq!(response.set_cookie().value, "");
    assert_eq!(request("/", Some(&response)).body, "none");
}

/// Records what the middleware passes to the storage and what it gets back.
#[derive(Clone, Default)]
struct Recording {
    inner: MemoryStorage,
    reads: Arc<Mutex<Vec<Option<String>>>>,
    saves: Arc<Mutex<Vec<Option<String>>>>,
}

impl SessionStorage for Recording {
    async fn read(&self, cookie: Option<&str>) -> Result<Session, Error> {
        self.reads.lock().unwrap().push(cookie.map(str::to_string));
        self.inner.read(cookie).await
    }

    async fn save(&self, session: &Session) -> Result<Option<String>, Error> {
        let value = self.inner.save(session).await?;
        self.saves.lock().unwrap().push(value.clone());
        Ok(value)
    }
}

#[test]
fn passes_only_unexpired_storage_values_to_a_custom_backend() {
    set_now(1_000_000);
    let storage = Recording::default();
    let request = create_requests(session_cookie().max_age(60), storage.clone());
    let response = request("/start", None);
    let stored_value = storage.saves.lock().unwrap()[0].clone();
    assert!(stored_value.as_deref().is_some_and(|v| !v.is_empty()));
    request("/", Some(&response));
    assert_eq!(storage.reads.lock().unwrap()[1], stored_value);
    set_now(1_060_000);
    request("/", Some(&response));
    assert_eq!(storage.reads.lock().unwrap()[2], None);
}

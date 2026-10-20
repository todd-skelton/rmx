//! Load the request's [`Session`] from its cookie and save it after the handler runs.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use http::header::{HeaderValue, SET_COOKIE};
use rmx_cookie::Cookie;
use rmx_router::{Context, Error, IntoResponse, Middleware, Next};
use rmx_session::{Session, SessionStorage};
use serde_json::json;

/// Puts a [`Session`] in the context (read it with [`SessionExt::session`]) and saves it
/// when the response comes back, adding `Set-Cookie` when the cookie value changes.
///
/// The cookie must be signed. It defaults to `HttpOnly`, and to `Secure` on https
/// requests; settings on the cookie win. With a max age, the cookie value carries its
/// own expiry, so a replayed old cookie starts a new session.
///
/// # Panics
///
/// When the cookie has no secrets, and when a session is already in the context.
pub fn session<S: SessionStorage>(cookie: Cookie, storage: S) -> impl Middleware {
    assert!(cookie.is_signed(), "session cookie must be signed (Cookie::secrets)");
    if cookie.get_http_only() == Some(false) {
        eprintln!(
            "session cookie \"{}\" has http_only(false) and may be readable by client-side JavaScript",
            cookie.name()
        );
    }
    let shared = Arc::new((cookie, storage));
    move |mut ctx: Context, next: Next| {
        let shared = shared.clone();
        async move {
            let (cookie, storage) = &*shared;
            assert!(ctx.get::<Session>().is_none(), "existing session found, refusing to overwrite");
            let max_age = cookie.get_max_age();
            let mut value = cookie.parse(ctx.header("cookie"));
            if max_age.is_some() {
                value = value.and_then(|v| read_expiring(&v));
            }
            let session = match storage.read(value.as_deref()).await {
                Ok(s) => s,
                Err(e) => return Error::new(http::StatusCode::INTERNAL_SERVER_ERROR, format!("session read failed: {e}")).into_response(),
            };
            let https = ctx.url().scheme() == "https";
            ctx.set(session.clone());

            let mut res = next.run(ctx).await;

            let mut value = match storage.save(&session).await {
                Ok(Some(v)) => v,
                Ok(None) => return res,
                Err(e) => return Error::new(http::StatusCode::INTERNAL_SERVER_ERROR, format!("session save failed: {e}")).into_response(),
            };
            if let (Some(age), false) = (max_age, value.is_empty()) {
                value = json!({ "value": value, "expires": now_ms() + age * 1000 }).to_string();
            }
            let set = cookie.serialize_with(&value, cookie.get_http_only().unwrap_or(true), cookie.get_secure().unwrap_or(https));
            if let Ok(header) = HeaderValue::from_str(&set) {
                res.headers_mut().append(SET_COOKIE, header);
            }
            res
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The inner value of an `{"value": .., "expires": ms}` envelope that has not expired.
fn read_expiring(cookie: &str) -> Option<String> {
    let data: serde_json::Value = serde_json::from_str(cookie).ok()?;
    let expires = data.get("expires")?.as_f64().filter(|e| e.is_finite())?;
    let value = data.get("value")?.as_str()?;
    (expires > now_ms() as f64).then(|| value.to_string())
}

/// `ctx.session()`: the session loaded by [`session()`].
pub trait SessionExt {
    /// # Panics
    ///
    /// When the session middleware did not run for this request.
    fn session(&self) -> &Session;
}

impl SessionExt for Context {
    fn session(&self) -> &Session {
        self.get::<Session>().expect("no session: add the session() middleware before reading ctx.session()")
    }
}

#[cfg(test)]
mod tests {
    use rmx_cookie::Cookie;
    use rmx_fetch::{block_on, Body, Response};
    use rmx_router::{Context, Router};
    use rmx_session::MemoryStorage;

    use super::*;

    fn counter_router(cookie: Cookie) -> Router {
        let mut router = Router::new().with(session(cookie, MemoryStorage::new()));
        router.get("/", |ctx: Context| async move {
            let n = ctx.session().get::<i64>("n").unwrap_or(0) + 1;
            ctx.session().set("n", n);
            n.to_string()
        });
        router.get("/peek", |ctx: Context| async move { ctx.session().get::<i64>("n").unwrap_or(0).to_string() });
        router.get("/logout", |ctx: Context| async move {
            ctx.session().destroy();
            "bye"
        });
        router
    }

    fn get(router: &Router, url: &str, cookie: Option<&str>) -> (String, Option<String>) {
        let mut req = http::Request::builder().uri(url);
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        let res: Response = block_on(router.fetch(req.body(Body::empty()).unwrap()));
        let set = res.headers().get(SET_COOKIE).map(|v| v.to_str().unwrap().to_string());
        (block_on(res.into_body().text()).unwrap(), set)
    }

    fn pair(set_cookie: &str) -> &str {
        set_cookie.split(';').next().unwrap()
    }

    #[test]
    fn persists_across_requests_with_secure_defaults() {
        let router = counter_router(Cookie::new("sid").secrets(vec!["s3cret".into()]).path("/"));
        let (body, set) = get(&router, "http://x/", None);
        assert_eq!(body, "1");
        let set = set.unwrap();
        assert!(set.contains("HttpOnly") && !set.contains("Secure"), "{set}");
        let (body, again) = get(&router, "http://x/", Some(pair(&set)));
        assert_eq!(body, "2");
        assert!(again.is_some(), "dirty session rewrites the cookie");
        let (body, unchanged) = get(&router, "http://x/peek", Some(pair(&set)));
        assert_eq!((body.as_str(), unchanged), ("2", None), "clean session leaves the cookie alone");

        let (_, https) = get(&router, "https://x/", None);
        assert!(https.unwrap().contains("; Secure"));

        let (_, cleared) = get(&router, "http://x/logout", Some(pair(&set)));
        assert_eq!(pair(&cleared.unwrap()), "sid=");
        assert_eq!(get(&router, "http://x/peek", Some(pair(&set))).0, "0", "destroyed in storage");
    }

    #[test]
    fn rejects_unsigned_or_tampered_cookies() {
        let router = counter_router(Cookie::new("sid").secrets(vec!["s3cret".into()]));
        let (_, set) = get(&router, "http://x/", None);
        let id = pair(set.as_deref().unwrap()).trim_start_matches("sid=").split('.').next().unwrap().to_string();
        assert_eq!(get(&router, "http://x/peek", Some(&format!("sid={id}"))).0, "0");
        let forged = rmx_cookie::sign(&id, "wrong");
        assert_eq!(get(&router, "http://x/peek", Some(&format!("sid={forged}"))).0, "0");
    }

    #[test]
    fn max_age_wraps_the_value_with_an_expiry() {
        let router = counter_router(Cookie::new("sid").secrets(vec!["k".into()]).max_age(60));
        let (_, set) = get(&router, "http://x/", None);
        let set = set.unwrap();
        let signed = pair(&set).trim_start_matches("sid=");
        let inner = rmx_url::decode(rmx_cookie::unsign(signed, "k").unwrap());
        let envelope: serde_json::Value = serde_json::from_str(&inner).unwrap();
        assert!(envelope["value"].is_string() && envelope["expires"].as_i64().unwrap() > now_ms());
        assert_eq!(get(&router, "http://x/peek", Some(pair(&set))).0, "1");

        let expired = json!({ "value": envelope["value"], "expires": now_ms() - 1 }).to_string();
        let replay = Cookie::new("sid").secrets(vec!["k".into()]).serialize(&expired);
        assert_eq!(get(&router, "http://x/peek", Some(pair(&replay))).0, "0");
    }

    #[test]
    #[should_panic(expected = "must be signed")]
    fn requires_a_signed_cookie() {
        let _ = session(Cookie::new("sid"), MemoryStorage::new());
    }
}

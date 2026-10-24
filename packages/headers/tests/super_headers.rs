//! Ports of Remix's `super-headers.test.ts`. `SuperHeaders` is a trait on `http::HeaderMap`, so
//! there is no subclass, no initializer object and no cache: each read parses the current
//! text and each write goes straight to the map. The lazy parsing, cache identity, proxy
//! (`defineProperty`, `delete`), stale value, invalid date and method shadowing cases test
//! JavaScript object behavior that has no counterpart; the rest are ported here.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::header::{self, HeaderMap, HeaderValue};
use rsmix_headers::raw::stringify;
use rsmix_headers::{Accept, AcceptEncoding, AcceptLanguage, CacheControl, ContentType, Cookie, IfMatch, IfNoneMatch, Range, SetCookie, SuperHeaders, Vary};

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

fn cookie(name: &str, value: &str) -> SetCookie {
    SetCookie { name: name.into(), value: value.into(), ..Default::default() }
}

fn set_cookie_text(headers: &HeaderMap) -> Vec<String> {
    headers.get_all(header::SET_COOKIE).iter().map(|v| v.to_str().unwrap().to_string()).collect()
}

#[test]
fn typed_values_initialize_headers() {
    let mut headers = HeaderMap::new();
    assert_eq!(headers.get_joined("content-type"), None);
    headers.set_list(header::ALLOW, ["GET", "POST"]).unwrap();
    headers.set_typed(&CacheControl { public: true, max_age: Some(3600), ..Default::default() }).unwrap();
    headers.set_typed(&ContentType { media_type: some("text/html"), charset: some("utf-8"), boundary: None }).unwrap();
    headers.replace_set_cookies(&[SetCookie { path: some("/"), ..cookie("session", "abc") }, cookie("theme", "dark")]).unwrap();
    assert_eq!(headers.get_joined("Allow").as_deref(), Some("GET, POST"));
    assert_eq!(headers.get_joined("Cache-Control").as_deref(), Some("public, max-age=3600"));
    assert_eq!(headers.get_joined("Content-Type").as_deref(), Some("text/html; charset=utf-8"));
    assert_eq!(set_cookie_text(&headers), ["session=abc; Path=/", "theme=dark"]);
}

#[test]
fn applies_typed_values() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ALLOW, HeaderValue::from_static("GET"));
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
    headers.insert(header::COOKIE, HeaderValue::from_static("session=abc"));
    headers.insert(header::SET_COOKIE, HeaderValue::from_static("session=abc"));
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));

    let mut allow = HeaderMap::new();
    allow.insert(header::ALLOW, HeaderValue::from_static("POST"));
    headers.apply(&allow);
    headers.apply_typed(&ContentType { media_type: some("application/json"), charset: some("utf-8"), boundary: None }).unwrap();
    headers.apply_typed(&[("theme", "dark")].into_iter().collect::<Cookie>()).unwrap();
    headers.apply_typed(&["Accept-Encoding", "Accept-Language"].into_iter().collect::<Vary>()).unwrap();
    let mut theme = HeaderMap::new();
    theme.insert(header::SET_COOKIE, HeaderValue::from_str(&cookie("theme", "dark").to_string()).unwrap());
    headers.apply(&theme);

    assert_eq!(headers.get_joined("Allow").as_deref(), Some("GET, POST"));
    assert_eq!(headers.get_joined("Content-Type").as_deref(), Some("application/json; charset=utf-8"));
    assert_eq!(headers.get_joined("Cookie").as_deref(), Some("session=abc; theme=dark"));
    assert_eq!(set_cookie_text(&headers), ["session=abc", "theme=dark"]);
    assert_eq!(headers.get_joined("Vary").as_deref(), Some("accept-encoding, accept-language"));
}

#[test]
fn applies_raw_values_with_header_semantics() {
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("content-type", "text/html"),
        ("link", "</style.css>; rel=preload"),
        ("set-cookie", "session=abc"),
        ("vary", "Accept-Encoding"),
        ("x-test", "old"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    let mut first = HeaderMap::new();
    for (name, value) in [
        ("content-type", "application/json"),
        ("link", "</script.js>; rel=preload"),
        ("set-cookie", "theme=dark"),
        ("vary", "Accept-Language"),
        ("x-test", "new"),
    ] {
        first.insert(name, HeaderValue::from_static(value));
    }
    let mut second = HeaderMap::new();
    for (name, value) in [("link", "</image.png>; rel=preload"), ("set-cookie", "locale=en"), ("vary", "User-Agent")] {
        second.insert(name, HeaderValue::from_static(value));
    }
    headers.apply(&first).apply(&second);

    assert_eq!(headers.get_joined("Content-Type").as_deref(), Some("application/json"));
    assert_eq!(
        headers.get_joined("Link").as_deref(),
        Some("</style.css>; rel=preload, </script.js>; rel=preload, </image.png>; rel=preload")
    );
    assert_eq!(set_cookie_text(&headers), ["session=abc", "theme=dark", "locale=en"]);
    assert_eq!(headers.get_joined("Vary").as_deref(), Some("accept-encoding, accept-language, user-agent"));
    assert_eq!(headers.get_joined("X-Test").as_deref(), Some("new"));

    // Empty values never add list entries.
    let mut empty = HeaderMap::new();
    empty.insert(header::LINK, HeaderValue::from_static(""));
    empty.insert(header::SET_COOKIE, HeaderValue::from_static(""));
    headers.apply(&empty);
    assert_eq!(headers.get_all(header::LINK).iter().count(), 3);
    assert_eq!(set_cookie_text(&headers).len(), 3);
}

#[test]
fn empty_typed_values_remove_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
    headers.apply_typed(&ContentType::default()).unwrap();
    assert!(!headers.contains_key(header::CONTENT_TYPE));
    headers.set_typed(&ContentType::default()).unwrap();
    assert!(headers.is_empty(), "absent typed values never create headers");
    assert_eq!(headers.typed::<ContentType>().to_string(), "");
}

#[test]
fn updates_write_back() {
    let mut headers = HeaderMap::new();
    headers.update(|t: &mut ContentType| t.charset = some("utf-8")).unwrap();
    assert!(!headers.contains_key(header::CONTENT_TYPE), "not printable yet");
    headers.update(|t: &mut ContentType| t.media_type = some("text/html")).unwrap();
    // Remix keeps the cached object, so its charset survives; here nothing printable was
    // stored, so the second edit starts from an empty value.
    assert_eq!(headers.get_joined("Content-Type").as_deref(), Some("text/html"));
    headers.update(|t: &mut ContentType| t.charset = some("utf-8")).unwrap();
    assert_eq!(headers.get_joined("Content-Type").as_deref(), Some("text/html; charset=utf-8"));
}

#[test]
fn typed_mutators_write_back() {
    let mut headers = HeaderMap::new();
    headers.update(|a: &mut Accept| {
        a.set("text/html", 1.0);
        a.set("application/json", 0.8);
    }).unwrap();
    headers.update(|c: &mut Cookie| {
        c.set("theme", "dark");
        c.append("theme", "light");
    }).unwrap();
    headers.update(|v: &mut Vary| v.add("Accept-Encoding")).unwrap();
    assert_eq!(headers.get_joined("Accept").as_deref(), Some("text/html,application/json;q=0.8"));
    assert_eq!(headers.get_joined("Cookie").as_deref(), Some("theme=dark; theme=light"));
    assert_eq!(headers.typed::<Cookie>().get_all("theme"), ["dark", "light"]);
    assert_eq!(headers.get_joined("Vary").as_deref(), Some("accept-encoding"));

    headers.update(|a: &mut Accept| a.delete("text/html")).unwrap();
    assert_eq!(headers.get_joined("Accept").as_deref(), Some("application/json;q=0.8"));
    headers.update(|a: &mut Accept| a.clear()).unwrap();
    assert!(!headers.contains_key(header::ACCEPT));
    headers.update(|e: &mut AcceptEncoding| e.set("gzip", 1.0)).unwrap();
    assert_eq!(headers.get_joined("Accept-Encoding").as_deref(), Some("gzip"));
    headers.update(|e: &mut AcceptEncoding| e.delete("gzip")).unwrap();
    assert!(!headers.contains_key(header::ACCEPT_ENCODING));
    headers.update(|l: &mut AcceptLanguage| l.set("en-US", 1.0)).unwrap();
    assert_eq!(headers.get_joined("Accept-Language").as_deref(), Some("en-us"));
    headers.update(|l: &mut AcceptLanguage| l.clear()).unwrap();
    assert!(!headers.contains_key(header::ACCEPT_LANGUAGE));
    headers.update(|c: &mut Cookie| c.delete("theme")).unwrap();
    assert!(!headers.contains_key(header::COOKIE));
    headers.update(|v: &mut Vary| v.clear()).unwrap();
    assert!(!headers.contains_key(header::VARY));
}

#[test]
fn nested_values_write_back() {
    let mut headers = HeaderMap::new();
    headers.update(|m: &mut IfMatch| m.tags.push("\"abc\"".into())).unwrap();
    assert_eq!(headers.get_joined("If-Match").as_deref(), Some("\"abc\""));
    headers.update(|m: &mut IfNoneMatch| m.tags.push("\"def\"".into())).unwrap();
    assert_eq!(headers.get_joined("If-None-Match").as_deref(), Some("\"def\""));
    headers.update(|r: &mut Range| {
        r.unit = "bytes".into();
        r.ranges.push((Some(0), Some(99)));
    }).unwrap();
    assert_eq!(headers.get_joined("Range").as_deref(), Some("bytes=0-99"));
    headers.update(|r: &mut Range| r.ranges[0].1 = Some(199)).unwrap();
    assert_eq!(headers.get_joined("Range").as_deref(), Some("bytes=0-199"));
}

#[test]
fn reads_follow_native_changes() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("text/html"));
    assert!(headers.typed::<Accept>().has("text/html"));
    headers.append(header::ACCEPT, HeaderValue::from_static("application/json"));
    assert!(headers.typed::<Accept>().has("application/json"), "appended values are read too");
    headers.insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
    let accept: Accept = headers.typed();
    assert!(!accept.has("text/html") && accept.has("text/plain"));
    headers.remove(header::ACCEPT);
    assert!(headers.typed::<Accept>().is_empty());
}

#[test]
fn scalar_headers() {
    let mut headers = HeaderMap::new();
    let date = UNIX_EPOCH + Duration::from_secs(1_609_459_200);
    headers.set_list(header::ALLOW, ["GET", "POST"]).unwrap();
    headers.insert(header::AGE, HeaderValue::from(42));
    headers.set_etag("abc123").unwrap();
    headers.set_date(header::LAST_MODIFIED, date);
    headers.set_list("permissions-policy".parse().unwrap(), ["geolocation=()", "camera=()"]).unwrap();
    headers.set_list("prefer".parse().unwrap(), vec!["respond-async".to_string(), "return=minimal".to_string()]).unwrap();
    headers.set_list("x-forwarded-for".parse().unwrap(), ["203.0.113.7", "70.41.3.18"]).unwrap();
    assert_eq!(headers.get_joined("Allow").as_deref(), Some("GET, POST"));
    assert_eq!(headers.number(header::AGE), Some(42));
    assert_eq!(headers.get_joined("ETag").as_deref(), Some("\"abc123\""));
    headers.set_etag("W/\"weak\"").unwrap();
    assert_eq!(headers.get_joined("ETag").as_deref(), Some("W/\"weak\""), "quoted tags are kept");
    assert_eq!(headers.date(header::LAST_MODIFIED), Some(date));
    assert_eq!(headers.get_joined("Last-Modified").as_deref(), Some("Fri, 01 Jan 2021 00:00:00 GMT"));
    assert_eq!(headers.get_joined("Permissions-Policy").as_deref(), Some("geolocation=(), camera=()"));
    assert_eq!(headers.get_joined("Prefer").as_deref(), Some("respond-async, return=minimal"));
    assert_eq!(headers.get_joined("X-Forwarded-For").as_deref(), Some("203.0.113.7, 70.41.3.18"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12abc"));
    assert_eq!(headers.number(header::CONTENT_LENGTH), Some(12), "read like parseInt");
    assert_eq!(headers.number(header::MAX_FORWARDS), None);
    assert_eq!(headers.date(header::EXPIRES), None);
    let _: SystemTime = date;
}

#[test]
fn stringifies_like_native_headers() {
    let mut headers = HeaderMap::new();
    headers.set_typed(&ContentType::parse(Some("text/html"))).unwrap();
    assert_eq!(stringify(&headers), "Content-Type: text/html");
}

#[test]
fn duplicate_cookie_values() {
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, HeaderValue::from_static("session=child; session=parent; theme=dark"));
    let cookies: Cookie = headers.typed();
    assert_eq!(cookies.get("session"), Some("child"));
    assert_eq!(cookies.get_all("session"), ["child", "parent"]);
    assert_eq!(cookies.iter().collect::<Vec<_>>(), [("session", "child"), ("session", "parent"), ("theme", "dark")]);
    assert_eq!(headers.get_joined("Cookie").as_deref(), Some("session=child; session=parent; theme=dark"));
}

#[test]
fn multiple_set_cookie_values() {
    let mut headers = HeaderMap::new();
    headers.replace_set_cookies(&[SetCookie::parse("session=abc").unwrap()]).unwrap();
    assert_eq!(set_cookie_text(&headers), ["session=abc"]);
    let mut cookies = headers.set_cookies();
    cookies.push(cookie("theme", "dark"));
    headers.replace_set_cookies(&cookies).unwrap();
    assert_eq!(set_cookie_text(&headers), ["session=abc", "theme=dark"]);
    cookies[0].secure = true;
    cookies[1] = cookie("lang", "en");
    headers.replace_set_cookies(&cookies).unwrap();
    assert_eq!(set_cookie_text(&headers), ["session=abc; Secure", "lang=en"]);
    headers.append(header::SET_COOKIE, HeaderValue::from_static("extra=1"));
    assert_eq!(headers.set_cookies().len(), 3);
    for c in &mut cookies {
        c.http_only = true;
        c.path = some("/");
    }
    headers.replace_set_cookies(&cookies[..1]).unwrap();
    assert_eq!(set_cookie_text(&headers), ["session=abc; HttpOnly; Path=/; Secure"]);
    headers.replace_set_cookies(&[SetCookie::default(), cookie("kept", "1")]).unwrap();
    assert_eq!(set_cookie_text(&headers), ["kept=1"], "unnamed cookies are skipped");
    headers.remove(header::SET_COOKIE);
    assert!(headers.set_cookies().is_empty());
}

// super-headers.test.ts (remix 3.1): numeric values with no integer prefix read as null.
#[test]
fn numeric_headers_without_an_integer_prefix_are_none() {
    let mut headers = HeaderMap::new();
    for name in ["access-control-max-age", "age", "content-length", "max-forwards", "upgrade-insecure-requests"] {
        headers.insert(name.parse::<http::header::HeaderName>().unwrap(), HeaderValue::from_static("invalid"));
        assert_eq!(headers.number(name), None, "{name}");
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12bytes"));
    assert_eq!(headers.number(header::CONTENT_LENGTH), Some(12));
}

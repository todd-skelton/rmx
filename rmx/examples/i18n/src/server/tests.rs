//! app/router.test.ts, app/middleware/i18n.test.ts and app/i18n/config.test.ts.

use rmx::fetch::{block_on, Body};
use rmx::headers::Vary;

use super::*;

const ORIGIN: &str = "http://localhost:44100";

fn app() -> Router {
    router(concat!(env!("CARGO_MANIFEST_DIR"), "/public"))
}

fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Response {
    let mut req = http::Request::get(format!("{ORIGIN}{path}"));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    block_on(router.fetch(req.body(Body::empty()).unwrap()))
}

fn post_language(form: &[(&str, &str)], cookie: Option<&str>) -> Response {
    let body = form.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let mut req = http::Request::post(format!("{ORIGIN}{}", ROUTES.language.href(()))).header("content-type", "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    block_on(app().fetch(req.body(Body::from(body)).unwrap()))
}

fn header<'a>(res: &'a Response, name: &str) -> Option<&'a str> {
    res.headers().get(name).and_then(|v| v.to_str().ok())
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn cookie_header(locale: Language) -> String {
    LOCALE_COOKIE.serialize(locale.code()).split(';').next().unwrap().to_string()
}

#[test]
fn sets_localization_headers_for_negotiated_and_explicit_locales() {
    let router = app();
    let negotiated = get(&router, &home_href(None), &[]);
    let ja = cookie_header(Language::Ja);
    let explicit = get(&router, &home_href(Some(Language::Es)), &[("cookie", &ja), ("accept-language", "fr-FR,fr;q=0.9")]);

    assert_eq!(negotiated.status(), 200);
    assert_eq!(header(&negotiated, "content-language"), Some("en"));
    let vary = Vary::parse(header(&negotiated, "vary"));
    assert!(vary.has("Accept-Language") && vary.has("Cookie"), "{vary}");

    assert_eq!(explicit.status(), 200);
    assert_eq!(header(&explicit, "content-language"), Some("es"));
    assert_eq!(header(&explicit, "vary"), None);
}

#[test]
fn renders_concurrent_requests_with_localized_metadata_and_plural_forms() {
    let router = app();
    let fetch = |l: Language| {
        let res = router.fetch(http::Request::get(format!("{ORIGIN}{}", home_href(Some(l)))).body(Body::empty()).unwrap());
        async move {
            let res = res.await;
            assert_eq!(res.status(), 200);
            assert_eq!(header(&res, "content-language"), Some(l.code()));
            res.into_body().text().await.unwrap()
        }
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (japanese, arabic, french, spanish) =
        rt.block_on(async { tokio::join!(fetch(Language::Ja), fetch(Language::Ar), fetch(Language::Fr), fetch(Language::Es)) });

    assert!(japanese.contains(r#"<html lang="ja" dir="ltr">"#));
    assert!(japanese.contains("<title>Remix i18n デモ</title>"));
    assert!(japanese.contains("1 件の保留中タスクがあります"));
    assert!(arabic.contains(r#"<html lang="ar" dir="rtl">"#));
    assert!(arabic.contains("<title>مثال التدويل في Remix</title>"));
    for text in ["سلة تسوق تفاعلية", "سلة التسوق فارغة", "ليس لديك مهام معلقة", "لديك مهمة واحدة معلقة", "لديك مهمتان معلقتان", "لديك 5 مهام معلقة", "لديك 11 مهمة معلقة", "لديك 100 مهمة معلقة"] {
        assert!(arabic.contains(text), "{text}");
    }
    assert!(french.contains("Vous avez 1000000 tâches en attente"));
    assert!(spanish.contains("Tienes 1000000 tareas pendientes"));
}

#[test]
fn renders_formatted_values_and_client_entries() {
    let html = text(get(&app(), "/en", &[]));
    for text in ["Wednesday, April 15, 2026 at 2:30 PM", "1,250,000", "$49.99", "3 days ago", "Welcome back, Ada Lovelace!", "You have no pending tasks", "Your cart is empty"] {
        assert!(html.contains(text), "{text}");
    }
    assert!(html.contains("CartPreview") && html.contains("NumberPreview"));
    // Server-rendered client entries start disabled until hydration.
    assert!(html.contains("disabled"));
}

#[test]
fn serves_the_browser_entry() {
    let res = get(&app(), "/boot.js", &[]);
    assert_eq!(res.status(), 200);
    assert!(header(&res, "content-type").is_some_and(|v| v.starts_with("text/javascript") || v.starts_with("application/javascript")));
}

#[test]
fn saves_a_language_preference() {
    let res = post_language(&[("intent", "save"), ("locale", "fr")], None);
    assert_eq!(res.status(), 303);
    assert_eq!(header(&res, "location"), Some(home_href(Some(Language::Fr)).as_str()));
    let set_cookie = header(&res, "set-cookie").unwrap();
    assert_eq!(LOCALE_COOKIE.parse(Some(set_cookie.split(';').next().unwrap())).as_deref(), Some("fr"));
    for attr in ["Max-Age=31536000", "Path=/", "HttpOnly", "SameSite=Lax"] {
        assert!(set_cookie.contains(attr), "{set_cookie}");
    }
}

#[test]
fn clears_a_language_preference() {
    let res = post_language(&[("intent", "clear")], Some(&cookie_header(Language::Fr)));
    assert_eq!(res.status(), 303);
    assert_eq!(header(&res, "location"), Some(home_href(None).as_str()));
    assert!(header(&res, "set-cookie").unwrap().contains("Max-Age=0"));
}

#[test]
fn rejects_unsupported_locales() {
    assert_eq!(get(&app(), "/de", &[]).status(), 404);
    assert_eq!(post_language(&[("intent", "save"), ("locale", "de")], None).status(), 400);
    assert_eq!(post_language(&[("intent", "nope")], None).status(), 400);
}

#[test]
fn detects_language_in_priority_order() {
    let es = cookie_header(Language::Es);
    let mut headers = http::HeaderMap::new();
    headers.insert("cookie", es.parse().unwrap());
    headers.insert("accept-language", "fr-FR,fr;q=0.9".parse().unwrap());
    assert_eq!(detect_language(&headers, Some("ja")), (Language::Ja, DetectionSource::Path));
    assert_eq!(detect_language(&headers, None), (Language::Es, DetectionSource::Cookie));
    headers.remove("cookie");
    assert_eq!(detect_language(&headers, None), (Language::Fr, DetectionSource::Browser));
    assert_eq!(detect_language(&http::HeaderMap::new(), None), (Language::En, DetectionSource::Browser));
}

#[test]
fn ignores_invalid_cookies_and_negotiates_supported_languages_by_quality() {
    let mut headers = http::HeaderMap::new();
    headers.insert("cookie", "locale=not-base64!".parse().unwrap());
    headers.insert("accept-language", "de,fr;q=0,es-MX;q=0.8,en;q=0.5".parse().unwrap());
    assert_eq!(detect_language(&headers, None), (Language::Es, DetectionSource::Browser));
}

#[test]
fn reads_catalog_subtrees_by_key_path() {
    let t = I18nState { locale: Language::En, ..Default::default() }.t();
    assert_eq!(t.get("pluralization"), resources(Language::En).get("pluralization"));
    assert_eq!(t.get("pluralization.cart_demo.title").and_then(Value::as_str), Some("Interactive cart"));
}

#[test]
fn falls_back_to_english_and_uses_the_zero_override() {
    let fr = I18nState { locale: Language::Fr, ..Default::default() }.t();
    // French plural rules put 0 in "one"; i18next's _zero key wins first.
    assert_eq!(fr.t_count("pluralization.tasks", 0), fr.t("pluralization.tasks_zero"));
    assert_eq!(fr.t("no.such.key"), "no.such.key");
}

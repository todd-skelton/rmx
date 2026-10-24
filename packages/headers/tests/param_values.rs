//! Ports of Remix's `headers/src/lib/param-values.test.ts`: [`parse_params`] takes the delimiter
//! Remix defaults to `;`, and a key without a value is `None` where Remix has `undefined`.

use rsmix_headers::parse_params;

fn p(input: &str) -> Vec<(String, Option<String>)> {
    parse_params(input, b';')
}

fn kv(pairs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.map(str::to_string))).collect()
}

#[test]
fn unterminated_quoted_values() {
    assert_eq!(p(r#"boundary="abc"#), kv(&[("boundary", Some("abc"))]));
    assert_eq!(p(r#"boundary=""#), kv(&[("boundary", Some(""))]));
    assert_eq!(p(r#"note="value; boundary=abc"#), kv(&[("note", Some("value; boundary=abc"))]));
    assert_eq!(parse_params(r#"private="field, max-age=60"#, b','), kv(&[("private", Some("field, max-age=60"))]));
    assert_eq!(p(r#"note="value\\\"; boundary=abc"#), kv(&[("note", Some(r#"value\"; boundary=abc"#))]));
    assert_eq!(p(r#"note="value\"#), kv(&[("note", Some(r"value\"))]));
}

#[test]
fn quoted_values() {
    assert_eq!(p("note=\" \tvalue \t\"; boundary=abc"), kv(&[("note", Some(" \tvalue \t")), ("boundary", Some("abc"))]));
    assert_eq!(p("note=\" \tvalue \t"), kv(&[("note", Some(" \tvalue \t"))]));
    assert_eq!(p(r#"attachment; filename="one;two\".txt""#), kv(&[("attachment", None), ("filename", Some(r#"one;two".txt"#))]));
}

#[test]
fn content_type() {
    assert_eq!(p("text/html; charset=utf-8"), kv(&[("text/html", None), ("charset", Some("utf-8"))]));
    assert_eq!(p("application/json"), kv(&[("application/json", None)]));
    assert_eq!(
        p("multipart/form-data; boundary=----WebKitFormBoundaryABC123"),
        kv(&[("multipart/form-data", None), ("boundary", Some("----WebKitFormBoundaryABC123"))])
    );
}

#[test]
fn content_disposition() {
    assert_eq!(p("form-data; name=fieldName"), kv(&[("form-data", None), ("name", Some("fieldName"))]));
    assert_eq!(
        p(r#"form-data; name="fieldName"; filename="filename.jpg""#),
        kv(&[("form-data", None), ("name", Some("fieldName")), ("filename", Some("filename.jpg"))])
    );
    let expected = kv(&[("attachment", None), ("filename", Some("photo.jpg")), ("filename*", Some("UTF-8''%E7%85%A7%E7%89%87.jpg"))]);
    assert_eq!(p("attachment; filename=photo.jpg; filename*=UTF-8''%E7%85%A7%E7%89%87.jpg"), expected);
    assert_eq!(p(r#"attachment; filename="photo.jpg"; filename*="UTF-8''%E7%85%A7%E7%89%87.jpg""#), expected);
}

#[test]
fn set_cookie() {
    assert_eq!(
        p("session_id=abc123; Path=/; HttpOnly; Secure"),
        kv(&[("session_id", Some("abc123")), ("Path", Some("/")), ("HttpOnly", None), ("Secure", None)])
    );
    assert_eq!(
        p(r#"user_pref="dark_mode"; Max-Age=31536000; SameSite=Lax"#),
        kv(&[("user_pref", Some("dark_mode")), ("Max-Age", Some("31536000")), ("SameSite", Some("Lax"))])
    );
    assert_eq!(
        p(r#"preferences={"font":"Arial","size":"12pt"}; Expires=Fri, 31 Dec 2023 23:59:59 GMT"#),
        kv(&[("preferences", Some(r#"{"font":"Arial","size":"12pt"}"#)), ("Expires", Some("Fri, 31 Dec 2023 23:59:59 GMT"))])
    );
    assert_eq!(
        p(r#"cart_items="[\"item1\",\"item2\"]"; Path=/cart; HttpOnly"#),
        kv(&[("cart_items", Some(r#"["item1","item2"]"#)), ("Path", Some("/cart")), ("HttpOnly", None)])
    );
    assert_eq!(
        p(r#"account_type="premium,\"gold\""; Domain=example.com; Secure"#),
        kv(&[("account_type", Some(r#"premium,"gold""#)), ("Domain", Some("example.com")), ("Secure", None)])
    );
    assert_eq!(
        p("a2f_token=987654; Path=/2fa; Secure; HttpOnly; SameSite=Strict; Max-Age=300"),
        kv(&[
            ("a2f_token", Some("987654")),
            ("Path", Some("/2fa")),
            ("Secure", None),
            ("HttpOnly", None),
            ("SameSite", Some("Strict")),
            ("Max-Age", Some("300")),
        ])
    );
}

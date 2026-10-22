//! Ports of Remix's `headers/src/lib/set-cookie.test.ts`. `new SetCookie(string)` and
//! `SetCookie.from` are [`SetCookie::parse`], which returns `None` for an empty value where Remix
//! returns a cookie without a name; an object init is a struct literal, and `SameSite` is an
//! enum, so the lowercase init cases are the parse cases.

use std::time::{Duration, UNIX_EPOCH};

use rmx_headers::{SameSite, SetCookie};

fn parse(value: &str) -> SetCookie {
    SetCookie::parse(value).expect("a cookie")
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[test]
fn initializes() {
    assert_eq!(SetCookie::parse(""), None);
    let h = parse("session=abc123; Domain=example.com; Path=/; Expires=Wed, 21 Oct 2015 07:28:00 GMT; Secure; HttpOnly");
    assert_eq!((h.name.as_str(), h.value.as_str()), ("session", "abc123"));
    assert_eq!((h.domain.clone(), h.path.clone()), (some("example.com"), some("/")));
    assert_eq!(h.expires, Some(UNIX_EPOCH + Duration::from_secs(1_445_412_480)));
    assert!(h.secure && h.http_only);
    let built = SetCookie {
        name: "session".into(),
        value: "abc123".into(),
        domain: some("example.com"),
        path: some("/"),
        expires: Some(UNIX_EPOCH + Duration::from_secs(1_445_412_480)),
        secure: true,
        http_only: true,
        ..Default::default()
    };
    assert_eq!(built, h);
    assert_eq!(h.clone(), h);
    let plain = SetCookie { name: "session".into(), value: "abc123".into(), ..Default::default() };
    assert!(!plain.http_only && !plain.secure);
    let from = parse("session=abc123; Path=/; HttpOnly");
    assert_eq!((from.name.as_str(), from.value.as_str(), from.path, from.http_only), ("session", "abc123", some("/"), true));
}

#[test]
fn parses_values() {
    let h = parse("user=john");
    assert_eq!((h.name.as_str(), h.value.as_str()), ("user", "john"));
    let commas = parse("list=apple,banana,cherry; Domain=example.com");
    assert_eq!((commas.value.as_str(), commas.domain), ("apple,banana,cherry", some("example.com")));
    let semis = parse(r#"complex="value; with; semicolons"; Path=/"#);
    assert_eq!((semis.name.as_str(), semis.value.as_str(), semis.path), ("complex", "value; with; semicolons", some("/")));
    let equals = parse(r#"equation="1+1=2"; Secure"#);
    assert_eq!((equals.value.as_str(), equals.secure), ("1+1=2", true));
    assert_eq!(parse(r#"complex="quoted value; with semicolon""#).value, "quoted value; with semicolon");
    let empty = parse("name=");
    assert_eq!((empty.name.as_str(), empty.value.as_str()), ("name", ""));
    let cased = parse("TestName=TestValue");
    assert_eq!((cased.name.as_str(), cased.value.as_str()), ("TestName", "TestValue"));
}

#[test]
fn attributes() {
    let mut h = parse("test=value");
    h.domain = some("example.org");
    h.path = some("/api");
    h.max_age = Some(3600);
    h.secure = true;
    h.http_only = true;
    h.same_site = Some(SameSite::Strict);
    assert_eq!((h.domain, h.path, h.max_age, h.same_site), (some("example.org"), some("/api"), Some(3600), Some(SameSite::Strict)));
    assert_eq!(parse("test=value; Path=/; Path=/api").path, some("/api"), "the last one wins");
    assert_eq!(parse("test=value; Unknown=something").to_string(), "test=value");
    let missing = parse("test=value; Domain=; Path");
    assert_eq!((missing.domain, missing.path), (some(""), None));
}

#[test]
fn max_age() {
    assert_eq!(parse("test=value; Max-Age=3600").max_age, Some(3600));
    assert_eq!(parse("test=value; Max-Age=invalid").max_age, None);
    // Like parseInt: leading digits count and a sign is allowed.
    assert_eq!(parse("test=value; Max-Age=60s").max_age, Some(60));
    assert_eq!(parse("test=value; Max-Age=1.5").max_age, Some(1));
    assert_eq!(parse("test=value; Max-Age=-1").max_age, Some(-1));
    assert_eq!(parse("test=value; Max-Age=60; Max-Age=x").max_age, Some(60), "an invalid repeat keeps the earlier one");
}

#[test]
fn same_site() {
    assert_eq!(parse("test=value; SameSite=lax").same_site, Some(SameSite::Lax));
    assert_eq!(parse("test=value; SameSite=STRICT").same_site, Some(SameSite::Strict));
    assert_eq!(parse("test=value; SameSite=NoNe").same_site, Some(SameSite::None));
    assert_eq!(parse("test=value; SameSite=lax").to_string(), "test=value; SameSite=Lax");
    assert_eq!(parse("test=value; SameSite=strict").to_string(), "test=value; SameSite=Strict");
    for invalid in ["nonesense", "strictly", "laxness"] {
        assert_eq!(parse(&format!("test=value; SameSite={invalid}")).same_site, None, "{invalid}");
    }
}

#[test]
fn to_string() {
    let mut h = parse("session=abc123");
    h.domain = some("example.com");
    h.path = some("/");
    h.secure = true;
    h.http_only = true;
    h.same_site = Some(SameSite::Lax);
    h.max_age = Some(0);
    assert_eq!(h.to_string(), "session=abc123; Domain=example.com; HttpOnly; Max-Age=0; Path=/; SameSite=Lax; Secure");
    let unnamed = SetCookie { value: "test".into(), ..Default::default() };
    assert_eq!(unnamed.to_string(), "");
    let mut renamed = parse("old=value");
    renamed.name = "new".into();
    renamed.value = "newvalue".into();
    assert_eq!(renamed.to_string(), "new=newvalue");
    renamed.value = "need; quotes".into();
    assert_eq!(renamed.to_string(), r#"new="need; quotes""#);
    // Empty Domain and Path are left out, like Remix's truthiness checks.
    assert_eq!(parse("test=value; Domain=; Path=").to_string(), "test=value");
}

#[test]
fn expires() {
    let mut h = parse("test=value; Expires=Wed, 21 Oct 2015 07:28:00 GMT");
    assert_eq!(h.expires, Some(UNIX_EPOCH + Duration::from_secs(1_445_412_480)));
    h.expires = Some(UNIX_EPOCH + Duration::from_secs(1_445_498_880));
    assert_eq!(h.to_string(), "test=value; Expires=Thu, 22 Oct 2015 07:28:00 GMT");
}

//! Remix's `cookie.test.ts`.

use std::time::{Duration, UNIX_EPOCH};

use rmx_cookie::{Cookie, CookieProperties, SameSite};
use rmx_headers::SetCookie;

fn cookie_from_set_cookie(set_cookie: &str) -> String {
    let header = SetCookie::parse(set_cookie).unwrap();
    format!("{}={}", header.name, header.value)
}

fn set_cookie_value(set_cookie: &str) -> String {
    SetCookie::parse(set_cookie).unwrap().value
}

#[test]
fn leaves_secure_undefined_when_it_is_not_configured() {
    let cookie = Cookie::new("my-cookie");
    assert_eq!(cookie.get_secure(), None);
    assert!(!SetCookie::parse(&cookie.serialize("value")).unwrap().secure);
    assert_eq!(Cookie::new("my-cookie").secure(true).get_secure(), Some(true));
    assert_eq!(Cookie::new("my-cookie").secure(false).get_secure(), Some(false));
}

#[test]
fn leaves_http_only_undefined_by_default() {
    assert_eq!(Cookie::new("my-cookie").get_http_only(), None);
}

#[test]
fn preserves_explicit_http_only_values() {
    assert_eq!(Cookie::new("my-cookie").http_only(true).get_http_only(), Some(true));
    assert_eq!(Cookie::new("my-cookie").http_only(false).get_http_only(), Some(false));
}

#[test]
fn defaults_path_to_slash() {
    let cookie = Cookie::new("my-cookie");
    assert_eq!(cookie.get_path(), "/");
    assert!(cookie.serialize("hello world").contains("Path=/"));
}

#[test]
fn defaults_same_site_to_lax() {
    let cookie = Cookie::new("my-cookie");
    assert_eq!(cookie.get_same_site(), SameSite::Lax);
    assert!(cookie.serialize("hello world").contains("SameSite=Lax"));
}

/// Remix normalizes lowercase strings; here `SameSite` is an enum, so each value serializes.
#[test]
fn serializes_each_same_site_value() {
    let cookie = Cookie::new("my-cookie").same_site(SameSite::Lax);
    assert_eq!(cookie.get_same_site(), SameSite::Lax);
    assert!(cookie.serialize("hello world").contains("SameSite=Lax"));
    let cookie = Cookie::new("my-cookie").same_site(SameSite::Strict);
    assert_eq!(cookie.get_same_site(), SameSite::Strict);
    assert!(cookie.serialize("hello world").contains("SameSite=Strict"));
    let cookie = Cookie::new("my-cookie").same_site(SameSite::None).secure(true);
    assert_eq!(cookie.get_same_site(), SameSite::None);
    assert!(cookie.serialize("hello world").contains("SameSite=None"));
}

#[test]
fn exposes_same_site_values_compatible_with_set_cookie() {
    let cookie = Cookie::new("my-cookie").same_site(SameSite::Lax);
    let header = SetCookie { name: cookie.name().into(), value: "value".into(), same_site: Some(cookie.get_same_site()), ..Default::default() };
    assert_eq!(header.to_string(), "my-cookie=value; SameSite=Lax");
}

#[test]
fn defaults_secure_to_true_when_partitioned_is_true() {
    let cookie = Cookie::new("my-cookie").partitioned(true);
    assert_eq!(cookie.get_secure(), Some(true));
    let set = SetCookie::parse(&cookie.secure(false).serialize("v")).unwrap();
    assert!(set.secure && set.partitioned, "partitioned wins over secure(false)");
}

#[test]
fn parses_and_serializes_empty_string_values() {
    let cookie = Cookie::new("my-cookie");
    let set_cookie = cookie.serialize("");
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))), Some(String::new()));
}

#[test]
fn parses_and_serializes_unsigned_string_values() {
    let cookie = Cookie::new("my-cookie");
    let set_cookie = cookie.serialize("hello world");
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("hello world"));
}

#[test]
fn encodes_values_as_base64_of_utf8_like_remix() {
    assert_eq!(set_cookie_value(&Cookie::new("c").serialize("hello world")), "aGVsbG8gd29ybGQ=");
    assert_eq!(set_cookie_value(&Cookie::new("c").serialize("\u{65e5}\u{672c}\u{8a9e}")), "5pel5pys6Kqe");
    assert_eq!(Cookie::new("c").parse(Some("c=not-base64!")), None);
    assert_eq!(Cookie::new("c").parse(Some("c=/w==")), None, "invalid UTF-8");
}

#[test]
fn parses_and_serializes_signed_string_values() {
    let cookie = Cookie::new("my-cookie").secrets(vec!["secret1".into()]);
    let set_cookie = cookie.serialize("hello michael");
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("hello michael"));
}

#[test]
fn uses_custom_encode_decode_functions_as_the_unsigned_cookie_value_codec() {
    let cookie = Cookie::new("my-cookie").encode(|v| v.replace(' ', "-")).decode(|v| Some(v.replace('-', " ")));
    let set_cookie = cookie.serialize("hello michael");
    assert_eq!(set_cookie_value(&set_cookie), "hello-michael");
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("hello michael"));
}

#[test]
fn signs_custom_encoded_values_without_wrapping_them_in_base64() {
    let cookie = Cookie::new("my-cookie")
        .secrets(vec!["secret1".into()])
        .encode(|v| format!("debug.{}", v.replace(' ', "-")))
        .decode(|v| Some(v.strip_prefix("debug.").unwrap_or(v).replace('-', " ")));
    let set_cookie = cookie.serialize("hello michael");
    assert!(set_cookie_value(&set_cookie).starts_with("debug.hello-michael."));
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("hello michael"));
}

#[test]
fn unsigns_custom_encoded_values_that_contain_periods() {
    let codec = |secret: &str| {
        Cookie::new("my-cookie")
            .secrets(vec![secret.into()])
            .encode(|v| format!("debug.{}.v1", v.replace(' ', ".")))
            .decode(|v| {
                let v = v.strip_prefix("debug.").unwrap_or(v);
                Some(v.strip_suffix(".v1").unwrap_or(v).replace('.', " "))
            })
    };
    let cookie = codec("secret1");
    let set_cookie = cookie.serialize("hello michael");
    assert!(set_cookie_value(&set_cookie).starts_with("debug.hello.michael.v1."));
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("hello michael"));
    assert_eq!(codec("secret2").parse(Some(&cookie_from_set_cookie(&set_cookie))), None);
}

#[test]
fn parses_and_serializes_string_values_containing_utf8_characters() {
    let cookie = Cookie::new("my-cookie");
    let set_cookie = cookie.serialize("\u{65e5}\u{672c}\u{8a9e}");
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some("\u{65e5}\u{672c}\u{8a9e}"));
}

#[test]
fn fails_to_parse_signed_string_values_with_invalid_signature() {
    let set_cookie = Cookie::new("my-cookie").secrets(vec!["secret1".into()]).serialize("hello michael");
    let other = Cookie::new("my-cookie").secrets(vec!["secret2".into()]);
    assert_eq!(other.parse(Some(&cookie_from_set_cookie(&set_cookie))), None);
}

#[test]
fn fails_to_parse_signed_string_values_with_invalid_signature_encoding() {
    let set_cookie = Cookie::new("my-cookie").secrets(vec!["secret1".into()]).serialize("hello michael");
    let other = Cookie::new("my-cookie").secrets(vec!["secret2".into()]);
    assert_eq!(other.parse(Some(&(cookie_from_set_cookie(&set_cookie) + "%^&"))), None);
}

#[test]
fn parses_and_serializes_signed_object_values() {
    let cookie = Cookie::new("my-cookie").secrets(vec!["secret1".into()]);
    let set_cookie = cookie.serialize(r#"{"hello":"mjackson"}"#);
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).as_deref(), Some(r#"{"hello":"mjackson"}"#));
}

#[test]
fn supports_secret_rotation() {
    let cookie = Cookie::new("my-cookie").secrets(vec!["secret1".into()]);
    let set_cookie = cookie.serialize("mjackson");
    let value = cookie.parse(Some(&cookie_from_set_cookie(&set_cookie))).unwrap();
    assert_eq!(value, "mjackson");

    // A new secret enters the rotation...
    let cookie = Cookie::new("my-cookie").secrets(vec!["secret2".into(), "secret1".into()]);
    // ...old cookies still parse...
    let old_value = cookie.parse(Some(&cookie_from_set_cookie(&set_cookie)));
    assert_eq!(old_value.as_deref(), Some(value.as_str()));
    // ...and new ones are signed with the new secret.
    let set_cookie2 = cookie.serialize(&value);
    assert_ne!(set_cookie, set_cookie2);
    assert_eq!(cookie.parse(Some(&cookie_from_set_cookie(&set_cookie2))), old_value);
}

#[test]
fn is_not_signed_by_default() {
    assert!(!Cookie::new("my-cookie").is_signed());
    assert!(!Cookie::new("my-cookie2").secrets(Vec::new()).is_signed());
}

#[test]
fn supports_overriding_cookie_properties_in_the_constructor() {
    let cookie = Cookie::new("my-cookie").domain("remix.run").path("/about").max_age(3600).same_site(SameSite::None).secure(true).http_only(true);
    let set_cookie = cookie.serialize("hello world");
    for part in ["Domain=remix.run", "Path=/about", "Max-Age=3600", "SameSite=None", "Secure", "HttpOnly"] {
        assert!(set_cookie.contains(part), "{part} in {set_cookie}");
    }
}

#[test]
fn supports_overriding_cookie_properties_in_the_serialize_method() {
    let props = CookieProperties {
        domain: Some("remix.run".into()),
        path: Some("/about".into()),
        max_age: Some(3600),
        same_site: Some(SameSite::None),
        secure: Some(true),
        http_only: Some(true),
        ..Default::default()
    };
    let set_cookie = Cookie::new("my-cookie").serialize_with("hello world", &props);
    for part in ["Domain=remix.run", "Path=/about", "Max-Age=3600", "SameSite=None", "Secure", "HttpOnly"] {
        assert!(set_cookie.contains(part), "{part} in {set_cookie}");
    }
}

#[test]
fn serializes_expires() {
    let at = UNIX_EPOCH + Duration::from_secs(784_111_777);
    let cookie = Cookie::new("my-cookie").expires(at);
    assert_eq!(cookie.get_expires(), Some(at));
    assert!(cookie.serialize("v").contains("Expires=Sun, 06 Nov 1994 08:49:37 GMT"));
    let cleared = cookie.serialize_with("", &CookieProperties { expires: Some(UNIX_EPOCH), max_age: Some(0), ..Default::default() });
    assert_eq!(cleared, "my-cookie=; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax");
}

#[test]
fn parses_empty_values_as_null() {
    assert_eq!(Cookie::new("my-cookie").parse(None), None);
    assert_eq!(Cookie::new("my-cookie").parse(Some("")), None);
}

#[test]
fn parses_missing_cookies_as_null() {
    assert_eq!(Cookie::new("my-cookie").parse(Some("other-cookie=hello")), None);
}

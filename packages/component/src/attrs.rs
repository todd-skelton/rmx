//! Attribute rules shared by the server renderer and the browser runtime: Remix's
//! `isAllowedHostPropName`, `sanitizeUrlAttribute` and booleanish string attributes.

use std::borrow::Cow;

use crate::AttrValue;

/// What a `javascript:` URL is replaced with.
pub(crate) const BLOCKED_JAVASCRIPT_URL: &str =
    "javascript:throw new Error('rsmix has blocked a javascript: URL as a security precaution.')";

/// Whether an attribute name may be written. Inline event handler attributes (`on*`),
/// names that could break out of the tag, `__proto__` and `outerHTML` are dropped.
pub(crate) fn is_allowed_name(name: &str) -> bool {
    !name.is_empty()
        && !name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("on"))
        && !name.bytes().any(|b| matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'"' | b'\'' | b'/' | b'>' | b'='))
        && name != "__proto__"
        && name != "outerHTML"
}

/// Remix's `unsafeHTML` rules for host props: `innerHTML`, `srcdoc` and `srcDoc` take only
/// values from [`unsafe_html`](crate::unsafe_html), and `outerHTML` is never allowed. Checked
/// when a node is built, so a bad prop never reaches the DOM.
pub(crate) fn check_html_props(attrs: &[(&'static str, AttrValue)]) {
    for (name, value) in attrs {
        match *name {
            "innerHTML" | "srcdoc" | "srcDoc" if matches!(value, AttrValue::Str(_) | AttrValue::Bool(true)) => {
                invalid_html_prop(name)
            }
            "outerHTML" => panic!("Invalid outerHTML prop: outerHTML is not supported on host elements"),
            _ => {}
        }
    }
}

/// Whether `name` holds raw HTML (`innerHTML`, `srcdoc`, `srcDoc`).
pub(crate) fn is_html_prop(name: &str) -> bool {
    matches!(name, "innerHTML" | "srcdoc" | "srcDoc")
}

#[cold]
#[track_caller]
pub(crate) fn invalid_html_prop(name: &str) -> ! {
    panic!("Invalid {name} prop: expected a value created with unsafe_html()")
}

/// SVG and HTML attributes whose `false` is the string `"false"`, not an absent attribute.
pub(crate) fn is_booleanish_string(name: &str) -> bool {
    matches!(
        name,
        "autoReverse" | "contenteditable" | "draggable" | "externalResourcesRequired" | "focusable" | "preserveAlpha" | "spellcheck"
    )
}

/// Replace a `javascript:` URL in an attribute that browsers navigate to or load.
pub(crate) fn sanitize_url<'a>(tag: &str, name: &str, value: &'a str) -> Cow<'a, str> {
    let executable = matches!(name, "href" | "src" | "action" | "formaction" | "xlink:href")
        || (name == "data" && tag.eq_ignore_ascii_case("object"));
    if executable && is_javascript_url(value) {
        Cow::Borrowed(BLOCKED_JAVASCRIPT_URL)
    } else {
        Cow::Borrowed(value)
    }
}

/// Browsers ignore leading C0 controls and spaces, and tabs and newlines inside the
/// scheme, so `" \tjava\nscript:"` is still a `javascript:` URL.
fn is_javascript_url(value: &str) -> bool {
    let mut bytes = value.bytes().skip_while(|b| *b <= 0x20).filter(|b| !matches!(b, b'\t' | b'\n' | b'\r'));
    "javascript:".bytes().all(|want| bytes.next().is_some_and(|b| b.eq_ignore_ascii_case(&want)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_javascript_urls_in_executable_attributes() {
        for url in ["javascript:alert(1)", "JavaScript:alert(1)", " \u{1}java\tscript:alert(1)", "\njav\nascript:x"] {
            assert_eq!(sanitize_url("a", "href", url), BLOCKED_JAVASCRIPT_URL, "{url:?}");
        }
        assert_eq!(sanitize_url("object", "data", "javascript:x"), BLOCKED_JAVASCRIPT_URL);
        assert_eq!(sanitize_url("form", "formaction", "javascript:x"), BLOCKED_JAVASCRIPT_URL);
        assert_eq!(sanitize_url("a", "href", "/javascript:x"), "/javascript:x");
        assert_eq!(sanitize_url("a", "href", "https://example.com"), "https://example.com");
        assert_eq!(sanitize_url("a", "title", "javascript:x"), "javascript:x");
        assert_eq!(sanitize_url("div", "data", "javascript:x"), "javascript:x");
    }

    #[test]
    fn checks_raw_html_props() {
        check_html_props(&[("innerHTML", AttrValue::Html("<b>x</b>".into())), ("srcdoc", AttrValue::Absent), ("title", AttrValue::Str("t".into()))]);
        for name in ["innerHTML", "srcdoc", "srcDoc"] {
            let err = std::panic::catch_unwind(|| check_html_props(&[(name, AttrValue::Str("<img>".into()))])).unwrap_err();
            let msg = err.downcast_ref::<String>().unwrap();
            assert_eq!(msg, &format!("Invalid {name} prop: expected a value created with unsafe_html()"));
        }
        let err = std::panic::catch_unwind(|| check_html_props(&[("outerHTML", AttrValue::Html("<p>".into()))])).unwrap_err();
        assert_eq!(err.downcast_ref::<&str>(), Some(&"Invalid outerHTML prop: outerHTML is not supported on host elements"));
    }

    // Ports of Remix's `attributes.test.ts` (`isAllowedHostPropName`).
    #[test]
    fn blocks_event_prop_names_in_every_ascii_letter_case() {
        for name in ["onclick", "onClick", "OnClick", "oNclick", "ONCLICK", "on"] {
            assert!(!is_allowed_name(name), "{name}");
        }
    }

    #[test]
    fn keeps_event_names_inside_ordinary_attribute_names() {
        for name in ["data-onclick", "aria-controls", "nonce", "className"] {
            assert!(is_allowed_name(name), "{name}");
        }
    }

    #[test]
    fn blocks_delimiters_and_reserved_prop_names() {
        for name in ["", "__proto__", "outerHTML"] {
            assert!(!is_allowed_name(name), "{name:?}");
        }
        for c in ['\u{0}', '\t', '\n', '\u{c}', '\r', ' ', '"', '\'', '/', '>', '='] {
            let name = format!("data{c}value");
            assert!(!is_allowed_name(&name), "{name:?}");
        }
    }

    #[test]
    fn filters_unsafe_attribute_names() {
        for ok in ["href", "data-on", "aria-label", "xlink:href", "class"] {
            assert!(is_allowed_name(ok), "{ok}");
        }
        for bad in ["onclick", "ONload", "a b", "x=y", "a\"", "__proto__", "outerHTML", ""] {
            assert!(!is_allowed_name(bad), "{bad:?}");
        }
    }
}

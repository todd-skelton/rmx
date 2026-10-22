//! Reusable checks for [`Schema::pipe`](crate::Schema::pipe) (`remix/data-schema/checks`).

use crate::{Check, Value};

/// JavaScript's `string.length` (UTF-16 code units).
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// At least `length` characters (`string.min_length`).
pub fn min_length(length: usize) -> Check<String> {
    Check::new(move |s: &String| js_len(s) >= length)
        .code("string.min_length")
        .value("min", length)
        .message(format!("Expected at least {length} characters"))
}

/// At most `length` characters (`string.max_length`).
pub fn max_length(length: usize) -> Check<String> {
    Check::new(move |s: &String| js_len(s) <= length)
        .code("string.max_length")
        .value("max", length)
        .message(format!("Expected at most {length} characters"))
}

/// Remix's `/^[^\s@]+@[^\s@]+\.[^\s@]+$/`.
fn is_email(s: &str) -> bool {
    if s.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else { return false };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    domain.char_indices().any(|(i, c)| c == '.' && i > 0 && i + 1 < domain.len())
}

/// An email-like string (`string.email`).
pub fn email() -> Check<String> {
    Check::new(|s: &String| is_email(s)).code("string.email").message("Expected valid email")
}

/// A URL with a scheme. Web schemes (`http`, `https`, `ws`, `wss`, `ftp`, `file`) must
/// also have a host, as `new URL()` requires.
fn is_url(s: &str) -> bool {
    let s = s.trim();
    let Some((scheme, _)) = s.split_once(':') else { return false };
    let valid_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !valid_scheme {
        return false;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" | "ws" | "wss" | "ftp" => rmx_url::Url::parse(s).is_ok_and(|u| !u.host().is_empty()),
        "file" => s[5..].starts_with("//"),
        _ => true,
    }
}

/// A valid URL (`string.url`).
pub fn url() -> Check<String> {
    Check::new(|s: &String| is_url(s)).code("string.url").message("Expected valid URL")
}

/// At least `limit` (`number.min`).
pub fn min(limit: f64) -> Check<f64> {
    Check::new(move |n: &f64| *n >= limit)
        .code("number.min")
        .value("min", Value::Number(limit))
        .message(format!("Expected number greater than or equal to {}", crate::js_number(limit)))
}

/// At most `limit` (`number.max`).
pub fn max(limit: f64) -> Check<f64> {
    Check::new(move |n: &f64| *n <= limit)
        .code("number.max")
        .value("max", Value::Number(limit))
        .message(format!("Expected number less than or equal to {}", crate::js_number(limit)))
}

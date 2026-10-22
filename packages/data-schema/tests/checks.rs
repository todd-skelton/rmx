//! Port of data-schema's checks.test.ts, one test per case.

use rmx_data_schema::checks::{email, max, max_length, min, min_length, url};
use rmx_data_schema::{self as s, Schema, Value};

#[test]
fn exposes_code_and_values_for_message_mapping() {
    let min_length_check = min_length(2);
    let max_length_check = max_length(4);
    assert_eq!(min_length_check.code, Some("string.min_length"));
    assert_eq!(min_length_check.values, [("min", Value::from(2))]);
    assert_eq!(max_length_check.code, Some("string.max_length"));
    assert_eq!(max_length_check.values, [("max", Value::from(4))]);
}

#[test]
fn supports_common_string_checks() {
    let schema = s::string().pipe(min_length(2)).pipe(max_length(4));
    assert!(s::parse(&schema, "test").is_ok());
    assert!(s::parse(&schema, "a").is_err());
    assert!(s::parse(&schema, "toolong").is_err());
}

#[test]
fn supports_email_checks() {
    let schema = s::string().pipe(email());
    assert!(s::parse(&schema, "user@example.com").is_ok());
    assert!(s::parse(&schema, "not-an-email").is_err());
}

#[test]
fn supports_url_checks() {
    let schema = s::string().pipe(url());
    assert!(s::parse(&schema, "https://example.com").is_ok());
    assert!(s::parse(&schema, "not-a-url").is_err());
}

#[test]
fn supports_number_checks() {
    let schema = s::number().pipe(min(3.0)).pipe(max(5.0));
    assert!(s::parse(&schema, 4).is_ok());
    assert!(s::parse(&schema, 2).is_err());
    assert!(s::parse(&schema, 6).is_err());
}

//! rmx-only cases. The Remix test files are ported one per case in `tests/` (schema,
//! parse, checks, coerce, pipe, variant, lazy and form_data).

use crate::checks::{email, max, min, url};
use crate::{self as s, coerce, Issue, PathSegment, Schema, Value};

fn messages<S: Schema>(schema: &S, value: impl Into<Value>) -> Vec<String> {
    s::parse(schema, value).err().expect("validation should fail").issues.into_iter().map(|i| i.message).collect()
}

fn paths<S: Schema>(schema: &S, value: impl Into<Value>) -> Vec<String> {
    s::parse(schema, value).err().expect("validation should fail").issues.iter().map(ToString::to_string).collect()
}

#[test]
fn rejects_malformed_emails() {
    let schema = s::string().pipe(email());
    for bad in ["a@b", "a b@c.d", "@c.d", "a@.d", "a@c."] {
        assert!(s::parse(&schema, bad).is_err(), "{bad}");
    }
}

#[test]
fn accepts_any_absolute_url_with_a_scheme() {
    let schema = s::string().pipe(url());
    assert!(s::parse(&schema, "mailto:ada@example.com").is_ok());
    assert!(s::parse(&schema, "https://").is_err());
}

#[test]
fn number_checks_name_their_limit() {
    let schema = s::number().pipe(min(3.0)).pipe(max(5.0));
    assert_eq!(messages(&schema, 2), ["Expected number greater than or equal to 3"]);
    assert_eq!(messages(&schema, 6), ["Expected number less than or equal to 5"]);
}

#[test]
fn coerces_numbers_with_javascript_number_syntax() {
    assert_eq!(s::parse(&coerce::number(), "1e3").unwrap(), 1000.0);
    assert_eq!(s::parse(&coerce::number(), "0x10").unwrap(), 16.0);
    for bad in ["", "abc", "inf", "NaN"] {
        assert_eq!(messages(&coerce::number(), bad), ["Expected number"], "{bad:?}");
    }
    assert!(s::parse(&coerce::number(), true).is_err());
}

#[test]
fn refine_without_a_message_reports_refinement_failed() {
    assert_eq!(messages(&s::number().refine(|n| *n > 0.0), -1), ["Refinement failed"]);
}

#[test]
fn issues_display_with_their_path() {
    let issue = Issue::new("Expected number", vec![PathSegment::from("items"), PathSegment::from(2), PathSegment::from("qty")]);
    assert_eq!(issue.to_string(), "items.2.qty: Expected number");
    assert_eq!(Issue::new("Expected object", Vec::new()).to_string(), "Expected object");
}

#[test]
fn tuples_report_every_item_and_their_length() {
    let schema = s::tuple((s::string(), s::number()));
    assert_eq!(paths(&schema, vec![Value::from(1), Value::from("x")]), ["0: Expected string", "1: Expected number"]);
    assert_eq!(messages(&schema, vec![Value::from("a")]), ["Expected tuple length 2"]);
}

#[test]
fn unions_of_objects_report_each_options_paths() {
    let schema = s::union([s::object(|o| o.field("a", s::string())).boxed(), s::object(|o| o.field("b", s::string())).boxed()]);
    assert_eq!(paths(&schema, Value::object(Vec::<(String, Value)>::new())), ["a: Expected string", "b: Expected string"]);
}

#[test]
fn keys_kept_by_rest_are_not_unknown_to_strict() {
    let schema = s::object(|o| (o.field("name", s::string()), o.rest())).strict();
    let value = Value::object([("name", "Ada"), ("extra", "x")]);
    assert_eq!(s::parse(&schema, value).unwrap().1, [("extra".to_string(), Value::from("x"))]);
}

#[test]
fn enum_and_file_messages() {
    assert_eq!(messages(&s::enum_(["admin", "member"]), "guest"), ["Expected one of: admin, member"]);
    assert_eq!(messages(&s::file(), "a.txt"), ["Expected instance of File"]);
}

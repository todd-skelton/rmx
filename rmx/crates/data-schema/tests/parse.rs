//! Port of data-schema's parse.test.ts, one test per case. `parse` returns a `Result`, so
//! Remix's `parse` (throws) and `parseSafe` (returns a result) cases both use it.

use std::sync::{Arc, Mutex};

use rmx_data_schema::checks::min_length;
use rmx_data_schema::{self as s, ParseOptions, PathSegment, Schema, ValidationError, Value};

#[derive(Debug, Default, PartialEq)]
struct Person {
    name: String,
    age: f64,
}

fn person() -> impl Schema<Output = Person> {
    s::object(|o| Person { name: o.field("name", s::string()), age: o.field("age", s::number()) })
}

fn ada() -> Value {
    Value::object([("name", Value::from("Ada")), ("age", Value::from(37))])
}

fn invalid() -> Value {
    Value::object([("name", Value::from(123)), ("age", Value::from("x"))])
}

fn keys(path: &[PathSegment]) -> Vec<String> {
    path.iter().map(ToString::to_string).collect()
}

// parse

#[test]
fn returns_validated_output() {
    assert_eq!(s::parse(&person(), ada()).unwrap(), Person { name: "Ada".into(), age: 37.0 });
}

#[test]
fn throws_validation_error_with_issues() {
    let error: ValidationError = s::parse(&person(), invalid()).unwrap_err();
    let as_error: &dyn std::error::Error = &error;
    assert_eq!(as_error.to_string(), "Validation failed");
    assert!(!error.issues.is_empty());
}

// parseSafe

#[test]
fn returns_success_with_value() {
    let result = s::parse(&person(), ada());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Person { name: "Ada".into(), age: 37.0 });
}

#[test]
fn returns_issues_on_failure() {
    assert_eq!(s::parse(&person(), invalid()).unwrap_err().issues.len(), 2);
}

#[test]
fn supports_abort_early_option() {
    let options = ParseOptions { abort_early: true, ..ParseOptions::default() };
    assert_eq!(s::parse_with(&person(), invalid(), &options).unwrap_err().issues.len(), 1);
}

#[test]
fn supports_error_map_for_custom_issue_messages() {
    let options = ParseOptions::with_error_map(|cx| (cx.code == "type.string").then(|| "Expected text input".to_string()));
    let issues = s::parse_with(&person(), invalid(), &options).unwrap_err().issues;
    assert_eq!(issues[0].message, "Expected text input");
    assert_eq!(issues[1].message, "Expected number");
}

#[test]
fn passes_locale_and_values_to_error_map() {
    let captured = Arc::new(Mutex::new(None));
    let seen = captured.clone();
    let mut options = ParseOptions::with_error_map(move |cx| {
        *seen.lock().unwrap() = Some((cx.code.to_string(), cx.locale.map(String::from), cx.values.to_vec()));
        (cx.code == "string.min_length").then(|| format!("Debe tener al menos {} caracteres", cx.values[0].1))
    });
    options.locale = Some("es".into());
    let issues = s::parse_with(&s::string().pipe(min_length(3)), "ab", &options).unwrap_err().issues;
    assert_eq!(issues[0].message, "Debe tener al menos 3 caracteres");
    assert_eq!(*captured.lock().unwrap(), Some(("string.min_length".into(), Some("es".into()), vec![("min", Value::from(3))])));
}

#[derive(Debug, Default)]
struct User {
    _name: String,
    _nickname: String,
    _age: f64,
}

#[test]
fn preserves_paths_in_retained_error_map_contexts() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let seen = captured.clone();
    let options = ParseOptions::with_error_map(move |cx| {
        seen.lock().unwrap().push(keys(cx.path));
        None
    });
    let schema = s::object(|o| {
        o.field(
            "user",
            s::object(|u| User {
                _name: u.field("name", s::string()),
                _nickname: u.field("nickname", s::string().pipe(min_length(3))),
                _age: u.field("age", s::number().refine(|age| *age >= 0.0)),
            }),
        )
    });
    let value = Value::object([("user", Value::object([("name", Value::from(123)), ("nickname", Value::from("a")), ("age", Value::from(-1))]))]);
    let issues = s::parse_with(&schema, value, &options).unwrap_err().issues;
    let expected = [vec!["user", "name"], vec!["user", "nickname"], vec!["user", "age"]];
    assert_eq!(*captured.lock().unwrap(), expected);
    assert_eq!(issues.iter().map(|i| keys(&i.path)).collect::<Vec<_>>(), expected);
}

#[test]
fn falls_back_to_default_message_when_error_map_returns_undefined() {
    let options = ParseOptions::with_error_map(|_| None);
    assert_eq!(s::parse_with(&s::number(), "x", &options).unwrap_err().issues[0].message, "Expected number");
}

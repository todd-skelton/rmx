//! Port of data-schema's coerce.test.ts, one test per case (16 of 22).
//!
//! Not ported: the four `coerceDate` cases (rmx has no date type), "passes through bigint
//! values" and "coerces symbols to strings" (JavaScript-only values). `coerce::integer` is
//! Remix's `coerceBigint` with an `i64` output.

use rmx_data_schema::{self as s, coerce, Schema, Value};

fn ok<S: Schema>(schema: &S, value: impl Into<Value>) -> S::Output {
    s::parse(schema, value).unwrap()
}

fn fails<S: Schema>(schema: &S, value: impl Into<Value>) -> bool {
    s::parse(schema, value).is_err()
}

#[test]
fn coerces_numbers_from_strings() {
    let schema = coerce::number();
    assert_eq!(ok(&schema, "42"), 42.0);
    assert_eq!(ok(&schema, " 3.5 "), 3.5);
    assert!(fails(&schema, "nope"));
}

#[test]
fn coerces_numbers_and_passes_through_valid_numbers() {
    // Remix uses 3.14; any non-integer works, and clippy reads 3.14 as an approximate PI.
    assert_eq!(ok(&coerce::number(), 3.25), 3.25);
}

#[test]
fn rejects_infinity_and_nan_for_coerced_numbers() {
    let schema = coerce::number();
    assert!(fails(&schema, f64::INFINITY));
    assert!(fails(&schema, f64::NEG_INFINITY));
    assert!(fails(&schema, f64::NAN));
    assert!(fails(&schema, "Infinity"));
    assert!(fails(&schema, "-Infinity"));
}

#[test]
fn rejects_empty_strings_for_coerced_numbers() {
    assert!(fails(&coerce::number(), "   "));
}

#[test]
fn coerces_booleans_from_strings() {
    let schema = coerce::boolean();
    assert!(ok(&schema, "true"));
    assert!(!ok(&schema, "FALSE"));
    assert!(fails(&schema, "yes"));
}

#[test]
fn passes_through_boolean_values() {
    let schema = coerce::boolean();
    assert!(ok(&schema, true));
    assert!(!ok(&schema, false));
}

#[test]
fn trims_whitespace_for_boolean_strings() {
    assert!(ok(&coerce::boolean(), "  TRUE  "));
}

#[test]
fn rejects_non_boolean_non_string_values_for_coerce_boolean() {
    let schema = coerce::boolean();
    assert!(fails(&schema, 1));
    assert!(fails(&schema, Value::Object(Vec::new())));
}

#[test]
fn coerces_bigint_from_strings_and_integers() {
    let schema = coerce::integer();
    assert_eq!(ok(&schema, "9007199254740993"), 9_007_199_254_740_993);
    assert_eq!(ok(&schema, 10), 10);
    assert!(fails(&schema, 1.5));
}

#[test]
fn rejects_empty_strings_for_coerce_bigint() {
    assert!(fails(&coerce::integer(), "   "));
}

#[test]
fn rejects_invalid_bigint_strings() {
    assert!(fails(&coerce::integer(), "12.5"));
}

#[test]
fn rejects_infinity_and_nan_for_coerce_bigint() {
    let schema = coerce::integer();
    assert!(fails(&schema, f64::INFINITY));
    assert!(fails(&schema, f64::NAN));
}

#[test]
fn coerces_strings_from_primitives() {
    let schema = coerce::string();
    assert_eq!(ok(&schema, 5), "5");
    assert_eq!(ok(&schema, false), "false");
    assert!(fails(&schema, Value::Object(Vec::new())));
}

#[test]
fn passes_through_string_values() {
    assert_eq!(ok(&coerce::string(), "hello"), "hello");
}

#[test]
fn rejects_null_and_undefined_for_coerce_string() {
    let schema = coerce::string();
    assert!(fails(&schema, Value::Null));
    assert!(fails(&schema, Value::Undefined));
}

#[test]
fn rejects_arrays_and_objects_for_coerce_string() {
    let schema = coerce::string();
    assert!(fails(&schema, vec![1, 2]));
    assert!(fails(&schema, Value::object([("a", 1)])));
}

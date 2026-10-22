//! Port of data-schema's schema.test.ts, one test per case (74 of 92).
//!
//! Not ported, as JavaScript-only values with no `Value` counterpart: `bigint` and
//! `symbol` (2), sparse arrays and custom array iterators (2), `map` (6), `set` (6), and
//! `instanceof_` with custom classes and subclasses (2). `instanceof_(File)` is `file()`.
//! A union's options share one output type in Rust, so the union cases map each option into
//! an enum.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rmx_data_schema::checks::min_length;
use rmx_data_schema::{self as s, Check, File, Issue, ParseOptions, PathSegment, Schema, Value};

fn ok<S: Schema>(schema: &S, value: impl Into<Value>) -> S::Output {
    s::parse(schema, value).unwrap()
}

fn issues<S: Schema>(schema: &S, value: impl Into<Value>) -> Vec<Issue> {
    s::parse(schema, value).err().expect("validation should fail").issues
}

fn abort_early() -> ParseOptions {
    ParseOptions { abort_early: true, ..ParseOptions::default() }
}

fn early_issues<S: Schema>(schema: &S, value: impl Into<Value>) -> Vec<Issue> {
    s::parse_with(schema, value, &abort_early()).err().expect("validation should fail").issues
}

fn path(segments: &[PathSegment]) -> Vec<String> {
    segments.iter().map(ToString::to_string).collect()
}

fn obj<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::object(entries)
}

fn arr<T: Into<Value>, const N: usize>(items: [T; N]) -> Value {
    Value::Array(items.into_iter().map(Into::into).collect())
}

// ---------------------------------------------------------------------------------------
// primitives
// ---------------------------------------------------------------------------------------

#[test]
fn validates_strings() {
    assert_eq!(ok(&s::string(), "ok"), "ok");
}

#[test]
fn rejects_non_strings() {
    let found = issues(&s::string(), 123);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "Expected string");
}

#[test]
fn validates_numbers_and_rejects_nan() {
    assert_eq!(ok(&s::number(), 42), 42.0);
    assert!(s::parse(&s::number(), f64::NAN).is_err());
}

#[test]
fn validates_booleans() {
    assert!(ok(&s::boolean(), true));
}

#[test]
fn validates_null_and_undefined() {
    ok(&s::null_(), Value::Null);
    ok(&s::undefined_(), Value::Undefined);
}

#[test]
fn validates_literals() {
    assert_eq!(ok(&s::literal("yes"), "yes"), "yes");
    assert!(s::parse(&s::literal("yes"), "no").is_err());
}

#[test]
fn rejects_infinity_and_negative_infinity_for_numbers() {
    assert!(s::parse(&s::number(), f64::INFINITY).is_err());
    assert!(s::parse(&s::number(), f64::NEG_INFINITY).is_err());
}

#[test]
fn rejects_non_booleans() {
    assert_eq!(issues(&s::boolean(), "true")[0].message, "Expected boolean");
}

#[test]
fn rejects_non_null_for_null_schema() {
    assert_eq!(issues(&s::null_(), Value::Undefined)[0].message, "Expected null");
}

#[test]
fn rejects_non_undefined_for_undefined_schema() {
    assert_eq!(issues(&s::undefined_(), Value::Null)[0].message, "Expected undefined");
}

// ---------------------------------------------------------------------------------------
// array
// ---------------------------------------------------------------------------------------

#[test]
fn validates_array_elements_and_provides_paths() {
    let found = issues(&s::array(s::string()), arr([Value::from("ok"), Value::from(123)]));
    assert_eq!(found[0].path, [PathSegment::Index(1)]);
}

#[test]
fn returns_validated_array_on_success() {
    assert_eq!(ok(&s::array(s::number()), arr([1, 2, 3])), [1.0, 2.0, 3.0]);
}

#[test]
fn preserves_explicit_undefined_elements() {
    // Remix's sparse-input case; a Rust array has no holes, so the element is an explicit
    // undefined.
    assert_eq!(ok(&s::array(s::optional(s::string())), arr([Value::Undefined])), [None]);
}

#[test]
fn rejects_non_array_values() {
    let array_like = obj([("0", "a".into()), ("length", 1.into())]);
    assert_eq!(issues(&s::array(s::string()), array_like)[0].message, "Expected array");
}

#[test]
fn array_collects_all_issues_by_default() {
    assert_eq!(issues(&s::array(s::number()), arr(["a", "b", "c"])).len(), 3);
}

#[test]
fn array_returns_first_issue_when_abort_early_is_enabled() {
    let found = early_issues(&s::array(s::number()), arr(["a", "b", "c"]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, [PathSegment::Index(0)]);
}

// ---------------------------------------------------------------------------------------
// object
// ---------------------------------------------------------------------------------------

#[test]
fn preserves_paths_passed_to_custom_child_schemas() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let seen = captured.clone();
    let custom = s::create_schema(move |value, cx| {
        *seen.lock().unwrap() = cx.path().to_vec();
        match value {
            Value::String(v) => Ok(v.clone()),
            _ => Err(vec![cx.plain_issue("Expected string")]),
        }
    });
    let schema = s::object(|o| o.field("name", &custom));
    assert_eq!(ok(&schema, obj([("name", "Ada".into())])), "Ada");
    assert_eq!(path(&captured.lock().unwrap()), ["name"]);
}

#[test]
fn preserves_issue_paths_returned_by_custom_child_schemas() {
    let custom = s::create_schema(|_, cx| Err::<(), _>(vec![Issue::new("Invalid", cx.path().to_vec())]));
    let schema = s::object(|o| o.field("name", &custom));
    assert_eq!(path(&issues(&schema, obj([("name", "Ada".into())]))[0].path), ["name"]);
}

#[derive(Debug, Default, PartialEq)]
struct Paths {
    optional: Vec<String>,
    nullable: Vec<String>,
    defaulted: Vec<String>,
    union: Vec<String>,
    variant: Vec<String>,
    derived: Vec<String>,
}

#[test]
fn preserves_custom_paths_through_schema_wrappers() {
    let custom = || s::create_schema(|_, cx| Ok(path(cx.path())));
    let schema = s::object(|o| Paths {
        optional: o.field("optional", s::optional(custom())).unwrap_or_default(),
        nullable: o.field("nullable", s::nullable(custom())).unwrap_or_default(),
        defaulted: o.field("defaulted", s::defaulted(custom(), Vec::new())),
        union: o.field("union", s::union([s::string().transform(|v| vec![v]).boxed(), custom().boxed()])),
        variant: o.field("variant", s::variant("type", [("custom", custom().boxed())])),
        derived: o.field("derived", s::optional(custom()).pipe(Check::new(|_| true)).refine(|_| true).transform(|v| v)).unwrap_or_default(),
    });
    let value = obj([
        ("optional", 1.into()),
        ("nullable", 1.into()),
        ("defaulted", 1.into()),
        ("union", 1.into()),
        ("variant", obj([("type", "custom".into())])),
        ("derived", 1.into()),
    ]);
    let p = |key: &str| vec![key.to_string()];
    assert_eq!(
        ok(&schema, value),
        Paths { optional: p("optional"), nullable: p("nullable"), defaulted: p("defaulted"), union: p("union"), variant: p("variant"), derived: p("derived") }
    );
}

#[test]
fn preserves_issues_retained_by_wrapped_custom_schemas() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let custom = || {
        let captured = captured.clone();
        s::create_schema(move |_, cx| {
            let issue = Issue::new("Invalid", cx.path().to_vec());
            captured.lock().unwrap().push(issue.clone());
            Err::<(), _>(vec![issue])
        })
    };
    let schema = s::object(|o| (o.field("name", s::optional(custom())), o.field("age", s::nullable(custom()))));
    let found = issues(&schema, obj([("name", "Ada".into()), ("age", 37.into())]));
    let expected = vec![Issue::new("Invalid", vec!["name".into()]), Issue::new("Invalid", vec!["age".into()])];
    assert_eq!(*captured.lock().unwrap(), expected);
    assert_eq!(found, expected);
}

#[test]
fn strips_unknown_keys_by_default() {
    let schema = s::object(|o| o.field("name", s::string()));
    assert_eq!(ok(&schema, obj([("name", "Ada".into()), ("extra", "x".into())])), "Ada");
}

#[test]
fn passes_through_unknown_keys_when_configured() {
    let schema = s::object(|o| (o.field("name", s::string()), o.rest()));
    let (name, rest) = ok(&schema, obj([("name", "Ada".into()), ("extra", "x".into())]));
    assert_eq!(name, "Ada");
    assert_eq!(rest, [("extra".to_string(), Value::from("x"))]);
}

#[test]
fn errors_on_unknown_keys_when_configured() {
    let schema = s::object(|o| o.field("name", s::string())).strict();
    assert_eq!(path(&issues(&schema, obj([("name", "Ada".into()), ("extra", "x".into())]))[0].path), ["extra"]);
}

#[test]
fn rejects_non_object_values() {
    let schema = s::object(|o| o.field("name", s::string()));
    let from_string = issues(&schema, "not-object");
    assert!(s::parse(&schema, arr(["a", "b"])).is_err());
    assert!(s::parse(&schema, Value::Null).is_err());
    assert_eq!(from_string[0].message, "Expected object");
}

#[test]
fn validates_nested_objects_with_paths() {
    let schema = s::object(|o| o.field("user", s::object(|u| u.field("profile", s::object(|p| p.field("email", s::string()))))));
    let value = obj([("user", obj([("profile", obj([("email", 123.into())]))]))]);
    assert_eq!(path(&issues(&schema, value)[0].path), ["user", "profile", "email"]);
}

#[derive(Debug, Default, PartialEq)]
struct Person {
    name: String,
    age: f64,
}

fn person() -> impl Schema<Output = Person> {
    s::object(|o| Person { name: o.field("name", s::string()), age: o.field("age", s::number()) })
}

fn invalid_person() -> Value {
    obj([("name", 123.into()), ("age", "x".into())])
}

#[test]
fn object_collects_all_issues_by_default() {
    assert_eq!(issues(&person(), invalid_person()).len(), 2);
}

#[test]
fn object_returns_first_issue_when_abort_early_is_enabled() {
    assert_eq!(early_issues(&person(), invalid_person()).len(), 1);
}

#[test]
fn handles_missing_keys_by_passing_undefined_to_schema() {
    let schema = s::object(|o| (o.field("name", s::string()), o.field("age", s::optional(s::number()))));
    assert_eq!(ok(&schema, obj([("name", "Ada".into())])), ("Ada".to_string(), None));
}

#[test]
fn does_not_include_undefined_values_unless_present_in_input() {
    // A struct always has the field; a missing optional key is None, not a value.
    let schema = s::object(|o| (o.field("a", s::optional(s::string())), o.field("b", s::string())));
    let (a, b) = ok(&schema, obj([("b", "hello".into())]));
    assert_eq!((a, b.as_str()), (None, "hello"));
}

// ---------------------------------------------------------------------------------------
// modifiers
// ---------------------------------------------------------------------------------------

#[test]
fn supports_optional_values() {
    assert_eq!(ok(&s::optional(s::string()), Value::Undefined), None);
}

#[test]
fn optional_still_validates_non_undefined_values() {
    assert_eq!(ok(&s::optional(s::string()), "hello").as_deref(), Some("hello"));
    assert!(s::parse(&s::optional(s::string()), 123).is_err());
}

#[test]
fn supports_defaulted_values() {
    assert_eq!(ok(&s::defaulted(s::string(), "hello".to_string()), Value::Undefined), "hello");
}

#[test]
fn supports_defaulted_with_function() {
    let calls = AtomicUsize::new(0);
    let schema = s::defaulted_with(s::number(), || (calls.fetch_add(1, Ordering::SeqCst) + 1) as f64);
    assert_eq!(ok(&schema, Value::Undefined), 1.0);
    assert_eq!(ok(&schema, Value::Undefined), 2.0);
}

#[test]
fn defaulted_still_validates_non_undefined_values() {
    let schema = s::defaulted(s::number(), 0.0);
    assert_eq!(ok(&schema, 42), 42.0);
    assert!(s::parse(&schema, "not-a-number").is_err());
}

#[test]
fn supports_nullable_values() {
    assert_eq!(ok(&s::nullable(s::string()), Value::Null), None);
}

#[test]
fn nullable_still_validates_non_null_values() {
    assert_eq!(ok(&s::nullable(s::string()), "hello").as_deref(), Some("hello"));
    assert!(s::parse(&s::nullable(s::string()), 123).is_err());
}

#[test]
fn supports_refine_predicates() {
    let schema = s::number().refine(|value| *value > 0.0);
    assert_eq!(ok(&schema, 1), 1.0);
    assert!(s::parse(&schema, -1).is_err());
}

#[test]
fn refine_uses_custom_message_when_provided() {
    let schema = s::number().refine_with(|value| *value > 0.0, "Must be positive");
    assert_eq!(issues(&schema, -1)[0].message, "Must be positive");
}

#[test]
fn supports_transform_functions() {
    let schema = s::number().transform(|n| n.to_string());
    let value: String = ok(&schema, 42);
    assert_eq!(value, "42");
    assert!(s::parse(&schema, "not-a-number").is_err());
}

// ---------------------------------------------------------------------------------------
// tuple
// ---------------------------------------------------------------------------------------

#[test]
fn validates_tuples_and_enforces_length() {
    assert!(s::parse(&s::tuple((s::string(), s::number())), arr(["ok"])).is_err());
}

#[test]
fn returns_validated_tuple_on_success() {
    let schema = s::tuple((s::string(), s::number(), s::boolean()));
    assert_eq!(ok(&schema, arr([Value::from("hello"), Value::from(42), Value::from(true)])), ("hello".to_string(), 42.0, true));
}

#[test]
fn tuple_rejects_non_array_values() {
    assert_eq!(issues(&s::tuple((s::string(),)), "not-array")[0].message, "Expected array");
}

#[test]
fn reports_length_mismatch_for_extra_elements() {
    let found = issues(&s::tuple((s::string(), s::number())), arr([Value::from("a"), Value::from(1), Value::from("extra")]));
    assert!(found[0].message.contains("Expected tuple length"), "{}", found[0].message);
}

#[test]
fn validates_element_types_with_paths() {
    let found = issues(&s::tuple((s::string(), s::number())), arr(["ok", "not-a-number"]));
    assert_eq!(found[0].path, [PathSegment::Index(1)]);
}

// ---------------------------------------------------------------------------------------
// union
// ---------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum StringOrNumber {
    String(String),
    Number(f64),
}

fn string_or_number<'s>() -> s::Union<'s, StringOrNumber> {
    s::union([s::string().transform(StringOrNumber::String).boxed(), s::number().transform(StringOrNumber::Number).boxed()])
}

#[derive(Debug, PartialEq)]
enum Nested {
    Text(String),
    Person(Person),
    Numbers(Vec<f64>),
}

#[test]
fn preserves_paths_from_every_failing_nested_variant() {
    let schema = s::object(|o| {
        let value = o.try_field(
            "value",
            s::union([s::string().transform(Nested::Text).boxed(), person().transform(Nested::Person).boxed(), s::array(s::number()).transform(Nested::Numbers).boxed()]),
        );
        (value, o.field("other", s::number()))
    });
    let value = obj([("value", obj([("name", 123.into()), ("age", "bad".into())])), ("other", "bad".into())]);
    let paths: Vec<Vec<String>> = issues(&schema, value).iter().map(|i| path(&i.path)).collect();
    assert_eq!(paths, [vec!["value"], vec!["value", "name"], vec!["value", "age"], vec!["value"], vec!["other"]]);
}

#[test]
fn returns_the_first_successful_variant() {
    assert_eq!(ok(&string_or_number(), 123), StringOrNumber::Number(123.0));
}

#[test]
fn fails_when_no_variant_matches() {
    assert_eq!(issues(&string_or_number(), false).len(), 2);
}

#[test]
fn returns_first_variant_issues_when_abort_early_is_enabled() {
    let found = early_issues(&string_or_number(), false);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "Expected string");
}

#[test]
fn handles_empty_schemas_array() {
    let schema: s::Union<'_, Value> = s::union([]);
    assert_eq!(issues(&schema, "anything")[0].message, "No union variant matched");
}

// ---------------------------------------------------------------------------------------
// record
// ---------------------------------------------------------------------------------------

#[test]
fn validates_record_values() {
    let value = ok(&s::record(s::string(), s::number()), obj([("a", 1.into()), ("b", 2.into())]));
    assert_eq!(value, BTreeMap::from([("a".to_string(), 1.0), ("b".to_string(), 2.0)]));
}

#[test]
fn reports_invalid_record_values_with_paths() {
    assert_eq!(path(&issues(&s::record(s::string(), s::number()), obj([("a", "nope".into())]))[0].path), ["a"]);
}

#[test]
fn validates_record_keys() {
    let email_key = s::string().refine(|key| key.contains('@'));
    assert_eq!(path(&issues(&s::record(email_key, s::number()), obj([("not-an-email", 1.into())]))[0].path), ["not-an-email"]);
}

#[test]
fn record_rejects_non_object_values() {
    let schema = s::record(s::string(), s::number());
    let from_array = issues(&schema, arr([1, 2]));
    assert!(s::parse(&schema, Value::Null).is_err());
    assert!(s::parse(&schema, "string").is_err());
    assert_eq!(from_array[0].message, "Expected object");
}

#[test]
fn handles_empty_records() {
    assert!(ok(&s::record(s::string(), s::number()), Value::Object(Vec::new())).is_empty());
}

// ---------------------------------------------------------------------------------------
// any
// ---------------------------------------------------------------------------------------

#[test]
fn accepts_any_value() {
    let schema = s::any();
    for value in [Value::from("hello"), Value::from(42), Value::Null, Value::Undefined, obj([("a", 1.into())]), arr([1, 2])] {
        assert!(s::parse(&schema, value).is_ok());
    }
}

#[test]
fn preserves_the_original_value() {
    let value = obj([("nested", true.into())]);
    assert_eq!(ok(&s::any(), value.clone()), value);
}

// ---------------------------------------------------------------------------------------
// enum_
// ---------------------------------------------------------------------------------------

#[test]
fn accepts_allowed_values() {
    let schema = s::enum_(["active", "inactive", "pending"]);
    for value in ["active", "inactive", "pending"] {
        assert_eq!(ok(&schema, value), value);
    }
}

#[test]
fn rejects_values_not_in_the_list() {
    let message = issues(&s::enum_(["active", "inactive"]), "deleted").remove(0).message;
    assert!(message.contains("active") && message.contains("inactive"), "{message}");
}

#[test]
fn works_with_numeric_values() {
    let schema = s::enum_([0, 1, 2]);
    assert_eq!(ok(&schema, 1), 1);
    assert!(s::parse(&schema, 3).is_err());
}

#[test]
fn uses_strict_equality() {
    assert!(s::parse(&s::enum_([1, 2, 3]), "1").is_err());
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    Active,
    Inactive,
    Pending,
}

impl From<Status> for Value {
    fn from(status: Status) -> Value {
        Value::from(match status {
            Status::Active => "active",
            Status::Inactive => "inactive",
            Status::Pending => "pending",
        })
    }
}

#[test]
fn infers_literal_types_without_as_const() {
    // The output is the type of the listed values: a Rust enum gives Remix's union of
    // literal types.
    let schema = s::enum_([Status::Active, Status::Inactive, Status::Pending]);
    let status: Status = ok(&schema, "pending");
    assert_eq!(status, Status::Pending);
    let number: i32 = ok(&s::enum_([0, 1, 2]), 2);
    assert_eq!(number, 2);
}

// ---------------------------------------------------------------------------------------
// instanceof_ (file)
// ---------------------------------------------------------------------------------------

#[test]
fn accepts_instances_of_the_class() {
    let file = File::new("a.txt", "text/plain", "a");
    assert_eq!(ok(&s::file(), file.clone()), file);
}

#[test]
fn rejects_non_instances() {
    let message = issues(&s::file(), "2025-01-01").remove(0).message;
    assert!(message.contains("File"), "{message}");
}

#[test]
fn provides_path_in_nested_contexts() {
    let schema = s::object(|o| o.try_field("created", s::file()));
    assert_eq!(path(&issues(&schema, obj([("created", "not-a-date".into())]))[0].path), ["created"]);
}

// ---------------------------------------------------------------------------------------
// modifiers (additional)
// ---------------------------------------------------------------------------------------

#[test]
fn preserves_paths_for_custom_check_messages() {
    let schema = s::object(|o| {
        let name = o.field("name", s::string().pipe(Check::new(|_| false).message("Invalid name")));
        let age = o.field("age", s::number().refine_with(|value| *value > 0.0, "Must be positive"));
        (name, age)
    });
    let found = issues(&schema, obj([("name", "Ada".into()), ("age", (-1).into())]));
    assert_eq!(found, [Issue::new("Invalid name", vec!["name".into()]), Issue::new("Must be positive", vec!["age".into()])]);
}

#[test]
fn refine_propagates_path_inside_objects() {
    let schema = s::object(|o| o.field("age", s::number().refine_with(|v| *v > 0.0, "Must be positive")));
    let found = issues(&schema, obj([("age", (-1).into())]));
    assert_eq!(found[0].message, "Must be positive");
    assert_eq!(path(&found[0].path), ["age"]);
}

#[test]
fn defaulted_fills_missing_keys_in_objects() {
    let schema = s::object(|o| (o.field("name", s::string()), o.field("role", s::defaulted(s::string(), "user".to_string()))));
    assert_eq!(ok(&schema, obj([("name", "Ada".into())])), ("Ada".to_string(), "user".to_string()));
}

#[test]
fn defaulted_fills_explicit_undefined_in_objects() {
    let schema = s::object(|o| o.field("role", s::defaulted(s::string(), "user".to_string())));
    assert_eq!(ok(&schema, obj([("role", Value::Undefined)])), "user");
}

#[test]
fn chains_pipe_then_refine() {
    let schema = s::string().pipe(min_length(3)).refine_with(|v| v.starts_with('a'), "Must start with a");
    assert!(s::parse(&schema, "abc").is_ok());
    assert!(s::parse(&schema, "ab").is_err());
    assert!(s::parse(&schema, "bcd").is_err());
}

#[test]
fn chains_refine_then_pipe() {
    let schema = s::string().refine_with(|v| v.starts_with('a'), "Must start with a").pipe(min_length(3));
    assert!(s::parse(&schema, "abc").is_ok());
    assert!(s::parse(&schema, "bcd").is_err());
    assert!(s::parse(&schema, "ab").is_err());
}

#[test]
fn chains_transform_then_refine() {
    let schema = s::number().transform(|n| n.to_string()).refine_with(|v| v.len() == 2, "Must be two digits");
    assert!(s::parse(&schema, 42).is_ok());
    assert!(s::parse(&schema, 7).is_err());
}

#[test]
fn chains_refine_then_transform() {
    let schema = s::number().refine_with(|n| *n > 0.0, "Must be positive").transform(|n| n.to_string());
    assert_eq!(ok(&schema, 42), "42");
    assert!(s::parse(&schema, -1).is_err());
}

// ---------------------------------------------------------------------------------------
// abortEarly
// ---------------------------------------------------------------------------------------

#[test]
fn collects_all_issues_by_default() {
    assert_eq!(issues(&person(), invalid_person()).len(), 2);
}

#[test]
fn returns_the_first_issue_when_enabled() {
    assert_eq!(early_issues(&person(), invalid_person()).len(), 1);
}

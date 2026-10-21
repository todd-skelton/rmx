//! Ports of Remix's data-schema tests (`schema`, `parse`, `checks`, `coerce`, `pipe`,
//! `variant`, `lazy` and `form-data`). Cases about JavaScript-only types (bigint,
//! symbol, Map, Set, class instances, Date, sparse arrays and custom iterators) have no
//! Rust counterpart.

use crate::checks::{email, max, max_length, min, min_length, url};
use crate::form_data as f;
use crate::{self as s, coerce, File, FormData, Issue, ParseOptions, PathSegment, Schema, SearchParams, Value};

fn validate<S: Schema>(schema: &S, value: impl Into<Value>) -> Result<S::Output, Vec<Issue>> {
    s::parse(schema, value).map_err(|e| e.issues)
}

fn messages(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|i| i.message.as_str()).collect()
}

fn path(segments: &[PathSegment]) -> Vec<String> {
    segments.iter().map(ToString::to_string).collect()
}

fn obj(entries: &[(&str, Value)]) -> Value {
    Value::object(entries.iter().cloned())
}

// primitives

#[test]
fn validates_and_rejects_primitives() {
    assert_eq!(validate(&s::string(), "ok").unwrap(), "ok");
    assert_eq!(messages(&validate(&s::string(), 123).unwrap_err()), ["Expected string"]);
    assert_eq!(validate(&s::number(), 42).unwrap(), 42.0);
    assert!(validate(&s::number(), f64::NAN).is_err());
    assert!(validate(&s::number(), f64::INFINITY).is_err());
    assert!(validate(&s::number(), f64::NEG_INFINITY).is_err());
    assert!(validate(&s::boolean(), true).unwrap());
    assert_eq!(messages(&validate(&s::boolean(), "true").unwrap_err()), ["Expected boolean"]);
    assert!(validate(&s::null_(), Value::Null).is_ok());
    assert_eq!(messages(&validate(&s::null_(), Value::Undefined).unwrap_err()), ["Expected null"]);
    assert!(validate(&s::undefined_(), Value::Undefined).is_ok());
    assert_eq!(messages(&validate(&s::undefined_(), Value::Null).unwrap_err()), ["Expected undefined"]);
    assert_eq!(validate(&s::literal("yes"), "yes").unwrap(), "yes");
    assert!(validate(&s::literal("yes"), "no").is_err());
    assert_eq!(validate(&s::any(), obj(&[("a", 1.into())])).unwrap(), obj(&[("a", 1.into())]));
}

// array

#[test]
fn validates_array_items_with_paths() {
    let errors = validate(&s::array(s::string()), vec![Value::from("ok"), Value::from(123)]).unwrap_err();
    assert_eq!(errors[0].path, [PathSegment::Index(1)]);
    assert_eq!(validate(&s::array(s::number()), vec![1, 2, 3]).unwrap(), [1.0, 2.0, 3.0]);
    assert_eq!(messages(&validate(&s::array(s::string()), "nope").unwrap_err()), ["Expected array"]);
}

#[test]
fn collects_all_issues_unless_abort_early() {
    let schema = s::array(s::string());
    assert_eq!(validate(&schema, vec![1, 2]).unwrap_err().len(), 2);
    let options = ParseOptions { abort_early: true, ..Default::default() };
    assert_eq!(s::parse_with(&schema, vec![1, 2], &options).unwrap_err().issues.len(), 1);
}

// object

#[derive(Debug, Default, PartialEq)]
struct User {
    name: String,
    age: f64,
}

fn user() -> impl Schema<Output = User> {
    s::object(|o| User { name: o.field("name", s::string()), age: o.field("age", s::number()) })
}

#[test]
fn validates_objects_and_strips_unknown_keys() {
    let value = obj(&[("name", "Ada".into()), ("age", 37.into()), ("extra", true.into())]);
    assert_eq!(validate(&user(), value).unwrap(), User { name: "Ada".into(), age: 37.0 });
    assert_eq!(messages(&validate(&user(), "nope").unwrap_err()), ["Expected object"]);
}

#[test]
fn errors_on_unknown_keys_when_strict() {
    let schema = s::object(|o| o.field("name", s::string())).strict();
    let errors = validate(&schema, obj(&[("name", "Ada".into()), ("extra", true.into())])).unwrap_err();
    assert_eq!(messages(&errors), ["Unknown key"]);
    assert_eq!(path(&errors[0].path), ["extra"]);
}

#[test]
fn validates_nested_objects_with_paths_and_collects_issues() {
    let schema = s::object(|o| o.field("user", user()));
    let errors = validate(&schema, obj(&[("user", obj(&[("name", 1.into()), ("age", "x".into())]))])).unwrap_err();
    assert_eq!(errors.iter().map(|e| path(&e.path)).collect::<Vec<_>>(), [["user", "name"], ["user", "age"]]);
    let options = ParseOptions { abort_early: true, ..Default::default() };
    let value = obj(&[("name", 1.into()), ("age", "x".into())]);
    assert_eq!(s::parse_with(&user(), value, &options).unwrap_err().issues.len(), 1);
}

#[test]
fn passes_undefined_for_missing_keys() {
    let schema = s::object(|o| o.field("nickname", s::optional(s::string())));
    assert_eq!(validate(&schema, obj(&[])).unwrap(), None);
    let errors = validate(&user(), obj(&[("name", "Ada".into())])).unwrap_err();
    assert_eq!(errors[0].to_string(), "age: Expected number");
}

// modifiers

#[test]
fn optional_nullable_and_defaulted() {
    assert_eq!(validate(&s::optional(s::string()), Value::Undefined).unwrap(), None);
    assert!(validate(&s::optional(s::string()), 1).is_err());
    assert_eq!(validate(&s::nullable(s::string()), Value::Null).unwrap(), None);
    assert!(validate(&s::nullable(s::string()), 1).is_err());
    assert_eq!(validate(&s::defaulted(s::string(), "x".into()), Value::Undefined).unwrap(), "x");
    assert!(validate(&s::defaulted(s::string(), "x".into()), 1).is_err());
    let mut calls = 0;
    let counter = std::cell::Cell::new(0);
    let schema = s::defaulted_with(s::number(), || {
        counter.set(counter.get() + 1);
        7.0
    });
    assert_eq!(validate(&schema, Value::Undefined).unwrap(), 7.0);
    calls += counter.get();
    assert_eq!(calls, 1);
    let filled = s::object(|o| o.field("tag", s::defaulted(s::string(), "none".into())));
    assert_eq!(validate(&filled, obj(&[])).unwrap(), "none");
    assert_eq!(validate(&filled, obj(&[("tag", Value::Undefined)])).unwrap(), "none");
}

#[test]
fn refine_and_transform() {
    let positive = s::number().refine(|n| *n > 0.0);
    assert_eq!(messages(&validate(&positive, -1).unwrap_err()), ["Refinement failed"]);
    let custom = s::number().refine_with(|n| *n > 0.0, "Must be positive");
    assert_eq!(messages(&validate(&custom, -1).unwrap_err()), ["Must be positive"]);
    let nested = s::object(|o| o.field("n", s::number().refine_with(|n| *n > 0.0, "Must be positive")));
    assert_eq!(path(&validate(&nested, obj(&[("n", (-1).into())])).unwrap_err()[0].path), ["n"]);
    let length = s::string().transform(|s| s.len());
    assert_eq!(validate(&length, "abc").unwrap(), 3);
    let chained = s::string().transform(|s| s.len()).refine(|n| *n > 2);
    assert!(validate(&chained, "ab").is_err());
    let chained = s::string().refine(|s| s.starts_with('a')).transform(|s| s.to_uppercase());
    assert_eq!(validate(&chained, "abc").unwrap(), "ABC");
    let chained = s::string().pipe(min_length(2)).refine_with(|s| s != "no", "No");
    assert_eq!(messages(&validate(&chained, "a").unwrap_err()), ["Expected at least 2 characters"]);
    assert_eq!(messages(&validate(&chained, "no").unwrap_err()), ["No"]);
}

#[test]
fn applies_checks_in_order() {
    let schema = s::string().pipe(min_length(3)).pipe(max_length(5));
    assert_eq!(messages(&validate(&schema, "ab").unwrap_err()), ["Expected at least 3 characters"]);
    assert_eq!(messages(&validate(&schema, "abcdef").unwrap_err()), ["Expected at most 5 characters"]);
    assert!(validate(&schema, "abcd").is_ok());
}

// tuple, union, record, enum

#[test]
fn validates_tuples_and_their_length() {
    let schema = s::tuple((s::string(), s::number()));
    assert_eq!(validate(&schema, vec![Value::from("a"), Value::from(1)]).unwrap(), ("a".to_string(), 1.0));
    assert_eq!(messages(&validate(&schema, vec![Value::from("a")]).unwrap_err()), ["Expected tuple length 2"]);
    let extra = vec![Value::from("a"), Value::from(1), Value::from(2)];
    assert_eq!(messages(&validate(&schema, extra).unwrap_err()), ["Expected tuple length 2"]);
    let errors = validate(&schema, vec![Value::from(1), Value::from("x")]).unwrap_err();
    assert_eq!(errors.iter().map(|e| path(&e.path)).collect::<Vec<_>>(), [["0"], ["1"]]);
    assert_eq!(messages(&validate(&schema, "x").unwrap_err()), ["Expected array"]);
}

#[test]
fn unions_return_the_first_match_or_every_issue() {
    let schema = s::union([s::string().transform(|s| s.len() as f64).boxed(), s::number().boxed()]);
    assert_eq!(validate(&schema, "abc").unwrap(), 3.0);
    assert_eq!(validate(&schema, 5).unwrap(), 5.0);
    assert_eq!(messages(&validate(&schema, true).unwrap_err()), ["Expected string", "Expected number"]);
    let options = ParseOptions { abort_early: true, ..Default::default() };
    assert_eq!(s::parse_with(&schema, true, &options).unwrap_err().issues.len(), 1);
    let empty = s::union::<f64>([]);
    assert_eq!(messages(&validate(&empty, 1).unwrap_err()), ["No union variant matched"]);
    let nested = s::union([s::object(|o| o.field("a", s::string())).boxed(), s::object(|o| o.field("b", s::string())).boxed()]);
    let errors = validate(&nested, obj(&[])).unwrap_err();
    assert_eq!(errors.iter().map(|e| path(&e.path)).collect::<Vec<_>>(), [["a"], ["b"]]);
}

#[test]
fn validates_records() {
    let schema = s::record(s::string(), s::number());
    let out = validate(&schema, obj(&[("a", 1.into()), ("b", 2.into())])).unwrap();
    assert_eq!(out.into_iter().collect::<Vec<_>>(), [("a".to_string(), 1.0), ("b".to_string(), 2.0)]);
    let errors = validate(&schema, obj(&[("a", "x".into())])).unwrap_err();
    assert_eq!(path(&errors[0].path), ["a"]);
    let keys = s::record(s::string().pipe(min_length(2)), s::number());
    assert!(validate(&keys, obj(&[("a", 1.into())])).is_err());
    assert_eq!(messages(&validate(&schema, Value::Array(vec![])).unwrap_err()), ["Expected object"]);
    assert!(validate(&schema, obj(&[])).unwrap().is_empty());
}

#[test]
fn enums_accept_listed_values() {
    let schema = s::enum_(["admin", "member"]);
    assert_eq!(validate(&schema, "admin").unwrap(), "admin");
    assert_eq!(messages(&validate(&schema, "guest").unwrap_err()), ["Expected one of: admin, member"]);
    let numbers = s::enum_([1, 2]);
    assert_eq!(validate(&numbers, 2).unwrap(), 2);
    assert!(validate(&numbers, "1").is_err(), "strict equality");
}

#[test]
fn files_are_validated_like_instanceof_file() {
    let file = File::new("a.txt", "text/plain", "a");
    assert_eq!(validate(&s::file(), file.clone()).unwrap(), file);
    assert_eq!(messages(&validate(&s::file(), "a.txt").unwrap_err()), ["Expected instance of File"]);
    let nested = s::object(|o| o.try_field("upload", s::file()));
    assert_eq!(path(&validate(&nested, obj(&[])).unwrap_err()[0].path), ["upload"]);
}

// parse

#[test]
fn error_map_replaces_messages_by_code() {
    let options = ParseOptions::with_error_map(|cx| (cx.code == "type.string").then(|| "Expected text input".to_string()));
    let value = obj(&[("name", 123.into()), ("age", "x".into())]);
    let errors = s::parse_with(&user(), value, &options).unwrap_err().issues;
    assert_eq!(messages(&errors), ["Expected text input", "Expected number"]);
}

#[test]
fn error_map_receives_locale_values_and_paths() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = captured.clone();
    let mut options = ParseOptions::with_error_map(move |cx| {
        seen.lock().unwrap().push((cx.code.to_string(), cx.locale.map(String::from), cx.values.to_vec(), path(cx.path)));
        (cx.code == "string.min_length").then(|| format!("Debe tener al menos {} caracteres", cx.values[0].1))
    });
    options.locale = Some("es".into());
    let schema = s::object(|o| o.field("nickname", s::string().pipe(min_length(3))));
    let errors = s::parse_with(&schema, obj(&[("nickname", "ab".into())]), &options).unwrap_err().issues;
    assert_eq!(messages(&errors), ["Debe tener al menos 3 caracteres"]);
    let captured = captured.lock().unwrap();
    assert_eq!(*captured, [("string.min_length".into(), Some("es".into()), vec![("min", Value::from(3))], vec!["nickname".to_string()])]);
}

#[test]
fn error_map_falls_back_to_the_default_message() {
    let options = ParseOptions::with_error_map(|_| None);
    assert_eq!(messages(&s::parse_with(&s::number(), "x", &options).unwrap_err().issues), ["Expected number"]);
}

// checks

#[test]
fn checks_expose_codes_and_values() {
    let check = min_length(2);
    assert_eq!((check.code, check.values.clone()), (Some("string.min_length"), vec![("min", Value::from(2))]));
    assert_eq!(max_length(4).code, Some("string.max_length"));
}

#[test]
fn string_and_number_checks() {
    let length = s::string().pipe(min_length(2)).pipe(max_length(4));
    assert!(validate(&length, "test").is_ok());
    assert!(validate(&length, "a").is_err());
    assert!(validate(&length, "toolong").is_err());
    let mail = s::string().pipe(email());
    assert!(validate(&mail, "user@example.com").is_ok());
    for bad in ["not-an-email", "a@b", "a b@c.d", "@c.d", "a@.d", "a@c."] {
        assert!(validate(&mail, bad).is_err(), "{bad}");
    }
    let link = s::string().pipe(url());
    assert!(validate(&link, "https://example.com").is_ok());
    assert!(validate(&link, "mailto:ada@example.com").is_ok());
    assert!(validate(&link, "not-a-url").is_err());
    assert!(validate(&link, "https://").is_err());
    let range = s::number().pipe(min(3.0)).pipe(max(5.0));
    assert!(validate(&range, 4).is_ok());
    assert_eq!(messages(&validate(&range, 2).unwrap_err()), ["Expected number greater than or equal to 3"]);
    assert_eq!(messages(&validate(&range, 6).unwrap_err()), ["Expected number less than or equal to 5"]);
}

// coerce

#[test]
fn coerces_numbers() {
    assert_eq!(validate(&coerce::number(), "42").unwrap(), 42.0);
    assert_eq!(validate(&coerce::number(), " 3.5 ").unwrap(), 3.5);
    assert_eq!(validate(&coerce::number(), "1e3").unwrap(), 1000.0);
    assert_eq!(validate(&coerce::number(), "0x10").unwrap(), 16.0);
    assert_eq!(validate(&coerce::number(), 7).unwrap(), 7.0);
    for bad in ["", "  ", "abc", "Infinity", "inf", "NaN"] {
        assert_eq!(messages(&validate(&coerce::number(), bad).unwrap_err()), ["Expected number"], "{bad:?}");
    }
    assert!(validate(&coerce::number(), f64::INFINITY).is_err());
    assert!(validate(&coerce::number(), true).is_err());
}

#[test]
fn coerces_booleans_integers_and_strings() {
    assert!(validate(&coerce::boolean(), " TRUE ").unwrap());
    assert!(!validate(&coerce::boolean(), "false").unwrap());
    assert!(validate(&coerce::boolean(), true).unwrap());
    assert_eq!(messages(&validate(&coerce::boolean(), "yes").unwrap_err()), ["Expected boolean"]);
    assert!(validate(&coerce::boolean(), 1).is_err());
    assert_eq!(validate(&coerce::integer(), "123").unwrap(), 123);
    assert_eq!(validate(&coerce::integer(), 5).unwrap(), 5);
    for bad in [Value::from(""), Value::from("1.5"), Value::from(1.5), Value::Number(f64::NAN)] {
        assert!(validate(&coerce::integer(), bad).is_err());
    }
    assert_eq!(validate(&coerce::string(), 42).unwrap(), "42");
    assert_eq!(validate(&coerce::string(), true).unwrap(), "true");
    assert_eq!(validate(&coerce::string(), "x").unwrap(), "x");
    for bad in [Value::Null, Value::Undefined, Value::Array(vec![]), obj(&[])] {
        assert_eq!(messages(&validate(&coerce::string(), bad).unwrap_err()), ["Expected string"]);
    }
}

// variant, lazy

#[derive(Debug, PartialEq)]
enum Event {
    Created(String),
    Updated(String, f64),
}

fn event() -> impl Schema<Output = Event> {
    s::variant(
        "type",
        [
            ("created", s::object(|o| Event::Created(o.field("id", s::string()))).boxed()),
            ("updated", s::object(|o| Event::Updated(o.field("id", s::string()), o.field("version", s::number()))).boxed()),
        ],
    )
}

#[test]
fn variants_pick_a_schema_by_discriminator() {
    let created = obj(&[("type", "created".into()), ("id", "e1".into())]);
    assert_eq!(validate(&event(), created).unwrap(), Event::Created("e1".into()));
    let updated = obj(&[("type", "updated".into()), ("id", "e1".into()), ("version", 2.into())]);
    assert_eq!(validate(&event(), updated).unwrap(), Event::Updated("e1".into(), 2.0));
    let unknown = validate(&event(), obj(&[("type", "deleted".into())])).unwrap_err();
    assert_eq!((messages(&unknown), path(&unknown[0].path)), (vec!["Unknown discriminator"], vec!["type".to_string()]));
    let missing = validate(&event(), obj(&[("id", "e1".into())])).unwrap_err();
    assert_eq!((messages(&missing), path(&missing[0].path)), (vec!["Expected discriminator"], vec!["type".to_string()]));
}

#[derive(Debug, Default, PartialEq)]
struct Tree {
    name: String,
    children: Vec<Tree>,
}

fn tree() -> s::BoxSchema<'static, Tree> {
    s::object(|o| Tree { name: o.field("name", s::string()), children: o.field("children", s::defaulted_with(s::array(s::lazy(tree)), Vec::new)) }).boxed()
}

#[test]
fn lazy_schemas_validate_recursive_shapes() {
    let leaf = obj(&[("name", "leaf".into())]);
    let root = obj(&[("name", "root".into()), ("children", Value::Array(vec![leaf]))]);
    assert_eq!(validate(&tree(), root).unwrap().children[0].name, "leaf");
    let bad = obj(&[("name", "root".into()), ("children", Value::Array(vec![obj(&[("name", 1.into())])]))]);
    assert_eq!(path(&validate(&tree(), bad).unwrap_err()[0].path), ["children", "0", "name"]);
}

// form-data

fn form(entries: &[(&str, &str)]) -> FormData {
    let mut form = FormData::new();
    for (k, v) in entries {
        form.append(*k, *v);
    }
    form
}

#[test]
fn parses_text_fields_from_form_data_and_search_params() {
    let schema = f::object(|o| (o.field("email", s::string()), o.field("password", s::string().pipe(min_length(1)))));
    let expected = ("ada@example.com".to_string(), "secret".to_string());
    assert_eq!(validate(&schema, form(&[("email", "ada@example.com"), ("password", "secret")])).unwrap(), expected);
    assert_eq!(validate(&schema, SearchParams::parse("email=ada%40example.com&password=secret")).unwrap(), expected);
}

#[test]
fn parses_repeated_text_fields() {
    let schema = f::object(|o| o.fields("tags", s::array(s::string())));
    assert_eq!(validate(&schema, form(&[("tags", "one"), ("tags", "two")])).unwrap(), ["one", "two"]);
    assert_eq!(validate(&schema, SearchParams::parse("tags=one&tags=two")).unwrap(), ["one", "two"]);
}

#[test]
fn parses_single_and_repeated_file_fields() {
    let mut data = FormData::new();
    data.append_file("avatar", File::new("avatar.png", "image/png", "avatar"));
    data.append_file("attachments", File::new("one.txt", "text/plain", "one"));
    data.append_file("attachments", File::new("two.txt", "text/plain", "two"));
    let schema = f::object(|o| (o.file("avatar", s::file()), o.files("attachments", s::array(s::file()))));
    let (avatar, attachments) = validate(&schema, data).unwrap();
    assert_eq!(avatar.unwrap().name, "avatar.png");
    assert_eq!(attachments.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["one.txt", "two.txt"]);
    assert_eq!(&attachments[1].bytes[..], b"two");
}

#[test]
fn passes_undefined_for_missing_fields() {
    let schema = f::object(|o| (o.field("nickname", s::optional(s::string())), o.field("query", s::defaulted(s::string(), String::new()))));
    assert_eq!(validate(&schema, FormData::new()).unwrap(), (None, String::new()));
}

#[test]
fn reports_failures_with_field_paths() {
    let schema = f::object(|o| o.field("age", coerce::number()));
    let error = s::parse(&schema, form(&[("age", "not-a-number")])).unwrap_err();
    assert_eq!(path(&error.issues[0].path), ["age"]);
    let ages = f::object(|o| o.fields("ages", s::array(coerce::number())));
    let errors = validate(&ages, form(&[("ages", "1"), ("ages", "x")])).unwrap_err();
    assert_eq!((messages(&errors), path(&errors[0].path)), (vec!["Expected number"], vec!["ages".to_string(), "1".to_string()]));
}

#[test]
fn reports_text_and_file_kind_mismatches() {
    let mut data = FormData::new();
    data.append_file("avatar", File::new("avatar.png", "image/png", "avatar"));
    let errors = validate(&f::object(|o| o.field("avatar", s::string())), data).unwrap_err();
    assert_eq!((messages(&errors), path(&errors[0].path)), (vec!["Expected text field \"avatar\""], vec!["avatar".to_string()]));
    let errors = validate(&f::object(|o| o.file("avatar", s::file())), SearchParams::parse("avatar=avatar.png")).unwrap_err();
    assert_eq!((messages(&errors), path(&errors[0].path)), (vec!["Expected file field \"avatar\""], vec!["avatar".to_string()]));
}

#[test]
fn rejects_unsupported_root_values() {
    let errors = validate(&f::object(|o| o.field("email", s::string())), obj(&[("email", "ada@example.com".into())])).unwrap_err();
    assert_eq!(messages(&errors), ["Expected FormData or URLSearchParams"]);
    assert!(errors[0].path.is_empty());
}

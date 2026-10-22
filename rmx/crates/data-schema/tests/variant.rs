//! Port of data-schema's variant.test.ts, one test per case.

use rmx_data_schema::{self as s, PathSegment, Schema, Value};

/// `{ type: 'created'; id: string } | { type: 'updated'; id: string; version: number }`
#[derive(Debug, Default, PartialEq)]
enum Event {
    #[default]
    None,
    Created { id: String },
    Updated { id: String, version: f64 },
}

fn schema() -> impl Schema<Output = Event> {
    s::variant(
        "type",
        [
            ("created", s::object(|o| Event::Created { id: o.field("id", s::string()) }).boxed()),
            ("updated", s::object(|o| Event::Updated { id: o.field("id", s::string()), version: o.field("version", s::number()) }).boxed()),
        ],
    )
}

#[test]
fn infers_literal_discriminator_values() {
    // The output type is the enum the variants build, checked by the compiler.
    fn output<S: Schema<Output = Event>>(_: &S) {}
    output(&schema());
}

#[test]
fn validates_based_on_discriminator() {
    let created = Value::object([("type", "created"), ("id", "a")]);
    let updated = Value::object([("type", Value::from("updated")), ("id", Value::from("b")), ("version", Value::from(2))]);
    assert_eq!(s::parse(&schema(), created).unwrap(), Event::Created { id: "a".into() });
    assert_eq!(s::parse(&schema(), updated).unwrap(), Event::Updated { id: "b".into(), version: 2.0 });
}

#[test]
fn reports_unknown_discriminator_values() {
    let issues = s::parse(&schema(), Value::object([("type", "other"), ("id", "a")])).unwrap_err().issues;
    assert_eq!(issues[0].path, [PathSegment::from("type")]);
}

#[test]
fn reports_missing_discriminator() {
    let issues = s::parse(&schema(), Value::object([("id", "a")])).unwrap_err().issues;
    assert_eq!(issues[0].path, [PathSegment::from("type")]);
}

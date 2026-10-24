//! Port of data-schema's form-data.test.ts, one test per case.

use rsmix_data_schema::checks::min_length;
use rsmix_data_schema::{self as s, coerce, form_data as f, File, FormData, PathSegment, Schema, SearchParams, ValidationError, Value};

#[derive(Debug, Default, PartialEq)]
struct Login {
    email: String,
    password: String,
}

fn login() -> impl Schema<Output = Login> {
    f::object(|o| Login { email: o.field("email", s::string()), password: o.field("password", s::string().pipe(min_length(1))) })
}

fn ada() -> Login {
    Login { email: "ada@example.com".into(), password: "secret".into() }
}

#[test]
fn parses_single_text_fields() {
    let mut form = FormData::new();
    form.set("email", "ada@example.com");
    form.set("password", "secret");
    assert_eq!(s::parse(&login(), form).unwrap(), ada());
}

#[test]
fn supports_custom_form_field_names() {
    let mut form = FormData::new();
    form.set("user-email", "ada@example.com");
    let schema = f::object(|o| Login { email: o.field("user-email", s::string()), password: String::new() });
    assert_eq!(s::parse(&schema, form).unwrap().email, "ada@example.com");
}

#[test]
fn parses_repeated_text_fields() {
    let mut form = FormData::new();
    form.append("tags", "one");
    form.append("tags", "two");
    let schema = f::object(|o| o.fields("tags", s::array(s::string())));
    assert_eq!(s::parse(&schema, form).unwrap(), ["one", "two"]);
}

#[test]
fn parses_text_fields_from_url_search_params() {
    let mut params = SearchParams::new();
    params.set("email", "ada@example.com");
    params.set("password", "secret");
    assert_eq!(s::parse(&login(), params).unwrap(), ada());
}

#[test]
fn parses_repeated_text_fields_from_url_search_params() {
    let mut params = SearchParams::new();
    params.append("tags", "one");
    params.append("tags", "two");
    let schema = f::object(|o| o.fields("tags", s::array(s::string())));
    assert_eq!(s::parse(&schema, params).unwrap(), ["one", "two"]);
}

#[test]
fn parses_a_single_file_field() {
    let mut form = FormData::new();
    form.append_file("avatar", File::new("avatar.png", "image/png", "avatar"));
    let avatar = s::parse(&f::object(|o| o.file("avatar", s::file())), form).unwrap().unwrap();
    assert_eq!(avatar.name, "avatar.png");
    assert_eq!(&avatar.bytes[..], b"avatar");
}

#[test]
fn parses_repeated_file_fields() {
    let mut form = FormData::new();
    form.append_file("attachments", File::new("one.txt", "text/plain", "one"));
    form.append_file("attachments", File::new("two.txt", "text/plain", "two"));
    let attachments = s::parse(&f::object(|o| o.files("attachments", s::array(s::file()))), form).unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].name, "one.txt");
    assert_eq!(&attachments[1].bytes[..], b"two");
}

#[test]
fn passes_undefined_to_optional_fields_when_a_value_is_missing() {
    let schema = f::object(|o| o.field("nickname", s::optional(s::string())));
    assert_eq!(s::parse(&schema, FormData::new()).unwrap(), None);
}

#[test]
fn infers_defaulted_fields_as_required_outputs() {
    let schema = f::object(|o| o.field("query", s::defaulted(s::string(), String::new())));
    let query: String = s::parse(&schema, FormData::new()).unwrap();
    assert_eq!(query, "");
}

#[test]
fn throws_a_validation_error_when_parsing_fails() {
    let mut form = FormData::new();
    form.set("age", "not-a-number");
    let error: ValidationError = s::parse(&f::object(|o| o.field("age", coerce::number())), form).unwrap_err();
    assert_eq!(error.issues[0].path, [PathSegment::from("age")]);
}

#[test]
fn reports_text_file_kind_mismatches_in_the_safe_result() {
    let mut form = FormData::new();
    form.append_file("avatar", File::new("avatar.png", "image/png", "avatar"));
    let issues = s::parse(&f::object(|o| o.field("avatar", s::string())), form).unwrap_err().issues;
    assert_eq!(issues[0].message, "Expected text field \"avatar\"");
    assert_eq!(issues[0].path, [PathSegment::from("avatar")]);
}

#[test]
fn reports_file_mismatches_from_url_search_params_in_the_safe_result() {
    let mut params = SearchParams::new();
    params.set("avatar", "avatar.png");
    let issues = s::parse(&f::object(|o| o.file("avatar", s::file())), params).unwrap_err().issues;
    assert_eq!(issues[0].message, "Expected file field \"avatar\"");
    assert_eq!(issues[0].path, [PathSegment::from("avatar")]);
}

#[test]
fn prefixes_nested_schema_issues_with_the_parsed_field_path() {
    let mut form = FormData::new();
    form.append("ages", "1");
    form.append("ages", "x");
    let issues = s::parse(&f::object(|o| o.fields("ages", s::array(coerce::number()))), form).unwrap_err().issues;
    assert_eq!(issues[0].path, [PathSegment::from("ages"), PathSegment::from(1)]);
    assert_eq!(issues[0].message, "Expected number");
}

#[test]
fn rejects_unsupported_root_input_values() {
    let schema = f::object(|o| o.field("email", s::string()));
    let issues = s::parse(&schema, Value::object([("email", "ada@example.com")])).unwrap_err().issues;
    assert_eq!(issues[0].message, "Expected FormData or URLSearchParams");
    assert!(issues[0].path.is_empty());
}

//! Typed values from a [`FormData`] or [`SearchParams`] (`remix/data-schema/form-data`).
//!
//! ```
//! use rmx_data_schema::{self as s, coerce, form_data as f, SearchParams};
//!
//! let limit = f::object(|o| o.field("limit", s::optional(coerce::number())));
//! assert_eq!(s::parse(&limit, SearchParams::parse("limit=5")).unwrap(), Some(5.0));
//! assert_eq!(s::parse(&limit, SearchParams::parse("")).unwrap(), None);
//! ```
//!
//! Remix names each output key and can read it from a differently named field; here the
//! form field name is also the issue path, and the closure picks the Rust field it fills.

use crate::{Context, FormData, FormValue, Issue, Schema, SearchParams, Value};

/// A schema that reads fields from a [`FormData`] or [`SearchParams`] value and builds
/// `T` with `build`. Issue paths start with the field name.
pub fn object<T, F: Fn(&mut FormFields<'_, '_>) -> T>(build: F) -> FormObject<F> {
    FormObject(build)
}

/// [`object`].
pub struct FormObject<F>(F);

enum Source<'a> {
    Form(&'a FormData),
    Search(&'a SearchParams),
}

impl Source<'_> {
    fn all(&self, name: &str) -> Vec<FormValue> {
        match self {
            Source::Form(form) => form.get_all(name).into_iter().cloned().collect(),
            Source::Search(params) => params.get_all(name).into_iter().map(|v| FormValue::Text(v.to_string())).collect(),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Field,
    Fields,
    File,
    Files,
}

/// Reads fields for [`object`]. As with [`crate::object`], a field that fails yields its
/// output type's `Default` while the other fields are checked.
pub struct FormFields<'a, 'o> {
    source: Source<'a>,
    cx: &'a mut Context<'o>,
    issues: Vec<Issue>,
}

impl FormFields<'_, '_> {
    /// One text field (the first value): a string, or `undefined` when it is missing.
    pub fn field<S: Schema>(&mut self, name: &str, schema: S) -> S::Output
    where
        S::Output: Default,
    {
        self.read(name, Kind::Field, schema).unwrap_or_default()
    }

    /// Every value of a repeated text field, as an array of strings.
    pub fn fields<S: Schema>(&mut self, name: &str, schema: S) -> S::Output
    where
        S::Output: Default,
    {
        self.read(name, Kind::Fields, schema).unwrap_or_default()
    }

    /// One file field: a file, or `undefined` when it is missing. Use [`crate::file`] or
    /// `optional(file())` as the schema.
    pub fn file<S: Schema>(&mut self, name: &str, schema: S) -> Option<S::Output> {
        self.read(name, Kind::File, schema)
    }

    /// Every file of a repeated file field, as an array of files.
    pub fn files<S: Schema>(&mut self, name: &str, schema: S) -> S::Output
    where
        S::Output: Default,
    {
        self.read(name, Kind::Files, schema).unwrap_or_default()
    }

    /// Like [`field`](FormFields::field), returning `None` on failure.
    pub fn try_field<S: Schema>(&mut self, name: &str, schema: S) -> Option<S::Output> {
        self.read(name, Kind::Field, schema)
    }

    fn read<S: Schema>(&mut self, name: &str, kind: Kind, schema: S) -> Option<S::Output> {
        if self.cx.abort_early() && !self.issues.is_empty() {
            return None;
        }
        let values = self.source.all(name);
        let result = self.cx.at(name, |cx| {
            let value = input(name, kind, values, cx)?;
            schema.validate(&value, cx)
        });
        match result {
            Ok(v) => Some(v),
            Err(found) => {
                self.issues.extend(found);
                None
            }
        }
    }
}

/// The value a field kind passes to its schema, or a kind mismatch.
fn input(name: &str, kind: Kind, values: Vec<FormValue>, cx: &mut Context<'_>) -> Result<Value, Vec<Issue>> {
    let text = |cx: &Context<'_>| cx.plain_issue(format!("Expected text field \"{name}\""));
    let file = |cx: &Context<'_>| cx.plain_issue(format!("Expected file field \"{name}\""));
    match kind {
        Kind::Field => match values.into_iter().next() {
            None => Ok(Value::Undefined),
            Some(FormValue::Text(s)) => Ok(Value::String(s)),
            Some(FormValue::File(_)) => Err(vec![text(cx)]),
        },
        Kind::File => match values.into_iter().next() {
            None => Ok(Value::Undefined),
            Some(FormValue::File(f)) => Ok(Value::File(f)),
            Some(FormValue::Text(_)) => Err(vec![file(cx)]),
        },
        Kind::Fields | Kind::Files => {
            let mut issues = Vec::new();
            let mut out = Vec::new();
            for (i, value) in values.into_iter().enumerate() {
                match (kind, value) {
                    (Kind::Fields, FormValue::Text(s)) => out.push(Value::String(s)),
                    (Kind::Files, FormValue::File(f)) => out.push(Value::File(f)),
                    (Kind::Fields, _) => issues.push(cx.at(i, |cx| text(cx))),
                    _ => issues.push(cx.at(i, |cx| file(cx))),
                }
            }
            if issues.is_empty() {
                Ok(Value::Array(out))
            } else {
                Err(issues)
            }
        }
    }
}

impl<T, F: Fn(&mut FormFields<'_, '_>) -> T> Schema for FormObject<F> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        let source = match value {
            Value::FormData(form) => Source::Form(form),
            Value::SearchParams(params) => Source::Search(params),
            _ => return cx.fail("type.form_data_source", "Expected FormData or URLSearchParams", value),
        };
        let mut fields = FormFields { source, cx, issues: Vec::new() };
        let out = (self.0)(&mut fields);
        if fields.issues.is_empty() {
            Ok(out)
        } else {
            Err(fields.issues)
        }
    }
}

//! Data validation: Remix's `remix/data-schema` in Rust.
//!
//! A [`Schema`] validates a [`Value`] (the counterpart of JavaScript's `unknown`) and
//! produces a typed output. Schemas compose: wrap them ([`optional`], [`defaulted`],
//! [`array`], [`object`]), add checks with [`Schema::pipe`] and [`Schema::refine`], and
//! change the output with [`Schema::transform`]. Validation is synchronous and collects
//! every issue unless [`ParseOptions::abort_early`] is set; issue messages can be replaced
//! (or localized) with an [`ErrorMap`].
//!
//! [`form_data`] reads typed values from a [`FormData`] or [`SearchParams`], [`checks`]
//! has the reusable checks and [`coerce`] the schemas that convert strings.
//!
//! ```
//! use rmx_data_schema::{self as s, checks::min_length, coerce, form_data as f, Schema, SearchParams};
//!
//! #[derive(Debug)]
//! struct Signup {
//!     email: String,
//!     password: String,
//!     age: f64,
//! }
//!
//! let schema = f::object(|o| Signup {
//!     email: o.field("email", s::defaulted(s::string(), String::new())),
//!     password: o.field("password", s::string().pipe(min_length(8))),
//!     age: o.field("age", coerce::number()),
//! });
//!
//! let signup = s::parse(&schema, SearchParams::parse("email=ada%40example.com&password=correct-horse&age=37")).unwrap();
//! assert_eq!((signup.email.as_str(), signup.age), ("ada@example.com", 37.0));
//!
//! let error = s::parse(&schema, SearchParams::parse("password=short&age=x")).unwrap_err();
//! let messages: Vec<String> = error.issues.iter().map(|issue| issue.to_string()).collect();
//! assert_eq!(messages, ["password: Expected at least 8 characters", "age: Expected number"]);
//! ```
//!
//! Differences from Remix: there are no `bigint`, `symbol`, `map`, `set`, `instanceof_`
//! or `coerce.date` schemas (JavaScript-only types; [`file`] stands in for
//! `instanceof_(File)`), and [`coerce::integer`] is added for Rust's integer types.
//! [`object`] builds a Rust value with a closure, so a field whose check fails takes its
//! type's `Default` while the remaining fields are validated; the object then fails with
//! every issue. `parse` returns a `Result`, so there is no separate `parseSafe`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

pub mod checks;
pub mod coerce;
pub mod form_data;

pub use rmx_fetch::{File, FormData, FormValue};
pub use rmx_url::SearchParams;

// ---------------------------------------------------------------------------------------
// Values, issues and options
// ---------------------------------------------------------------------------------------

/// A value to validate: the JavaScript values a Remix schema sees, plus the form sources.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    /// Properties in insertion order.
    Object(Vec<(String, Value)>),
    File(File),
    FormData(FormData),
    SearchParams(SearchParams),
}

impl Value {
    /// An object value from `(key, value)` pairs.
    pub fn object<K: Into<String>, V: Into<Value>>(entries: impl IntoIterator<Item = (K, V)>) -> Value {
        Value::Object(entries.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }

    /// A property of an object value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }
}

/// A number the way JavaScript's `String(n)` writes it.
pub(crate) fn js_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if n == 0.0 {
        "0".into()
    } else {
        n.to_string()
    }
}

/// JavaScript's `String(value)`.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undefined => f.write_str("undefined"),
            Value::Null => f.write_str("null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) => f.write_str(&js_number(*n)),
            Value::String(s) => f.write_str(s),
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    if !matches!(item, Value::Undefined | Value::Null) {
                        write!(f, "{item}")?;
                    }
                }
                Ok(())
            }
            Value::Object(_) => f.write_str("[object Object]"),
            Value::File(_) => f.write_str("[object File]"),
            Value::FormData(_) => f.write_str("[object FormData]"),
            Value::SearchParams(p) => write!(f, "{p}"),
        }
    }
}

macro_rules! from_number {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(n: $t) -> Value {
                Value::Number(n as f64)
            }
        }
    )*};
}
from_number!(f64, f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(s)
    }
}

impl From<&String> for Value {
    fn from(s: &String) -> Value {
        Value::String(s.clone())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(items: Vec<T>) -> Value {
        Value::Array(items.into_iter().map(Into::into).collect())
    }
}

/// `None` is `undefined`.
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Value {
        v.map_or(Value::Undefined, Into::into)
    }
}

impl From<&Value> for Value {
    fn from(v: &Value) -> Value {
        v.clone()
    }
}

impl From<File> for Value {
    fn from(f: File) -> Value {
        Value::File(f)
    }
}

impl From<FormData> for Value {
    fn from(f: FormData) -> Value {
        Value::FormData(f)
    }
}

impl From<&FormData> for Value {
    fn from(f: &FormData) -> Value {
        Value::FormData(f.clone())
    }
}

impl From<SearchParams> for Value {
    fn from(p: SearchParams) -> Value {
        Value::SearchParams(p)
    }
}

impl From<&SearchParams> for Value {
    fn from(p: &SearchParams) -> Value {
        Value::SearchParams(p.clone())
    }
}

#[cfg(feature = "json")]
impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(items) => Value::Array(items.into_iter().map(Value::from).collect()),
            serde_json::Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, Value::from(v))).collect()),
        }
    }
}

#[cfg(feature = "json")]
impl From<&serde_json::Value> for Value {
    fn from(v: &serde_json::Value) -> Value {
        Value::from(v.clone())
    }
}

/// One step of an issue's path: an object key or an array index.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

impl From<&str> for PathSegment {
    fn from(key: &str) -> PathSegment {
        PathSegment::Key(key.to_string())
    }
}

impl From<String> for PathSegment {
    fn from(key: String) -> PathSegment {
        PathSegment::Key(key)
    }
}

impl From<usize> for PathSegment {
    fn from(index: usize) -> PathSegment {
        PathSegment::Index(index)
    }
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathSegment::Key(k) => f.write_str(k),
            PathSegment::Index(i) => write!(f, "{i}"),
        }
    }
}

/// A validation failure: a message and where in the input it happened (empty for the
/// root value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub message: String,
    pub path: Vec<PathSegment>,
}

impl Issue {
    /// Remix's `createIssue`.
    pub fn new(message: impl Into<String>, path: Vec<PathSegment>) -> Issue {
        Issue { message: message.into(), path }
    }
}

/// `message`, or `a.b.0: message` with a path.
impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.path.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{segment}")?;
        }
        if !self.path.is_empty() {
            f.write_str(": ")?;
        }
        f.write_str(&self.message)
    }
}

/// Returned by [`parse`] when validation fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub issues: Vec<Issue>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Validation failed")
    }
}

impl std::error::Error for ValidationError {}

/// What an [`ErrorMap`] receives for an issue with a code.
#[derive(Debug)]
pub struct ErrorMapContext<'a> {
    /// Such as `type.string` or `string.min_length`.
    pub code: &'a str,
    pub default_message: &'a str,
    pub path: &'a [PathSegment],
    /// The check's parameters, such as `("min", 3)`.
    pub values: &'a [(&'static str, Value)],
    pub input: &'a Value,
    pub locale: Option<&'a str>,
}

/// Replaces issue messages: return `None` to keep the default message.
pub type ErrorMap = Arc<dyn Fn(&ErrorMapContext<'_>) -> Option<String> + Send + Sync>;

/// Options for [`parse_with`].
#[derive(Clone, Default)]
pub struct ParseOptions {
    /// Stop at the first issue instead of collecting all of them.
    pub abort_early: bool,
    pub error_map: Option<ErrorMap>,
    /// Passed to the error map.
    pub locale: Option<String>,
}

impl ParseOptions {
    /// Options with `f` as the error map.
    pub fn with_error_map(f: impl Fn(&ErrorMapContext<'_>) -> Option<String> + Send + Sync + 'static) -> ParseOptions {
        ParseOptions { error_map: Some(Arc::new(f)), ..ParseOptions::default() }
    }
}

impl fmt::Debug for ParseOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParseOptions")
            .field("abort_early", &self.abort_early)
            .field("error_map", &self.error_map.as_ref().map(|_| "Fn"))
            .field("locale", &self.locale)
            .finish()
    }
}

/// Where validation is in the input, and the parse options. Passed to
/// [`Schema::validate`]; custom schemas use it to create issues.
pub struct Context<'o> {
    path: Vec<PathSegment>,
    options: &'o ParseOptions,
}

impl<'o> Context<'o> {
    pub fn new(options: &'o ParseOptions) -> Context<'o> {
        Context { path: Vec::new(), options }
    }

    pub fn path(&self) -> &[PathSegment] {
        &self.path
    }

    pub fn options(&self) -> &ParseOptions {
        self.options
    }

    pub fn abort_early(&self) -> bool {
        self.options.abort_early
    }

    /// Run `f` with `segment` added to the path.
    pub fn at<T>(&mut self, segment: impl Into<PathSegment>, f: impl FnOnce(&mut Context<'o>) -> T) -> T {
        self.path.push(segment.into());
        let out = f(self);
        self.path.pop();
        out
    }

    /// An issue at the current path, its message from the error map when it has one.
    pub fn issue(&self, code: &str, default_message: &str, input: &Value, values: &[(&'static str, Value)]) -> Issue {
        let context = ErrorMapContext {
            code,
            default_message,
            path: &self.path,
            values,
            input,
            locale: self.options.locale.as_deref(),
        };
        let message = self.options.error_map.as_ref().and_then(|map| map(&context)).unwrap_or_else(|| default_message.to_string());
        Issue::new(message, self.path.clone())
    }

    /// An issue at the current path with a fixed message (the error map is not consulted).
    pub fn plain_issue(&self, message: impl Into<String>) -> Issue {
        Issue::new(message, self.path.clone())
    }

    /// `Err` with one [`issue`](Context::issue) (Remix's `fail`).
    pub fn fail<T>(&self, code: &str, default_message: &str, input: &Value) -> Result<T, Vec<Issue>> {
        Err(vec![self.issue(code, default_message, input, &[])])
    }
}

// ---------------------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------------------

/// Validates a [`Value`] into `Output`.
///
/// Implement it for custom schemas; the provided methods add checks and transforms.
pub trait Schema {
    type Output;

    /// Validate `value`, returning its output or every issue found (the first one only
    /// when [`Context::abort_early`]).
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<Self::Output, Vec<Issue>>;

    /// Apply a reusable [`Check`] after this schema succeeds. Chain `pipe` calls for
    /// several checks; they run in order and the first failure is reported.
    fn pipe(self, check: Check<Self::Output>) -> Pipe<Self>
    where
        Self: Sized,
    {
        Pipe { inner: self, check }
    }

    /// Fail with `Refinement failed` (code `refine.failed`) unless `predicate` holds.
    fn refine<F>(self, predicate: F) -> Refine<Self, F>
    where
        Self: Sized,
        F: Fn(&Self::Output) -> bool,
    {
        Refine { inner: self, predicate, message: None }
    }

    /// Fail with `message` unless `predicate` holds.
    fn refine_with<F>(self, predicate: F, message: impl Into<String>) -> Refine<Self, F>
    where
        Self: Sized,
        F: Fn(&Self::Output) -> bool,
    {
        Refine { inner: self, predicate, message: Some(message.into()) }
    }

    /// Map the output once validation succeeds.
    fn transform<F, U>(self, f: F) -> Transform<Self, F>
    where
        Self: Sized,
        F: Fn(Self::Output) -> U,
    {
        Transform { inner: self, f }
    }

    /// Box the schema, for [`union`], [`variant`] and recursive schemas.
    fn boxed<'s>(self) -> BoxSchema<'s, Self::Output>
    where
        Self: Sized + Send + Sync + 's,
    {
        Box::new(self)
    }
}

/// A boxed schema producing `T`.
pub type BoxSchema<'s, T> = Box<dyn Schema<Output = T> + Send + Sync + 's>;

impl<S: Schema + ?Sized> Schema for Box<S> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        (**self).validate(value, cx)
    }
}

impl<S: Schema + ?Sized> Schema for &S {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        (**self).validate(value, cx)
    }
}

impl<S: Schema + ?Sized> Schema for Arc<S> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        (**self).validate(value, cx)
    }
}

/// Validate `value` with default options.
pub fn parse<S: Schema + ?Sized>(schema: &S, value: impl Into<Value>) -> Result<S::Output, ValidationError> {
    parse_with(schema, value, &ParseOptions::default())
}

/// Validate `value` with `options` (abort early, error map, locale).
pub fn parse_with<S: Schema + ?Sized>(schema: &S, value: impl Into<Value>, options: &ParseOptions) -> Result<S::Output, ValidationError> {
    let value = value.into();
    let mut cx = Context::new(options);
    schema.validate(&value, &mut cx).map_err(|issues| ValidationError { issues })
}

/// A reusable check for [`Schema::pipe`]: a predicate plus the code, values and message
/// of the issue it reports.
pub struct Check<T: ?Sized> {
    test: Box<dyn Fn(&T) -> bool + Send + Sync>,
    pub message: Option<String>,
    /// Passed to the error map; a check without a code reports its message as is.
    pub code: Option<&'static str>,
    pub values: Vec<(&'static str, Value)>,
}

impl<T: ?Sized> Check<T> {
    pub fn new(test: impl Fn(&T) -> bool + Send + Sync + 'static) -> Check<T> {
        Check { test: Box::new(test), message: None, code: None, values: Vec::new() }
    }

    pub fn message(mut self, message: impl Into<String>) -> Check<T> {
        self.message = Some(message.into());
        self
    }

    pub fn code(mut self, code: &'static str) -> Check<T> {
        self.code = Some(code);
        self
    }

    pub fn value(mut self, key: &'static str, value: impl Into<Value>) -> Check<T> {
        self.values.push((key, value.into()));
        self
    }

    pub fn check(&self, value: &T) -> bool {
        (self.test)(value)
    }
}

impl<T: ?Sized> fmt::Debug for Check<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Check").field("code", &self.code).field("values", &self.values).field("message", &self.message).finish()
    }
}

/// [`Schema::pipe`].
pub struct Pipe<S: Schema> {
    inner: S,
    check: Check<S::Output>,
}

impl<S: Schema> Schema for Pipe<S> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        let out = self.inner.validate(value, cx)?;
        if self.check.check(&out) {
            return Ok(out);
        }
        let message = self.check.message.as_deref().unwrap_or("Check failed");
        Err(vec![match self.check.code {
            Some(code) => cx.issue(code, message, value, &self.check.values),
            None => cx.plain_issue(message),
        }])
    }
}

/// [`Schema::refine`].
pub struct Refine<S, F> {
    inner: S,
    predicate: F,
    message: Option<String>,
}

impl<S: Schema, F: Fn(&S::Output) -> bool> Schema for Refine<S, F> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        let out = self.inner.validate(value, cx)?;
        if (self.predicate)(&out) {
            return Ok(out);
        }
        Err(vec![match &self.message {
            Some(message) => cx.plain_issue(message.clone()),
            None => cx.issue("refine.failed", "Refinement failed", value, &[]),
        }])
    }
}

/// [`Schema::transform`].
pub struct Transform<S, F> {
    inner: S,
    f: F,
}

impl<S: Schema, F: Fn(S::Output) -> U, U> Schema for Transform<S, F> {
    type Output = U;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<U, Vec<Issue>> {
        self.inner.validate(value, cx).map(&self.f)
    }
}

/// A schema from a validation function (Remix's `createSchema`).
pub fn create_schema<T, F>(validate: F) -> FnSchema<F>
where
    F: Fn(&Value, &mut Context<'_>) -> Result<T, Vec<Issue>>,
{
    FnSchema(validate)
}

/// [`create_schema`].
pub struct FnSchema<F>(F);

impl<T, F: Fn(&Value, &mut Context<'_>) -> Result<T, Vec<Issue>>> Schema for FnSchema<F> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        (self.0)(value, cx)
    }
}

macro_rules! primitive {
    ($(#[$doc:meta])* $fn:ident, $ty:ident, $out:ty, $code:literal, $message:literal, |$v:ident| $pat:expr) => {
        $(#[$doc])*
        pub fn $fn() -> $ty {
            $ty
        }

        #[derive(Clone, Copy, Debug, Default)]
        pub struct $ty;

        impl Schema for $ty {
            type Output = $out;
            fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<$out, Vec<Issue>> {
                let $v = value;
                match $pat {
                    Some(out) => Ok(out),
                    None => cx.fail($code, $message, value),
                }
            }
        }
    };
}

primitive!(
    /// Any value, unchanged.
    any, AnySchema, Value, "any", "", |v| Some(v.clone())
);
primitive!(
    /// A string.
    string, StringSchema, String, "type.string", "Expected string", |v| match v {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
);
primitive!(
    /// A finite number (not `NaN` or an infinity).
    number, NumberSchema, f64, "type.number", "Expected number", |v| match v {
        Value::Number(n) if n.is_finite() => Some(*n),
        _ => None,
    }
);
primitive!(
    /// A boolean.
    boolean, BooleanSchema, bool, "type.boolean", "Expected boolean", |v| match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
);
primitive!(
    /// `null`.
    null_, NullSchema, (), "type.null", "Expected null", |v| matches!(v, Value::Null).then_some(())
);
primitive!(
    /// `undefined` (a missing value).
    undefined_, UndefinedSchema, (), "type.undefined", "Expected undefined", |v| matches!(v, Value::Undefined).then_some(())
);
primitive!(
    /// A [`File`] (Remix's `instanceof_(File)`).
    file, FileSchema, File, "instanceof.invalid_type", "Expected instance of File", |v| match v {
        Value::File(f) => Some(f.clone()),
        _ => None,
    }
);

/// Exactly `value`.
pub fn literal<T: Clone + Into<Value>>(value: T) -> Literal<T> {
    Literal { expected: value.clone().into(), value }
}

/// [`literal`].
pub struct Literal<T> {
    value: T,
    expected: Value,
}

impl<T: Clone> Schema for Literal<T> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        if *value == self.expected {
            return Ok(self.value.clone());
        }
        Err(vec![cx.issue("literal.invalid_value", "Expected literal value", value, &[("expected", self.expected.clone())])])
    }
}

/// One of `values` (Remix's `enum_`).
pub fn enum_<T: Clone + Into<Value>>(values: impl IntoIterator<Item = T>) -> Enum<T> {
    Enum { allowed: values.into_iter().map(|v| (v.clone().into(), v)).collect() }
}

/// [`enum_`].
pub struct Enum<T> {
    allowed: Vec<(Value, T)>,
}

impl<T: Clone> Schema for Enum<T> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        if let Some((_, out)) = self.allowed.iter().find(|(v, _)| v == value) {
            return Ok(out.clone());
        }
        let list: Vec<String> = self.allowed.iter().map(|(v, _)| v.to_string()).collect();
        let values = [("values", Value::Array(self.allowed.iter().map(|(v, _)| v.clone()).collect()))];
        Err(vec![cx.issue("enum.invalid_value", &format!("Expected one of: {}", list.join(", ")), value, &values)])
    }
}

/// `schema`, or `None` for `undefined`.
pub fn optional<S: Schema>(schema: S) -> Optional<S> {
    Optional(schema)
}

/// [`optional`].
pub struct Optional<S>(S);

impl<S: Schema> Schema for Optional<S> {
    type Output = Option<S::Output>;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<Self::Output, Vec<Issue>> {
        match value {
            Value::Undefined => Ok(None),
            _ => self.0.validate(value, cx).map(Some),
        }
    }
}

/// `schema`, or `None` for `null`.
pub fn nullable<S: Schema>(schema: S) -> Nullable<S> {
    Nullable(schema)
}

/// [`nullable`].
pub struct Nullable<S>(S);

impl<S: Schema> Schema for Nullable<S> {
    type Output = Option<S::Output>;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<Self::Output, Vec<Issue>> {
        match value {
            Value::Null => Ok(None),
            _ => self.0.validate(value, cx).map(Some),
        }
    }
}

/// `schema`, or `default` for `undefined`.
pub fn defaulted<S: Schema>(schema: S, default: S::Output) -> Defaulted<S, impl Fn() -> S::Output>
where
    S::Output: Clone,
{
    Defaulted { schema, default: move || default.clone() }
}

/// `schema`, or the result of `default()` for `undefined`.
pub fn defaulted_with<S: Schema, F: Fn() -> S::Output>(schema: S, default: F) -> Defaulted<S, F> {
    Defaulted { schema, default }
}

/// [`defaulted`].
pub struct Defaulted<S, F> {
    schema: S,
    default: F,
}

impl<S: Schema, F: Fn() -> S::Output> Schema for Defaulted<S, F> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        match value {
            Value::Undefined => Ok((self.default)()),
            _ => self.schema.validate(value, cx),
        }
    }
}

/// An array whose items all match `items`.
pub fn array<S: Schema>(items: S) -> Array<S> {
    Array(items)
}

/// [`array`].
pub struct Array<S>(S);

impl<S: Schema> Schema for Array<S> {
    type Output = Vec<S::Output>;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<Self::Output, Vec<Issue>> {
        let Value::Array(items) = value else { return cx.fail("type.array", "Expected array", value) };
        let mut issues = Vec::new();
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            match cx.at(i, |cx| self.0.validate(item, cx)) {
                Ok(v) => out.push(v),
                Err(found) if cx.abort_early() => return Err(found),
                Err(found) => issues.extend(found),
            }
        }
        if issues.is_empty() {
            Ok(out)
        } else {
            Err(issues)
        }
    }
}

/// A fixed-length array: `tuple((s::string(), s::number()))`.
pub fn tuple<T: TupleItems>(items: T) -> Tuple<T> {
    Tuple(items)
}

/// [`tuple`].
pub struct Tuple<T>(T);

/// Tuples of schemas that [`tuple`] accepts (up to six).
pub trait TupleItems {
    type Output;
    const LEN: usize;
    fn validate_items(&self, items: &[Value], cx: &mut Context<'_>, issues: &mut Vec<Issue>) -> Option<Self::Output>;
}

macro_rules! tuple_items {
    ($len:literal; $($s:ident $i:tt),+) => {
        impl<$($s: Schema),+> TupleItems for ($($s,)+) {
            type Output = ($($s::Output,)+);
            const LEN: usize = $len;
            #[allow(non_snake_case)]
            fn validate_items(&self, items: &[Value], cx: &mut Context<'_>, issues: &mut Vec<Issue>) -> Option<Self::Output> {
                $(
                    let $s = match items.get($i) {
                        Some(item) if !(cx.abort_early() && !issues.is_empty()) => match cx.at($i, |cx| self.$i.validate(item, cx)) {
                            Ok(v) => Some(v),
                            Err(found) => {
                                issues.extend(if cx.abort_early() { found.into_iter().take(1).collect() } else { found });
                                None
                            }
                        },
                        _ => None,
                    };
                )+
                Some(($($s?,)+))
            }
        }
    };
}
tuple_items!(1; A 0);
tuple_items!(2; A 0, B 1);
tuple_items!(3; A 0, B 1, C 2);
tuple_items!(4; A 0, B 1, C 2, D 3);
tuple_items!(5; A 0, B 1, C 2, D 3, E 4);
tuple_items!(6; A 0, B 1, C 2, D 3, E 4, F 5);

impl<T: TupleItems> Schema for Tuple<T> {
    type Output = T::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T::Output, Vec<Issue>> {
        let Value::Array(items) = value else { return cx.fail("type.array", "Expected array", value) };
        let mut issues = Vec::new();
        if items.len() != T::LEN {
            issues.push(cx.issue("tuple.length", &format!("Expected tuple length {}", T::LEN), value, &[("length", Value::from(T::LEN))]));
            if cx.abort_early() {
                return Err(issues);
            }
        }
        let out = self.0.validate_items(items, cx, &mut issues);
        match out {
            Some(out) if issues.is_empty() => Ok(out),
            _ => Err(issues),
        }
    }
}

/// An object with a fixed shape, built into a Rust value by `build`:
///
/// ```
/// use rmx_data_schema::{self as s, Value};
///
/// #[derive(Debug, PartialEq)]
/// struct User { name: String, age: f64 }
///
/// let schema = s::object(|o| User { name: o.field("name", s::string()), age: o.field("age", s::number()) });
/// let user = s::parse(&schema, Value::object([("name", Value::from("Ada")), ("age", Value::from(37))])).unwrap();
/// assert_eq!(user, User { name: "Ada".into(), age: 37.0 });
/// ```
///
/// Missing keys are `undefined`. Keys the shape does not read are ignored (Remix's default
/// `unknownKeys: 'strip'`); [`Object::strict`] reports them instead.
pub fn object<T, F: Fn(&mut Fields<'_, '_>) -> T>(build: F) -> Object<F> {
    Object { build, strict: false }
}

/// [`object`].
pub struct Object<F> {
    build: F,
    strict: bool,
}

impl<F> Object<F> {
    /// Report keys the shape does not read (Remix's `unknownKeys: 'error'`).
    pub fn strict(mut self) -> Object<F> {
        self.strict = true;
        self
    }
}

/// Reads an object's fields for [`object`].
pub struct Fields<'a, 'o> {
    input: &'a [(String, Value)],
    cx: &'a mut Context<'o>,
    issues: Vec<Issue>,
    read: Vec<String>,
}

impl Fields<'_, '_> {
    /// The value of `key` validated by `schema`. On failure the issues are recorded and
    /// the output type's `Default` stands in, so the remaining fields are still checked.
    pub fn field<S: Schema>(&mut self, key: &str, schema: S) -> S::Output
    where
        S::Output: Default,
    {
        self.try_field(key, schema).unwrap_or_default()
    }

    /// Like [`field`](Fields::field), returning `None` on failure (for outputs without a
    /// `Default`).
    pub fn try_field<S: Schema>(&mut self, key: &str, schema: S) -> Option<S::Output> {
        self.read.push(key.to_string());
        if self.cx.abort_early() && !self.issues.is_empty() {
            return None;
        }
        let input = self.input;
        let value = input.iter().find(|(k, _)| k == key).map_or(&Value::Undefined, |(_, v)| v);
        match self.cx.at(key, |cx| schema.validate(value, cx)) {
            Ok(v) => Some(v),
            Err(found) => {
                self.issues.extend(found);
                None
            }
        }
    }
}

impl<T, F: Fn(&mut Fields<'_, '_>) -> T> Schema for Object<F> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        let Value::Object(input) = value else { return cx.fail("type.object", "Expected object", value) };
        let mut fields = Fields { input, cx, issues: Vec::new(), read: Vec::new() };
        let out = (self.build)(&mut fields);
        let Fields { cx, mut issues, read, .. } = fields;
        if self.strict && !(cx.abort_early() && !issues.is_empty()) {
            for (key, v) in input.iter().filter(|(k, _)| !read.contains(k)) {
                let issue = cx.at(key.as_str(), |cx| cx.issue("object.unknown_key", "Unknown key", v, &[("key", Value::from(key))]));
                issues.push(issue);
                if cx.abort_early() {
                    break;
                }
            }
        }
        if issues.is_empty() {
            Ok(out)
        } else {
            Err(issues)
        }
    }
}

/// An object used as a map: every key matches `keys` and every value `values`.
pub fn record<K: Schema, V: Schema>(keys: K, values: V) -> Record<K, V>
where
    K::Output: Ord,
{
    Record { keys, values }
}

/// [`record`].
pub struct Record<K, V> {
    keys: K,
    values: V,
}

impl<K: Schema, V: Schema> Schema for Record<K, V>
where
    K::Output: Ord,
{
    type Output = BTreeMap<K::Output, V::Output>;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<Self::Output, Vec<Issue>> {
        let Value::Object(input) = value else { return cx.fail("type.object", "Expected object", value) };
        let mut issues = Vec::new();
        let mut out = BTreeMap::new();
        for (key, v) in input {
            let result = cx.at(key.as_str(), |cx| {
                let k = self.keys.validate(&Value::from(key), cx)?;
                Ok((k, self.values.validate(v, cx)?))
            });
            match result {
                Ok((k, v)) => {
                    out.insert(k, v);
                }
                Err(found) if cx.abort_early() => return Err(found),
                Err(found) => issues.extend(found),
            }
        }
        if issues.is_empty() {
            Ok(out)
        } else {
            Err(issues)
        }
    }
}

/// The first of `schemas` that matches. Without abort early, a failure reports the issues
/// of every option.
pub fn union<'s, T>(schemas: impl IntoIterator<Item = BoxSchema<'s, T>>) -> Union<'s, T> {
    Union(schemas.into_iter().collect())
}

/// [`union`].
pub struct Union<'s, T>(Vec<BoxSchema<'s, T>>);

impl<T> Schema for Union<'_, T> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        if self.0.is_empty() {
            return cx.fail("union.no_variants", "No union variant matched", value);
        }
        let mut issues = Vec::new();
        for schema in &self.0 {
            match schema.validate(value, cx) {
                Ok(v) => return Ok(v),
                Err(found) if cx.abort_early() => return Err(found),
                Err(found) => issues.extend(found),
            }
        }
        Err(issues)
    }
}

/// A discriminated union: the object's `discriminator` property picks the schema.
pub fn variant<'s, T, K: Into<Value>>(discriminator: &str, variants: impl IntoIterator<Item = (K, BoxSchema<'s, T>)>) -> Variant<'s, T> {
    Variant { discriminator: discriminator.to_string(), variants: variants.into_iter().map(|(k, s)| (k.into(), s)).collect() }
}

/// [`variant`].
pub struct Variant<'s, T> {
    discriminator: String,
    variants: Vec<(Value, BoxSchema<'s, T>)>,
}

impl<T> Schema for Variant<'_, T> {
    type Output = T;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<T, Vec<Issue>> {
        if !matches!(value, Value::Object(_)) {
            return cx.fail("type.object", "Expected object", value);
        }
        let values = [("discriminator", Value::from(&self.discriminator))];
        let tag = value.get(&self.discriminator).unwrap_or(&Value::Undefined);
        if tag.is_undefined() {
            return Err(vec![cx.at(self.discriminator.as_str(), |cx| cx.issue("variant.missing_discriminator", "Expected discriminator", value, &values))]);
        }
        match self.variants.iter().find(|(k, _)| k == tag) {
            Some((_, schema)) => schema.validate(value, cx),
            None => Err(vec![cx.at(self.discriminator.as_str(), |cx| cx.issue("variant.unknown_discriminator", "Unknown discriminator", tag, &values))]),
        }
    }
}

/// A schema built when it is used, for recursive shapes:
///
/// ```
/// use rmx_data_schema::{self as s, BoxSchema, Schema, Value};
///
/// #[derive(Debug, Default, PartialEq)]
/// struct Node { name: String, children: Vec<Node> }
///
/// fn node() -> BoxSchema<'static, Node> {
///     s::object(|o| Node {
///         name: o.field("name", s::string()),
///         children: o.field("children", s::defaulted_with(s::array(s::lazy(node)), Vec::new)),
///     })
///     .boxed()
/// }
///
/// let leaf = Value::object([("name", Value::from("leaf"))]);
/// let tree = Value::object([("name", Value::from("root")), ("children", Value::Array(vec![leaf]))]);
/// assert_eq!(s::parse(&node(), tree).unwrap().children[0].name, "leaf");
/// ```
pub fn lazy<S: Schema, F: Fn() -> S>(build: F) -> Lazy<F> {
    Lazy(build)
}

/// [`lazy`].
pub struct Lazy<F>(F);

impl<S: Schema, F: Fn() -> S> Schema for Lazy<F> {
    type Output = S::Output;
    fn validate(&self, value: &Value, cx: &mut Context<'_>) -> Result<S::Output, Vec<Issue>> {
        (self.0)().validate(value, cx)
    }
}

#[cfg(test)]
mod tests;

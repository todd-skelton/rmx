//! The form schemas the Remix bookstore controllers share (`remix/data-schema`).

use rsmix::data_schema::{self as s, form_data as f, Schema};
use rsmix::middleware::form_data::FormDataExt;
use rsmix::router::Context;

/// `textField`: `f.field(s.defaulted(s.string(), ''))`
pub fn text_field() -> impl Schema<Output = String> {
    s::defaulted(s::string(), String::new())
}

/// `f.field(s.optional(s.string()))`
pub fn optional_text() -> impl Schema<Output = Option<String>> {
    s::optional(s::string())
}

/// Parse the request's form data. The error lists the issues, one per line, for a 400
/// response (Remix's `s.parse` throws a `ValidationError`).
pub fn parse_form<S: Schema>(ctx: &Context, schema: S) -> Result<S::Output, String> {
    s::parse(&schema, ctx.form_data()).map_err(|e| e.issues.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
}

/// One text field (`textField`), `""` when missing.
pub fn text(ctx: &Context, name: &str) -> String {
    parse_form(ctx, f::object(|o| o.field(name, text_field()))).unwrap_or_default()
}

/// The `bookId` field (`bookIdField`).
pub fn book_id(ctx: &Context) -> Option<String> {
    parse_form(ctx, f::object(|o| o.field("bookId", optional_text()))).ok().flatten()
}

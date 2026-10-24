//! Escaped HTML templates, ported from `remix/html-template`.
//!
//! [html!] takes a format-style string literal and escapes every interpolation unless it is
//! already [SafeHtml]. [html_raw!] is the same without escaping, for trusted snippets.
//! Placeholders are `{name}` (a named argument or a variable in scope), `{}` and `{0}`
//! (positional arguments); `{{` and `}}` are literal braces.
//!
//! ```
//! use rsmix_html_template::{html, html_raw, SafeHtml};
//!
//! let unsafe_text = "<script>alert(1)</script>";
//! assert_eq!(html!("<h1>{unsafe_text}</h1>"), "<h1>&lt;script&gt;alert(1)&lt;/script&gt;</h1>");
//!
//! let icon: SafeHtml = html_raw!("<b>{}</b>", "OK");
//! let items = vec![html!("<li>{}</li>", 1), html!("<li>{}</li>", 2)];
//! assert_eq!(html!("{icon}<ul>{items}</ul>"), "<b>OK</b><ul><li>1</li><li>2</li></ul>");
//! ```
//!
//! Mistakes are compile errors, as with `format!`:
//!
//! ```compile_fail
//! let _ = rsmix_html_template::html!("<p>{}</p>"); // no argument for {}
//! ```
//!
//! ```compile_fail
//! let _ = rsmix_html_template::html!("<p>{x:?}</p>", x = 1); // format specs are not supported
//! ```

use std::borrow::Cow;
use std::fmt;

pub use rsmix_macros::{html, html_raw};

// The macros expand to `::rsmix_html_template::..`, which must also resolve inside this crate.
extern crate self as rsmix_html_template;

/// A string that is safe to render as HTML without escaping.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SafeHtml(String);

impl SafeHtml {
    /// Mark trusted markup as safe. Prefer [html!], which escapes what it interpolates.
    pub fn from_trusted(markup: impl Into<String>) -> SafeHtml {
        SafeHtml(markup.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for SafeHtml {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<SafeHtml> for String {
    fn from(html: SafeHtml) -> String {
        html.0
    }
}

impl PartialEq<str> for SafeHtml {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for SafeHtml {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// Escape `& < > " '` for text and attribute values.
pub fn escape_html(text: &str) -> Cow<'_, str> {
    if !text.contains(['&', '<', '>', '"', '\'']) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    push_escaped(&mut out, text);
    Cow::Owned(out)
}

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// A value that can be interpolated into a template. Strings are escaped by [html!],
/// [SafeHtml] never is, `None` renders nothing and sequences are concatenated.
pub trait ToHtml {
    /// Append, escaping anything that is not already safe.
    fn write_html(&self, out: &mut String);
    /// Append without escaping.
    fn write_raw(&self, out: &mut String);
}

impl ToHtml for SafeHtml {
    fn write_html(&self, out: &mut String) {
        out.push_str(&self.0);
    }
    fn write_raw(&self, out: &mut String) {
        out.push_str(&self.0);
    }
}

impl ToHtml for str {
    fn write_html(&self, out: &mut String) {
        push_escaped(out, self);
    }
    fn write_raw(&self, out: &mut String) {
        out.push_str(self);
    }
}

impl ToHtml for String {
    fn write_html(&self, out: &mut String) {
        self.as_str().write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        out.push_str(self);
    }
}

impl ToHtml for Cow<'_, str> {
    fn write_html(&self, out: &mut String) {
        self.as_ref().write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        out.push_str(self);
    }
}

impl ToHtml for char {
    fn write_html(&self, out: &mut String) {
        self.encode_utf8(&mut [0; 4]).write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        out.push(*self);
    }
}

macro_rules! display_to_html {
    ($($t:ty),*) => {$(
        impl ToHtml for $t {
            fn write_html(&self, out: &mut String) {
                use std::fmt::Write;
                let _ = write!(out, "{self}");
            }
            fn write_raw(&self, out: &mut String) {
                self.write_html(out);
            }
        }
    )*};
}

display_to_html!(bool, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

impl<T: ToHtml + ?Sized> ToHtml for &T {
    fn write_html(&self, out: &mut String) {
        (**self).write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        (**self).write_raw(out);
    }
}

impl<T: ToHtml + ?Sized> ToHtml for Box<T> {
    fn write_html(&self, out: &mut String) {
        (**self).write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        (**self).write_raw(out);
    }
}

impl<T: ToHtml> ToHtml for Option<T> {
    fn write_html(&self, out: &mut String) {
        if let Some(value) = self {
            value.write_html(out);
        }
    }
    fn write_raw(&self, out: &mut String) {
        if let Some(value) = self {
            value.write_raw(out);
        }
    }
}

impl<T: ToHtml> ToHtml for [T] {
    fn write_html(&self, out: &mut String) {
        self.iter().for_each(|item| item.write_html(out));
    }
    fn write_raw(&self, out: &mut String) {
        self.iter().for_each(|item| item.write_raw(out));
    }
}

impl<T: ToHtml, const N: usize> ToHtml for [T; N] {
    fn write_html(&self, out: &mut String) {
        self.as_slice().write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        self.as_slice().write_raw(out);
    }
}

impl<T: ToHtml> ToHtml for Vec<T> {
    fn write_html(&self, out: &mut String) {
        self.as_slice().write_html(out);
    }
    fn write_raw(&self, out: &mut String) {
        self.as_slice().write_raw(out);
    }
}

#[doc(hidden)]
pub mod __private {
    use super::{SafeHtml, ToHtml};

    /// What [html!](crate::html) expands to.
    pub struct Builder(String);

    impl Builder {
        pub fn new(capacity: usize) -> Builder {
            Builder(String::with_capacity(capacity))
        }
        pub fn push_str(&mut self, markup: &'static str) {
            self.0.push_str(markup);
        }
        pub fn push<T: ToHtml + ?Sized>(&mut self, value: &T) {
            value.write_html(&mut self.0);
        }
        pub fn push_raw<T: ToHtml + ?Sized>(&mut self, value: &T) {
            value.write_raw(&mut self.0);
        }
        pub fn finish(self) -> SafeHtml {
            SafeHtml(self.0)
        }
    }
}

#[cfg(test)]
mod tests;

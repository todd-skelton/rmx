//! Schemas that convert strings (form and query values) into other types
//! (`remix/data-schema/coerce`).

use crate::{Context, Issue, Schema, Value};

/// JavaScript's `Number(trimmed)` for the inputs a form sends: decimal and exponent
/// notation, and `0x`, `0o`, `0b` integers.
fn js_number(s: &str) -> Option<f64> {
    let radix = |prefix: &str, radix| s.get(..2).filter(|p| p.eq_ignore_ascii_case(prefix)).and_then(|_| u64::from_str_radix(&s[2..], radix).ok()).map(|n| n as f64);
    if let Some(n) = radix("0x", 16).or_else(|| radix("0o", 8)).or_else(|| radix("0b", 2)) {
        return Some(n);
    }
    // Rust also accepts "inf" and "nan"; JavaScript does not (and they are not finite).
    if s.bytes().any(|b| b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E')) {
        return None;
    }
    s.parse::<f64>().ok()
}

macro_rules! coercion {
    ($(#[$doc:meta])* $fn:ident, $ty:ident, $out:ty, $code:literal, $message:literal, |$v:ident| $body:expr) => {
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
                let out: Option<$out> = $body;
                match out {
                    Some(out) => Ok(out),
                    None => cx.fail($code, $message, value),
                }
            }
        }
    };
}

coercion!(
    /// A finite number, or a string that parses to one (an empty string does not).
    number, CoerceNumber, f64, "coerce.number", "Expected number", |v| match v {
        Value::Number(n) => Some(*n),
        Value::String(s) => js_number(s.trim()),
        _ => None,
    }
    .filter(|n| n.is_finite())
);
coercion!(
    /// A boolean, or `"true"` / `"false"` (trimmed, any case).
    boolean, CoerceBoolean, bool, "coerce.boolean", "Expected boolean", |v| match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
);
coercion!(
    /// A whole number, or a string holding one (Remix's `coerce.bigint`, as an `i64`).
    integer, CoerceInteger, i64, "coerce.bigint", "Expected bigint", |v| match v {
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= i64::MAX as f64 => Some(*n as i64),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
);
coercion!(
    /// A string, or a number or boolean written as one.
    string, CoerceString, String, "coerce.string", "Expected string", |v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(v.to_string()),
        _ => None,
    }
);

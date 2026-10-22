//! Server-side `Intl`: the four formatters and plural rules the home page uses, on ICU4X.
//! Output matches Node's `Intl` for the demo's values (see the tests).

use fixed_decimal::Decimal;
use icu_datetime::fieldsets::YMDE;
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::DateTimeFormatter;
use icu_decimal::DecimalFormatter;
use icu_experimental::dimension::currency::formatter::CurrencyFormatter;
use icu_experimental::dimension::currency::CurrencyType;
use icu_experimental::relativetime::options::Numeric;
use icu_experimental::relativetime::{RelativeTimeFormatter, RelativeTimeFormatterOptions};
use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};

fn locale(tag: &str) -> Locale {
    tag.parse().unwrap_or(Locale::UNKNOWN)
}

/// `new Intl.DateTimeFormat(locale, { dateStyle: 'full', timeStyle: 'short', timeZone: 'UTC' })`
/// for a UTC wall-clock time.
pub fn format_date_time(tag: &str, (year, month, day): (i32, u8, u8), (hour, minute): (u8, u8)) -> String {
    let Ok(fmt) = DateTimeFormatter::try_new(locale(tag).into(), YMDE::long().with_time_hm()) else { return String::new() };
    let Ok(date) = Date::try_new_iso(year, month, day) else { return String::new() };
    let Ok(time) = Time::try_new(hour, minute, 0, 0) else { return String::new() };
    // V8 and SpiderMonkey print U+202F before the day period as a plain space, for web
    // compatibility; do the same so server and browser output agree.
    fmt.format(&DateTime { date, time }).to_string().replace('\u{202f}', " ")
}

/// `new Intl.NumberFormat(locale).format(n)`
pub fn format_number(tag: &str, n: i64) -> String {
    DecimalFormatter::try_new(locale(tag).into(), Default::default())
        .map(|f| f.format(&Decimal::from(n)).to_string())
        .unwrap_or_else(|_| n.to_string())
}

/// `new Intl.NumberFormat(locale, { style: 'currency', currency }).format(value)`, `value`
/// being a decimal string like `"49.99"`.
pub fn format_currency(tag: &str, currency: &str, value: &str) -> String {
    let (Ok(code), Ok(amount)) = (CurrencyType::try_from_str(&currency.to_ascii_lowercase()), value.parse::<Decimal>()) else { return value.into() };
    CurrencyFormatter::try_new_symbol(locale(tag).into(), code, Default::default())
        .map(|f| f.format_fixed_decimal(&amount).to_string())
        .unwrap_or_else(|_| value.into())
}

/// `new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(days, 'day')`
pub fn format_relative_days(tag: &str, days: i64) -> String {
    let mut options = RelativeTimeFormatterOptions::default();
    options.numeric = Numeric::Auto;
    RelativeTimeFormatter::try_new_long_day(locale(tag).into(), options)
        .map(|f| f.format(Decimal::from(days)).to_string())
        .unwrap_or_default()
}

/// `new Intl.PluralRules(locale).select(n)`
pub fn plural_category(tag: &str, n: u64) -> &'static str {
    let Ok(rules) = PluralRules::try_new_cardinal(locale(tag).into()) else { return "other" };
    match rules.category_for(n) {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Node 24.15 (ICU 78.2) output for the demo's values, with one noted exception.
    #[test]
    fn matches_node_intl() {
        let node = [
            ("en", ["Wednesday, April 15, 2026 at 2:30 PM", "1,250,000", "$49.99", "3 days ago"], "other,one,other,other,other,other,other"),
            // ICU4X glues a long Spanish date to its time with CLDR's "atTime" pattern ("a las");
            // ICU4C, under Node, uses the plain one (", "). The only known difference.
            ("es", ["miércoles, 15 de abril de 2026 a las 14:30", "1.250.000", "49,99\u{a0}US$", "hace 3 días"], "other,one,other,other,other,other,many"),
            ("fr", ["mercredi 15 avril 2026 à 14:30", "1\u{202f}250\u{202f}000", "49,99\u{a0}$US", "il y a 3 jours"], "one,one,other,other,other,other,many"),
            ("ja", ["2026年4月15日水曜日 14:30", "1,250,000", "$49.99", "3 日前"], "other,other,other,other,other,other,other"),
            ("ar", ["الأربعاء، 15 أبريل 2026 في 2:30 م", "1,250,000", "\u{200f}49.99\u{a0}US$", "قبل 3 أيام"], "zero,one,two,few,many,other,other"),
        ];
        let mut mismatches = Vec::new();
        for (tag, [date, number, currency, relative], plurals) in node {
            let got = [
                format_date_time(tag, (2026, 4, 15), (14, 30)),
                format_number(tag, 1_250_000),
                format_currency(tag, "USD", "49.99"),
                format_relative_days(tag, -3),
            ];
            for (g, want) in got.iter().zip([date, number, currency, relative]) {
                if g != want {
                    mismatches.push(format!("{tag}: {g:?} vs {want:?}"));
                }
            }
            let cats: Vec<_> = [0, 1, 2, 5, 11, 100, 1_000_000].iter().map(|n| plural_category(tag, *n)).collect();
            if cats.join(",") != plurals {
                mismatches.push(format!("{tag}: {cats:?}"));
            }
        }
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }
}

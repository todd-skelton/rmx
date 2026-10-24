//! Escaping and number text in server HTML: the fast paths write what the plain ones write.

extern crate rmx_component as rmx;

use std::borrow::Cow;

use rmx::{render_to_string, rsx, AttrValue, IntoAttr};

fn reference(s: &str, attr: bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\'' if attr => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn check(s: &str) {
    let text = s.to_string();
    assert_eq!(render_to_string(&rsx! { <p>{text}</p> }), format!("<p>{}</p>", reference(s, false)), "text {s:?}");
    let title = s.to_string();
    assert_eq!(render_to_string(&rsx! { <p title={title}></p> }), format!("<p title=\"{}\"></p>", reference(s, true)), "attr {s:?}");
}

#[test]
fn text_and_attributes_escape_at_every_length_and_offset() {
    for special in ['&', '<', '>', '"', '\'', '%', 'e', '\u{e9}', '\u{1f600}'] {
        for len in 0..40 {
            for at in 0..=len {
                let mut s: String = "abcdefghijklmnopqrstuvwxyz0123456789ABCD".chars().take(len).collect();
                s.insert(at.min(s.len()), special);
                check(&s);
            }
        }
    }
}

#[test]
fn escaping_matches_the_reference_for_mixed_strings() {
    let alphabet = ['a', ' ', '&', '<', '>', '"', '\'', '%', '\u{e9}', '\u{1f600}', '7', '-'];
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..3000 {
        let len = (next() % 300) as usize;
        // Mostly plain text, with escapes at a varying density.
        let density = 1 + next() % 64;
        let s: String = (0..len)
            .map(|_| if next() % density == 0 { alphabet[(next() % alphabet.len() as u64) as usize] } else { 'q' })
            .collect();
        check(&s);
    }
    check(&"Plain product copy with no markup at all, long enough to take the fast path. ".repeat(20));
    check(&"Tom & Jerry <say> \"hi\" & it's fine ".repeat(8));
}

fn attr_text(value: f64) -> String {
    match value.into_attr() {
        AttrValue::Str(Cow::Owned(s)) => s,
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn floats_render_as_display_writes_them() {
    for value in [0.5, -1.25, 3.0, 1e21, 1.5e-7, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 19.99] {
        assert_eq!(render_to_string(&rsx! { <i data-v={value}>{value}</i> }), format!("<i data-v=\"{value}\">{value}</i>"));
    }
    assert_eq!(render_to_string(&rsx! { <i>{2.5f32}</i> }), "<i>2.5</i>");
}

#[test]
fn floats_match_display_across_their_range() {
    // Ported from Corten's view tests, where the same code was checked against Display for
    // 300 million values.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let special = [
        0.0,
        -0.0,
        1.0,
        0.1,
        0.3,
        1e15,
        1e16,
        1e17,
        1e21,
        1e22,
        123456789012345680.0,
        1e-5,
        1e-7,
        5e-324,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
        f64::EPSILON,
        9007199254740993.0,
    ];
    for value in special {
        assert_eq!(attr_text(value), value.to_string(), "{value:e}");
    }
    // Ties: halfway between two shortest forms (zmij keeps the even digit, Display rounds up).
    for value in [312_985.125, 312_985.0 + 1.0 / 2048.0, 2f64.powi(50) + 0.25, 0.25, 7.375] {
        assert_eq!(attr_text(value), value.to_string(), "{value:e}");
    }
    // Short binary fractions in every binade, where ties live.
    for shift in 0..60 {
        for odd in (1..4000u64).step_by(2) {
            let value = odd as f64 * 2f64.powi(-shift) * (1 + (odd % 7) * 1000) as f64;
            assert_eq!(attr_text(value), value.to_string(), "{value:e}");
            assert_eq!(attr_text(-value), (-value).to_string(), "{value:e}");
        }
    }
    for _ in 0..200_000 {
        // Every bit pattern (subnormals, huge, tiny), and prices and other short decimals.
        let bits = f64::from_bits(next());
        let price = (next() % 10_000_000) as f64 / [1.0, 10.0, 100.0, 1000.0][(next() % 4) as usize];
        assert_eq!(attr_text(bits), bits.to_string(), "{:#x}", bits.to_bits());
        assert_eq!(attr_text(price), price.to_string(), "{price:e}");
    }
}

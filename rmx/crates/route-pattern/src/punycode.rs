//! Punycode labels to Unicode (Remix's `match/punycode.ts`, after punycode.js), so
//! hostnames match in the form patterns are written in.

const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;
const MAX_INT: u32 = 0x7fff_ffff;

/// Decode every `xn--` label of a hostname to Unicode (`decodeHostname`). Labels that are
/// not valid punycode are left as they are, where Remix throws a `RangeError`.
pub fn decode_hostname(input: &str) -> String {
    if !input.contains("xn--") {
        return input.to_string();
    }
    let (local, domain) = match input.split_once('@') {
        Some((local, domain)) => (Some(local), domain),
        None => (None, input),
    };
    let domain = domain.replace(['\u{3002}', '\u{ff0e}', '\u{ff61}'], ".");
    let labels: Vec<String> = domain
        .split('.')
        .map(|label| match label.strip_prefix("xn--") {
            Some(encoded) => decode(&encoded.to_lowercase()).unwrap_or_else(|| label.to_string()),
            None => label.to_string(),
        })
        .collect();
    match local {
        Some(local) => format!("{local}@{}", labels.join(".")),
        None => labels.join("."),
    }
}

fn basic_to_digit(code: u32) -> u32 {
    match code {
        0x30..=0x39 => 26 + (code - 0x30),
        0x41..=0x5a => code - 0x41,
        0x61..=0x7a => code - 0x61,
        _ => BASE,
    }
}

fn adapt(delta: u32, num_points: u32, first_time: bool) -> u32 {
    let mut k = 0;
    let mut delta = if first_time { delta / DAMP } else { delta >> 1 };
    delta += delta / num_points;
    while delta > ((BASE - T_MIN) * T_MAX) >> 1 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + ((BASE - T_MIN + 1) * delta) / (delta + SKEW)
}

fn decode(input: &str) -> Option<String> {
    let input: Vec<u32> = input.chars().map(u32::from).collect();
    let basic = input.iter().rposition(|&c| c == u32::from('-')).unwrap_or(0);
    let mut output: Vec<u32> = Vec::new();
    for &c in &input[..basic] {
        if c >= 0x80 {
            return None;
        }
        output.push(c);
    }
    let (mut i, mut n, mut bias) = (0u32, INITIAL_N, INITIAL_BIAS);
    let mut index = if basic > 0 { basic + 1 } else { 0 };
    while index < input.len() {
        let old_i = i;
        let mut w = 1u32;
        let mut k = BASE;
        loop {
            let digit = basic_to_digit(*input.get(index)?);
            index += 1;
            if digit >= BASE || digit > (MAX_INT - i) / w {
                return None;
            }
            i += digit * w;
            let t = if k <= bias { T_MIN } else if k >= bias + T_MAX { T_MAX } else { k - bias };
            if digit < t {
                break;
            }
            if w > MAX_INT / (BASE - t) {
                return None;
            }
            w *= BASE - t;
            k += BASE;
        }
        let out = output.len() as u32 + 1;
        bias = adapt(i - old_i, out, old_i == 0);
        if i / out > MAX_INT - n {
            return None;
        }
        n += i / out;
        i %= out;
        output.insert(i as usize, n);
        i += 1;
    }
    output.into_iter().map(char::from_u32).collect()
}

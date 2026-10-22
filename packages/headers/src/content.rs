//! `Content-Type`, `Content-Disposition` and `Content-Range`.

use std::fmt;

use crate::params::{parse_params, quote};

/// `Content-Type`: a media type with optional `charset` and `boundary`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentType {
    pub media_type: Option<String>,
    pub charset: Option<String>,
    pub boundary: Option<String>,
}

impl ContentType {
    pub fn parse(value: Option<&str>) -> ContentType {
        let mut h = ContentType::default();
        let Some(value) = value else { return h };
        let mut params = parse_params(value.trim(), b';').into_iter();
        if let Some((media_type, _)) = params.next() {
            h.media_type = Some(media_type);
            for (name, val) in params {
                match name.to_lowercase().as_str() {
                    "boundary" if h.boundary.is_none() => h.boundary = val,
                    "charset" if h.charset.is_none() => h.charset = val,
                    _ => {}
                }
            }
        }
        h
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(media_type) = self.media_type.as_deref().filter(|m| !m.is_empty()) else { return Ok(()) };
        f.write_str(media_type)?;
        // Empty values are left out, like Remix's truthiness checks.
        if let Some(c) = self.charset.as_deref().filter(|c| !c.is_empty()) {
            write!(f, "; charset={}", quote(c))?;
        }
        if let Some(b) = self.boundary.as_deref().filter(|b| !b.is_empty()) {
            write!(f, "; boundary={}", quote(b))?;
        }
        Ok(())
    }
}

/// `Content-Disposition`: `attachment; filename="report.pdf"`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentDisposition {
    /// `inline`, `attachment` or `form-data`.
    pub disposition: Option<String>,
    pub name: Option<String>,
    pub filename: Option<String>,
    /// The RFC 8187 `filename*` value, still encoded (`UTF-8''na%C3%AFve.txt`).
    pub filename_ext: Option<String>,
}

impl ContentDisposition {
    pub fn parse(value: Option<&str>) -> ContentDisposition {
        let mut h = ContentDisposition::default();
        let Some(value) = value else { return h };
        let mut params = parse_params(value, b';').into_iter();
        if let Some((ty, _)) = params.next() {
            h.disposition = Some(ty);
            for (name, val) in params {
                match name.as_str() {
                    "filename" => h.filename = val,
                    "filename*" => h.filename_ext = val,
                    "name" => h.name = val,
                    _ => {}
                }
            }
        }
        h
    }

    /// The decoded `filename*`, else `filename` (RFC 6266). Labels a `TextDecoder` knows
    /// are decoded in that charset; for other labels the percent-decoded text is used as is,
    /// where Remix also warns. Values from clients are untrusted: never use them as paths.
    pub fn preferred_filename(&self) -> Option<String> {
        let decoded = self.filename_ext.as_deref().filter(|v| !v.is_empty()).and_then(decode_ext_value);
        decoded.filter(|d| !d.is_empty()).or_else(|| self.filename.clone())
    }
}

/// `charset'lang'percent-encoded` (RFC 8187), read like Remix's `decodeFilenameSplat`.
fn decode_ext_value(value: &str) -> Option<String> {
    let (charset, rest) = value.split_once('\'')?;
    let (_, encoded) = rest.split_once('\'')?;
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    if charset.is_empty() || encoded.is_empty() || !charset.bytes().all(word) {
        return None;
    }
    // Percent escapes become the code points 0 to 255; like `charCodeAt` into a
    // `Uint8Array`, each UTF-16 unit of any other text keeps only its low byte.
    let mut text = String::with_capacity(encoded.len());
    let mut chars = encoded.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut ahead = chars.clone();
            if let (Some(h), Some(l)) = (ahead.next().and_then(|d| d.to_digit(16)), ahead.next().and_then(|d| d.to_digit(16))) {
                text.push(char::from((h * 16 + l) as u8));
                chars = ahead;
                continue;
            }
        }
        text.push(c);
    }
    let bytes: Vec<u8> = text.encode_utf16().map(|unit| unit as u8).collect();
    Some(crate::charset::decode(charset, &bytes).unwrap_or(text))
}

impl fmt::Display for ContentDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(ty) = self.disposition.as_deref().filter(|t| !t.is_empty()) else { return Ok(()) };
        f.write_str(ty)?;
        let params = [("name", &self.name), ("filename", &self.filename), ("filename*", &self.filename_ext)];
        for (key, value) in params {
            if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
                write!(f, "; {key}={}", quote(v))?;
            }
        }
        Ok(())
    }
}

/// `Content-Range: bytes 200-999/5000` (or `bytes */5000` for unsatisfiable ranges).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentRange {
    pub unit: String,
    /// Inclusive start and end, `None` for `*`.
    pub range: Option<(u64, u64)>,
    /// Total size, `None` for `*`.
    pub size: Option<u64>,
}

impl ContentRange {
    pub fn bytes(start: u64, end: u64, size: u64) -> ContentRange {
        ContentRange { unit: "bytes".into(), range: Some((start, end)), size: Some(size) }
    }

    pub fn unsatisfied(size: u64) -> ContentRange {
        ContentRange { unit: "bytes".into(), range: None, size: Some(size) }
    }

    pub fn parse(value: Option<&str>) -> Option<ContentRange> {
        let (unit, rest) = value?.split_once(char::is_whitespace)?;
        let (range, size) = rest.trim_start().split_once('/')?;
        if unit.is_empty() || !unit.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        let digits = |s: &str| (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten();
        let range = if range == "*" {
            None
        } else {
            let (a, b) = range.split_once('-')?;
            Some((digits(a)?, digits(b)?))
        };
        let size = if size == "*" { None } else { Some(digits(size)?) };
        Some(ContentRange { unit: unit.into(), range, size })
    }
}

impl fmt::Display for ContentRange {
    /// Empty when there is no unit, like an unset header.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit.is_empty() {
            return Ok(());
        }
        write!(f, "{} ", self.unit)?;
        match self.range {
            Some((a, b)) => write!(f, "{a}-{b}")?,
            None => f.write_str("*")?,
        }
        match self.size {
            Some(s) => write!(f, "/{s}"),
            None => f.write_str("/*"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type() {
        let h = ContentType::parse(Some("multipart/form-data; boundary=----x; charset=UTF-8"));
        assert_eq!(h.media_type.as_deref(), Some("multipart/form-data"));
        assert_eq!(h.boundary.as_deref(), Some("----x"));
        assert_eq!(h.to_string(), "multipart/form-data; charset=UTF-8; boundary=----x");
        assert_eq!(ContentType::parse(None).to_string(), "");
    }

    #[test]
    fn content_disposition() {
        let h = ContentDisposition::parse(Some(r#"attachment; filename="plain.txt"; filename*=UTF-8''na%C3%AFve%20file.txt"#));
        assert_eq!(h.disposition.as_deref(), Some("attachment"));
        assert_eq!(h.preferred_filename().as_deref(), Some("naïve file.txt"));
        let h = ContentDisposition { disposition: Some("form-data".into()), name: Some("avatar".into()), filename: Some("my photo.png".into()), ..Default::default() };
        assert_eq!(h.to_string(), r#"form-data; name=avatar; filename="my photo.png""#);
    }

    #[test]
    fn content_range() {
        assert_eq!(ContentRange::bytes(0, 99, 1000).to_string(), "bytes 0-99/1000");
        assert_eq!(ContentRange::unsatisfied(1000).to_string(), "bytes */1000");
        assert_eq!(ContentRange::parse(Some("bytes 200-1000/67589")), Some(ContentRange::bytes(200, 1000, 67589)));
        assert_eq!(ContentRange::parse(Some("bytes 0-1/*")).unwrap().size, None);
        assert_eq!(ContentRange::parse(Some("bytes x-1/2")), None);
    }
}

//! `FormData`: parsed form fields and uploaded files, from URL-encoded or multipart bodies.

use bytes::Bytes;

use crate::{Body, BoxError, SearchParams};

/// An uploaded file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// The client-supplied file name. Untrusted: never use it as a filesystem path.
    pub name: String,
    pub content_type: String,
    pub bytes: Bytes,
}

/// One form value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormValue {
    Text(String),
    File(File),
}

impl FormValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            FormValue::Text(s) => Some(s),
            FormValue::File(_) => None,
        }
    }

    pub fn as_file(&self) -> Option<&File> {
        match self {
            FormValue::File(f) => Some(f),
            FormValue::Text(_) => None,
        }
    }
}

/// Ordered form entries, like the web's `FormData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormData {
    entries: Vec<(String, FormValue)>,
}

/// Limits for multipart parsing.
#[derive(Clone, Copy, Debug)]
pub struct FormLimits {
    pub max_total_size: usize,
    pub max_file_size: usize,
    pub max_files: usize,
    pub max_parts: usize,
}

impl Default for FormLimits {
    fn default() -> Self {
        FormLimits { max_total_size: 20 * 1024 * 1024, max_file_size: 10 * 1024 * 1024, max_files: 20, max_parts: 1000 }
    }
}

impl FormData {
    pub fn new() -> FormData {
        FormData::default()
    }

    /// The first text value for `name` (files are skipped).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().filter(|(k, _)| k == name).find_map(|(_, v)| v.as_text())
    }

    /// The first value for `name`, text or file.
    pub fn get_value(&self, name: &str) -> Option<&FormValue> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn get_all(&self, name: &str) -> Vec<&FormValue> {
        self.entries.iter().filter(|(k, _)| k == name).map(|(_, v)| v).collect()
    }

    /// The first file for `name`.
    pub fn file(&self, name: &str) -> Option<&File> {
        self.entries.iter().filter(|(k, _)| k == name).find_map(|(_, v)| v.as_file())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == name)
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.push((name.into(), FormValue::Text(value.into())));
    }

    pub fn append_file(&mut self, name: impl Into<String>, file: File) {
        self.entries.push((name.into(), FormValue::File(file)));
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.entries.retain(|(k, _)| *k != name);
        self.entries.push((name, FormValue::Text(value.into())));
    }

    pub fn delete(&mut self, name: &str) {
        self.entries.retain(|(k, _)| k != name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FormValue)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The text fields as `SearchParams` (files contribute their names).
    pub fn to_search_params(&self) -> SearchParams {
        self.entries
            .iter()
            .map(|(k, v)| match v {
                FormValue::Text(s) => (k.clone(), s.clone()),
                FormValue::File(f) => (k.clone(), f.name.clone()),
            })
            .collect()
    }

    /// Parse a body according to its `Content-Type`. Other content types give an empty form.
    pub async fn parse(content_type: Option<&str>, body: Body, limits: FormLimits) -> Result<FormData, BoxError> {
        let Some(ct) = content_type else { return Ok(FormData::new()) };
        let essence = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        match essence.as_str() {
            "application/x-www-form-urlencoded" => {
                let bytes = read_limited(body, limits.max_total_size).await?;
                let params = SearchParams::parse(&String::from_utf8_lossy(&bytes));
                Ok(FormData { entries: params.iter().map(|(k, v)| (k.to_string(), FormValue::Text(v.to_string()))).collect() })
            }
            "multipart/form-data" => {
                let boundary = header_param(ct, "boundary").ok_or("multipart/form-data without a boundary")?;
                let bytes = read_limited(body, limits.max_total_size).await?;
                parse_multipart(&bytes, &boundary, limits)
            }
            "text/plain" => {
                let bytes = read_limited(body, limits.max_total_size).await?;
                let text = String::from_utf8_lossy(&bytes);
                let mut form = FormData::new();
                for line in text.split("\r\n").filter(|l| !l.is_empty()) {
                    let (k, v) = line.split_once('=').unwrap_or((line, ""));
                    form.append(k, v);
                }
                Ok(form)
            }
            _ => Ok(FormData::new()),
        }
    }
}

impl From<SearchParams> for FormData {
    fn from(p: SearchParams) -> FormData {
        FormData { entries: p.iter().map(|(k, v)| (k.to_string(), FormValue::Text(v.to_string()))).collect() }
    }
}

async fn read_limited(mut body: Body, max: usize) -> Result<Vec<u8>, BoxError> {
    let mut buf = Vec::new();
    while let Some(chunk) = body.chunk().await {
        buf.extend_from_slice(&chunk?);
        if buf.len() > max {
            return Err(format!("form body exceeds {max} bytes").into());
        }
    }
    Ok(buf)
}

/// A parameter from a header value such as `multipart/form-data; boundary=x`.
pub fn header_param(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        if !k.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        let v = v.trim();
        Some(v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v).to_string())
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn parse_multipart(body: &[u8], boundary: &str, limits: FormLimits) -> Result<FormData, BoxError> {
    let delim = format!("--{boundary}");
    let delim = delim.as_bytes();
    let mut form = FormData::new();
    let mut pos = find(body, delim, 0).ok_or("multipart body has no boundary")? + delim.len();
    let (mut parts, mut files) = (0, 0);
    loop {
        if body[pos..].starts_with(b"--") {
            return Ok(form);
        }
        // Skip the CRLF after the boundary.
        if body[pos..].starts_with(b"\r\n") {
            pos += 2;
        }
        let header_end = find(body, b"\r\n\r\n", pos).ok_or("multipart part without headers")?;
        let headers = String::from_utf8_lossy(&body[pos..header_end]).into_owned();
        let content_start = header_end + 4;
        let mut end_delim = b"\r\n".to_vec();
        end_delim.extend_from_slice(delim);
        let content_end = find(body, &end_delim, content_start).ok_or("multipart part is not terminated")?;
        let content = &body[content_start..content_end];
        pos = content_end + end_delim.len();

        parts += 1;
        if parts > limits.max_parts {
            return Err("too many multipart parts".into());
        }
        let mut name = None;
        let mut filename = None;
        let mut content_type = String::from("application/octet-stream");
        for line in headers.split("\r\n") {
            let Some((k, v)) = line.split_once(':') else { continue };
            if k.trim().eq_ignore_ascii_case("content-disposition") {
                name = header_param(v, "name");
                filename = header_param(v, "filename");
            } else if k.trim().eq_ignore_ascii_case("content-type") {
                content_type = v.trim().to_string();
            }
        }
        let Some(name) = name else { continue };
        match filename {
            Some(filename) => {
                files += 1;
                if files > limits.max_files {
                    return Err("too many files".into());
                }
                if content.len() > limits.max_file_size {
                    return Err(format!("file {filename:?} exceeds {} bytes", limits.max_file_size).into());
                }
                // Browsers send an empty part for an empty file input.
                if filename.is_empty() && content.is_empty() {
                    continue;
                }
                form.append_file(name, File { name: filename, content_type, bytes: Bytes::copy_from_slice(content) });
            }
            None => form.append(name, String::from_utf8_lossy(content).into_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;

    #[test]
    fn parses_multipart() {
        let body = "--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHello world\r\n--XyZ\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\r\nbody\r\n--XyZ--\r\n";
        let form = block_on(FormData::parse(Some("multipart/form-data; boundary=XyZ"), Body::from(body), FormLimits::default())).unwrap();
        assert_eq!(form.get("title"), Some("Hello world"));
        let f = form.file("cover").unwrap();
        assert_eq!(f.name, "a.txt");
        assert_eq!(f.content_type, "text/plain");
        assert_eq!(&f.bytes[..], b"file\r\nbody");
    }

    #[test]
    fn parses_urlencoded_and_ignores_other_types() {
        let form = block_on(FormData::parse(Some("application/x-www-form-urlencoded"), Body::from("a=1&b=x%20y"), FormLimits::default())).unwrap();
        assert_eq!(form.get("b"), Some("x y"));
        let form = block_on(FormData::parse(Some("application/json"), Body::from("{}"), FormLimits::default())).unwrap();
        assert!(form.is_empty());
    }
}

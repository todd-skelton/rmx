//! Helpers shared by the backend tests.

#![allow(dead_code)]

use rmx_fetch::{Body, File, FormData, FormLimits};

pub fn text_file(body: &str, name: &str) -> File {
    File::new(name, "text/plain", body.as_bytes().to_vec())
}

pub fn text(file: &File) -> String {
    String::from_utf8(file.bytes.to_vec()).unwrap()
}

/// The multipart body Remix's form-data-parser integration tests send.
pub fn upload_request(boundary: &str, field: &str) -> (String, Body) {
    let body = [
        format!("--{boundary}"),
        format!("Content-Disposition: form-data; name=\"{field}\"; filename=\"hello.txt\""),
        "Content-Type: text/plain".to_string(),
        String::new(),
        "Hello, world!".to_string(),
        format!("--{boundary}--"),
    ]
    .join("\r\n");
    (format!("multipart/form-data; boundary={boundary}"), Body::from(body))
}

/// Parse an upload, handing each file to `store` (Remix's `parseFormData(request, handler)`).
pub async fn parse_uploads<F, Fut>(content_type: &str, body: Body, mut store: F)
where
    F: FnMut(File) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    FormData::parse_with(Some(content_type), body, FormLimits::default(), |upload| {
        let stored = store(upload.file);
        async move {
            stored.await;
            None
        }
    })
    .await
    .unwrap();
}

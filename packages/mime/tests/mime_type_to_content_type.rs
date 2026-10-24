//! Ports of Remix's `mime/src/lib/mime-type-to-content-type.test.ts`.

use rsmix_mime::mime_type_to_content_type;

#[test]
fn content_types() {
    let cases = [
        ("text/plain", "text/plain; charset=utf-8"),
        ("text/html", "text/html; charset=utf-8"),
        ("text/css", "text/css; charset=utf-8"),
        ("text/javascript", "text/javascript; charset=utf-8"),
        ("text/markdown", "text/markdown; charset=utf-8"),
        ("text/csv", "text/csv; charset=utf-8"),
        ("application/json", "application/json; charset=utf-8"),
        ("application/manifest+json", "application/manifest+json; charset=utf-8"),
        ("application/ld+json", "application/ld+json; charset=utf-8"),
        ("application/geo+json", "application/geo+json; charset=utf-8"),
        ("application/javascript", "application/javascript; charset=utf-8"),
        ("text/xml", "text/xml"),
        ("image/png", "image/png"),
        ("image/jpeg", "image/jpeg"),
        ("video/mp4", "video/mp4"),
        ("audio/mpeg", "audio/mpeg"),
        ("application/pdf", "application/pdf"),
        ("application/zip", "application/zip"),
        ("application/octet-stream", "application/octet-stream"),
        ("font/woff2", "font/woff2"),
        ("text/plain; charset=utf-8", "text/plain; charset=utf-8"),
        ("text/html;charset=iso-8859-1", "text/html;charset=iso-8859-1"),
        ("application/x-custom", "application/x-custom"),
    ];
    for (mime_type, expected) in cases {
        assert_eq!(mime_type_to_content_type(mime_type), expected);
    }
}

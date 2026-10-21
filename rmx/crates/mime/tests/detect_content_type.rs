//! Ports of Remix's `mime/src/lib/detect-content-type.test.ts`.

use rmx_mime::detect_content_type;

fn check(cases: &[(&str, Option<&str>)]) {
    for (input, expected) in cases {
        assert_eq!(detect_content_type(input).as_deref(), *expected, "{input:?}");
    }
}

#[test]
fn content_types() {
    check(&[
        ("css", Some("text/css; charset=utf-8")),
        (".css", Some("text/css; charset=utf-8")),
        ("style.css", Some("text/css; charset=utf-8")),
        ("js", Some("text/javascript; charset=utf-8")),
        ("mjs", Some("text/javascript; charset=utf-8")),
        ("json", Some("application/json; charset=utf-8")),
        ("data.json", Some("application/json; charset=utf-8")),
        ("png", Some("image/png")),
        ("jpg", Some("image/jpeg")),
        ("gif", Some("image/gif")),
        ("pdf", Some("application/pdf")),
        ("zip", Some("application/zip")),
        ("txt", Some("text/plain; charset=utf-8")),
        ("html", Some("text/html; charset=utf-8")),
        ("md", Some("text/markdown; charset=utf-8")),
        ("csv", Some("text/csv; charset=utf-8")),
        ("xml", Some("text/xml")),
        ("svg", Some("image/svg+xml")),
        ("unknown", None),
        (".xxyyzz", None),
        ("file.xxyyzz", None),
        ("path/to/style.css", Some("text/css; charset=utf-8")),
        ("/absolute/path/image.png", Some("image/png")),
        ("CSS", Some("text/css; charset=utf-8")),
        ("JSON", Some("application/json; charset=utf-8")),
        ("PNG", Some("image/png")),
    ]);
}

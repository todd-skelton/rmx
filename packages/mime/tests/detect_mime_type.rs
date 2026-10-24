//! Ports of Remix's `mime/src/lib/detect-mime-type.test.ts`.

use rsmix_mime::detect_mime_type;

fn check(cases: &[(&str, Option<&str>)]) {
    for (input, expected) in cases {
        assert_eq!(detect_mime_type(input), *expected, "{input:?}");
    }
}

#[test]
fn extensions_and_file_names() {
    check(&[
        ("txt", Some("text/plain")),
        ("html", Some("text/html")),
        ("json", Some("application/json")),
        ("js", Some("text/javascript")),
        ("css", Some("text/css")),
        (".txt", Some("text/plain")),
        (".html", Some("text/html")),
        (".json", Some("application/json")),
        ("file.txt", Some("text/plain")),
        ("index.html", Some("text/html")),
        ("data.json", Some("application/json")),
        ("script.js", Some("text/javascript")),
        ("style.css", Some("text/css")),
        ("file.backup.txt", Some("text/plain")),
        ("app.min.js", Some("text/javascript")),
        ("data.v1.json", Some("application/json")),
        ("path/to/file.txt", Some("text/plain")),
        ("/absolute/path/file.html", Some("text/html")),
        ("../relative/file.json", Some("application/json")),
    ]);
}

#[test]
fn case_and_whitespace() {
    check(&[
        ("TXT", Some("text/plain")),
        (".HTML", Some("text/html")),
        ("FILE.JSON", Some("application/json")),
        ("TxT", Some("text/plain")),
        ("HtMl", Some("text/html")),
        ("file.JsOn", Some("application/json")),
        ("  txt  ", Some("text/plain")),
        ("  .html  ", Some("text/html")),
        ("  file.json  ", Some("application/json")),
    ]);
}

#[test]
fn unknown_and_empty() {
    check(&[
        ("notarealextension", None),
        (".unknown", None),
        ("file.notarealextension", None),
        ("", None),
        ("   ", None),
    ]);
}

#[test]
fn common_formats() {
    check(&[
        ("png", Some("image/png")),
        ("jpg", Some("image/jpeg")),
        ("jpeg", Some("image/jpeg")),
        ("gif", Some("image/gif")),
        ("svg", Some("image/svg+xml")),
        ("webp", Some("image/webp")),
        ("ico", Some("image/x-icon")),
        ("mp4", Some("video/mp4")),
        ("webm", Some("video/webm")),
        ("mov", Some("video/quicktime")),
        ("mp3", Some("audio/mpeg")),
        ("wav", Some("audio/wav")),
        ("ogg", Some("audio/ogg")),
        ("zip", Some("application/zip")),
        ("gz", Some("application/gzip")),
        ("pdf", Some("application/pdf")),
        ("doc", Some("application/msword")),
        ("rtf", Some("text/rtf")),
    ]);
}

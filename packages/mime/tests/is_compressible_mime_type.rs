//! Ports of Remix's `mime/src/lib/is-compressible-mime-type.test.ts`.

use rsmix_mime::is_compressible_mime_type;

#[test]
fn compressible() {
    for mime_type in [
        "text/html",
        "text/plain",
        "application/json",
        "application/javascript",
        "text/css",
        "text/custom",
        "text/markdown",
        "application/vnd.api+json",
        "application/custom+xml",
        "application/something+text",
        "text/html; charset=utf-8",
        "application/json; charset=utf-8",
        "text/plain; charset=iso-8859-1",
        "text/html ; charset=utf-8",
        " text/html;charset=utf-8",
        "  application/json  ;  charset=utf-8",
        "application/wasm",
    ] {
        assert!(is_compressible_mime_type(mime_type), "{mime_type:?}");
    }
}

#[test]
fn not_compressible() {
    for mime_type in [
        "image/png",
        "image/jpeg",
        "video/mp4",
        "audio/mpeg",
        "",
        "multipart/form-data; boundary=----WebKitFormBoundary",
        "image/png; name=\"photo.png\"",
    ] {
        assert!(!is_compressible_mime_type(mime_type), "{mime_type:?}");
    }
}

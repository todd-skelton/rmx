//! Ports of Remix's `mime/src/lib/define-mime-type.test.ts`. Registrations are process-wide,
//! so each test holds a lock and starts from [`reset_mime_types`], like Remix's `beforeEach`.

use std::sync::{Mutex, MutexGuard, PoisonError};

use rsmix_mime::{
    define_mime_type, detect_content_type, detect_mime_type, is_compressible_mime_type, mime_type_to_content_type,
    reset_mime_types, MimeTypeDefinition,
};

static LOCK: Mutex<()> = Mutex::new(());

fn isolated() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    reset_mime_types();
    guard
}

fn define(extensions: &[&str], mime_type: &str) -> MimeTypeDefinition {
    MimeTypeDefinition::new(extensions.iter().copied(), mime_type)
}

#[test]
fn custom_types() {
    let _guard = isolated();
    define_mime_type(define(&["customext1"], "text/custom1"));
    assert_eq!(detect_mime_type("customext1"), Some("text/custom1"));

    assert_eq!(detect_mime_type("ts"), Some("video/mp2t"));
    define_mime_type(define(&["ts"], "text/typescript"));
    assert_eq!(detect_mime_type("ts"), Some("text/typescript"), "overrides a builtin");

    define_mime_type(define(&["MDX"], "text/mdx"));
    assert_eq!((detect_mime_type("mdx"), detect_mime_type("MDX")), (Some("text/mdx"), Some("text/mdx")));

    define_mime_type(define(&[".mdx2"], "text/mdx2"));
    assert_eq!((detect_mime_type("mdx2"), detect_mime_type(".mdx2")), (Some("text/mdx2"), Some("text/mdx2")));

    define_mime_type(define(&["  mdx3  "], "text/mdx3"));
    assert_eq!(detect_mime_type("mdx3"), Some("text/mdx3"));

    define_mime_type(define(&["jpg", "jpeg", "jpe"], "image/jpeg"));
    for ext in ["jpg", "jpeg", "jpe"] {
        assert_eq!(detect_mime_type(ext), Some("image/jpeg"));
    }
}

#[test]
fn multi_part_extensions() {
    let _guard = isolated();
    define_mime_type(define(&["pit"], "application/x-pit-document"));
    define_mime_type(define(&["be.pit"], "application/x-be-pit-document"));
    define_mime_type(define(&["fr.pit"], "application/x-fr-pit-document"));
    let cases = [
        ("filename.pit", Some("application/x-pit-document")),
        ("filename.be.pit", Some("application/x-be-pit-document")),
        ("filename.fr.pit", Some("application/x-fr-pit-document")),
        ("be.pit", Some("application/x-be-pit-document")),
        (".fr.pit", Some("application/x-fr-pit-document")),
        ("path/to/be.pit", Some("application/x-be-pit-document")),
        ("path/to/.fr.pit", Some("application/x-fr-pit-document")),
        ("path/to/pit", None),
        ("PATH/TO/FILENAME.BE.PIT", Some("application/x-be-pit-document")),
    ];
    for (input, expected) in cases {
        assert_eq!(detect_mime_type(input), expected, "{input}");
    }
}

#[test]
fn content_type_of_custom_types() {
    let _guard = isolated();
    define_mime_type(define(&["mdx"], "text/mdx"));
    assert_eq!(detect_content_type("mdx").as_deref(), Some("text/mdx; charset=utf-8"));
    assert_eq!(detect_content_type("file.mdx").as_deref(), Some("text/mdx; charset=utf-8"));
}

#[test]
fn custom_compressibility() {
    let _guard = isolated();
    define_mime_type(define(&["myext"], "application/x-myformat").compressible(true));
    assert!(is_compressible_mime_type("application/x-myformat"));
    assert!(is_compressible_mime_type("application/x-myformat; charset=utf-8"));
    define_mime_type(define(&["myext"], "application/x-myformat").compressible(false));
    assert!(!is_compressible_mime_type("application/x-myformat"));

    assert!(is_compressible_mime_type("text/html"));
    define_mime_type(define(&["html"], "text/html").compressible(false));
    assert!(!is_compressible_mime_type("text/html"), "overrides builtin compressibility");

    define_mime_type(define(&["mdx"], "text/mdx"));
    assert!(is_compressible_mime_type("text/mdx"), "default rules without a setting");
    define_mime_type(define(&["mybin"], "application/x-mybin"));
    assert!(!is_compressible_mime_type("application/x-mybin"));
}

#[test]
fn custom_charset() {
    let _guard = isolated();
    define_mime_type(define(&["myext"], "application/x-myformat").charset("utf-8"));
    assert_eq!(mime_type_to_content_type("application/x-myformat"), "application/x-myformat; charset=utf-8");
    assert_eq!(
        mime_type_to_content_type("application/x-myformat; charset=iso-8859-1"),
        "application/x-myformat; charset=iso-8859-1",
        "an existing charset is kept"
    );
    define_mime_type(define(&["myext"], "application/x-myformat").charset("iso-8859-1"));
    assert_eq!(mime_type_to_content_type("application/x-myformat"), "application/x-myformat; charset=iso-8859-1");

    define_mime_type(define(&["mdx"], "text/mdx"));
    assert_eq!(mime_type_to_content_type("text/mdx"), "text/mdx; charset=utf-8");
    define_mime_type(define(&["mybin"], "application/x-mybin"));
    assert_eq!(mime_type_to_content_type("application/x-mybin"), "application/x-mybin");

    define_mime_type(define(&["xml"], "text/xml").charset("utf-8"));
    assert_eq!(mime_type_to_content_type("text/xml"), "text/xml", "XML declares its own encoding");
}

#[test]
fn combined_and_repeated_registrations() {
    let _guard = isolated();
    define_mime_type(define(&["myext"], "application/x-myformat").compressible(true).charset("utf-8"));
    assert_eq!(detect_mime_type("myext"), Some("application/x-myformat"));
    assert_eq!(detect_mime_type("file.myext"), Some("application/x-myformat"));
    assert!(is_compressible_mime_type("application/x-myformat"));
    assert_eq!(mime_type_to_content_type("application/x-myformat"), "application/x-myformat; charset=utf-8");
    assert_eq!(detect_content_type("myext").as_deref(), Some("application/x-myformat; charset=utf-8"));

    define_mime_type(define(&["mdx"], "text/mdx").compressible(true));
    define_mime_type(define(&["prisma"], "text/x-prisma").compressible(true));
    assert_eq!((detect_mime_type("mdx"), detect_mime_type("prisma")), (Some("text/mdx"), Some("text/x-prisma")));

    define_mime_type(define(&["other"], "application/x-first"));
    define_mime_type(define(&["other"], "application/x-second"));
    assert_eq!(detect_mime_type("other"), Some("application/x-second"), "the last registration wins");
}

#[test]
fn reset_clears_registrations() {
    let _guard = isolated();
    define_mime_type(define(&["reset-test"], "application/x-reset-test").compressible(true).charset("utf-8"));
    assert_eq!(detect_mime_type("reset-test"), Some("application/x-reset-test"));
    assert!(is_compressible_mime_type("application/x-reset-test"));
    assert_eq!(mime_type_to_content_type("application/x-reset-test"), "application/x-reset-test; charset=utf-8");
    reset_mime_types();
    assert_eq!(detect_mime_type("reset-test"), None);
    assert!(!is_compressible_mime_type("application/x-reset-test"));
    assert_eq!(mime_type_to_content_type("application/x-reset-test"), "application/x-reset-test");

    define_mime_type(define(&["txt"], "text/custom"));
    assert_eq!(detect_mime_type("txt"), Some("text/custom"));
    reset_mime_types();
    assert_eq!(detect_mime_type("txt"), Some("text/plain"), "builtins come back");
}

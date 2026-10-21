//! Ports of Remix's `headers/src/lib/content-disposition.test.ts`. `new
//! ContentDisposition(string)` and `ContentDisposition.from` are [`ContentDisposition::parse`],
//! an object init is a struct literal, `type` is `disposition` and `filenameSplat` is
//! `filename_ext`. Remix's `console.warn` for an unknown charset has no counterpart: the
//! library does not log, and the fallback value is the same.

use rmx_headers::ContentDisposition;

fn parse(value: &str) -> ContentDisposition {
    ContentDisposition::parse(Some(value))
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

fn preferred(value: &str) -> Option<String> {
    parse(value).preferred_filename()
}

#[test]
fn initializes() {
    let empty = parse("");
    assert_eq!((empty.disposition, empty.filename), (None, None));
    assert_eq!(ContentDisposition::parse(None), ContentDisposition::default());
    let h = parse(r#"attachment; filename="example.txt""#);
    assert_eq!((h.disposition.clone(), h.filename.clone()), (some("attachment"), some("example.txt")));
    let built = ContentDisposition { disposition: some("attachment"), filename: some("example.txt"), ..Default::default() };
    assert_eq!(built, h);
    assert_eq!(h.clone(), h);
    let spaced = parse(r#" inline ;  filename = "document.pdf" "#);
    assert_eq!((spaced.disposition, spaced.filename), (some("inline"), some("document.pdf")));
}

#[test]
fn sets_fields() {
    let mut h = parse("attachment");
    h.disposition = some("inline");
    h.filename = some("example.txt");
    h.name = some("field1");
    h.filename_ext = some("UTF-8''%E6%96%87%E4%BB%B6.txt");
    assert_eq!(h.disposition, some("inline"));
    assert_eq!(h.filename, some("example.txt"));
    assert_eq!(h.name, some("field1"));
    assert_eq!(h.filename_ext, some("UTF-8''%E6%96%87%E4%BB%B6.txt"));
}

#[test]
fn parses_attributes() {
    assert_eq!(parse(r#"attachment; filename="file with spaces.txt""#).filename, some("file with spaces.txt"));
    let multi = parse(r#"form-data; name="field1"; filename="example.txt""#);
    assert_eq!((multi.disposition, multi.name, multi.filename), (some("form-data"), some("field1"), some("example.txt")));
    assert_eq!(parse("Attachment").disposition, some("Attachment"), "type keeps its case");
    let special = parse(r#"attachment; filename="file with spaces and (parentheses).txt""#);
    assert_eq!(special.filename, some("file with spaces and (parentheses).txt"));
    assert_eq!(parse("form-data; name=").name, some(""), "empty values are kept");
    assert_eq!(parse("attachment; filename").filename, None, "attributes without values are ignored");
    let upload = parse(r#"form-data; name="uploadedfile"; filename="example.txt""#);
    assert_eq!((upload.name, upload.filename), (some("uploadedfile"), some("example.txt")));
}

#[test]
fn filename_ext() {
    assert_eq!(parse("attachment; filename*=UTF-8''%E6%96%87%E4%BB%B6.txt").filename_ext, some("UTF-8''%E6%96%87%E4%BB%B6.txt"));
    let both = parse(r#"attachment; filename="fallback.txt"; filename*=UTF-8''%E6%96%87%E4%BB%B6.txt"#);
    assert_eq!(both.filename, some("fallback.txt"));
    assert_eq!(both.filename_ext, some("UTF-8''%E6%96%87%E4%BB%B6.txt"));
}

#[test]
fn to_string() {
    assert_eq!(parse(r#"attachment; filename="example.txt""#).to_string(), "attachment; filename=example.txt");
    let untyped = ContentDisposition { filename: some("example.txt"), ..Default::default() };
    assert_eq!(untyped.to_string(), "", "no type prints empty");
    let quoted = ContentDisposition { disposition: some("attachment"), filename: some(r#"file "with" quotes.txt"#), ..Default::default() };
    assert_eq!(quoted.to_string(), r#"attachment; filename="file \"with\" quotes.txt""#);
    let ordered = parse(r#"form-data; name="field1"; filename="example.txt""#);
    assert_eq!(ordered.to_string(), "form-data; name=field1; filename=example.txt");
    // Remix checks each value for truthiness, so empty ones are left out.
    let blank = ContentDisposition { disposition: some("form-data"), name: some(""), filename: some(""), filename_ext: some("") };
    assert_eq!(blank.to_string(), "form-data");
    assert_eq!(ContentDisposition { disposition: some(""), ..blank }.to_string(), "");
}

#[test]
fn preferred_filename() {
    assert_eq!(preferred(r#"attachment; filename="old.txt"; filename*=UTF-8''new.txt"#), some("new.txt"));
    assert_eq!(preferred(r#"attachment; filename="document.pdf""#), some("document.pdf"));
    assert_eq!(preferred("attachment; filename*=UTF-8''special%20file.txt"), some("special file.txt"));
    assert_eq!(preferred("attachment; filename*=UTF-8''a+b.txt"), some("a+b.txt"), "plus stays a plus");
    assert_eq!(preferred("attachment; filename*=UTF-8''a+b%20c.txt"), some("a+b c.txt"));
    assert_eq!(preferred("attachment; filename*=UTF-8''%E6%96%87%E4%BB%B6.txt"), some("\u{6587}\u{4ef6}.txt"));
    assert_eq!(preferred("attachment; filename*=UTF-8''hello%20world%21%20%26%20goodbye.txt"), some("hello world! & goodbye.txt"));
    assert_eq!(preferred("attachment"), None);
    assert_eq!(preferred(r#"attachment; filename="fallback.txt"; filename*=invalid"#), some("fallback.txt"));
}

#[test]
fn preferred_filename_charsets() {
    assert_eq!(preferred("attachment; filename*=ISO-8859-1''f%F6o.txt"), some("f\u{f6}o.txt"));
    assert_eq!(preferred("attachment; filename*=ISO-8859-15''file%A4.txt"), some("file\u{20ac}.txt"));
    assert_eq!(preferred("attachment; filename*=windows-1252''file%80.txt"), some("file\u{20ac}.txt"));
    // A TextDecoder reads ISO-8859-1 as windows-1252 too.
    assert_eq!(preferred("attachment; filename*=iso-8859-1''file%80.txt"), some("file\u{20ac}.txt"));
    // Unknown charsets fall back to the percent-decoded code points.
    assert_eq!(preferred("attachment; filename*=unknown-charset''file%FF.txt"), some("file\u{ff}.txt"));
}

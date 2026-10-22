//! Remix's `form-data-parser/src/lib/form-data.test.ts`. `parseFormData(request, options,
//! handler)` is `FormData::parse_with(content_type, body, limits, handler)`; Remix's error
//! classes are `FormDataError` variants (limits wrap `MultipartError`).

use std::sync::atomic::{AtomicUsize, Ordering};

use rmx_fetch::{block_on, Body, File, FileUpload, FormData, FormDataError, FormLimits, FormValue};
use rmx_multipart_parser::MultipartError;

const WEBKIT: &str = "----WebKitFormBoundary7MA4YWxkTrZu0gW";

fn multipart(boundary: &str, lines: &[&str]) -> (String, Body) {
    (format!("multipart/form-data; boundary={boundary}"), Body::from(lines.join("\r\n")))
}

fn parse(content_type: &str, body: Body, limits: FormLimits) -> Result<FormData, FormDataError> {
    block_on(FormData::parse(Some(content_type), body, limits))
}

fn parse_default(request: (String, Body)) -> FormData {
    parse(&request.0, request.1, FormLimits::default()).unwrap()
}

fn with_handler<R: rmx_fetch::UploadResult>(request: (String, Body), handler: impl FnMut(FileUpload) -> R) -> Result<FormData, FormDataError> {
    let mut handler = handler;
    block_on(FormData::parse_with(Some(&request.0), request.1, FormLimits::default(), |upload| std::future::ready(handler(upload))))
}

fn text(file: &File) -> &str {
    std::str::from_utf8(&file.bytes).unwrap()
}

fn limit(err: FormDataError) -> MultipartError {
    match err {
        FormDataError::Limit(e) => e,
        other => panic!("expected a limit error, got {other:?}"),
    }
}

fn texts(form: &FormData, name: &str) -> Vec<String> {
    form.get_all(name).into_iter().map(|v| v.as_text().unwrap().to_string()).collect()
}

#[test]
fn reports_the_uploaded_media_type_independently_of_the_filename_extension() {
    let form = parse_default(multipart(
        "ExampleBoundary",
        &[
            "--ExampleBoundary",
            "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"",
            "Content-Type: Text/HTML; charset=UTF-8",
            "",
            "<p>Example</p>",
            "--ExampleBoundary--",
        ],
    ));
    let file = form.file("file").unwrap();
    assert_eq!(file.name, "example.txt");
    // The part's media type is its essence (`Text/HTML`), which `File` lowercases.
    assert_eq!(file.content_type, "text/html");
    assert_eq!(File::new("x", "Image/PNG", vec![]).content_type, "image/png");
    assert_eq!(File::new("x", "text/plainé", vec![]).content_type, "", "non-ASCII types are dropped");
    assert_eq!(text(file), "<p>Example</p>");
}

fn mixed_case_multipart() -> (String, Body) {
    (
        "Multipart/Form-Data; boundary=\"ExampleBoundary\"".into(),
        Body::from(
            [
                "--ExampleBoundary",
                "Content-Disposition: form-data; name=\"text\"",
                "",
                "hello",
                "--ExampleBoundary",
                "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"",
                "Content-Type: text/plain",
                "",
                "world",
                "--ExampleBoundary--",
            ]
            .join("\r\n"),
        ),
    )
}

#[test]
fn calls_the_upload_handler_for_mixed_case_multipart_media_types() {
    let form = with_handler(mixed_case_multipart(), |upload| {
        assert_eq!(upload.file.name, "example.txt");
        Some(FormValue::Text(format!("stored: {}", text(&upload.file))))
    })
    .unwrap();
    assert_eq!(form.get("text"), Some("hello"));
    assert_eq!(form.get("file"), Some("stored: world"));
}

#[test]
fn enforces_max_files_for_mixed_case_multipart_media_types() {
    let (ct, body) = mixed_case_multipart();
    assert_eq!(parse(&ct, body, FormLimits { max_files: 0, ..Default::default() }).unwrap_err(), FormDataError::MaxFilesExceeded(0));
}

#[test]
fn enforces_max_file_size_for_mixed_case_multipart_media_types() {
    let (ct, body) = mixed_case_multipart();
    let err = parse(&ct, body, FormLimits { max_file_size: 1, ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxFileSizeExceeded(1));
}

#[test]
fn enforces_max_header_size_for_mixed_case_multipart_media_types() {
    let (ct, body) = mixed_case_multipart();
    let err = parse(&ct, body, FormLimits { max_header_size: 1, ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxHeaderSizeExceeded(1));
}

#[test]
fn enforces_max_parts_for_mixed_case_multipart_media_types() {
    let (ct, body) = mixed_case_multipart();
    let err = parse(&ct, body, FormLimits { max_parts: 1, ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxPartsExceeded(1));
}

#[test]
fn enforces_max_total_size_for_mixed_case_multipart_media_types() {
    let (ct, body) = mixed_case_multipart();
    let err = parse(&ct, body, FormLimits { max_total_size: Some(1), ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxTotalSizeExceeded(1));
}

#[test]
fn enforces_max_parts_for_mixed_case_urlencoded_media_types() {
    let err = parse("Application/X-Www-Form-Urlencoded; charset=UTF-8", Body::from("tag=red&tag=green"), FormLimits { max_parts: 1, ..Default::default() })
        .unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxPartsExceeded(1));
}

#[test]
fn enforces_max_total_size_for_mixed_case_urlencoded_media_types() {
    let err = parse("APPLICATION/X-WWW-FORM-URLENCODED", Body::from("tag=red"), FormLimits { max_total_size: Some(1), ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxTotalSizeExceeded(1));
}

#[test]
fn preserves_repeated_fields_for_mixed_case_urlencoded_media_types() {
    let form = parse("Application/X-Www-Form-Urlencoded; charset=UTF-8", Body::from("tag=red&tag=green"), FormLimits { max_parts: 2, ..Default::default() })
        .unwrap();
    assert_eq!(texts(&form, "tag"), ["red", "green"]);
}

#[test]
fn does_not_parse_media_types_that_only_start_with_the_urlencoded_media_type() {
    // The body is never read, so the 7-byte limit and the part count do not apply.
    let limits = FormLimits { max_parts: 1, max_total_size: Some(7), ..Default::default() };
    let err = parse("application/x-www-form-urlencoded-extra", Body::from("tag=red&tag=green"), limits).unwrap_err();
    assert_eq!(err, FormDataError::Parse("Cannot parse form data".into()));
}

#[test]
fn rejects_unsupported_media_types_even_if_a_native_parser_would_accept_them() {
    // Remix overrides `request.formData` to prove it is not consulted; rmx has no such fallback.
    for content_type in ["application/x-www-form-urlencoded-extra", "application/json", ""] {
        let err = parse(content_type, Body::from("tag=red&tag=green"), FormLimits::default()).unwrap_err();
        assert_eq!(err, FormDataError::Parse("Cannot parse form data".into()), "{content_type:?}");
        assert!(!err.is_limit());
    }
    let err = block_on(FormData::parse(None, Body::from("a=1"), FormLimits::default())).unwrap_err();
    assert_eq!(err, FormDataError::Parse("Cannot parse form data".into()));
}

#[test]
fn parses_an_application_x_www_form_urlencoded_request() {
    let form = parse("application/x-www-form-urlencoded", Body::from("text=Hello%2C%20World!"), FormLimits::default()).unwrap();
    assert_eq!(form.get("text"), Some("Hello, World!"));
}

#[test]
fn throws_when_urlencoded_form_data_exceeds_max_parts() {
    let err = parse("application/x-www-form-urlencoded", Body::from("field1=value1&field2=value2"), FormLimits { max_parts: 1, ..Default::default() })
        .unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxPartsExceeded(1));
}

#[test]
fn throws_when_urlencoded_form_data_exceeds_max_total_size() {
    let err =
        parse("application/x-www-form-urlencoded", Body::from("field1=value1&field2=value2"), FormLimits { max_total_size: Some(1), ..Default::default() })
            .unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxTotalSizeExceeded(1));
}

#[test]
fn preserves_repeated_urlencoded_form_fields() {
    let form = parse("application/x-www-form-urlencoded", Body::from("tag=red&tag=green&tag=blue"), FormLimits::default()).unwrap();
    assert_eq!(texts(&form, "tag"), ["red", "green", "blue"]);
}

#[test]
fn parses_a_multipart_form_data_request() {
    let form = parse_default(multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"text\"",
            "",
            "Hello, World!",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"",
            "Content-Type: text/plain",
            "",
            "This is an example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    ));
    assert_eq!(form.get("text"), Some("Hello, World!"));
    let file = form.file("file").unwrap();
    assert_eq!((file.name.as_str(), file.content_type.as_str(), text(file)), ("example.txt", "text/plain", "This is an example file."));
}

fn one_file() -> (String, Body) {
    multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"",
            "Content-Type: text/plain",
            "",
            "This is an example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    )
}

#[test]
fn calls_the_file_upload_handler_for_each_file_part() {
    let request = multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file1\"; filename=\"example.txt\"",
            "Content-Type: text/plain",
            "",
            "This is an example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file2\"; filename=\"example.txt\"",
            "Content-Type: text/plain",
            "",
            "This is another example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    );
    let calls = AtomicUsize::new(0);
    with_handler(request, |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        None
    })
    .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn allows_returning_none_from_the_upload_handler() {
    let form = with_handler(one_file(), |_| None).unwrap();
    assert_eq!(form.get_value("file"), None);
}

#[test]
fn allows_returning_strings_from_the_upload_handler() {
    let form = with_handler(one_file(), |upload| Some(FormValue::Text(text(&upload.file).to_string()))).unwrap();
    assert_eq!(form.get("file"), Some("This is an example file."));
}

#[test]
fn allows_returning_files_from_the_upload_handler() {
    let mut uploaded = None;
    let form = with_handler(one_file(), |upload| {
        let file = File::new("example.txt", "text/plain", upload.file.bytes.clone());
        uploaded = Some(file.clone());
        Some(FormValue::File(file))
    })
    .unwrap();
    let file = form.file("file").unwrap();
    assert_eq!(file.name, "example.txt");
    assert_eq!(file.content_type, uploaded.unwrap().content_type);
    assert_eq!(text(file), "This is an example file.");
}

#[derive(Debug)]
struct UploadFailed;

impl std::fmt::Display for UploadFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Upload failed")
    }
}

impl std::error::Error for UploadFailed {}

#[test]
fn allows_errors_returned_by_the_upload_handler_to_propagate_directly() {
    let err = with_handler(one_file(), |_| Err::<Option<FormValue>, _>(UploadFailed)).unwrap_err();
    let FormDataError::Upload(cause) = &err else { panic!("expected an upload error, got {err:?}") };
    assert!(cause.downcast_ref::<UploadFailed>().is_some(), "the handler's own error comes back");
    assert_eq!(err.to_string(), "Upload failed");
    assert!(!err.is_limit());
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
#[should_panic(expected = "Upload failed")]
fn a_panicking_upload_handler_propagates_its_panic() {
    let _ = with_handler(one_file(), |_| -> Option<FormValue> { panic!("Upload failed") });
}

#[test]
fn throws_max_files_exceeded_when_the_number_of_files_exceeds_the_limit() {
    let (ct, body) = multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file1\"; filename=\"example1.txt\"",
            "Content-Type: text/plain",
            "",
            "This is the first example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file2\"; filename=\"example2.txt\"",
            "Content-Type: text/plain",
            "",
            "This is the second example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file3\"; filename=\"example3.txt\"",
            "Content-Type: text/plain",
            "",
            "This is the third example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    );
    let err = parse(&ct, body, FormLimits { max_files: 2, ..Default::default() }).unwrap_err();
    assert_eq!(err, FormDataError::MaxFilesExceeded(2));
    assert_eq!(err.to_string(), "Maximum number of files exceeded: 2");
}

#[test]
fn throws_when_the_number_of_multipart_parts_exceeds_max_parts() {
    let (ct, body) = multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"field1\"",
            "",
            "value1",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file1\"; filename=\"example1.txt\"",
            "Content-Type: text/plain",
            "",
            "This is the first example file.",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"field2\"",
            "",
            "value2",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    );
    let err = parse(&ct, body, FormLimits { max_parts: 2, ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxPartsExceeded(2));
}

#[test]
fn throws_when_aggregate_multipart_content_size_exceeds_max_total_size() {
    let (ct, body) = multipart(
        WEBKIT,
        &[
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"field1\"",
            "",
            "hello",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW",
            "Content-Disposition: form-data; name=\"file1\"; filename=\"example1.txt\"",
            "Content-Type: text/plain",
            "",
            "world",
            "------WebKitFormBoundary7MA4YWxkTrZu0gW--",
        ],
    );
    let err = parse(&ct, body, FormLimits { max_total_size: Some(9), ..Default::default() }).unwrap_err();
    assert_eq!(limit(err), MultipartError::MaxTotalSizeExceeded(9));
}

#[test]
fn throws_when_the_request_does_not_contain_parseable_content() {
    let err = parse("text/plain", Body::from("Hello, World!"), FormLimits::default()).unwrap_err();
    assert_eq!(err, FormDataError::Parse("Cannot parse form data".into()));
}

#[test]
fn throws_when_the_request_contains_malformed_multipart_form_data() {
    let err = parse(&format!("multipart/form-data; boundary={WEBKIT}"), Body::from("invalid"), FormLimits::default()).unwrap_err();
    let FormDataError::Parse(message) = &err else { panic!("expected a parse error, got {err:?}") };
    assert!(message.starts_with("Cannot parse form data: "), "{message}");
    assert!(!err.is_limit());
}

#[test]
fn parses_a_multipart_file_without_a_media_type() {
    let form = parse_default(multipart(
        "----BOUNDARY",
        &["------BOUNDARY", "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"", "", "This is an example file.", "------BOUNDARY--"],
    ));
    let file = form.file("file").unwrap();
    assert_eq!((file.name.as_str(), file.content_type.as_str(), text(file)), ("example.txt", "application/octet-stream", "This is an example file."));
}

#[test]
fn parses_multipart_uploads_with_non_ascii_filenames() {
    let form = parse_default(multipart(
        "----BOUNDARY",
        &[
            "------BOUNDARY",
            "Content-Disposition: form-data; name=\"japanese\"; filename=\"テスト画像.png\"",
            "Content-Type: image/png",
            "",
            "Japanese file content.",
            "------BOUNDARY",
            "Content-Disposition: form-data; name=\"chinese\"; filename=\"文件.png\"",
            "Content-Type: image/png",
            "",
            "Chinese file content.",
            "------BOUNDARY",
            "Content-Disposition: form-data; name=\"korean\"; filename=\"파일.png\"",
            "Content-Type: image/png",
            "",
            "Korean file content.",
            "------BOUNDARY--",
        ],
    ));
    for (field, name, content) in [
        ("japanese", "テスト画像.png", "Japanese file content."),
        ("chinese", "文件.png", "Chinese file content."),
        ("korean", "파일.png", "Korean file content."),
    ] {
        let file = form.file(field).unwrap();
        assert_eq!((file.name.as_str(), text(file)), (name, content));
    }
}

#[test]
fn preserves_non_ascii_multipart_field_names_and_filenames() {
    let form = parse_default(multipart(
        "----BOUNDARY",
        &[
            "------BOUNDARY",
            "Content-Disposition: form-data; name=\"名前\"; filename=\"テスト画像.png\"",
            "Content-Type: image/png",
            "",
            "This is an example file.",
            "------BOUNDARY--",
        ],
    ));
    let file = form.file("名前").unwrap();
    assert_eq!((file.name.as_str(), file.content_type.as_str(), text(file)), ("テスト画像.png", "image/png", "This is an example file."));
}

#[test]
fn preserves_literal_percent_sequences_in_multipart_filenames() {
    let form = parse_default(multipart(
        "----BOUNDARY",
        &[
            "------BOUNDARY",
            "Content-Disposition: form-data; name=\"file\"; filename=\"%2Fetc%2Fpasswd\"",
            "Content-Type: text/plain",
            "",
            "This is an example file.",
            "------BOUNDARY--",
        ],
    ));
    assert_eq!(form.file("file").unwrap().name, "%2Fetc%2Fpasswd");
}

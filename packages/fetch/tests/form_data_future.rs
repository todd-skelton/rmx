//! Streaming uploads (`form_data::future`), after Remix's `future/form-data.test.ts`.

use std::sync::{Arc, Mutex};

use rsmix_fetch::form_data::future::{parse, parse_with, FileUpload};
use rsmix_fetch::{block_on, Body, File, FormData, FormDataError, FormLimits, FormValue};
use rsmix_multipart_parser::MultipartError;

const B: &str = "----WebKitFormBoundary7MA4YWxkTrZu0gW";

fn content_type() -> String {
    format!("multipart/form-data; boundary={B}")
}

fn file_part(name: &str, filename: &str, content_type: &str, content: &str) -> String {
    format!("--{B}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n{content}\r\n")
}

fn text_part(name: &str, value: &str) -> String {
    format!("--{B}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
}

fn body(parts: &[String]) -> String {
    format!("{}--{B}--\r\n", parts.concat())
}

/// A form without file times, to compare parses made at different moments.
fn entries(form: &FormData) -> Vec<(String, String)> {
    form.iter()
        .map(|(name, value)| match value {
            FormValue::Text(text) => (name.to_string(), text.clone()),
            FormValue::File(f) => (name.to_string(), format!("file {} {} {:?}", f.name, f.content_type, f.bytes)),
        })
        .collect()
}

fn outcome(result: Result<FormData, FormDataError>) -> Result<Vec<(String, String)>, (String, bool)> {
    result.as_ref().map(entries).map_err(|e| (e.to_string(), e.is_limit()))
}

#[test]
fn gives_the_same_results_as_the_stable_parser() {
    let mixed = body(&[text_part("title", "Dune"), file_part("cover", "dune.png", "image/png", "PNG"), file_part("empty", "", "application/octet-stream", ""), text_part("", "nameless"), file_part("notes", "n.txt", "Text/Plain", "a\r\n--b")]);
    let many_files = body(&[file_part("a", "a", "text/plain", "1"), file_part("b", "b", "text/plain", "2"), file_part("c", "c", "text/plain", "3")]);
    let cases: Vec<(&str, String, String, FormLimits)> = vec![
        ("mixed", content_type(), mixed.clone(), FormLimits::default()),
        ("mixed case media type", format!("Multipart/Form-Data; boundary={B}"), mixed.clone(), FormLimits::default()),
        ("max files", content_type(), many_files.clone(), FormLimits { max_files: 2, ..Default::default() }),
        ("max file size", content_type(), mixed.clone(), FormLimits { max_file_size: 3, ..Default::default() }),
        ("max parts", content_type(), many_files.clone(), FormLimits { max_parts: 2, ..Default::default() }),
        ("max total size", content_type(), many_files.clone(), FormLimits { max_total_size: Some(2), ..Default::default() }),
        ("max header size", content_type(), mixed.clone(), FormLimits { max_header_size: 20, ..Default::default() }),
        ("malformed", content_type(), format!("--{B}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nno end"), FormLimits::default()),
        ("missing boundary", "multipart/form-data".into(), mixed.clone(), FormLimits::default()),
        ("urlencoded", "application/x-www-form-urlencoded".into(), "a=1&a=2&b=x%20y".into(), FormLimits::default()),
        ("urlencoded max parts", "Application/X-WWW-Form-Urlencoded".into(), "a=1&b=2&c=3".into(), FormLimits { max_parts: 2, ..Default::default() }),
        ("unsupported", "application/json".into(), "{}".into(), FormLimits::default()),
        ("urlencoded prefix", "application/x-www-form-urlencodedx".into(), "a=1".into(), FormLimits::default()),
    ];
    for (label, ct, message, limits) in cases {
        let stable = block_on(FormData::parse(Some(&ct), Body::from(message.clone()), limits));
        let future = block_on(parse(Some(&ct), Body::from(message), limits));
        assert_eq!(outcome(future), outcome(stable), "{label}");
    }
}

#[test]
fn streams_file_contents_to_the_upload_handler() {
    let content = "x".repeat(200_000);
    let message = body(&[file_part("big", "big.bin", "application/octet-stream", &content), text_part("after", "ok")]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let form = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits { max_file_size: 1 << 20, ..Default::default() }, move |mut upload: FileUpload| {
        let log = log.clone();
        async move {
            assert_eq!(upload.size(), None, "the size is not known before the content is read");
            let mut total = 0;
            while let Some(chunk) = upload.chunk().await {
                total += chunk?.len();
            }
            log.lock().unwrap().push((upload.field_name.clone(), upload.name.clone(), total, upload.size()));
            Ok::<_, FormDataError>(format!("stored:{total}"))
        }
    }))
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), [("big".to_string(), "big.bin".to_string(), 200_000, Some(200_000))]);
    assert_eq!(entries(&form), [("big".to_string(), "stored:200000".to_string()), ("after".to_string(), "ok".to_string())]);
}

#[test]
fn calls_the_upload_handler_before_the_file_contents_arrive() {
    let (sender, request_body) = Body::channel();
    sender.send(format!("--{B}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"f.txt\"\r\n\r\n"));
    let sender = Mutex::new(Some(sender));
    let form = block_on(parse_with(Some(&content_type()), request_body, FormLimits::default(), |upload: FileUpload| {
        // The rest of the request is sent only once the handler has the upload.
        let sender = sender.lock().unwrap().take().expect("one file");
        sender.send(format!("late content\r\n--{B}--\r\n"));
        drop(sender);
        upload.text()
    }))
    .unwrap();
    assert_eq!(form.get("f"), Some("late content"));
}

#[test]
fn reads_contents_with_bytes_and_text_once() {
    let message = body(&[file_part("a", "a.txt", "text/plain", "alpha"), file_part("b", "b.txt", "text/plain", "beta")]);
    let form = block_on(parse_with(Some(&content_type()), Body::from(message.clone()), FormLimits::default(), |upload: FileUpload| async move {
        if upload.field_name == "a" {
            let bytes = upload.bytes().await?;
            Ok::<_, FormDataError>(format!("{} bytes", bytes.len()))
        } else {
            upload.text().await
        }
    }))
    .unwrap();
    assert_eq!(entries(&form), [("a".to_string(), "5 bytes".to_string()), ("b".to_string(), "beta".to_string())]);

    let err = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits::default(), |mut upload: FileUpload| async move {
        upload.chunk().await;
        upload.bytes().await.map(|_| ())
    }))
    .unwrap_err();
    assert_eq!(err.to_string(), "The contents of a file upload can only be read once");
}

#[test]
fn discards_contents_the_upload_handler_does_not_read() {
    let message = body(&[file_part("skip", "s.bin", "application/octet-stream", &"s".repeat(50_000)), text_part("next", "still parsed")]);
    let form = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits::default(), |_upload: FileUpload| async { "skipped" })).unwrap();
    assert_eq!(entries(&form), [("skip".to_string(), "skipped".to_string()), ("next".to_string(), "still parsed".to_string())]);
}

#[test]
fn stores_the_upload_as_a_file_when_the_handler_returns_it_unread() {
    let message = body(&[file_part("f", "f.txt", "Text/Plain", "kept")]);
    let form = block_on(parse_with(Some(&content_type()), Body::from(message.clone()), FormLimits::default(), |upload: FileUpload| async { upload })).unwrap();
    let file: &File = form.file("f").unwrap();
    assert_eq!((file.name.as_str(), file.content_type.as_str(), &file.bytes[..]), ("f.txt", "text/plain", &b"kept"[..]));
    let default = block_on(parse(Some(&content_type()), Body::from(message), FormLimits::default())).unwrap();
    assert_eq!(entries(&default), entries(&form));
}

#[test]
fn throws_when_the_handler_returns_the_upload_after_reading_it() {
    let message = body(&[file_part("f", "f.txt", "text/plain", "kept")]);
    let err = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits::default(), |mut upload: FileUpload| async move {
        upload.chunk().await;
        upload
    }))
    .unwrap_err();
    assert!(matches!(err, FormDataError::Upload(_)));
    assert!(err.to_string().starts_with("An upload handler cannot return a file upload after reading its contents"));
}

#[test]
fn throws_when_the_handler_keeps_the_upload() {
    let message = body(&[file_part("f", "f.txt", "text/plain", "kept")]);
    let kept = Mutex::new(Vec::new());
    let err = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits::default(), |upload: FileUpload| {
        kept.lock().unwrap().push(upload);
        std::future::ready(())
    }))
    .unwrap_err();
    assert!(err.to_string().starts_with("An upload handler cannot keep a file upload after it returns"));
}

#[test]
fn throws_max_file_size_exceeded_from_the_upload_stream_and_the_parser() {
    let message = body(&[file_part("f", "f.bin", "application/octet-stream", &"x".repeat(5000))]);
    let limits = FormLimits { max_file_size: 1000, ..Default::default() };
    let stream_errors = Arc::new(Mutex::new(Vec::new()));
    let log = stream_errors.clone();
    // The handler swallows the error; the parser still reports it.
    let err = block_on(parse_with(Some(&content_type()), Body::from(message.clone()), limits, move |mut upload: FileUpload| {
        let log = log.clone();
        async move {
            while let Some(chunk) = upload.chunk().await {
                if let Err(e) = chunk {
                    log.lock().unwrap().push(e);
                    break;
                }
            }
        }
    }))
    .unwrap_err();
    assert_eq!(err, FormDataError::Limit(MultipartError::MaxFileSizeExceeded(1000)));
    assert_eq!(*stream_errors.lock().unwrap(), [FormDataError::Limit(MultipartError::MaxFileSizeExceeded(1000))]);

    let unread = block_on(parse_with(Some(&content_type()), Body::from(message.clone()), limits, |_upload: FileUpload| async {})).unwrap_err();
    assert_eq!(unread, FormDataError::Limit(MultipartError::MaxFileSizeExceeded(1000)), "files the handler does not read count too");

    let propagated = block_on(parse_with(Some(&content_type()), Body::from(message), limits, |upload: FileUpload| upload.text())).unwrap_err();
    assert!(propagated.is_limit(), "a limit error returned by the handler stays a limit error: {propagated:?}");
}

#[test]
fn throws_max_files_exceeded_before_calling_the_upload_handler() {
    let message = body(&[file_part("a", "a", "text/plain", "1"), file_part("b", "b", "text/plain", "2")]);
    let mut calls = 0;
    let err = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits { max_files: 1, ..Default::default() }, |_upload: FileUpload| {
        calls += 1;
        std::future::ready(())
    }))
    .unwrap_err();
    assert_eq!(err, FormDataError::MaxFilesExceeded(1));
    assert_eq!(calls, 1);
}

#[test]
fn lets_errors_from_the_upload_handler_propagate() {
    let message = body(&[file_part("f", "f.txt", "text/plain", "x")]);
    let err = block_on(parse_with(Some(&content_type()), Body::from(message), FormLimits::default(), |_upload: FileUpload| async {
        Err::<(), _>(std::io::Error::other("storage is down"))
    }))
    .unwrap_err();
    let FormDataError::Upload(cause) = &err else { panic!("expected an upload error, got {err:?}") };
    assert_eq!(cause.to_string(), "storage is down");
}

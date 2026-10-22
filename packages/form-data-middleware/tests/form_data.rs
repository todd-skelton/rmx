//! Remix's `form-data-middleware/src/lib/form-data.test.ts`. Where Remix's middleware throws
//! (and the request rejects), rmx answers: 413 for limits, 400 for a malformed body and 500
//! for a failed upload handler, without running the handler.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rmx_fetch::{block_on, Body, FormData, FormLimits, FormValue, Response};
use rmx_form_data_middleware::{form_data, FileUpload, FormDataExt};
use rmx_fetch_router::{action, Context, Middleware, Next, Router};

const BOUNDARY: &str = "----WebKitFormBoundary1234567890";

fn post(router: &Router, content_type: Option<&str>, body: &str) -> Response {
    let mut req = http::Request::post("https://remix.run/");
    if let Some(ct) = content_type {
        req = req.header("content-type", ct);
    }
    block_on(router.fetch(req.body(Body::from(body.to_string())).unwrap()))
}

fn post_multipart(router: &Router, lines: &[&str]) -> Response {
    post(router, Some(&format!("multipart/form-data; boundary={BOUNDARY}")), &lines.join("\r\n"))
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

/// `[name, value]` pairs, files as their names.
fn entries(form: &FormData) -> String {
    form.to_search_params().to_string()
}

fn two_files() -> Vec<String> {
    let b = format!("--{BOUNDARY}");
    vec![
        b.clone(),
        "Content-Disposition: form-data; name=\"file1\"; filename=\"test1.txt\"".into(),
        "Content-Type: text/plain".into(),
        String::new(),
        "test 1".into(),
        b,
        "Content-Disposition: form-data; name=\"file2\"; filename=\"test2.txt\"".into(),
        "Content-Type: text/plain".into(),
        String::new(),
        "test 2".into(),
        format!("--{BOUNDARY}--"),
    ]
}

fn one_file(name: &str) -> Vec<String> {
    vec![
        format!("--{BOUNDARY}"),
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\""),
        "Content-Type: text/plain".into(),
        String::new(),
        "test".into(),
        format!("--{BOUNDARY}--"),
    ]
}

fn lines(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// A router whose POST handler records that it ran.
fn guarded(parser: impl Middleware) -> (Router, Arc<AtomicBool>) {
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    let mut router = Router::new().with(parser);
    router.post("/", move |_ctx: Context| {
        flag.store(true, Ordering::Relaxed);
        async { "ok" }
    });
    (router, called)
}

#[test]
fn parses_mixed_case_urlencoded_media_types_with_parameters() {
    let mut router = Router::new().with(form_data());
    router.post("/", |ctx: Context| async move { format!("{:?}", ctx.form_data().get_all("tag").iter().map(|v| v.as_text().unwrap()).collect::<Vec<_>>()) });
    let res = post(&router, Some("Application/X-Www-Form-Urlencoded; charset=UTF-8"), "tag=red&tag=green");
    assert_eq!(text(res), r#"["red", "green"]"#);
}

#[test]
fn runs_the_upload_handler_for_mixed_case_multipart_media_types() {
    let parser = form_data().upload_handler(|upload: FileUpload| async move {
        Some(FormValue::Text(format!("stored: {}", String::from_utf8_lossy(&upload.file.bytes))))
    });
    let mut router = Router::new().with(parser);
    router.post("/", |ctx: Context| async move { ctx.form_data().get("file").unwrap_or_default().to_string() });
    let body = [
        format!("--{BOUNDARY}"),
        "Content-Disposition: form-data; name=\"file\"; filename=\"example.txt\"".into(),
        "Content-Type: application/octet-stream".into(),
        String::new(),
        "hello".into(),
        format!("--{BOUNDARY}--"),
    ];
    let res = post(&router, Some(&format!("Multipart/Form-Data; boundary={BOUNDARY}")), &body.join("\r\n"));
    assert_eq!(text(res), "stored: hello");
}

#[test]
fn does_not_suppress_limits_for_mixed_case_form_media_types() {
    let (router, called) = guarded(form_data().limits(FormLimits { max_parts: 1, ..Default::default() }).suppress_errors(true));
    let res = post(&router, Some("Application/X-Www-Form-Urlencoded"), "tag=red&tag=green");
    assert_eq!(res.status(), 413);
    assert_eq!(text(res), "Multipart part count exceeds maximum allowed count of 1");
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn ignores_media_types_that_only_start_with_the_urlencoded_media_type() {
    let mut router = Router::new().with(form_data());
    router.post("/", |mut ctx: Context| async move {
        let fields = entries(ctx.form_data());
        let body = ctx.take_body().text().await.unwrap();
        format!("fields=[{fields}] body={body}")
    });
    let res = post(&router, Some("application/x-www-form-urlencoded-extra"), "tag=red");
    assert_eq!(text(res), "fields=[] body=tag=red");
}

#[test]
fn parses_application_x_www_form_urlencoded_form_data_from_the_request_body() {
    let mut router = Router::new().with(form_data());
    router.post("/", |ctx: Context| async move { entries(ctx.form_data()) });
    let res = post(&router, Some("application/x-www-form-urlencoded"), "name=test");
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "name=test");
}

#[test]
fn parses_multipart_form_data_form_data_from_the_request_body() {
    let mut router = Router::new().with(form_data());
    router.post("/", |ctx: Context| async move { entries(ctx.get::<FormData>().unwrap()) });
    let b = format!("--{BOUNDARY}");
    let end = format!("--{BOUNDARY}--");
    let res = post_multipart(&router, &[&b, "Content-Disposition: form-data; name=\"name\"", "", "test", &end]);
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "name=test");
}

#[test]
fn stores_uploaded_files_in_the_context_form_data_on_a_multipart_request() {
    let mut router = Router::new().with(form_data());
    router.post("/", |ctx: Context| async move {
        let form = ctx.get::<FormData>().unwrap();
        ["file1", "file2"].map(|name| form.file(name).map(|f| format!("{}:{}", f.name, f.content_type)).unwrap_or_default()).join(" ")
    });
    let res = post_multipart(&router, &lines(&two_files()));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "test1.txt:text/plain test2.txt:text/plain");
}

#[test]
fn provides_an_empty_form_data_for_get_requests() {
    let mut router = Router::new().with(form_data());
    router.get("/", |ctx: Context| async move { format!("defined={} empty={}", ctx.get::<FormData>().is_some(), ctx.form_data().is_empty()) });
    let res = block_on(router.fetch(http::Request::get("https://remix.run/").body(Body::empty()).unwrap()));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "defined=true empty=true");
}

#[test]
fn provides_an_empty_form_data_for_head_requests() {
    let state = Arc::new(Mutex::new(String::new()));
    let sink = state.clone();
    let mut router = Router::new().with(form_data());
    router.head("/", move |ctx: Context| {
        *sink.lock().unwrap() = format!("defined={} empty={}", ctx.get::<FormData>().is_some(), ctx.form_data().is_empty());
        async { "" }
    });
    let res = block_on(router.fetch(http::Request::head("https://remix.run/").body(Body::empty()).unwrap()));
    assert_eq!(res.status(), 200);
    assert_eq!(*state.lock().unwrap(), "defined=true empty=true");
}

#[test]
fn rejects_a_malformed_multipart_form_data_body() {
    let (router, called) = guarded(form_data());
    let res = post(&router, Some("multipart/form-data"), "invalid");
    assert_eq!(res.status(), 400);
    assert!(text(res).starts_with("invalid form data: Cannot parse form data"));
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn suppresses_parse_errors_when_suppress_errors_is_true() {
    let mut router = Router::new().with(form_data().suppress_errors(true));
    router.post("/", |ctx: Context| async move { format!("[{}]", entries(ctx.get::<FormData>().unwrap())) });
    let res = post(&router, Some("multipart/form-data"), "invalid");
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "[]");
}

#[test]
fn sets_an_empty_form_data_when_parse_errors_are_suppressed() {
    let mut router = Router::new().with(form_data().suppress_errors(true));
    router.post("/", |ctx: Context| async move { format!("defined={} empty={}", ctx.get::<FormData>().is_some(), ctx.form_data().is_empty()) });
    let res = post(&router, Some("multipart/form-data"), "invalid");
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "defined=true empty=true");
}

#[test]
fn does_not_suppress_max_files_errors_when_suppress_errors_is_true() {
    let (router, called) = guarded(form_data().suppress_errors(true).limits(FormLimits { max_files: 1, ..Default::default() }));
    let res = post_multipart(&router, &lines(&two_files()));
    assert_eq!(res.status(), 413);
    assert_eq!(text(res), "Maximum number of files exceeded: 1");
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn does_not_suppress_max_header_size_errors_when_suppress_errors_is_true() {
    let (router, called) = guarded(form_data().suppress_errors(true).limits(FormLimits { max_header_size: 4 * 1024, ..Default::default() }));
    let big = format!("X-Large-Header: {}", "X".repeat(6 * 1024));
    let b = format!("--{BOUNDARY}");
    let end = format!("--{BOUNDARY}--");
    let res = post_multipart(&router, &[&b, "Content-Disposition: form-data; name=\"field1\"", &big, "", "value1", &end]);
    assert_eq!(res.status(), 413);
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn does_not_suppress_max_file_size_errors_when_suppress_errors_is_true() {
    let (router, called) = guarded(form_data().suppress_errors(true).limits(FormLimits { max_file_size: 4, ..Default::default() }));
    let b = format!("--{BOUNDARY}");
    let end = format!("--{BOUNDARY}--");
    let res = post_multipart(&router, &[&b, "Content-Disposition: form-data; name=\"file1\"; filename=\"test1.txt\"", "Content-Type: text/plain", "", "hello", &end]);
    assert_eq!(res.status(), 413);
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn does_not_suppress_max_parts_errors_when_suppress_errors_is_true() {
    let (router, called) = guarded(form_data().suppress_errors(true).limits(FormLimits { max_parts: 2, ..Default::default() }));
    let b = format!("--{BOUNDARY}");
    let end = format!("--{BOUNDARY}--");
    let res = post_multipart(
        &router,
        &[
            &b,
            "Content-Disposition: form-data; name=\"field1\"",
            "",
            "value1",
            &b,
            "Content-Disposition: form-data; name=\"file1\"; filename=\"test1.txt\"",
            "Content-Type: text/plain",
            "",
            "test1",
            &b,
            "Content-Disposition: form-data; name=\"field2\"",
            "",
            "value2",
            &end,
        ],
    );
    assert_eq!(res.status(), 413);
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn does_not_suppress_max_total_size_errors_when_suppress_errors_is_true() {
    let (router, called) = guarded(form_data().suppress_errors(true).limits(FormLimits { max_total_size: Some(9), ..Default::default() }));
    let b = format!("--{BOUNDARY}");
    let end = format!("--{BOUNDARY}--");
    let res = post_multipart(
        &router,
        &[
            &b,
            "Content-Disposition: form-data; name=\"field1\"",
            "",
            "hello",
            &b,
            "Content-Disposition: form-data; name=\"file1\"; filename=\"test1.txt\"",
            "Content-Type: text/plain",
            "",
            "world",
            &end,
        ],
    );
    assert_eq!(res.status(), 413);
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn invokes_a_custom_upload_handler_for_file_uploads() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = seen.clone();
    let parser = form_data().upload_handler(move |upload: FileUpload| {
        let f = &upload.file;
        sink.lock().unwrap().push(format!("{} {} {} {}", upload.field_name, f.name, f.content_type, String::from_utf8_lossy(&f.bytes)));
        async { None }
    });
    let mut router = Router::new().with(parser);
    router.post("/", |_ctx: Context| async { "home" });
    let res = post_multipart(&router, &lines(&two_files()));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "home");
    assert_eq!(*seen.lock().unwrap(), ["file1 test1.txt text/plain test 1", "file2 test2.txt text/plain test 2"]);
}

/// An upload handler that answers `{prefix}:{file name}` and counts its calls.
fn named_handler(prefix: &'static str, calls: &Arc<AtomicUsize>) -> impl Middleware {
    let calls = calls.clone();
    form_data().upload_handler(move |upload: FileUpload| {
        calls.fetch_add(1, Ordering::Relaxed);
        async move { Some(FormValue::Text(format!("{prefix}:{}", upload.file.name))) }
    })
}

#[test]
fn is_a_no_op_when_form_data_has_already_been_parsed_by_an_earlier_middleware() {
    let (first, second) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut router = Router::new().with(named_handler("first", &first)).with(named_handler("second", &second));
    router.post("/", |ctx: Context| async move { ctx.form_data().get("file").unwrap_or_default().to_string() });
    let res = post_multipart(&router, &lines(&one_file("test.txt")));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "first:test.txt");
    assert_eq!((first.load(Ordering::Relaxed), second.load(Ordering::Relaxed)), (1, 0));
}

#[test]
fn uses_form_data_that_an_earlier_middleware_set() {
    let set_form_data = |mut ctx: Context, next: Next| async move {
        let mut parsed = FormData::new();
        parsed.set("name", "test");
        ctx.set(parsed);
        next.run(ctx).await
    };
    let mut router = Router::new().with(set_form_data).with(form_data());
    router.post("/", |ctx: Context| async move { ctx.form_data().get("name").unwrap_or_default().to_string() });
    let res = post(&router, None, "");
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "test");
}

#[test]
fn is_a_no_op_when_form_data_has_already_been_parsed_by_earlier_request_pipeline_middleware() {
    let (global, route) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut router = Router::new().with(named_handler("global", &global));
    router.post(
        "/",
        action(|ctx: Context| async move { ctx.form_data().get("file").unwrap_or_default().to_string() }).with(named_handler("route", &route)),
    );
    let res = post_multipart(&router, &lines(&one_file("test.txt")));
    assert_eq!(res.status(), 200);
    assert_eq!(text(res), "global:test.txt");
    assert_eq!((global.load(Ordering::Relaxed), route.load(Ordering::Relaxed)), (1, 0));
}

#[test]
fn a_failed_upload_handler_answers_500_unless_errors_are_suppressed() {
    // Remix rethrows the handler's error; with `suppressErrors` it becomes an empty form.
    let failing = || form_data().upload_handler(|_: FileUpload| async { Err::<Option<FormValue>, _>("disk full") });
    let (router, called) = guarded(failing());
    let res = post_multipart(&router, &lines(&one_file("test.txt")));
    assert_eq!(res.status(), 500);
    assert_eq!(text(res), "disk full");
    assert!(!called.load(Ordering::Relaxed));

    let (router, called) = guarded(failing().suppress_errors(true));
    assert_eq!(post_multipart(&router, &lines(&one_file("test.txt"))).status(), 200);
    assert!(called.load(Ordering::Relaxed));
}

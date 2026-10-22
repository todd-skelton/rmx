//! Port of Remix's `response/src/lib/html.test.ts` (`createHtmlResponse` is `response::html`
//! and `html_with`). Remix's `Blob` body is an in-memory `File`; `ArrayBuffer`, `Uint8Array`
//! and `DataView` bodies are `Bytes`, `Vec<u8>` and `&[u8]`; a `ReadableStream` is a `Body`.

use http::StatusCode;
use rmx_response::{html, html_with, ResponseInit};
use rmx_fetch::{block_on, Body, File, Response};
use rmx_html_template::html as safe_html;

const PAGE: &str = "<html><body>Hello</body></html>";
const PAGE_WITH_DOCTYPE: &str = "<!DOCTYPE html><html><body>Hello</body></html>";

fn text(response: Response) -> String {
    block_on(response.into_body().text()).unwrap()
}

/// A stream that sends `chunks` and closes.
fn stream(chunks: Vec<String>) -> Body {
    Body::from_task(move |tx| async move {
        for chunk in chunks {
            tx.send(chunk);
        }
    })
}

#[test]
fn creates_a_response_with_html_content_type_header() {
    let response = html("<h1>Hello</h1>");

    assert_eq!(response.headers()["content-type"], "text/html; charset=UTF-8");
    assert_eq!(text(response), "<!DOCTYPE html><h1>Hello</h1>");
}

#[test]
fn preserves_custom_headers_and_status_from_init() {
    let response = html_with("<h1>Hello</h1>", ResponseInit::status(StatusCode::CREATED).header("X-Custom", "a"));

    assert_eq!(response.headers()["content-type"], "text/html; charset=UTF-8");
    assert_eq!(response.headers()["x-custom"], "a");
    assert_eq!(response.status(), 201);
    assert_eq!(text(response), "<!DOCTYPE html><h1>Hello</h1>");
}

#[test]
fn allows_overriding_content_type_header() {
    let response = html_with("<h1>Hello</h1>", ResponseInit::default().header("Content-Type", "text/plain"));

    assert_eq!(response.headers().get_all("content-type").iter().count(), 1);
    assert_eq!(response.headers()["content-type"], "text/plain");
}

#[test]
fn accepts_safe_html_from_escape_tag_without_re_escaping() {
    let snippet = safe_html!("<strong>{}</strong>", "Hi");

    assert_eq!(text(html(snippet)), "<!DOCTYPE html><strong>Hi</strong>");
}

// DOCTYPE prepending: string body

#[test]
fn prepends_doctype_to_string_body() {
    assert_eq!(text(html(PAGE)), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present() {
    assert_eq!(text(html(PAGE_WITH_DOCTYPE)), PAGE_WITH_DOCTYPE);
}

#[test]
fn handles_doctype_with_leading_whitespace() {
    let body = "  <!DOCTYPE html><html><body>Hello</body></html>";

    assert_eq!(text(html(body)), body);
}

#[test]
fn handles_doctype_case_insensitively() {
    let body = "<!doctype html><html><body>Hello</body></html>";

    assert_eq!(text(html(body)), body);
}

// SafeHtml body

#[test]
fn prepends_doctype_to_safe_html_body() {
    assert_eq!(text(html(safe_html!("<html><body>Hello</body></html>"))), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_safe_html() {
    assert_eq!(text(html(safe_html!("<!DOCTYPE html><html><body>Hello</body></html>"))), PAGE_WITH_DOCTYPE);
}

// Blob body

#[test]
fn prepends_doctype_to_blob_body() {
    assert_eq!(text(html(File::new("", "text/html", PAGE))), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_blob() {
    assert_eq!(text(html(File::new("", "text/html", PAGE_WITH_DOCTYPE))), PAGE_WITH_DOCTYPE);
}

// ArrayBuffer body

#[test]
fn prepends_doctype_to_array_buffer_body() {
    assert_eq!(text(html(rmx_fetch::Bytes::from_static(PAGE.as_bytes()))), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_array_buffer() {
    assert_eq!(text(html(rmx_fetch::Bytes::from_static(PAGE_WITH_DOCTYPE.as_bytes()))), PAGE_WITH_DOCTYPE);
}

// Uint8Array body

#[test]
fn prepends_doctype_to_uint8_array_body() {
    assert_eq!(text(html(PAGE.as_bytes().to_vec())), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_uint8_array() {
    assert_eq!(text(html(PAGE_WITH_DOCTYPE.as_bytes().to_vec())), PAGE_WITH_DOCTYPE);
}

// DataView body

#[test]
fn prepends_doctype_to_data_view_body() {
    assert_eq!(text(html(PAGE.as_bytes())), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_data_view() {
    assert_eq!(text(html(PAGE_WITH_DOCTYPE.as_bytes())), PAGE_WITH_DOCTYPE);
}

// ReadableStream body

#[test]
fn prepends_doctype_to_readable_stream_body() {
    assert_eq!(text(html(stream(vec![PAGE.into()]))), PAGE_WITH_DOCTYPE);
}

#[test]
fn does_not_prepend_doctype_if_already_present_in_readable_stream() {
    assert_eq!(text(html(stream(vec![PAGE_WITH_DOCTYPE.into()]))), PAGE_WITH_DOCTYPE);
}

/// Remix also counts the bytes `TextDecoder` decodes (under 1 KiB). That has no hook here;
/// the sniff reads a 64-byte prefix and grows it only while the prefix is whitespace.
#[test]
fn checks_a_short_prefix_without_decoding_a_large_first_chunk() {
    let content = format!("<!DOCTYPE html><p>{}</p>", "日本語🙂".repeat(5000));
    let response = html(stream(vec![content.clone()]));

    assert_eq!(block_on(response.into_body().bytes()).unwrap(), content.as_bytes());
}

#[test]
fn recognizes_a_doctype_that_crosses_the_prefix_boundary() {
    let content = format!("{}<!doctype html><p>日本語🙂</p>", " ".repeat(60));

    assert_eq!(text(html(stream(vec![content.clone()]))), content);
}

#[test]
fn recognizes_a_doctype_after_unicode_whitespace_split_at_the_prefix_boundary() {
    let content = format!("{}\u{2003}<!DOCTYPE html><p>日本語🙂</p>", " ".repeat(63));

    assert_eq!(text(html(stream(vec![content.clone()]))), content);
}

#[test]
fn prepends_a_doctype_after_long_whitespace_when_one_is_missing() {
    let content = format!("{}<p>日本語🙂</p>", "\u{2003}".repeat(50));

    assert_eq!(text(html(stream(vec![content.clone()]))), format!("<!DOCTYPE html>{content}"));
}

#[test]
fn handles_empty_readable_stream() {
    assert_eq!(text(html(stream(vec![]))), "<!DOCTYPE html>");
}

#[test]
fn handles_multi_chunk_readable_stream() {
    let chunks = vec!["<html>".into(), "<body>".into(), "Hello</body></html>".into()];

    assert_eq!(text(html(stream(chunks))), PAGE_WITH_DOCTYPE);
}

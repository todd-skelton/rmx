use std::sync::Mutex;

use rmx_fetch::{block_on, Body};
use rmx_router::Router;

use super::*;

const CSI: &str = "\x1b[";

fn paint(code: u8, value: &str) -> String {
    format!("{CSI}{code}m{value}{CSI}39m")
}

/// Run one request through `logger` and return the single logged message.
fn log_request(logger: LoggerMiddleware, method: &str, respond: fn() -> Response) -> String {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let sink = messages.clone();
    let mut router = Router::new().with(logger.log(move |m| sink.lock().unwrap().push(m.to_string())));
    router.route(rmx_router::routes::any("/"), move |_: Context| async move { respond() });
    let req = http::Request::builder().method(method).uri("https://remix.run/").body(Body::empty()).unwrap();
    block_on(router.fetch(req));
    let messages = messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    messages[0].clone()
}

fn with_headers(status: u16, headers: &[(&'static str, &'static str)]) -> Response {
    let mut res = rmx_fetch::response::text("Home");
    *res.status_mut() = http::StatusCode::from_u16(status).unwrap();
    for (name, value) in headers {
        res.headers_mut().insert(*name, value.parse().unwrap());
    }
    res
}

#[test]
fn provides_the_logger_on_the_request_context() {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let sink = messages.clone();
    let mut router = Router::new().with(logger().colors(false).format("access").log(move |m| sink.lock().unwrap().push(m.to_string())));
    router.get("/", |ctx: Context| async move {
        ctx.logger().log("direct");
        ctx.get::<Logger>().unwrap().log("keyed");
        "Home"
    });
    let res = block_on(router.fetch(http::Request::get("https://remix.run/").body(Body::empty()).unwrap()));
    assert_eq!(res.status(), 200);
    assert_eq!(*messages.lock().unwrap(), ["direct", "keyed", "access"]);
}

#[test]
fn logs_the_request_in_the_default_format() {
    let message = log_request(logger().colors(false), "GET", || with_headers(200, &[("content-length", "4"), ("content-type", "text/plain")]));
    let (date, rest) = message.strip_prefix('[').unwrap().split_once("] ").unwrap();
    assert_eq!(rest, "GET / 200 4");
    let b = date.as_bytes();
    assert!(date.len() == 26 && b[2] == b'/' && b[6] == b'/' && b[11] == b':' && date.ends_with(" +0000"), "{date}");
}

#[test]
fn colorizes_high_signal_tokens() {
    let message = log_request(logger().colors(true).format("%method %status %duration %contentLength"), "GET", || with_headers(200, &[("content-length", "2048")]));
    let parts: Vec<&str> = message.split(' ').collect();
    assert_eq!(parts[0], paint(32, "GET"));
    assert_eq!(parts[1], paint(32, "200"));
    assert!(["32", "33", "35", "31"].iter().any(|c| parts[2].starts_with(&format!("{CSI}{c}m"))) && parts[2].ends_with(&format!("{CSI}39m")));
    assert_eq!(parts[3], paint(36, "2048"));

    let message = log_request(logger().colors(true).format("%method %status %contentLength"), "DELETE", || with_headers(404, &[("content-length", "204800")]));
    assert_eq!(message, format!("{} {} {}", paint(31, "DELETE"), paint(33, "404"), paint(33, "204800")));
    assert_eq!(log_request(logger().colors(true).format("%contentLength"), "GET", || with_headers(200, &[("content-length", "bad")])), "bad");
    assert_eq!(log_request(logger().colors(false).format("%method %status %contentLength"), "GET", || with_headers(200, &[("content-length", "2048")])), "GET 200 2048");
}

#[test]
fn every_token() {
    let format = "%dateISO|%contentType|%host|%hostname|%method|%path|%pathname|%port|%protocol|%query|%search|%referer|%statusText|%url|%userAgent|%nope|100%";
    let messages = Arc::new(Mutex::new(Vec::new()));
    let sink = messages.clone();
    let mut router = Router::new().with(logger().colors(false).format(format).log(move |m| sink.lock().unwrap().push(m.to_string())));
    router.get("/a/b", |_: Context| async { with_headers(201, &[]) });
    let req = http::Request::get("http://remix.run:8080/a/b?x=1").header("referer", "https://ref.example/").header("user-agent", "test-agent").body(Body::empty()).unwrap();
    block_on(router.fetch(req));
    let message = messages.lock().unwrap()[0].clone();
    let (iso, rest) = message.split_once('|').unwrap();
    assert!(iso.len() == 24 && iso.ends_with('Z') && &iso[10..11] == "T", "{iso}");
    assert_eq!(
        rest,
        "text/plain; charset=UTF-8|remix.run:8080|remix.run|GET|/a/b?x=1|/a/b|8080|http:|?x=1|?x=1|https://ref.example/|Created|http://remix.run:8080/a/b?x=1|test-agent|-|100%"
    );
}

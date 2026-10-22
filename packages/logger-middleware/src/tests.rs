//! Remix's `logger-middleware/src/lib/logger.test.ts`, plus every format token. Remix's
//! environment and TTY cases mock `process`; here they feed the same detection
//! (`rmx_terminal::should_use_colors`, which `logger()` calls) an explicit environment.

use std::collections::HashMap;
use std::sync::Mutex;

use rmx_fetch::{block_on, Body};
use rmx_router::Router;

use super::*;

const CSI: &str = "\x1b[";

fn paint(code: u8, value: &str) -> String {
    format!("{CSI}{code}m{value}{CSI}39m")
}

/// Remix's `logRequest`: run one request through `logger` and return the single message.
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

fn home_2048() -> Response {
    with_headers(200, &[("content-length", "2048")])
}

fn home() -> Response {
    with_headers(200, &[])
}

/// The default color decision for an environment (`[(name, value)]`) and TTY state.
fn default_colors(env: &[(&str, &str)], is_tty: bool) -> bool {
    let env: HashMap<String, String> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    rmx_terminal::should_use_colors(|name| env.get(name).cloned(), is_tty)
}

#[test]
fn provides_the_configured_logger_on_request_context() {
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
    assert_eq!(block_on(res.into_body().text()).unwrap(), "Home");
    assert_eq!(*messages.lock().unwrap(), ["direct", "keyed", "access"]);
}

#[test]
fn logs_the_request() {
    let message = log_request(logger().colors(false), "GET", || with_headers(200, &[("content-length", "4"), ("content-type", "text/plain")]));
    let (date, rest) = message.strip_prefix('[').unwrap().split_once("] ").unwrap();
    assert_eq!(rest, "GET / 200 4");
    let b = date.as_bytes();
    assert!(date.len() == 26 && b[2] == b'/' && b[6] == b'/' && b[11] == b':' && date.ends_with(" +0000"), "{date}");
}

#[test]
fn colorizes_high_signal_tokens_when_colors_are_enabled() {
    let message = log_request(logger().colors(true).format("%method %status %duration %contentLength"), "GET", home_2048);
    let parts: Vec<&str> = message.split(' ').collect();
    assert_eq!(parts[0], paint(32, "GET"));
    assert_eq!(parts[1], paint(32, "200"));
    let color = ["32", "33", "35", "31"].into_iter().find(|c| parts[2].starts_with(&format!("{CSI}{c}m"))).unwrap();
    let digits = parts[2].strip_prefix(&format!("{CSI}{color}m")).unwrap().strip_suffix(&format!("{CSI}39m")).unwrap();
    assert!(!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()), "{digits}");
    assert_eq!(parts[3], paint(36, "2048"));
}

#[test]
fn colorizes_methods_statuses_and_content_lengths_by_severity() {
    let message = log_request(logger().colors(true).format("%method %status %contentLength"), "DELETE", || {
        with_headers(404, &[("content-length", "204800")])
    });
    assert_eq!(message, format!("{} {} {}", paint(31, "DELETE"), paint(33, "404"), paint(33, "204800")));
}

#[test]
fn opts_out_of_colors_with_the_colors_option() {
    assert_eq!(log_request(logger().colors(false).format("%method %status %contentLength"), "GET", home_2048), "GET 200 2048");
}

#[test]
fn respects_no_color_by_default() {
    let colors = default_colors(&[("NO_COLOR", "1")], true);
    assert!(!colors);
    assert_eq!(log_request(logger().colors(colors).format("%method %status %contentLength"), "GET", home_2048), "GET 200 2048");
}

#[test]
fn forces_colors_on_with_the_colors_option() {
    // NO_COLOR only changes the default; an explicit `colors(true)` wins.
    assert!(!default_colors(&[("NO_COLOR", "1")], true));
    let message = log_request(logger().colors(true).format("%method %status"), "GET", home);
    assert_eq!(message, format!("{} {}", paint(32, "GET"), paint(32, "200")));
}

#[test]
fn enables_colors_by_default_in_tty_environments() {
    let colors = default_colors(&[], true);
    assert!(colors);
    let message = log_request(logger().colors(colors).format("%method %status"), "GET", home);
    assert_eq!(message, format!("{} {}", paint(32, "GET"), paint(32, "200")));
}

#[test]
fn leaves_colors_off_by_default_outside_tty_environments() {
    let colors = default_colors(&[], false);
    assert!(!colors);
    assert_eq!(log_request(logger().colors(colors).format("%method %status %contentLength"), "GET", home_2048), "GET 200 2048");
}

#[test]
fn the_default_follows_this_process_environment_and_stdout() {
    assert_eq!(logger().colors, rmx_terminal::stdout_colors());
}

#[test]
fn leaves_invalid_content_lengths_uncolored() {
    let message = log_request(logger().colors(true).format("%contentLength"), "GET", || with_headers(200, &[("content-length", "bad")]));
    assert_eq!(message, "bad");
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

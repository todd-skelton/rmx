//! Remix's `join.test.ts`.

use rsmix_route_pattern::RoutePattern;

fn assert_join(a: &str, b: &str, expected: &str) { let joined = RoutePattern::parse(a).unwrap().join(&RoutePattern::parse(b).unwrap());
    assert_eq!(joined.to_string(), RoutePattern::parse(expected).unwrap().to_string(), "{a} + {b}");
}

#[test]
fn joins_protocol() {
    assert_join("http://", "://", "http://");
    assert_join("://", "://", "://");
    assert_join("://", "http://", "http://");

    assert_join("http://", "://example.com", "http://example.com");
    assert_join("://example.com", "http://", "http://example.com");

    assert_join("http://", "https://", "https://");
    assert_join("://example.com", "https://", "https://example.com");
    assert_join("http://example.com", "https://", "https://example.com");

    assert_join("http(s)://", "https://", "https://");
    assert_join("https://", "http(s)://", "http(s)://");
}

#[test]
fn joins_hostname() {
    assert_join("://example.com", "://*", "://example.com");
    assert_join("://*", "://*", "://*");
    assert_join("://*", "://example.com", "://example.com");

    assert_join("://example.com", "://*host", "://*host");
    assert_join("://*host", "://example.com", "://example.com");
    assert_join("://*host", "://*other", "://*other");
    assert_join("://*", "://*host", "://*host");

    assert_join("://example.com", "://other.com", "://other.com");
    assert_join("://", "://other.com", "://other.com");
    assert_join("http://example.com", "://other.com", "http://other.com");
    assert_join("://example.com/pathname", "://other.com", "://other.com/pathname");
    assert_join("/pathname", "://other.com", "://other.com/pathname");
}

#[test]
fn joins_port() {
    assert_join("://example.com:8000", "://", "://example.com:8000");
    assert_join("://example.com", "://example.com:8000", "://example.com:8000");
    assert_join("://example.com:8000", "://example.com:3000", "://example.com:3000");
    assert_join("http://example.com:4321", "://example.com:8000", "http://example.com:8000");
}

#[test]
fn joins_pathname() {
    assert_join("", "", "");
    assert_join("", "b", "b");
    assert_join("a", "", "a");

    assert_join("a", "b", "a/b");
    assert_join("a/", "b", "a/b");
    assert_join("a", "/b", "a/b");
    assert_join("a/", "/b", "a/b");

    assert_join("(a)", "(b)", "(a)/(b)");
    assert_join("(a/)", "(b)", "(a)/(b)");
    assert_join("(a)", "(/b)", "(a)(/b)");
    assert_join("(a/)", "(/b)", "(a)(/b)");
}

#[test]
fn joins_search() {
    assert_join("path", "?a", "path?a");
    assert_join("?a", "?b=1", "?a&b=1");
    assert_join("?a=1", "?b=2", "?a=1&b=2");
}

#[test]
fn joins_complex_combinations() {
    assert_join("http://example.com/a", "http(s)://*host/b", "http(s)://*host/a/b");
    assert_join("http://example.com:8000/a", "https:///b", "https://example.com:8000/a/b");
    assert_join("http://example.com:8000/a", "://other.com/b", "http://other.com:8000/a/b");

    assert_join("https://api.example.com:8000/v1/:resource", "/users/(admin/)posts?filter&sort=asc", "https://api.example.com:8000/v1/:resource/users/(admin/)posts?filter=&sort=asc" );

    assert_join("http(s)://example.com/base", "http(s)://other.com/path", "http(s)://other.com/base/path" );

    assert_join("http://old.com:3000/keep/this", "https://new.com:8080", "https://new.com:8080/keep/this" );

    assert_join("users/:id?tab=profile", "posts/:postId?sort=recent", "users/:id/posts/:postId?tab=profile&sort=recent" );

    assert_join("://(staging.)example.com/api(/:version)", "://*/resources/:id(.json)", "://(staging.)example.com/api(/:version)/resources/:id(.json)");
}

// types/join.test.ts: the type-level JoinPatterns cases, checked at run time. Remix's `never`
// (a `:proto` protocol) is a parse error here.
#[test]
fn join_type_cases() {
    for (a, b, expected) in [
        ("", "", "/"),
        ("/", "/", "/"),
        ("hello", "", "/hello"),
        ("", "hello", "/hello"),
        ("http://example.com:8080", "https://remix.run", "https://remix.run:8080/"),
        ("http://example.com:8080", "://remix.run", "http://remix.run:8080/"),
        ("http://example.com", "://remix.run:8080", "http://remix.run:8080/"),
        ("https://example.com:8080/base", "http:///next", "http://example.com:8080/base/next"),
        ("https://remix.run", "api", "https://remix.run/api"),
        ("https://remix.run/", "api", "https://remix.run/api"),
        ("https://remix.run:8080", "api", "https://remix.run:8080/api"),
        ("https://remix.run:8080/", "api", "https://remix.run:8080/api"),
        ("/", "hello", "/hello"),
        ("/", "/hello", "/hello"),
        ("hello", "/", "/hello"),
        ("/hello", "/", "/hello"),
        ("hello", "/world", "/hello/world"),
        ("/hello", "/world", "/hello/world"),
        ("hello/", "/world", "/hello/world"),
        ("/hello/", "/world", "/hello/world"),
        ("hello", "world", "/hello/world"),
        ("/hello", "world", "/hello/world"),
        ("hello/", "world", "/hello/world"),
        ("/hello/", "world", "/hello/world"),
        ("", "(/:lang)/world", "(/:lang)/world"),
        ("/", "(/:lang)/world", "(/:lang)/world"),
        ("hello", "(/:lang)/world", "/hello(/:lang)/world"),
        ("hello/", "(/:lang)/world", "/hello(/:lang)/world"),
        ("https://remix.run", "?q=1", "https://remix.run/?q=1"),
        ("https://remix.run?q=1", "?q=2", "https://remix.run/?q=1&q=2"),
    ] {
        assert_join(a, b, expected);
    }
    assert!(RoutePattern::parse(":proto://example.com/path").is_err());
}

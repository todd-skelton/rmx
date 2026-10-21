//! Remix's `route-pattern/split.test.ts`.

use crate::split::{split, SplitResult};

fn assert_split(source: &str, expected: SplitResult) {
    assert_eq!(split(source), expected, "{source}");
}

#[test]
fn extracts_protocol() {
    assert_split("http://", SplitResult { protocol: Some((0, 4)), ..Default::default() });
}

#[test]
fn extracts_hostname() {
    assert_split("://example.com", SplitResult { hostname: Some((3, 14)), ..Default::default() });
}

#[test]
fn extracts_port() {
    assert_split("://example.com:8000", SplitResult { hostname: Some((3, 14)), port: Some((15, 19)), ..Default::default() });
}

#[test]
fn extracts_pathname() {
    assert_split("pathname", SplitResult { pathname: Some((0, 8)), ..Default::default() });
    assert_split("/pathname", SplitResult { pathname: Some((1, 9)), ..Default::default() });
    assert_split("//pathname", SplitResult { pathname: Some((1, 10)), ..Default::default() });
}

#[test]
fn returns_null_for_empty_pathname() {
    assert_split("/", SplitResult { ..Default::default() });
    assert_split("http:///", SplitResult { protocol: Some((0, 4)), ..Default::default() });
    assert_split("://example/", SplitResult { hostname: Some((3, 10)), ..Default::default() });
}

#[test]
fn extracts_search() {
    assert_split("?q=1", SplitResult { search: Some((1, 4)), ..Default::default() });
}

#[test]
fn extracts_protocol_hostname() {
    assert_split("http://example.com", SplitResult { protocol: Some((0, 4)), hostname: Some((7, 18)), ..Default::default() });
}

#[test]
fn extracts_protocol_pathname() {
    assert_split("http:///pathname", SplitResult { protocol: Some((0, 4)), pathname: Some((8, 16)), ..Default::default() });
}

#[test]
fn extracts_hostname_pathname() {
    assert_split("://example.com/pathname", SplitResult { hostname: Some((3, 14)), pathname: Some((15, 23)), ..Default::default() });
}

#[test]
fn extracts_protocol_hostname_pathname() {
    assert_split("http://example.com/pathname", SplitResult { protocol: Some((0, 4)), hostname: Some((7, 18)), pathname: Some((19, 27)), ..Default::default() });
}

#[test]
fn extracts_protocol_hostname_port_pathname_search() {
    assert_split("http://example.com:8000/pathname?q=1", SplitResult { protocol: Some((0, 4)), hostname: Some((7, 18)), port: Some((19, 23)), pathname: Some((24, 32)), search: Some((33, 36)) });
}

#[test]
fn treats_slash_before_solidus_as_pathname() {
    assert_split("pathname/then://solidus", SplitResult { pathname: Some((0, 23)), ..Default::default() });
    assert_split("/pathname/then://solidus", SplitResult { pathname: Some((1, 24)), ..Default::default() });
}

#[test]
fn treats_question_mark_before_solidus_as_search() {
    assert_split("?search://solidus", SplitResult { search: Some((1, 17)), ..Default::default() });
}

#[test]
fn treats_question_mark_before_slash_as_search() {
    assert_split("?search/slash", SplitResult { search: Some((1, 13)), ..Default::default() });
}

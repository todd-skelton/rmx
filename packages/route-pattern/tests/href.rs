//! Remix's `href.test.ts`. Remix's `{ id: 123 }` number params are strings here, and
//! `null`/`undefined` params are left out.

use rsmix_route_pattern::{HrefError, HrefOptions, HrefParams, Params, RoutePattern};
use rsmix_url::{SearchParams, Url};

fn pattern(source: &str) -> RoutePattern {
    RoutePattern::parse(source).unwrap_or_else(|e| panic!("{e}"))
}

fn href(source: &str, params: impl HrefParams) -> String {
    pattern(source).href(params).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn assert_href(source: &str, params: impl HrefParams, expected: &str) {
    assert_eq!(href(source, params), expected, "{source}");
}

fn assert_href_error(source: &str, params: impl HrefParams, kind: &str) {
    let error = pattern(source).href(params).expect_err(source);
    assert_eq!(error.kind(), kind, "{source}: {error}");
}

fn assert_href_err(source: &str, params: impl HrefParams, expected: HrefError) {
    assert_eq!(pattern(source).href(params), Err(expected), "{source}");
}

fn search(pairs: &[(&str, &str)]) -> SearchParams {
    let mut search = SearchParams::new();
    for (k, v) in pairs {
        search.append(*k, *v);
    }
    search
}

fn assert_search(source: &str, pairs: &[(&str, &str)], expected: &str) {
    assert_eq!(pattern(source).href_with_search((), &search(pairs)).unwrap(), expected, "{source}");
}

fn relative(source: &str, params: impl HrefParams, base: &str) -> String {
    let options = HrefOptions { base_url: Some(Url::parse(base).unwrap()), ..Default::default() };
    pattern(source).href_with(params, &options).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn assert_relative(source: &str, params: impl HrefParams, base: &str, expected: &str) {
    assert_eq!(relative(source, params, base), expected, "{source} from {base}");
}

fn params(pairs: &[(&str, &str)]) -> Params {
    pairs.iter().copied().collect()
}

fn match_params(source: &str, url: &Url) -> Params {
    pattern(source).match_url(url).unwrap().unwrap_or_else(|| panic!("{source} should match {}", url.href())).params
}

#[test]
fn protocol_defaults_omitted_protocol_to_https() {
    assert_href("://example.com/path", (), "https://example.com/path");
}

#[test]
fn protocol_defaults_http_s_to_https() {
    assert_href("http(s)://example.com/path", (), "https://example.com/path");
}

#[test]
fn protocol_supports_explicit_http() {
    assert_href("http://example.com/path", (), "http://example.com/path");
}

#[test]
fn protocol_supports_explicit_https() {
    assert_href("https://example.com/path", (), "https://example.com/path");
}

#[test]
fn hostname_when_origin_is_present_throws_when_protocol_specified() {
    assert_href_error("https://*/path", (), "missing-hostname");
}

#[test]
fn hostname_when_origin_is_present_throws_when_protocol_with_named_wildcard_missing_param() {
    assert_href_error("http://*host/path", (), "missing-params");
}

#[test]
fn hostname_supports_static_hostname() {
    assert_href("://example.com/path", (), "https://example.com/path");
    assert_href("://example.com/path", (), "https://example.com/path");
    assert_href("://example.com/path", (), "https://example.com/path");
    assert_href("://example.com/path", (), "https://example.com/path");
}

#[test]
fn hostname_with_dynamic_segment_works_when_provided() {
    assert_href("://:host.com/path", [("host", "example")], "https://example.com/path");
}

#[test]
fn hostname_with_dynamic_segment_throws_when_missing() {
    assert_href_error("://:host/path", (), "missing-params");
}

#[test]
fn hostname_supports_multiple_dynamic_segments() {
    assert_href("://:subdomain.:domain.com/path", [("subdomain", "api"), ("domain", "example")], "https://api.example.com/path");
}

#[test]
fn hostname_supports_named_wildcard() {
    assert_href("://*env.example.com/path", [("env", "staging")], "https://staging.example.com/path");
}

#[test]
fn hostname_throws_for_nameless_wildcard() {
    assert_href_error("://*.example.com/path", (), "nameless-wildcard");
}

#[test]
fn hostname_includes_optional_with_static_content() {
    assert_href("://(www.)example.com/path", (), "https://www.example.com/path");
}

#[test]
fn hostname_rejects_structural_url_chars_and_in_hostname_variables() {
    assert_href_error("://:tenant.example.com/path", [("tenant", "acme.dev")], "invalid-hostname-variable");
    assert_href_error("://:tenant.example.com/path", [("tenant", "acme:staging")], "invalid-hostname-variable");
}

#[test]
fn hostname_allows_but_rejects_structural_url_chars_in_hostname_wildcards() {
    assert_href("://*tenant.example.com/path", [("tenant", "preview.acme")], "https://preview.acme.example.com/path");
    assert_href_error("://*tenant.example.com/path", [("tenant", "preview:acme")], "invalid-hostname-wildcard");
}

#[test]
fn port_supports_static_port() {
    assert_href("://example.com:8080/path", (), "https://example.com:8080/path");
}

#[test]
fn port_works_with_hostname_params() {
    assert_href("://:host:8080/path", [("host", "localhost")], "https://localhost:8080/path");
}

#[test]
fn pathname_supports_static_pathname() {
    assert_href("/posts", (), "/posts");
    assert_href("/posts", (), "/posts");
    assert_href("/posts", (), "/posts");
    assert_href("/posts", (), "/posts");
}

#[test]
fn pathname_normalizes_static_pathname_without_leading_slash() {
    assert_href("posts", (), "/posts");
}

#[test]
fn pathname_with_dynamic_segment_works_when_provided() {
    assert_href("/posts/:id", [("id", "123")], "/posts/123");
}

#[test]
fn pathname_with_dynamic_segment_works_with_number_params() {
    assert_href("/posts/:id", [("id", "123")], "/posts/123");
}

#[test]
fn pathname_with_dynamic_segment_throws_when_provided_an_empty_string() {
    assert_href_error("/posts/:id", [("id", "")], "invalid-pathname-variable");
}

#[test]
fn pathname_with_dynamic_segment_ignores_extra_params() {
    assert_href("/posts/:id", [("id", "123"), ("page", "2"), ("sort", "desc")], "/posts/123");
}

#[test]
fn pathname_with_dynamic_segment_throws_when_missing() {
    assert_href_error("/posts/:id", (), "missing-params");
}

#[test]
fn pathname_with_dynamic_segment_throws_when_params_is_null_required_params() {
    assert_href_error("/posts/:id", (), "missing-params");
}

#[test]
fn pathname_with_dynamic_segment_throws_when_params_is_undefined_required_params() {
    assert_href_error("/posts/:id", (), "missing-params");
}

#[test]
fn pathname_supports_multiple_dynamic_segments() {
    assert_href("/users/:userId/posts/:postId", [("userId", "42"), ("postId", "123")], "/users/42/posts/123");
}

#[test]
fn pathname_supports_named_wildcard() {
    assert_href("/files/*path", [("path", "docs/readme.md")], "/files/docs/readme.md");
    assert_href("images/*path.png", [("path", "images/hero")], "/images/images/hero.png");
}

#[test]
fn pathname_supports_wildcard_with_number_param() {
    assert_href("/files/*path", [("path", "123")], "/files/123");
}

#[test]
fn pathname_supports_wildcard_with_empty_string_param() {
    assert_href("/files/*path", [("path", "")], "/files/");
}

#[test]
fn pathname_preserves_internal_wildcard_slashes_when_normalizing_the_pathname() {
    assert_href("/files/*path", [("path", "/docs//readme.md/")], "/files//docs//readme.md/");
    assert_href("https://example.com/*path", [("path", "//docs//readme.md/")], "https://example.com/docs//readme.md/");
}

#[test]
fn pathname_normalizes_slash_only_wildcard_paths_to_the_root_pathname() {
    assert_href("/*path", [("path", "///")], "/");
    assert_href("https://example.com/*path", [("path", "///")], "https://example.com/");
}

#[test]
fn pathname_rejects_parent_segments_in_pathname_wildcards() {
    assert_href_err("/files/*path", [("path", "../../admin")], HrefError::InvalidPathnameWildcard { value: "../../admin".into(), segment: "..".into() });
    assert_href_err("/files/*path", [("path", "docs/../readme.md")], HrefError::InvalidPathnameWildcard { value: "docs/../readme.md".into(), segment: "..".into() });
    assert_href_err("/files/*path", [("path", "docs/..")], HrefError::InvalidPathnameWildcard { value: "docs/..".into(), segment: "..".into() });
}

#[test]
fn pathname_rejects_current_directory_segments_in_pathname_wildcards() {
    assert_href_err("/files/*path", [("path", ".")], HrefError::InvalidPathnameWildcard { value: ".".into(), segment: ".".into() });
    assert_href_err("/files/*path", [("path", "docs/./readme.md")], HrefError::InvalidPathnameWildcard { value: "docs/./readme.md".into(), segment: ".".into() });
}

#[test]
fn pathname_throws_for_unnamed_wildcard() {
    assert_href_error("/files/*", (), "nameless-wildcard");
}

#[test]
fn pathname_supports_repeated_params() {
    assert_href("/:lang/users/:userId/:lang/posts/:postId", [("lang", "en"), ("userId", "42"), ("postId", "123")], "/en/users/42/en/posts/123");
}

#[test]
fn pathname_encodes_dots_in_variables_so_they_remain_capture_data() {
    assert_href("/users/:id", [("id", "a.b")], "/users/a%2Eb");
}

#[test]
fn pathname_uses_one_value_for_every_repeated_capture_name() {
    assert_href("/:id/:id", [("id", "same")], "/same/same");
}

#[test]
fn pathname_encodes_structural_url_chars_including_for_variables() {
    assert_href("/posts/:slug", [("slug", "hello/world?draft=true#preview")], "/posts/hello%2Fworld%3Fdraft%3Dtrue%23preview");
}

#[test]
fn pathname_encodes_wildcard_segments_with_encodeuricomponent_semantics() {
    assert_href("/files/*path", [("path", "docs/@remix-run/ui?raw#v1")], "/files/docs/%40remix-run/ui%3Fraw%23v1");
}

#[test]
fn pathname_encodes_pathname_params_with_encodeuricomponent_semantics() {
    assert_href("/posts/:slug", [("slug", "hello world")], "/posts/hello%20world");
    assert_href("/files/*path", [("path", "docs/hello world")], "/files/docs/hello%20world");
}

#[test]
fn pattern_with_optionals_includes_optional_with_static_content() {
    assert_href("/posts(/edit)", (), "/posts/edit");
    assert_href("products(.md)", (), "/products.md");
}

#[test]
fn pattern_with_optionals_includes_optional_with_variable_when_provided() {
    assert_href("/posts(/:id)", [("id", "123")], "/posts/123");
}

#[test]
fn pattern_with_optionals_omits_optional_with_variable_when_omitted() {
    assert_href("/posts(/:id)", (), "/posts");
    assert_href("/posts(/:id)", (), "/posts");
    assert_href("/posts(/:id)", (), "/posts");
    assert_href("/posts(/:id)", (), "/posts");
    assert_href("/posts(/:id)", (), "/posts");
}

#[test]
fn pattern_with_optionals_throws_for_empty_optional_variable_when_provided() {
    assert_href_error("/posts(/:id)", [("id", "")], "invalid-pathname-variable");
}

#[test]
fn pattern_with_optionals_includes_optional_with_wildcard_when_provided() {
    assert_href("/files(/*path)", [("path", "docs/readme.md")], "/files/docs/readme.md");
}

#[test]
fn pattern_with_optionals_omits_optional_with_wildcard_when_omitted() {
    assert_href("/files(/*path)", (), "/files");
    assert_href("/files(/*path)", (), "/files");
    assert_href("/files(/*path)", (), "/files");
    assert_href("/files(/*path)", (), "/files");
    assert_href("/files(/*path)", (), "/files");
}

#[test]
fn pattern_with_optionals_omits_optional_with_nameless_wildcard() {
    assert_href("/files(/*)", (), "/files");
    assert_href("/files(/*)", (), "/files");
    assert_href("/files(/*)", (), "/files");
    assert_href("/files(/*)", (), "/files");
}

#[test]
fn pattern_with_optionals_with_nested_optionals_includes_all_when_all_provided() {
    assert_href("/blog/:year(/:month(/:day))", [("year", "2024"), ("month", "01"), ("day", "15")], "/blog/2024/01/15");
}

#[test]
fn pattern_with_optionals_with_nested_optionals_includes_only_outer_when_inner_omitted() {
    assert_href("/blog/:year(/:month(/:day))", [("year", "2024"), ("month", "01")], "/blog/2024/01");
}

#[test]
fn pattern_with_optionals_with_nested_optionals_omits_both_when_only_inner_provided() {
    assert_href("/blog/:year(/:month(/:day))", [("year", "2024"), ("day", "15")], "/blog/2024");
}

#[test]
fn pattern_with_optionals_with_nested_optionals_omits_nested_optionals_when_outer_variable_is_null() {
    assert_href("/blog/:year(/:month(/:day))", [("year", "2024"), ("day", "15")], "/blog/2024");
}

#[test]
fn pattern_with_optionals_with_nested_optionals_omits_both_when_neither_provided() {
    assert_href("/blog/:year(/:month(/:day))", [("year", "2024")], "/blog/2024");
}

#[test]
fn pattern_with_optionals_with_multiple_optionals_includes_both_when_both_provided() {
    assert_href("/posts(/id/:id)(/action/:action)", [("id", "123"), ("action", "edit")], "/posts/id/123/action/edit");
}

#[test]
fn pattern_with_optionals_with_multiple_optionals_includes_only_first_when_second_omitted() {
    assert_href("/posts(/id/:id)(/action/:action)", [("id", "123")], "/posts/id/123");
}

#[test]
fn pattern_with_optionals_with_multiple_optionals_includes_only_second_when_first_omitted() {
    assert_href("/posts(/id/:id)(/action/:action)", [("action", "edit")], "/posts/action/edit");
}

#[test]
fn pattern_with_optionals_with_multiple_optionals_omits_both_when_neither_provided() {
    assert_href("/posts(/id/:id)(/action/:action)", (), "/posts");
}

#[test]
fn pattern_with_optionals_normalizes_to_slash_when_entire_pattern_is_omitted_optional() {
    assert_href("(/locale/:locale)(/page/:page)", (), "/");
}

#[test]
fn search_params_with_key_only_constraint_q_includes_empty_value_without_user_param() {
    assert_href("/posts?filter", (), "/posts?filter=");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_uses_pattern_value_only() {
    assert_href("/posts?sort=asc", (), "/posts?sort=asc");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_preserves_pattern_search_when_only_path_params_passed() {
    assert_href("products?sort=asc&limit", (), "/products?sort=asc&limit=");
    assert_href("products/:id?sort=asc&limit", [("id", "1")], "/products/1?sort=asc&limit=");
}

#[test]
fn format_returns_relative_url_for_pathname_only() {
    assert_href("/posts/:id", [("id", "123")], "/posts/123");
}

#[test]
fn format_returns_absolute_url_with_protocol() {
    assert_href("https://example.com/path", (), "https://example.com/path");
}

#[test]
fn format_returns_absolute_url_with_hostname() {
    assert_href("://example.com/path", (), "https://example.com/path");
}

#[test]
fn format_returns_absolute_url_with_port() {
    assert_href("://example.com:8080/path", (), "https://example.com:8080/path");
}

#[test]
fn hostname_rejects_backslashes_in_hostname_variables() {
    let expected = HrefError::InvalidHostnameVariable { value: "preview\\".into(), char: '\\' };
    assert_href_err("://:tenant.example.com/path", [("tenant", "preview\\")], expected);
}

#[test]
fn hostname_rejects_backslashes_in_hostname_wildcards() {
    let expected = HrefError::InvalidHostnameWildcard { value: "preview.test\\".into(), char: '\\' };
    assert_href_err("://*tenant.example.com/path", [("tenant", "preview.test\\")], expected);
}

/// Remix also checks that `new URL` turns the labels into punycode; rsmix_url does no IDNA,
/// so only the generated hrefs are checked.
#[test]
fn hostname_preserves_host_labels_in_generated_urls() {
    assert_href("://:tenant.example.com/path", [("tenant", "caf\u{e9}")], "https://caf\u{e9}.example.com/path");
    assert_href("://*tenant.example.com/path", [("tenant", "preview.caf\u{e9}")], "https://preview.caf\u{e9}.example.com/path");
}

#[test]
fn pathname_normalizes_a_leading_wildcard_slash_on_the_current_origin() {
    let href = href("/*path", [("path", "/files/readme.md")]);
    let url = Url::parse("https://example.com/current/page").unwrap().join(&href).unwrap();
    assert_eq!(url.origin(), "https://example.com");
    assert_eq!(url.pathname(), "/files/readme.md");
    assert_eq!(href, "/files/readme.md");
    assert_eq!(match_params("/*path", &url), params(&[("path", "files/readme.md")]));
}

#[test]
fn pathname_normalizes_repeated_leading_wildcard_slashes_on_the_current_origin() {
    let href = href("/*path", [("path", "//files/readme.md")]);
    let url = Url::parse("https://example.com/current/page").unwrap().join(&href).unwrap();
    assert_eq!(url.origin(), "https://example.com");
    assert_eq!(url.pathname(), "/files/readme.md");
    assert_eq!(href, "/files/readme.md");
    assert_eq!(match_params("/*path", &url), params(&[("path", "files/readme.md")]));
}

#[test]
fn pathname_rejects_dot_segments_in_optional_wildcards_and_relative_hrefs() {
    let expected = HrefError::InvalidPathnameWildcard { value: "../readme.md".into(), segment: "..".into() };
    assert_href_err("/files(/*path)", [("path", "../readme.md")], expected.clone());
    let options = HrefOptions { base_url: Some(Url::parse("https://example.com/files/current").unwrap()), ..Default::default() };
    assert_eq!(pattern("/files/*path").href_with([("path", "../readme.md")], &options), Err(expected));
}

#[test]
fn pathname_round_trips_dotted_filenames_and_encoded_text_in_pathname_wildcards() {
    let path = ".hidden/.../readme..md/%2e%2e/%2F/\\draft\n";
    let href = href("/files/*path", [("path", path)]);
    let url = Url::parse("https://example.com/current").unwrap().join(&href).unwrap();
    assert_eq!(href, "/files/.hidden/.../readme..md/%252e%252e/%252F/%5Cdraft%0A");
    assert_eq!(url.pathname(), href);
    assert_eq!(match_params("/files/*path", &url), params(&[("path", path)]));
}

#[test]
fn pattern_with_optionals_omits_optional_joins_without_creating_double_slashes() {
    let joined = pattern("a/(:id)").join(&pattern("c"));
    assert_eq!(joined.href(()).unwrap(), "/a/c");
    assert_eq!(joined.href([("id", "b")]).unwrap(), "/a/b/c");
}

#[test]
fn search_params_works_with_no_constraints() {
    assert_search("/posts", &[("category", "books"), ("category", "electronics")], "/posts?category=books&category=electronics");
}

#[test]
fn search_params_accepts_search_params_without_losing_duplicate_order_or_mutating_it() {
    let search = search(&[("tag", "featured"), ("tag", "popular")]);
    let href = pattern("/posts?sort=recent").href_with_search((), &search).unwrap();
    assert_eq!(href, "/posts?tag=featured&tag=popular&sort=recent");
    assert_eq!(search.to_string(), "tag=featured&tag=popular");
}

#[test]
fn search_params_with_key_only_constraint_q_uses_user_param_value() {
    assert_search("/posts?filter", &[("filter", "active")], "/posts?filter=active");
}

/// Remix's nullish and empty-array values have no SearchParams counterpart; no values is the case left.
#[test]
fn search_params_with_key_only_constraint_q_keeps_the_key_when_user_params_are_empty() {
    assert_search("/posts?filter", &[], "/posts?filter=");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_prepends_user_params() {
    assert_search("/posts?sort=asc", &[("sort", "desc")], "/posts?sort=desc&sort=asc");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_deduplicates_when_user_matches_pattern() {
    assert_search("/posts?tag=featured", &[("tag", "featured")], "/posts?tag=featured");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_deduplicates_when_user_matches_one_of_multiple_pattern_values() {
    assert_search("/posts?tag=featured&tag=popular", &[("tag", "featured")], "/posts?tag=featured&tag=popular");
}

#[test]
fn search_params_with_specific_value_constraint_q_foo_handles_array_values() {
    let expected = "/posts?tag=tutorial&tag=beginner&tag=featured&tag=popular";
    assert_search("/posts?tag=featured&tag=popular", &[("tag", "tutorial"), ("tag", "beginner")], expected);
}

#[test]
fn search_params_supports_additional_user_params() {
    assert_search("/posts?sort=asc", &[("page", "2")], "/posts?page=2&sort=asc");
}

/// A base URL is a parsed absolute `Url`, so relative and authority-less bases cannot be passed.
#[test]
fn base_url_requires_a_base_url_that_can_resolve_same_origin_targets() {
    assert!(Url::parse("/admin/settings").is_err());
    assert!(Url::parse("mailto:user@example.com").is_err());
}

#[test]
fn base_url_returns_path_relative_hrefs_for_same_origin_targets() {
    let base = "https://example.com/admin/settings";
    assert_relative("/users/:id", [("id", "123")], base, "../users/123");
    assert_relative("/admin/users/:id", [("id", "123")], base, "users/123");
    assert_relative("/admin/", (), base, "./");
    assert_relative("/", (), base, "../");
}

#[test]
fn base_url_relativizes_explicit_same_origin_patterns_and_keeps_cross_origin_targets_absolute() {
    let base = "https://example.com/admin/settings";
    assert_relative("https://example.com/users/:id", [("id", "123")], base, "../users/123");
    assert_relative("https://cdn.example.com/users/:id", [("id", "123")], base, "https://cdn.example.com/users/123");
}

#[test]
fn base_url_uses_dot_slash_when_the_first_relative_path_segment_could_be_parsed_as_a_scheme() {
    assert_relative("/docs\\:latest", (), "https://example.com/current", "./docs:latest");
}

#[test]
fn base_url_normalizes_leading_wildcard_slashes_before_generating_relative_hrefs() {
    let base = "https://example.com/current";
    let slash_href = relative("/*path", [("path", "//files")], base);
    assert_eq!(slash_href, "files");
    assert_eq!(Url::parse(base).unwrap().join(&slash_href).unwrap().href(), "https://example.com/files");
    assert_relative("/*path", [("path", "///")], base, "./");
    assert_relative("https://example.com/*path", [("path", "//files")], "https://example.com/admin/current", "../files");
    assert_relative("https://cdn.example.com/*path", [("path", "//files")], base, "https://cdn.example.com/files");
}

#[test]
fn base_url_uses_dot_slash_when_leading_backslashes_could_be_parsed_as_an_authority() {
    let base = "https://example.com/current";
    let backslash_href = relative("/\\\\\\\\host/path", (), base);
    assert_eq!(backslash_href, "./\\\\host/path");
    assert_eq!(Url::parse(base).unwrap().join(&backslash_href).unwrap().href(), "https://example.com///host/path");
}

#[test]
fn base_url_round_trips_params_search_constraints_encoded_delimiters_and_trailing_slashes() {
    let base = "https://example.com/admin/settings";
    let source = "/files/:name.:ext/?view=full";
    let href = relative(source, [("name", "read/me.v1"), ("ext", "txt")], base);
    assert_eq!(href, "../files/read%2Fme%2Ev1.txt/?view=full");
    let url = Url::parse(base).unwrap().join(&href).unwrap();
    assert_eq!(match_params(source, &url), params(&[("name", "read/me.v1"), ("ext", "txt")]));
}

// CreateHrefError

#[test]
fn missing_hostname_shows_pattern() {
    let error = pattern("https://*:8080/api").href(()).unwrap_err();
    assert_eq!(error.kind(), "missing-hostname");
    assert_eq!(format!("CreateHrefError: {error}"), "CreateHrefError: pattern requires hostname\n\nPattern: https://:8080/api");
}

/// Remix adds a `Params:` line with the params as JSON; `HrefParams` can only look names up.
#[test]
fn missing_params_shows_missing_params_and_pattern() {
    let error = pattern("https://example.com/:collection/:id").href(()).unwrap_err();
    let expected = "CreateHrefError: missing param(s): 'collection', 'id'\n\nPattern: https://example.com/:collection/:id";
    assert_eq!(format!("CreateHrefError: {error}"), expected);
}

#[test]
fn missing_params_reports_all_missing_required_params() {
    let expected = HrefError::MissingParams { pattern: "/:a/:b".into(), missing: vec!["a".into(), "b".into()] };
    assert_href_err("/:a/:b", (), expected);
}

#[test]
fn nameless_wildcard_shows_error_message_with_pattern() {
    let error = pattern("https://example.com/api/*/users").href(()).unwrap_err();
    let expected = "CreateHrefError: pattern contains nameless wildcard\n\nPattern: https://example.com/api/*/users";
    assert_eq!(format!("CreateHrefError: {error}"), expected);
}

#[test]
fn invalid_pathname_variable_shows_param_pattern_and_value() {
    let error = pattern("/posts/:id").href([("id", "")]).unwrap_err();
    let expected = "CreateHrefError: invalid pathname variable param: 'id' cannot be empty\n\nPattern: /posts/:id\nValue: \"\"";
    assert_eq!(format!("CreateHrefError: {error}"), expected);
}

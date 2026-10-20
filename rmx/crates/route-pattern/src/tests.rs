use super::*;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn params(p: &str, u: &str) -> Option<Vec<(String, String)>> {
    RoutePattern::parse(p)
        .unwrap()
        .match_url(&url(u))
        .map(|m| m.params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn kv(pairs: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
    Some(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

#[test]
fn variables() {
    assert_eq!(params("users/:id", "https://x/users/123"), kv(&[("id", "123")]));
    assert_eq!(params("/users/:id", "https://x/users/123"), kv(&[("id", "123")]));
    assert_eq!(params("blog/:date/:slug", "https://x/blog/2024-01-15/hello"), kv(&[("date", "2024-01-15"), ("slug", "hello")]));
    assert_eq!(params("files/:name.:ext", "https://x/files/readme.md"), kv(&[("name", "readme"), ("ext", "md")]));
    assert_eq!(params("users/:id", "https://x/users/"), None);
    assert_eq!(params("users/:id", "https://x/users/1/2"), None);
    assert_eq!(params("users/:id", "https://x/users/a%2Fb"), kv(&[("id", "a/b")]));
}

#[test]
fn wildcards() {
    assert_eq!(params("files/*path", "https://x/files/images/logo.png"), kv(&[("path", "images/logo.png")]));
    assert_eq!(
        params("node_modules/*package/dist/index.js", "https://x/node_modules/@remix-run/router/dist/index.js"),
        kv(&[("package", "@remix-run/router")])
    );
    assert_eq!(params("files/*", "https://x/files/a/b"), kv(&[]));
    assert_eq!(params("assets/*path", "https://x/assets/app.css"), kv(&[("path", "app.css")]));
}

#[test]
fn optionals() {
    assert_eq!(params("api(/v:version)/users", "https://x/api/users"), kv(&[]));
    assert_eq!(params("api(/v:version)/users", "https://x/api/v2/users"), kv(&[("version", "2")]));
    assert_eq!(params("blog/:slug(.html)", "https://x/blog/hello"), kv(&[("slug", "hello")]));
    assert_eq!(params("blog/:slug(.html)", "https://x/blog/hello.html"), kv(&[("slug", "hello")]));
    assert_eq!(params("docs(/guides/:category)", "https://x/docs"), kv(&[]));
    assert_eq!(params("docs(/guides/:category)", "https://x/docs/guides/routing"), kv(&[("category", "routing")]));
    assert_eq!(params("api(/v:major(.:minor))", "https://x/api/v2.1"), kv(&[("major", "2"), ("minor", "1")]));
    assert_eq!(params("api(/v:major(.:minor))", "https://x/api"), kv(&[]));
}

#[test]
fn hostnames_protocols_ports_search() {
    assert_eq!(params("://:tenant.example.com/dashboard", "https://acme.example.com/dashboard"), kv(&[("tenant", "acme")]));
    assert_eq!(params("https://example.com/a", "http://example.com/a"), None);
    assert_eq!(params("http(s)://example.com/a", "http://example.com/a"), kv(&[]));
    assert_eq!(params("://localhost:3000/a", "http://localhost:3000/a"), kv(&[]));
    assert_eq!(params("://localhost:3000/a", "http://localhost:4000/a"), None);
    assert_eq!(params("search?q", "https://x/search?q=1"), kv(&[]));
    assert_eq!(params("search?q", "https://x/search"), None);
    assert_eq!(params("search?q=routing", "https://x/search?q=other"), None);
    assert_eq!(params("search?q=routing", "https://x/search?q=routing"), kv(&[]));
}

#[test]
fn escapes_and_encoding() {
    assert_eq!(params("time/12\\:30", "https://x/time/12:30"), kv(&[]));
    assert_eq!(params("wiki/Mercury_\\(planet\\)", "https://x/wiki/Mercury_(planet)"), kv(&[]));
    assert_eq!(params("/café", "https://x/caf%C3%A9"), kv(&[]));
}

#[test]
fn ranking() {
    let mut m = Matcher::new();
    m.add(RoutePattern::parse("files/*path").unwrap(), "wild");
    m.add(RoutePattern::parse("files/:name").unwrap(), "var");
    m.add(RoutePattern::parse("files/readme").unwrap(), "static");
    assert_eq!(*m.match_url(&url("https://x/files/readme")).unwrap().data, "static");
    assert_eq!(*m.match_url(&url("https://x/files/other")).unwrap().data, "var");
    assert_eq!(*m.match_url(&url("https://x/files/a/b")).unwrap().data, "wild");
    let order: Vec<_> = m.match_all(&url("https://x/files/readme")).into_iter().map(|m| *m.data).collect();
    assert_eq!(order, vec!["static", "var", "wild"]);

    let mut m = Matcher::new();
    m.add(RoutePattern::parse("/").unwrap(), "home");
    m.add(RoutePattern::parse("blog/:slug").unwrap(), "post");
    m.add(RoutePattern::parse("api(/v:version)/*path").unwrap(), "api");
    assert_eq!(*m.match_url(&url("https://x/")).unwrap().data, "home");
    let api = m.match_url(&url("https://x/api/v2/users/profile")).unwrap();
    assert_eq!(*api.data, "api");
    assert_eq!(api.params.get("version"), Some("2"));
    assert_eq!(api.params.get("path"), Some("users/profile"));
}

#[test]
fn hrefs() {
    let p = |s: &str| RoutePattern::parse(s).unwrap();
    assert_eq!(p("blog/:slug").href([("slug", "v3")]).unwrap(), "/blog/v3");
    assert_eq!(p("api(/v:version)/*path").href([("path", "users/profile")]).unwrap(), "/api/users/profile");
    assert_eq!(
        p("api(/v:version)/*path").href([("version", "2"), ("path", "users/profile")]).unwrap(),
        "/api/v2/users/profile"
    );
    assert_eq!(
        p("http(s)://:region.cdn.com/assets/*file.:ext")
            .href([("region", "us-west"), ("file", "images/logo"), ("ext", "png")])
            .unwrap(),
        "https://us-west.cdn.com/assets/images/logo.png"
    );
    let mut search = SearchParams::new();
    search.append("utm_source", "newsletter");
    assert_eq!(
        p("blog/:slug?ref=docs").href_with_search([("slug", "v3")], &search).unwrap(),
        "/blog/v3?utm_source=newsletter&ref=docs"
    );
    assert_eq!(p("users/:id").href([("id", "a.b")]).unwrap(), "/users/a%2Eb");
    assert_eq!(p("todos(/new)").href(()).unwrap(), "/todos/new");
    assert_eq!(p("/").href(()).unwrap(), "/");
    assert_eq!(p("users/:id").href(()), Err(HrefError::MissingParam("id".into())));
    assert_eq!(p("files/*path").href([("path", "../etc")]), Err(HrefError::InvalidWildcard("path".into())));
    // Hrefs round-trip through the matcher.
    let href = p("users/:id").href([("id", "a.b/c")]).unwrap();
    assert_eq!(params("users/:id", &format!("https://x{href}")), kv(&[("id", "a.b/c")]));
}

#[test]
fn join_and_display() {
    let p = |s: &str| RoutePattern::parse(s).unwrap();
    assert_eq!(p("users").join(&p(":id")).to_string(), "/users/:id");
    assert_eq!(p("api(/v:version)").join(&p("://remix.run/users/:id")).to_string(), "://remix.run/api(/v:version)/users/:id");
    assert_eq!(p("/").join(&p("/about")).to_string(), "/about");
    assert_eq!(p("/frames").join(&p("/sidebar")).to_string(), "/frames/sidebar");
    assert_eq!(p("blog/:slug(.html)").to_string(), "/blog/:slug(.html)");
    assert!(RoutePattern::parse("a/:").is_err());
    assert!(RoutePattern::parse("a/(b").is_err());
    assert!(RoutePattern::parse("a/()").is_err());
    assert!(RoutePattern::parse("*a*b").is_err());
}

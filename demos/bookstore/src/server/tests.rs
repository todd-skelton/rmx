//! Mirrors the Remix bookstore tests: router.test.ts and actions/**/controller.test.ts,
//! with test/helpers.ts as [`Client`].

use std::time::Duration;

use rsmix::fetch::{block_on, Body};
use rsmix::file_storage::MemoryFileStorage;
use rsmix::session::MemoryStorage;

use super::*;

const ORIGIN: &str = "https://remix.run";

/// `createTestRouter()` plus a cookie jar holding the `session` cookie.
struct Client {
    router: Router,
    session: Option<String>,
}

impl Client {
    fn new() -> Client {
        let options = BookstoreOptions {
            session_cookie: session_cookie(),
            session_storage: MemoryStorage::new(),
            uploads: MemoryFileStorage::new(),
            db: Db::seeded(),
            public_dir: concat!(env!("CARGO_MANIFEST_DIR"), "/public").into(),
            cart_delay: Duration::ZERO,
        };
        Client { router: router(options), session: None }
    }

    fn send(&mut self, req: http::request::Builder, body: Body) -> Response {
        let req = match &self.session {
            Some(cookie) => req.header("cookie", cookie.as_str()),
            None => req,
        };
        let res = block_on(self.router.fetch(req.body(body).unwrap()));
        for value in res.headers().get_all("set-cookie") {
            let pair = value.to_str().unwrap().split(';').next().unwrap();
            if pair.starts_with("session=") {
                self.session = Some(pair.to_string());
            }
        }
        res
    }

    fn get(&mut self, path: &str) -> Response {
        self.send(http::Request::get(format!("{ORIGIN}{path}")), Body::empty())
    }

    fn form(&mut self, method: &str, path: &str, fields: &[(&str, &str)]) -> Response {
        let body = fields.iter().copied().collect::<rsmix::fetch::SearchParams>().to_string();
        let req = http::Request::builder()
            .method(method)
            .uri(format!("{ORIGIN}{path}"))
            .header("content-type", "application/x-www-form-urlencoded;charset=UTF-8");
        self.send(req, Body::from(body))
    }

    fn login(&mut self, email: &str, password: &str) {
        let res = self.form("POST", "/login", &[("email", email), ("password", password)]);
        assert_eq!(res.status(), 302);
        assert!(self.session.is_some(), "login sets the session cookie");
    }

    fn login_as_admin(&mut self) {
        self.login("admin@bookstore.com", "admin123");
    }

    fn login_as_customer(&mut self) {
        self.login("customer@example.com", "password123");
    }
}

fn location(res: &Response) -> &str {
    res.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default()
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

#[track_caller]
fn assert_contains(html: &str, needle: &str) {
    assert!(html.contains(needle), "expected HTML to contain {needle:?}");
}

// --- router.test.ts --------------------------------------------------------------------

#[test]
fn responds_to_basic_get_request() {
    let res = Client::new().get("/");
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get("content-type").unwrap(), "text/html; charset=UTF-8");
    assert_contains(&text(res), "Welcome to the Bookstore");
}

#[test]
fn returns_404_for_unknown_routes() {
    assert_eq!(Client::new().get("/does-not-exist").status(), 404);
}

// --- actions/controller.test.ts --------------------------------------------------------

#[test]
fn home_about_and_search_render() {
    let mut client = Client::new();
    assert_contains(&text(client.get("/")), "Featured Books");
    assert_contains(&text(client.get("/about")), "About Our Bookstore");
    let html = text(client.get("/search?q=heavy"));
    assert_contains(&html, "Heavy Metal Guitar Riffs");
    assert_contains(&text(client.get("/search?q=zzzz-not-a-book")), "No books found matching your search.");
}

#[test]
fn uploads_serve_stored_files_and_404_otherwise() {
    let mut client = Client::new();
    let res = client.get("/uploads/missing.png");
    assert_eq!(res.status(), 404);
    assert_eq!(text(res), "File not found");
}

// --- actions/contact/controller.test.ts ------------------------------------------------

#[test]
fn contact_form_and_submission() {
    let mut client = Client::new();
    let page = client.get("/contact");
    assert_eq!(page.status(), 200);
    assert_contains(&text(page), "Contact Us");
    let res = client.form("POST", "/contact", &[("name", "Test"), ("email", "t@example.com"), ("message", "Hello")]);
    assert_eq!(res.status(), 200);
    assert_contains(&text(res), "Thank you for your message");
}

// --- actions/books/controller.test.ts --------------------------------------------------

#[test]
fn books_index_genre_and_show() {
    let mut client = Client::new();
    let html = text(client.get("/books"));
    assert_contains(&html, "Browse Books");
    assert_contains(&html, "Ash &amp; Smoke");
    let genre = client.get("/books/genre/cookbook");
    assert_eq!(genre.status(), 200);
    let show = client.get("/books/bbq");
    assert_eq!(show.status(), 200);
    let html = text(show);
    assert_contains(&html, "Ash &amp; Smoke");
    assert!(!html.contains("Ash & Smoke"), "titles are escaped");
    let missing = client.get("/books/not-a-real-book");
    assert_eq!(missing.status(), 404);
    assert_contains(&text(missing), "Book Not Found");
}

// --- actions/cart/controller.test.ts ---------------------------------------------------

#[test]
fn cart_add_redirects_to_cart() {
    let res = Client::new().form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    assert_eq!(res.status(), 302);
    assert!(location(&res).contains("/cart"));
}

#[test]
fn cart_shows_items_and_persists_across_requests() {
    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "2"), ("slug", "heavy-metal")]);
    assert!(client.session.is_some());
    let res = client.get("/cart");
    assert_eq!(res.status(), 200);
    let html = text(res);
    assert_contains(&html, "Shopping Cart");
    assert_contains(&html, "Heavy Metal Guitar Riffs");

    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    client.form("POST", "/cart/api/add", &[("bookId", "3"), ("slug", "three-ways")]);
    let html = text(client.get("/cart"));
    assert_contains(&html, "Ash &amp; Smoke");
    assert_contains(&html, "Three Ways to Change Your Life");
}

#[test]
fn cart_items_fragment_renders_table_totals_and_empty_state() {
    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "2"), ("slug", "heavy-metal")]);
    let res = client.get("/fragments/cart-items");
    assert_eq!(res.status(), 200);
    let html = text(res);
    for needle in ["<table>", "<th>Book</th>", "Heavy Metal Guitar Riffs", "Update", "Remove", "Total:"] {
        assert_contains(&html, needle);
    }

    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    let html = text(client.get("/fragments/cart-items"));
    for needle in ["Total:", "$16.99", "Continue Shopping", "Login to Checkout"] {
        assert_contains(&html, needle);
    }

    let res = Client::new().get("/fragments/cart-items");
    assert_eq!(res.status(), 200);
    let html = text(res);
    assert_contains(&html, "Your cart is empty.");
    assert_contains(&html, "Browse Books");
}

#[test]
fn cart_update_and_remove_return_204_when_redirect_is_none() {
    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    let res = client.form("PUT", "/cart/api/update", &[("bookId", "1"), ("quantity", "2"), ("redirect", "none")]);
    assert_eq!(res.status(), 204);
    assert_contains(&text(client.get("/fragments/cart-items")), "$33.98");
    let res = client.form("DELETE", "/cart/api/remove", &[("bookId", "1"), ("redirect", "none")]);
    assert_eq!(res.status(), 204);
    assert_contains(&text(client.get("/fragments/cart-items")), "Your cart is empty.");
}

#[test]
fn method_override_reaches_put_and_delete_handlers() {
    let mut client = Client::new();
    client.form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    let res = client.form("POST", "/cart/api/remove", &[("_method", "DELETE"), ("bookId", "1")]);
    assert_eq!(res.status(), 302);
    assert!(location(&res).contains("/cart"));
}

// --- actions/auth/controller.test.ts ---------------------------------------------------

#[test]
fn login_with_valid_credentials_sets_session_and_redirects() {
    let mut client = Client::new();
    let res = client.form("POST", "/login", &[("email", "admin@bookstore.com"), ("password", "admin123")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/account");
    assert!(client.session.is_some());
}

#[test]
fn login_with_invalid_credentials_flashes_an_error_once() {
    let mut client = Client::new();
    let res = client.form("POST", "/login", &[("email", "wrong@example.com"), ("password", "wrongpassword")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/login");
    assert_contains(&text(client.get("/login")), "Invalid email or password");
    assert!(!text(client.get("/login")).contains("Invalid email or password"), "flash cleared after display");
}

#[test]
fn login_does_not_treat_wildcards_as_matches() {
    let res = Client::new().form("POST", "/login", &[("email", "%@bookstore.com"), ("password", "admin123")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/login");
}

#[test]
fn register_creates_user_and_sets_session() {
    let mut client = Client::new();
    let res = client.form("POST", "/register", &[("name", "New User"), ("email", "newuser@example.com"), ("password", "password123")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/account");
    assert!(client.session.is_some());
    assert_contains(&text(client.get("/account")), "New User");
}

#[test]
fn protected_routes_redirect_to_login_with_return_to() {
    let mut client = Client::new();
    for (path, expected) in [("/checkout", "/login?returnTo=%2Fcheckout"), ("/account", "/login?returnTo=%2Faccount"), ("/admin", "/login?returnTo=%2Fadmin")] {
        let res = client.get(path);
        assert_eq!(res.status(), 302, "{path}");
        assert_eq!(location(&res), expected);
    }
}

#[test]
fn login_with_return_to_redirects_back_or_preserves_it() {
    let mut client = Client::new();
    let res = client.form("POST", "/login?returnTo=%2Fcheckout", &[("email", "customer@example.com"), ("password", "password123")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/checkout");

    let mut client = Client::new();
    let res = client.form("POST", "/login?returnTo=%2Fcheckout", &[("email", "wrong@example.com"), ("password", "wrongpassword")]);
    assert_eq!(res.status(), 302);
    let to = location(&res).to_string();
    assert!(to.contains("returnTo=%2Fcheckout"), "{to}");
    let html = text(client.get(&to));
    assert_contains(&html, "Invalid email or password");
    assert_contains(&html, "returnTo=%2Fcheckout");
}

#[test]
fn reset_password_rejects_mismatches_and_bad_tokens_then_resets() {
    let mut client = Client::new();
    let html = text(client.form("POST", "/forgot-password", &[("email", "customer@example.com")]));
    let start = html.find("/reset-password/").expect("reset link in demo response") + "/reset-password/".len();
    let token = &html[start..start + html[start..].find('"').unwrap()];

    let res = client.form("POST", &format!("/reset-password/{token}"), &[("password", "newpassword123"), ("confirmPassword", "different")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), format!("/reset-password/{token}"));
    assert_contains(&text(client.get(&format!("/reset-password/{token}"))), "Passwords do not match");

    let res = client.form("POST", "/reset-password/invalid-token-12345", &[("password", "newpassword123"), ("confirmPassword", "newpassword123")]);
    assert_eq!(res.status(), 302);
    assert_eq!(location(&res), "/reset-password/invalid-token-12345");
    assert_contains(&text(client.get("/reset-password/invalid-token-12345")), "Invalid or expired reset token");

    let res = client.form("POST", &format!("/reset-password/{token}"), &[("password", "newpassword123"), ("confirmPassword", "newpassword123")]);
    assert_eq!(res.status(), 200);
    let mut fresh = Client::new();
    fresh.router = client.router;
    fresh.login("customer@example.com", "newpassword123");
}

#[test]
fn logout_clears_the_identity() {
    let mut client = Client::new();
    client.login_as_customer();
    assert_eq!(client.get("/account").status(), 200);
    let res = client.form("POST", "/logout", &[]);
    assert_eq!(res.status(), 302);
    assert_eq!(client.get("/account").status(), 302);
}

// --- actions/account/controller.test.ts ------------------------------------------------

#[test]
fn account_pages_and_orders() {
    let mut client = Client::new();
    client.login_as_customer();
    let html = text(client.get("/account"));
    assert_contains(&html, "My Account");
    let html = text(client.get("/account/orders"));
    assert_contains(&html, "2 item(s)");
    assert_contains(&html, "1 item(s)");
    let settings = client.get("/account/settings");
    assert_eq!(settings.status(), 200);
    let res = client.form("PUT", "/account/settings", &[("name", "Renamed Customer"), ("email", "customer@example.com"), ("password", "")]);
    assert_eq!(res.status(), 302);
    assert_contains(&text(client.get("/account")), "Renamed Customer");
}

// --- actions/checkout/controller.test.ts -----------------------------------------------

#[test]
fn checkout_creates_order_when_authenticated_with_items() {
    let mut client = Client::new();
    client.login_as_customer();
    client.form("POST", "/cart/api/add", &[("bookId", "1"), ("slug", "bbq")]);
    let res = client.form("POST", "/checkout", &[("street", "123 Test St"), ("city", "Test City"), ("state", "TS"), ("zip", "12345")]);
    assert_eq!(res.status(), 302);
    let to = location(&res).to_string();
    let order_id = to.strip_prefix("/checkout/").and_then(|rest| rest.strip_suffix("/confirmation")).expect("confirmation url");
    assert_eq!(client.get(&to).status(), 200);
    let res = client.get(&format!("/account/orders/{order_id}"));
    assert_eq!(res.status(), 200);
    assert_contains(&text(res), "Ash &amp; Smoke");
}

// --- actions/admin/controller.test.ts and admin/books/controller.test.ts ---------------

#[test]
fn admin_requires_the_admin_role() {
    let mut client = Client::new();
    client.login_as_customer();
    assert_eq!(client.get("/admin").status(), 403);

    let mut admin = Client::new();
    admin.login_as_admin();
    let res = admin.get("/admin");
    assert_eq!(res.status(), 200);
    assert_contains(&text(res), "Admin Dashboard");
    for path in ["/admin/books", "/admin/users", "/admin/orders", "/admin/books/new", "/admin/books/1", "/admin/books/1/edit"] {
        assert_eq!(admin.get(path).status(), 200, "{path}");
    }
}

const NEW_BOOK: &[(&str, &str)] = &[
    ("slug", "test-book"),
    ("title", "Test Book"),
    ("author", "Test Author"),
    ("description", "Test description"),
    ("price", "29.99"),
    ("genre", "test"),
    ("isbn", "978-0000000000"),
    ("publishedYear", "2024"),
    ("inStock", "true"),
];

#[test]
fn admin_creates_books_from_urlencoded_forms() {
    let mut admin = Client::new();
    admin.login_as_admin();
    let res = admin.form("POST", "/admin/books", NEW_BOOK);
    assert_eq!(res.status(), 302);
    assert!(location(&res).contains("/admin/books"));
    let html = text(admin.get("/books/test-book"));
    assert_contains(&html, "Test Book");
}

#[test]
fn admin_creates_books_with_uploaded_covers() {
    let mut admin = Client::new();
    admin.login_as_admin();
    let boundary = "----rmxboundary";
    let mut body = Vec::new();
    for (name, value) in NEW_BOOK {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    let png: &[u8] = b"\x89PNG\r\n\x1a\nnot-really-a-png";
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"cover.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = http::Request::post(format!("{ORIGIN}/admin/books")).header("content-type", format!("multipart/form-data; boundary={boundary}"));
    let res = admin.send(req, Body::from(body));
    assert_eq!(res.status(), 302);

    let html = text(admin.get("/books"));
    let start = html.find("/uploads/").expect("cover url in the catalog");
    let url = &html[start..start + html[start..].find('"').unwrap()];
    assert!(url.ends_with(".png"), "{url}");
    let res = admin.get(url);
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get("content-type").unwrap(), "image/png");
    assert_eq!(res.headers().get("cache-control").unwrap(), "public, max-age=31536000");
    assert_eq!(block_on(res.into_body().bytes()).unwrap().as_ref(), png);
}

#[test]
fn admin_updates_and_deletes_books() {
    let mut admin = Client::new();
    admin.login_as_admin();
    admin.form("POST", "/admin/books", NEW_BOOK);
    let id = Db::seeded().count_books() + 1;
    let mut fields = NEW_BOOK.to_vec();
    fields[1] = ("title", "Renamed Book");
    let res = admin.form("PUT", &format!("/admin/books/{id}"), &fields);
    assert_eq!(res.status(), 302);
    assert_contains(&text(admin.get("/books/test-book")), "Renamed Book");
    let res = admin.form("DELETE", &format!("/admin/books/{id}"), &[]);
    assert_eq!(res.status(), 302);
    assert_eq!(admin.get("/books/test-book").status(), 404);
}

#[test]
fn admin_cannot_delete_books_that_were_ordered() {
    let mut admin = Client::new();
    admin.login_as_admin();
    let res = admin.form("DELETE", "/admin/books/1", &[]);
    assert_eq!(res.status(), 409);
}

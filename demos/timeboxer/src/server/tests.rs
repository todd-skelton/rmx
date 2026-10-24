//! Mirrors the Remix timeboxer tests: `router.security.test.ts`,
//! `actions/auth/controller.test.ts` and `actions/schedules/{controller,authorization,
//! validation}.test.ts`, with their request helpers as [`Client`].

use rsmix::fetch::{block_on, Body};
use rsmix::session::MemoryStorage;
use serde_json::{json, Value};

use super::*;

/// `createRouter` with test settings and the database it uses.
struct App {
    router: Router,
    db: Db,
}

fn app() -> App {
    let db = Db::new();
    let options = TimeboxerOptions {
        session_cookie: session_cookie("test-session-secret", false),
        session_storage: MemoryStorage::new(),
        db: db.clone(),
        assets: None,
    };
    App { router: router(options), db }
}

fn url(path: &str) -> String {
    format!("http://localhost{path}")
}

/// A browser-ish client: a cookie jar (`mergeCookie`) and the last CSRF token seen.
#[derive(Clone, Default)]
struct Client {
    cookie: String,
    csrf: Option<String>,
}

/// `extractCsrfToken(html)`
fn extract_csrf_token(html: &str) -> String {
    let input = html.split("<input").skip(1).map(|rest| &rest[..rest.find('>').unwrap()]).find(|attrs| attrs.contains(r#"name="_csrf""#));
    let attrs = input.expect("Expected rendered page to include a CSRF input");
    let value = &attrs[attrs.find(r#"value=""#).expect("a value attribute") + 7..];
    value[..value.find('"').unwrap()].to_string()
}

fn text(res: Response) -> String {
    block_on(res.into_body().text()).unwrap()
}

fn body_json(res: Response) -> Value {
    serde_json::from_str(&text(res)).unwrap()
}

fn location(res: &Response) -> String {
    let location = res.headers().get("location").expect("Expected a redirect location header").to_str().unwrap();
    location.strip_prefix("http://localhost").unwrap_or(location).to_string()
}

#[track_caller]
fn assert_redirect(res: &Response, path: &str, status: u16) {
    assert_eq!(res.status(), status);
    assert_eq!(location(res), path);
}

#[track_caller]
fn assert_contains(html: &str, needle: &str) {
    assert!(html.contains(needle), "expected {needle:?} in the response");
}

fn multipart(fields: &[(&str, &str)]) -> (String, String) {
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!("--rsmix\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
    }
    body.push_str("--rsmix--\r\n");
    ("multipart/form-data; boundary=rsmix".into(), body)
}

impl App {
    fn send(&self, client: &mut Client, req: http::request::Builder, body: Body) -> Response {
        let req = if client.cookie.is_empty() { req } else { req.header("cookie", client.cookie.as_str()) };
        let res = block_on(self.router.fetch(req.body(body).unwrap()));
        if let Some(pair) = res.headers().get("set-cookie").and_then(|v| v.to_str().ok()).and_then(|v| v.split(';').next()) {
            client.cookie = pair.to_string();
        }
        res
    }

    /// `fetchPage`: `Accept: text/html`.
    fn page(&self, client: &mut Client, path: &str) -> Response {
        self.send(client, http::Request::get(url(path)).header("accept", "text/html"), Body::empty())
    }

    /// `loadFormPage`/`fetchHtml`: the page's HTML, remembering its CSRF token.
    fn html(&self, client: &mut Client, path: &str) -> (u16, String) {
        let res = self.page(client, path);
        let status = res.status().as_u16();
        let html = text(res);
        if html.contains(r#"name="_csrf""#) {
            client.csrf = Some(extract_csrf_token(&html));
        }
        (status, html)
    }

    /// `formRequest`: a multipart POST, with `_csrf` when given.
    fn form(&self, client: &mut Client, path: &str, fields: &[(&str, &str)], csrf: Option<&str>) -> Response {
        let mut fields = fields.to_vec();
        if let Some(token) = csrf {
            fields.push(("_csrf", token));
        }
        let (content_type, body) = multipart(&fields);
        self.send(client, http::Request::post(url(path)).header("content-type", content_type), Body::from(body))
    }

    /// `submitForm` with the client's own token.
    fn submit(&self, client: &mut Client, path: &str, fields: &[(&str, &str)]) -> Response {
        let token = client.csrf.clone().expect("Expected a loaded CSRF token before submitting a form");
        self.form(client, path, fields, Some(&token))
    }

    /// `rawJsonRequest`: `Accept: application/json`, with `X-Csrf-Token` when given.
    fn raw_json(&self, client: &mut Client, method: &str, path: &str, body: Option<String>, csrf: Option<&str>) -> Response {
        let mut req = http::Request::builder().method(method).uri(url(path)).header("accept", "application/json");
        if body.is_some() {
            req = req.header("content-type", "application/json");
        }
        if let Some(token) = csrf {
            req = req.header("x-csrf-token", token);
        }
        self.send(client, req, body.map_or_else(Body::empty, Body::from))
    }

    /// `fetchJson` with the client's token.
    fn json(&self, client: &mut Client, method: &str, path: &str, body: Option<Value>) -> Response {
        let token = client.csrf.clone();
        self.raw_json(client, method, path, body.map(|b| b.to_string()), token.as_deref())
    }

    /// `createAnonymousClient`: the signup page's cookie and token.
    fn anonymous(&self) -> Client {
        let mut client = Client::default();
        let (status, _) = self.html(&mut client, "/auth/signup");
        assert_eq!(status, 200);
        client
    }

    /// `createAuthenticatedClient`: sign up, then load the home page for a fresh token.
    fn signed_in(&self, username: &str) -> Client {
        let mut client = self.anonymous();
        let res = self.submit(&mut client, "/auth/signup", &[("username", username), ("password", "password123")]);
        assert_redirect(&res, "/", 303);
        self.html(&mut client, "/");
        client
    }

    fn create_schedule(&self, client: &mut Client, name: &str) -> Value {
        let res = self.json(client, "POST", "/schedules", Some(json!({ "name": name })));
        assert_eq!(res.status(), 201);
        body_json(res)["schedule"].clone()
    }

    fn get_schedule(&self, client: &mut Client, id: &Value) -> Value {
        let res = self.json(client, "GET", &format!("/schedules/{id}"), None);
        assert_eq!(res.status(), 200);
        body_json(res)["schedule"].clone()
    }

    fn replace(&self, client: &mut Client, id: &Value, body: Value) -> Response {
        self.json(client, "PUT", &format!("/schedules/{id}"), Some(body))
    }
}

/// `block(overrides)` from validation.test.ts
fn block(overrides: Value) -> Value {
    let mut block = json!({ "id": "block", "name": "block", "color": "blue", "dayOfWeek": 0, "startMinute": 480, "endMinute": 540 });
    for (k, v) in overrides.as_object().unwrap() {
        block[k] = v.clone();
    }
    block
}

// --- router.security.test.ts ------------------------------------------------------------

#[track_caller]
fn assert_csrf_rejection(res: Response, reason: &str) {
    assert_eq!(res.status(), 403);
    assert_eq!(text(res), format!("Forbidden: {reason} CSRF token"));
}

#[test]
fn rejects_auth_mutations_with_missing_or_invalid_csrf_tokens() {
    let app = app();
    let mut anon = app.anonymous();
    let creds = [("username", "missing-signup"), ("password", "password123")];
    assert_csrf_rejection(app.form(&mut anon, "/auth/signup", &creds, None), "missing");
    assert_csrf_rejection(app.form(&mut anon, "/auth/signup", &creds, Some("invalid-token")), "invalid");
    assert_csrf_rejection(app.form(&mut anon, "/auth/login", &creds, None), "missing");
    assert_csrf_rejection(app.form(&mut anon, "/auth/login", &creds, Some("invalid-token")), "invalid");

    let mut user = app.signed_in("security-auth");
    assert_csrf_rejection(app.form(&mut user, "/auth/logout", &[], None), "missing");
    assert_csrf_rejection(app.form(&mut user, "/auth/logout", &[], Some("invalid-token")), "invalid");
}

#[test]
fn rejects_schedule_mutations_with_missing_or_invalid_csrf_tokens() {
    let app = app();
    let mut user = app.signed_in("security-schedules");
    let schedule = app.create_schedule(&mut user, "csrf baseline schedule");
    let path = format!("/schedules/{}", schedule["id"]);
    let create = Some(json!({ "name": "missing csrf schedule" }).to_string());
    let replace = Some(json!({ "name": schedule["name"], "baseRevision": schedule["revision"], "blocks": [] }).to_string());

    assert_csrf_rejection(app.raw_json(&mut user, "POST", "/schedules", create.clone(), None), "missing");
    assert_csrf_rejection(app.raw_json(&mut user, "POST", "/schedules", create, Some("invalid-token")), "invalid");
    assert_csrf_rejection(app.raw_json(&mut user, "PUT", &path, replace.clone(), None), "missing");
    assert_csrf_rejection(app.raw_json(&mut user, "PUT", &path, replace, Some("invalid-token")), "invalid");
    assert_csrf_rejection(app.raw_json(&mut user, "DELETE", &path, None, None), "missing");
    assert_csrf_rejection(app.raw_json(&mut user, "DELETE", &path, None, Some("invalid-token")), "invalid");
}

#[test]
fn accepts_valid_csrf_tokens_for_auth_and_schedule_mutations() {
    let app = app();
    let mut anon = app.anonymous();
    let res = app.submit(&mut anon, "/auth/signup", &[("username", "valid-signup"), ("password", "password123")]);
    assert_eq!(res.status(), 303);
    let mut user = app.signed_in("valid-csrf");
    app.create_schedule(&mut user, "valid csrf schedule");
}

// --- actions/auth/controller.test.ts ----------------------------------------------------

#[test]
fn validates_signup_credentials() {
    let app = app();
    let mut client = app.anonymous();
    let res = app.submit(&mut client, "/auth/signup", &[("username", "ab"), ("password", "short")]);
    assert_eq!(res.status(), 400);
    let html = text(res);
    assert_contains(&html, "Expected at least 3 characters");
    assert_contains(&html, "Expected at least 8 characters");
    assert_contains(&html, "aria-invalid");

    let res = app.submit(&mut client, "/auth/signup", &[("username", "bad username"), ("password", "password123")]);
    assert_eq!(res.status(), 400);
    assert_contains(&text(res), "Use only letters, numbers, underscores, and dashes.");
}

#[test]
fn rejects_duplicate_usernames() {
    let app = app();
    app.signed_in("taken-name");
    let mut client = app.anonymous();
    let res = app.submit(&mut client, "/auth/signup", &[("username", "taken-name"), ("password", "password123")]);
    assert_eq!(res.status(), 409);
    assert_contains(&text(res), "That username is already taken.");
}

#[test]
fn signs_up_and_starts_a_session() {
    let app = app();
    let mut client = app.signed_in("new-user");
    assert!(app.db.find_user_by_username("new-user").is_some());
    let (status, html) = app.html(&mut client, "/auth");
    assert_eq!(status, 200);
    assert_contains(&html, "Welcome back, new-user.");
    assert_contains(&html, "Sign out");
}

#[test]
fn trims_usernames_and_stores_scrypt_password_records() {
    let app = app();
    let mut client = app.anonymous();
    let res = app.submit(&mut client, "/auth/signup", &[("username", "  padded  "), ("password", "password123")]);
    assert_redirect(&res, "/", 303);
    let user = app.db.find_user_by_username("padded").expect("trimmed username");
    let record = app.db.password_for(user.id).unwrap();
    assert!(record.password_hash.starts_with("scrypt:64:"));
    assert!(passwords::verify_password("password123", &record.password_hash));
}

#[test]
fn flashes_invalid_login_credentials_and_renders_the_error() {
    let app = app();
    app.signed_in("flash-user");
    let mut client = app.anonymous();
    app.html(&mut client, "/auth/login");
    let res = app.submit(&mut client, "/auth/login", &[("username", "flash-user"), ("password", "wrong-password")]);
    assert_redirect(&res, "/auth/login", 303);
    let (status, html) = app.html(&mut client, "/auth/login");
    assert_eq!(status, 200);
    assert_contains(&html, "Invalid username or password.");
    assert_contains(&html, r#"role="alert""#);
    // A flash shows once.
    assert!(!app.html(&mut client, "/auth/login").1.contains("Invalid username or password."));
}

#[test]
fn logs_in_and_starts_a_session() {
    let app = app();
    app.signed_in("login-user");
    let mut client = app.anonymous();
    app.html(&mut client, "/auth/login");
    let res = app.submit(&mut client, "/auth/login", &[("username", " login-user "), ("password", "password123")]);
    assert_redirect(&res, "/", 303);
    let (status, html) = app.html(&mut client, "/auth");
    assert_eq!(status, 200);
    assert_contains(&html, "Welcome back, login-user.");
}

#[test]
fn logs_out_and_clears_the_session() {
    let app = app();
    let mut client = app.signed_in("logout-user");
    app.html(&mut client, "/auth");
    let res = app.submit(&mut client, "/auth/logout", &[]);
    assert_redirect(&res, "/auth/login", 303);
    assert_redirect(&app.page(&mut client, "/auth"), "/auth/login", 302);
}

#[test]
fn redirects_authenticated_login_and_signup_pages_home() {
    let app = app();
    let mut client = app.signed_in("redirect-user");
    assert_redirect(&app.page(&mut client, "/auth/login"), "/", 302);
    assert_redirect(&app.page(&mut client, "/auth/signup"), "/", 302);
}

#[test]
fn renders_account_status_when_signed_in_and_redirects_anonymous_users_to_login() {
    let app = app();
    assert_redirect(&app.page(&mut Client::default(), "/auth"), "/auth/login", 302);
    let mut client = app.signed_in("status-user");
    let (status, html) = app.html(&mut client, "/auth");
    assert_eq!(status, 200);
    assert_contains(&html, "Signed in");
    assert_contains(&html, "Welcome back, status-user.");
}

#[test]
fn renders_the_home_page_for_anonymous_visitors() {
    let app = app();
    let (status, html) = app.html(&mut Client::default(), "/");
    assert_eq!(status, 200);
    assert_contains(&html, "Plan your focused work.");
    let mut client = app.signed_in("empty-user");
    let (status, html) = app.html(&mut client, "/");
    assert_eq!(status, 200);
    assert_contains(&html, "Create a schedule to get started.");
}

// --- actions/schedules/controller.test.ts -----------------------------------------------

#[test]
fn requires_authentication() {
    let app = app();
    assert_eq!(app.raw_json(&mut Client::default(), "GET", "/schedules", None, None).status(), 401);
    assert_eq!(app.raw_json(&mut Client::default(), "GET", "/schedules/1/download.ics", None, None).status(), 401);
    let mut anon = app.anonymous();
    let res = app.json(&mut anon, "DELETE", "/schedules/1", None);
    assert_eq!(res.status(), 401);
    assert_eq!(body_json(res), json!({ "error": "Authentication required." }));
    // Pages send people to the login page instead.
    assert_redirect(&app.page(&mut Client::default(), "/schedules/1"), "/auth/login", 302);
}

#[test]
fn creates_schedules_and_replaces_the_full_schedule_document() {
    let app = app();
    let mut client = app.signed_in("regular-user");
    let created = app.create_schedule(&mut client, "regular schedule");
    assert_eq!((created["name"].as_str(), created["revision"].as_i64()), (Some("regular schedule"), Some(0)));
    assert_eq!(created["blocks"], json!([]));
    let id = created["id"].clone();

    let res = app.json(&mut client, "POST", "/schedules", Some(json!({ "name": "regular schedule" })));
    assert_eq!(res.status(), 409);
    assert_eq!(body_json(res), json!({ "error": "Name must be unique.", "fieldErrors": { "name": "Name must be unique." } }));

    let res = app.json(&mut client, "POST", "/schedules", Some(json!({ "name": " " })));
    assert_eq!(res.status(), 400);
    let invalid = body_json(res);
    assert_eq!(invalid["error"], "Validation failed.");
    assert_eq!(invalid["fieldErrors"]["name"], "Name is required.");

    let list = body_json(app.json(&mut client, "GET", "/schedules", None));
    assert_eq!(list["schedules"].as_array().unwrap().len(), 1);

    assert_redirect(&app.page(&mut client, "/"), &format!("/schedules/{id}"), 302);

    let (status, html) = app.html(&mut client, &format!("/schedules/{id}"));
    assert_eq!(status, 200);
    assert_contains(&html, "regular schedule");
    assert_contains(&html, "Download ICS");
    assert_contains(&html, r#"aria-label="Delete regular schedule""#);
    assert_contains(&html, r#"aria-current="page""#);

    let initial = app.get_schedule(&mut client, &id);
    assert_eq!(initial["revision"], 0);

    let read = json!({ "id": "read", "name": "read", "color": "blue", "dayOfWeek": 0, "startMinute": 390, "endMinute": 450 });
    let workout = json!({ "id": "workout", "name": "workout", "color": "green", "dayOfWeek": 0, "startMinute": 480, "endMinute": 540 });
    let res = app.replace(&mut client, &id, json!({ "name": "regular schedule", "baseRevision": 0, "blocks": [read, workout] }));
    assert_eq!(res.status(), 200);
    let saved = body_json(res)["schedule"].clone();
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["blocks"].as_array().unwrap().len(), 2);

    let res = app.page(&mut client, &format!("/schedules/{id}/download.ics"));
    assert_eq!(res.status(), 200);
    let header = |name: &str| res.headers().get(name).unwrap().to_str().unwrap().to_string();
    assert_eq!(header("cache-control"), "no-store");
    assert_eq!(header("content-disposition"), r#"attachment; filename="regular-schedule.ics""#);
    assert_eq!(header("content-type"), "text/calendar; charset=utf-8");
    let ics = text(res);
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
    assert_contains(&ics, "X-WR-CALNAME:regular schedule\r\n");
    assert_contains(&ics, "SUMMARY:read\r\n");
    assert!(ics.lines().any(|l| l.starts_with("DTSTART:") && l.ends_with("T063000")));
    assert!(ics.lines().any(|l| l.starts_with("DTEND:") && l.ends_with("T073000")));
    assert_contains(&ics, "RRULE:FREQ=WEEKLY;BYDAY=MO\r\n");
    assert_contains(&ics, "SUMMARY:workout\r\n");
    assert!(ics.ends_with("END:VCALENDAR\r\n"));

    let mut renamed_read = saved["blocks"][0].clone();
    renamed_read["name"] = json!("reading");
    let res = app.replace(&mut client, &id, json!({ "name": "regular schedule", "baseRevision": saved["revision"], "blocks": [renamed_read, saved["blocks"][1]] }));
    assert_eq!(res.status(), 200);
    let renamed = body_json(res)["schedule"].clone();
    assert_eq!((renamed["revision"].as_i64(), renamed["blocks"][0]["name"].as_str()), (Some(2), Some("reading")));

    let res = app.replace(&mut client, &id, json!({ "name": "regular schedule", "baseRevision": 0, "blocks": saved["blocks"] }));
    assert_eq!(res.status(), 409);
    assert_eq!(body_json(res), json!({ "error": "Schedule is out of date." }));

    let mut first = renamed["blocks"][0].clone();
    let mut second = renamed["blocks"][1].clone();
    (first["startMinute"], first["endMinute"]) = (json!(390), json!(480));
    (second["startMinute"], second["endMinute"]) = (json!(450), json!(540));
    let res = app.replace(&mut client, &id, json!({ "name": "regular schedule", "baseRevision": renamed["revision"], "blocks": [first, second] }));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), json!({ "error": "Scheduled blocks cannot overlap." }));

    let persisted = app.get_schedule(&mut client, &id);
    assert_eq!(persisted["revision"], 2);
    let times: Vec<(i64, i64)> = persisted["blocks"].as_array().unwrap().iter().map(|b| (b["startMinute"].as_i64().unwrap(), b["endMinute"].as_i64().unwrap())).collect();
    assert_eq!(times, [(390, 450), (480, 540)]);
}

#[test]
fn lists_schedules_by_creation_date_with_newest_first() {
    let app = app();
    let mut client = app.signed_in("ordering-user");
    app.db.mock_now(Some(1000));
    let older = app.create_schedule(&mut client, "older schedule");
    app.db.mock_now(Some(2000));
    let newer = app.create_schedule(&mut client, "newer schedule");
    app.db.mock_now(Some(3000));
    let res = app.replace(&mut client, &older["id"], json!({ "name": older["name"], "baseRevision": older["revision"], "blocks": [] }));
    app.db.mock_now(None);
    assert_eq!(res.status(), 200);

    let list = body_json(app.json(&mut client, "GET", "/schedules", None));
    let names: Vec<&str> = list["schedules"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["newer schedule", "older schedule"]);
    assert_redirect(&app.page(&mut client, "/"), &format!("/schedules/{}", newer["id"]), 302);
}

#[test]
fn deletes_schedules() {
    let app = app();
    let mut client = app.signed_in("delete-user");
    let created = app.create_schedule(&mut client, "temporary schedule");
    let id = created["id"].clone();

    let res = app.json(&mut client, "DELETE", &format!("/schedules/{id}"), None);
    assert_eq!(res.status(), 200);
    assert_eq!(body_json(res), json!({ "deletedScheduleId": id, "nextScheduleHref": "/" }));
    assert_eq!(body_json(app.json(&mut client, "GET", "/schedules", None)), json!({ "schedules": [] }));
    assert_eq!(app.json(&mut client, "GET", &format!("/schedules/{id}"), None).status(), 404);
    assert_eq!(app.db.find_schedule(id.as_i64().unwrap()).unwrap().status, "deleted");
}

#[test]
fn points_deletes_at_the_next_latest_schedule() {
    let app = app();
    let mut client = app.signed_in("next-user");
    let first = app.create_schedule(&mut client, "first");
    let second = app.create_schedule(&mut client, "second");
    let res = app.json(&mut client, "DELETE", &format!("/schedules/{}", second["id"]), None);
    assert_eq!(body_json(res)["nextScheduleHref"], format!("/schedules/{}", first["id"]));
}

#[test]
fn treats_duplicated_block_names_as_independent_events() {
    let app = app();
    let mut client = app.signed_in("duplicates-user");
    let created = app.create_schedule(&mut client, "multi-day schedule");
    let id = created["id"].clone();
    let monday = json!({ "id": "breakfast-monday", "name": "breakfast", "color": null, "dayOfWeek": 0, "startMinute": 480, "endMinute": 540 });
    let tuesday = json!({ "id": "breakfast-tuesday", "name": "breakfast", "color": null, "dayOfWeek": 1, "startMinute": 480, "endMinute": 540 });
    let res = app.replace(&mut client, &id, json!({ "name": "multi-day schedule", "baseRevision": 0, "blocks": [monday, tuesday] }));
    assert_eq!(res.status(), 200);
    let saved = body_json(res)["schedule"].clone();
    let summary = |s: &Value| -> Vec<(String, String, i64)> {
        s["blocks"].as_array().unwrap().iter().map(|b| (b["id"].as_str().unwrap().into(), b["name"].as_str().unwrap().into(), b["dayOfWeek"].as_i64().unwrap())).collect()
    };
    assert_eq!(summary(&saved), [("breakfast-monday".into(), "breakfast".into(), 0), ("breakfast-tuesday".into(), "breakfast".into(), 1)]);

    let mut early = saved["blocks"][0].clone();
    early["name"] = json!("early breakfast");
    let res = app.replace(&mut client, &id, json!({ "name": "multi-day schedule", "baseRevision": saved["revision"], "blocks": [early, saved["blocks"][1]] }));
    assert_eq!(res.status(), 200);
    assert_eq!(
        summary(&body_json(res)["schedule"]),
        [("breakfast-monday".into(), "early breakfast".into(), 0), ("breakfast-tuesday".into(), "breakfast".into(), 1)]
    );
}

#[test]
fn renaming_onto_another_schedule_name_conflicts() {
    let app = app();
    let mut client = app.signed_in("rename-user");
    app.create_schedule(&mut client, "taken");
    let other = app.create_schedule(&mut client, "other");
    let res = app.replace(&mut client, &other["id"], json!({ "name": "taken", "baseRevision": 0, "blocks": [] }));
    assert_eq!(res.status(), 409);
    assert_eq!(body_json(res), json!({ "error": "Schedule data conflicts with an existing record." }));
}

// --- actions/schedules/authorization.test.ts --------------------------------------------

#[track_caller]
fn assert_not_found_without_leak(res: Response) {
    assert_eq!(res.status(), 404);
    let body = text(res);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({ "error": "Schedule not found." }));
    assert!(!body.contains("user b private schedule") && !body.contains("private focus"));
}

#[test]
fn does_not_expose_another_user_active_schedule() {
    let app = app();
    let mut a = app.signed_in("user-a");
    let mut b = app.signed_in("user-b");
    let a_schedule = app.create_schedule(&mut a, "user a schedule");
    let b_schedule = app.create_schedule(&mut b, "user b private schedule");
    let focus = json!({ "id": "private-focus", "color": "purple", "dayOfWeek": 0, "startMinute": 540, "endMinute": 600, "name": "private focus" });
    let res = app.replace(&mut b, &b_schedule["id"], json!({ "name": b_schedule["name"], "baseRevision": 0, "blocks": [focus] }));
    assert_eq!(res.status(), 200);
    let b_updated = body_json(res)["schedule"].clone();

    let list = body_json(app.json(&mut a, "GET", "/schedules", None));
    let ids: Vec<(i64, &str)> = list["schedules"].as_array().unwrap().iter().map(|s| (s["id"].as_i64().unwrap(), s["name"].as_str().unwrap())).collect();
    assert_eq!(ids, [(a_schedule["id"].as_i64().unwrap(), "user a schedule")]);
    assert_redirect(&app.page(&mut a, "/"), &format!("/schedules/{}", a_schedule["id"]), 302);

    let b_path = format!("/schedules/{}", b_schedule["id"]);
    assert_not_found_without_leak(app.json(&mut a, "GET", &b_path, None));
    assert_not_found_without_leak(app.page(&mut a, &b_path));
    assert_not_found_without_leak(app.json(&mut a, "PUT", &b_path, Some(json!({ "name": "stolen schedule", "baseRevision": b_updated["revision"], "blocks": [] }))));
    assert_not_found_without_leak(app.json(&mut a, "DELETE", &b_path, None));
    assert_not_found_without_leak(app.page(&mut a, &format!("{b_path}/download.ics")));

    let owner = app.get_schedule(&mut b, &b_schedule["id"]);
    assert_eq!((&owner["name"], &owner["revision"]), (&b_schedule["name"], &b_updated["revision"]));
    assert_eq!(owner["blocks"][0]["id"], "private-focus");
}

// --- actions/schedules/validation.test.ts -----------------------------------------------

#[test]
fn rejects_invalid_json_request_bodies() {
    let app = app();
    let mut client = app.signed_in("json-user");
    let expected = json!({ "error": "Validation failed.", "fieldErrors": {}, "issues": [{ "message": "Expected a valid JSON request body." }] });
    let token = client.csrf.clone();
    let res = app.raw_json(&mut client, "POST", "/schedules", Some("{".into()), token.as_deref());
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), expected);

    let schedule = app.create_schedule(&mut client, "json validation schedule");
    let res = app.raw_json(&mut client, "PUT", &format!("/schedules/{}", schedule["id"]), Some("{".into()), token.as_deref());
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), expected);
}

#[test]
fn rejects_invalid_integer_schedule_ids() {
    let app = app();
    let mut client = app.signed_in("integer-user");
    let res = app.json(&mut client, "PUT", "/schedules/not-an-int", Some(json!({ "name": "integer validation schedule", "baseRevision": 0, "blocks": [] })));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), json!({ "error": "scheduleId must be an integer." }));
    assert_eq!(app.json(&mut client, "GET", "/schedules/01", None).status(), 400);
}

const BOUNDS_ERROR: &str = "Scheduled blocks must have valid dayOfWeek, startMinute, and endMinute values.";

#[test]
fn rejects_invalid_day_and_time_bounds_without_changing_the_schedule() {
    let app = app();
    let mut client = app.signed_in("bounds-user");
    let schedule = app.create_schedule(&mut client, "bounds validation schedule");
    let id = &schedule["id"];
    let with = |overrides: Value| json!({ "name": schedule["name"], "baseRevision": 0, "blocks": [block(overrides)] });

    let res = app.replace(&mut client, id, with(json!({ "dayOfWeek": -1 })));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), json!({ "error": BOUNDS_ERROR }));
    for overrides in [json!({ "dayOfWeek": 7 }), json!({ "startMinute": -1 }), json!({ "endMinute": 1441 })] {
        assert_eq!(app.replace(&mut client, id, with(overrides)).status(), 400);
    }
    let persisted = app.get_schedule(&mut client, id);
    assert_eq!((persisted["revision"].clone(), persisted["blocks"].clone()), (json!(0), json!([])));
}

#[test]
fn rejects_zero_and_negative_duration_blocks() {
    let app = app();
    let mut client = app.signed_in("duration-user");
    let schedule = app.create_schedule(&mut client, "duration validation schedule");
    let id = &schedule["id"];
    let with = |overrides: Value| json!({ "name": schedule["name"], "baseRevision": 0, "blocks": [block(overrides)] });

    let res = app.replace(&mut client, id, with(json!({ "endMinute": 480 })));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), json!({ "error": BOUNDS_ERROR }));
    assert_eq!(app.replace(&mut client, id, with(json!({ "startMinute": 500, "endMinute": 480 }))).status(), 400);
    let persisted = app.get_schedule(&mut client, id);
    assert_eq!((persisted["revision"].clone(), persisted["blocks"].clone()), (json!(0), json!([])));
}

#[test]
fn accepts_maximum_name_id_and_color_lengths_and_rejects_longer_values() {
    let app = app();
    let mut client = app.signed_in("length-user");
    let (max_name, max_id, max_block_name, max_color) = ("s".repeat(80), "i".repeat(80), "n".repeat(80), "c".repeat(64));
    let schedule = app.create_schedule(&mut client, &max_name);
    assert_eq!(schedule["name"], max_name);
    let id = &schedule["id"];

    let res = app.replace(&mut client, id, json!({ "name": max_name, "baseRevision": 0, "blocks": [block(json!({ "id": max_id, "name": max_block_name, "color": max_color }))] }));
    assert_eq!(res.status(), 200);
    let accepted = body_json(res)["schedule"].clone();
    assert_eq!(accepted["name"], max_name);
    assert_eq!(
        accepted["blocks"],
        json!([{ "id": max_id, "name": max_block_name, "color": max_color, "dayOfWeek": 0, "startMinute": 480, "endMinute": 540 }])
    );

    let res = app.json(&mut client, "POST", "/schedules", Some(json!({ "name": "s".repeat(81) })));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res)["fieldErrors"]["name"], "Name must be 80 characters or fewer.");

    for overrides in [json!({ "id": "i".repeat(81) }), json!({ "name": "n".repeat(81) }), json!({ "color": "c".repeat(65) })] {
        let res = app.replace(&mut client, id, json!({ "name": max_name, "baseRevision": accepted["revision"], "blocks": [block(overrides)] }));
        assert_eq!(res.status(), 400);
    }
    let persisted = app.get_schedule(&mut client, id);
    assert_eq!((&persisted["revision"], &persisted["blocks"]), (&accepted["revision"], &accepted["blocks"]));
}

#[test]
fn accepts_nullable_color_and_adjacent_blocks() {
    let app = app();
    let mut client = app.signed_in("adjacent-user");
    let schedule = app.create_schedule(&mut client, "adjacent validation schedule");
    let first = json!({ "id": "first", "name": "first", "color": null, "dayOfWeek": 0, "startMinute": 480, "endMinute": 510 });
    let second = json!({ "id": "second", "name": "second", "color": null, "dayOfWeek": 0, "startMinute": 510, "endMinute": 540 });
    let res = app.replace(&mut client, &schedule["id"], json!({ "name": schedule["name"], "baseRevision": 0, "blocks": [first, second] }));
    assert_eq!(res.status(), 200);
    let saved = body_json(res)["schedule"].clone();
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["blocks"], json!([first, second]));
}

#[test]
fn rolls_back_failed_replacements_and_keeps_the_persisted_revision_unchanged() {
    let app = app();
    let mut client = app.signed_in("rollback-user");
    let schedule = app.create_schedule(&mut client, "rollback validation schedule");
    let id = &schedule["id"];
    let res = app.replace(&mut client, id, json!({ "name": schedule["name"], "baseRevision": 0, "blocks": [block(json!({ "id": "original", "name": "original" }))] }));
    assert_eq!(res.status(), 200);
    let saved = body_json(res)["schedule"].clone();

    let duplicate_a = block(json!({ "id": "duplicate", "name": "updated first", "color": "green", "startMinute": 480, "endMinute": 510 }));
    let duplicate_b = block(json!({ "id": "duplicate", "name": "updated second", "color": "red", "startMinute": 510, "endMinute": 540 }));
    let res = app.replace(&mut client, id, json!({ "name": "should roll back", "baseRevision": saved["revision"], "blocks": [duplicate_a, duplicate_b] }));
    assert_eq!(res.status(), 400);
    assert_eq!(body_json(res), json!({ "error": "Scheduled block ids must be unique." }));

    let persisted = app.get_schedule(&mut client, id);
    assert_eq!((&persisted["name"], &persisted["revision"], &persisted["blocks"]), (&saved["name"], &saved["revision"], &saved["blocks"]));
}

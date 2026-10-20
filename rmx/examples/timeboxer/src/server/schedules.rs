//! `actions/schedules/controller.tsx` and `page.tsx`: the JSON schedule API, the ICS
//! download and the schedule page.

use http::{header, HeaderValue, StatusCode};
use rmx::{css, rsx, Component, Handle, Node};
use rmx::fetch::{response, Body, Response};
use rmx::router::{Context, Router};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::data::{ReplaceInput, Schedule, ScheduleDataError};
use super::ics::create_schedule_ics;
use super::ui::Document;
use super::{current_user, db, json, json_with, render, token};
use crate::grid::{ScheduleGrid, ScheduleGridProps};
use crate::layout::{Block, ScheduleDocument};
use crate::routes::{schedule_href, ROUTES};
use crate::sidebar::{ScheduleSidebar, SidebarSchedule};
use crate::theme::{surface, text};

pub fn routes(router: &mut Router) {
    let r = &ROUTES.schedules;
    router.route(&r.index, index);
    router.route(&r.create, create);
    router.route(&r.destroy, destroy);
    router.route(&r.download_ics, download_ics);
    router.route(&r.show, show);
    router.route(&r.update, update);
}

fn unauthorized() -> Response {
    json_with(StatusCode::UNAUTHORIZED, &json!({ "error": "Authentication required." }))
}

/// `Accept` asks for JSON and not HTML.
fn wants_json(ctx: &Context) -> bool {
    let accept = ctx.header("accept").unwrap_or_default();
    accept.contains("application/json") && !accept.contains("text/html")
}

/// Unauthenticated page requests go to the login page; API requests get a 401.
fn login_or_unauthorized(ctx: &Context) -> Response {
    if wants_json(ctx) {
        unauthorized()
    } else {
        // `new URL(href, request.url)`: an absolute URL on this origin.
        response::redirect(&format!("{}{}", ctx.url().origin(), ROUTES.auth.login.index.href(())))
    }
}

/// `parseIntegerParam`: digits that round-trip exactly (`String(parseInt(v)) === v`).
fn schedule_id(ctx: &Context) -> Option<i64> {
    let value = ctx.param("scheduleId").unwrap_or_default();
    value.parse::<i64>().ok().filter(|id| id.to_string() == value)
}

fn invalid_schedule_id() -> Response {
    json_with(StatusCode::BAD_REQUEST, &json!({ "error": "scheduleId must be an integer." }))
}

fn schedule_error(error: ScheduleDataError) -> Response {
    json_with(StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), &json!({ "error": error.message }))
}

fn field_error(field: &str, message: &str, status: StatusCode) -> Response {
    json_with(status, &json!({ "error": message, "fieldErrors": { field: message } }))
}

async fn index(ctx: Context) -> Response {
    let Some(user) = current_user(&ctx) else { return unauthorized() };
    json(&json!({ "schedules": db(&ctx).list_schedules(user.id) }))
}

async fn create(mut ctx: Context) -> Response {
    let Some(user_id) = current_user(&ctx).map(|u| u.id) else { return unauthorized() };
    let name = match parse_json_request(&mut ctx, parse_create).await {
        Ok(name) => name,
        Err(issues) => return validation_error(issues),
    };
    match db(&ctx).create_schedule(user_id, &name) {
        Ok(schedule) => json_with(StatusCode::CREATED, &json!({ "schedule": schedule })),
        Err(e) if e.message == "Name must be unique." || e.constraint => field_error("name", "Name must be unique.", StatusCode::CONFLICT),
        Err(e) => schedule_error(e),
    }
}

async fn destroy(ctx: Context) -> Response {
    let Some(user_id) = current_user(&ctx).map(|u| u.id) else { return unauthorized() };
    let Some(id) = schedule_id(&ctx) else { return invalid_schedule_id() };
    if let Err(e) = db(&ctx).delete_schedule(user_id, id) {
        return schedule_error(e);
    }
    let next = db(&ctx).list_schedules(user_id).first().map_or_else(|| ROUTES.home.href(()), |s| schedule_href(s.id));
    json(&json!({ "deletedScheduleId": id, "nextScheduleHref": next }))
}

async fn download_ics(ctx: Context) -> Response {
    let Some(user_id) = current_user(&ctx).map(|u| u.id) else { return login_or_unauthorized(&ctx) };
    let Some(id) = schedule_id(&ctx) else { return invalid_schedule_id() };
    let schedule = match db(&ctx).schedule_document(user_id, id) {
        Ok(schedule) => schedule,
        Err(e) => return schedule_error(e),
    };
    let mut res = Response::new(Body::from(create_schedule_ics(&schedule, db(&ctx).now())));
    let headers = res.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{}\"", download_filename(&schedule.name))) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/calendar; charset=utf-8"));
    res
}

async fn show(ctx: Context) -> Response {
    let Some(user_id) = current_user(&ctx).map(|u| u.id) else { return login_or_unauthorized(&ctx) };
    let Some(id) = schedule_id(&ctx) else { return invalid_schedule_id() };
    let schedule = match db(&ctx).schedule_document(user_id, id) {
        Ok(schedule) => schedule,
        Err(e) => return schedule_error(e),
    };
    if wants_json(&ctx) {
        return json(&json!({ "schedule": schedule }));
    }
    let schedules = db(&ctx).list_schedules(user_id);
    render(&ctx, rsx! { <SchedulePage active_schedule_id={schedule.id} csrf_token={token(&ctx)} schedule={schedule} schedules={schedules}/> })
}

async fn update(mut ctx: Context) -> Response {
    let Some(user_id) = current_user(&ctx).map(|u| u.id) else { return unauthorized() };
    let Some(id) = schedule_id(&ctx) else { return invalid_schedule_id() };
    let input = match parse_json_request(&mut ctx, parse_replace).await {
        Ok(input) => input,
        Err(issues) => return validation_error(issues),
    };
    match db(&ctx).replace_schedule_document(user_id, id, input) {
        Ok(schedule) => json(&json!({ "schedule": schedule })),
        Err(e) => schedule_error(e),
    }
}

/// `downloadFilename`: a lowercase slug, or `schedule`.
fn download_filename(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.strip_prefix('-').unwrap_or(&slug);
    let slug = slug.strip_suffix('-').unwrap_or(slug);
    format!("{}.ics", if slug.is_empty() { "schedule" } else { slug })
}

// ---------------------------------------------------------------------------------------
// Request validation (remix/data-schema)
// ---------------------------------------------------------------------------------------

/// A validation issue: a message and where in the input it applies.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Value>>,
}

fn issue(message: impl Into<String>, path: &[Value]) -> Issue {
    Issue { message: message.into(), path: Some(path.to_vec()) }
}

fn at(path: &[Value], key: impl Into<Value>) -> Vec<Value> {
    let mut p = path.to_vec();
    p.push(key.into());
    p
}

/// `parseJsonRequest(schema, request)`
async fn parse_json_request<T>(ctx: &mut Context, schema: fn(&Value) -> Result<T, Vec<Issue>>) -> Result<T, Vec<Issue>> {
    let text = ctx.take_body().text().await.unwrap_or_default();
    let value: Value = serde_json::from_str(&text).map_err(|_| vec![Issue { message: "Expected a valid JSON request body.".into(), path: None }])?;
    schema(&value)
}

fn validation_error(issues: Vec<Issue>) -> Response {
    let mut field_errors = Map::new();
    for issue in &issues {
        let Some(Value::String(field)) = issue.path.as_ref().and_then(|p| p.first()) else { continue };
        if !field_errors.contains_key(field) {
            field_errors.insert(field.clone(), Value::String(field_message(field, &issue.message)));
        }
    }
    json_with(StatusCode::BAD_REQUEST, &json!({ "error": "Validation failed.", "fieldErrors": field_errors, "issues": issues }))
}

/// `fieldMessage`: friendlier wording for the name field.
fn field_message(field: &str, message: &str) -> String {
    if field != "name" {
        return message.into();
    }
    let lower = message.to_lowercase();
    if ["80", "maximum", "max", "at most", "length"].iter().any(|w| lower.contains(w)) {
        return "Name must be 80 characters or fewer.".into();
    }
    if ["required", "minimum", "min", "at least", "empty"].iter().any(|w| lower.contains(w)) {
        return "Name is required.".into();
    }
    message.into()
}

fn object<'a>(value: &'a Value, path: &[Value]) -> Result<&'a Map<String, Value>, Vec<Issue>> {
    value.as_object().ok_or_else(|| vec![issue("Expected object", path)])
}

/// `s.string()`, optionally trimmed, then `minLength` and `maxLength`.
fn string(value: Option<&Value>, path: &[Value], trim: bool, min: usize, max: usize) -> Result<String, Issue> {
    let Some(Value::String(s)) = value else { return Err(issue("Expected string", path)) };
    let s = if trim { s.trim().to_string() } else { s.clone() };
    match super::auth::length_issue(&s, min, max) {
        Some(message) => Err(issue(message, path)),
        None => Ok(s),
    }
}

/// `s.number().refine(Number.isInteger, '<name> must be an integer.')`
fn integer(value: Option<&Value>, path: &[Value], name: &str) -> Result<i64, Issue> {
    let Some(n) = value.and_then(Value::as_f64) else { return Err(issue("Expected number", path)) };
    if n.fract() != 0.0 || !n.is_finite() {
        return Err(issue(format!("{name} must be an integer."), path));
    }
    Ok(n as i64)
}

fn collect<T>(issues: &mut Vec<Issue>, result: Result<T, Issue>) -> Option<T> {
    result.map_err(|e| issues.push(e)).ok()
}

/// `scheduleNameSchema` in `{ name }`
fn parse_create(value: &Value) -> Result<String, Vec<Issue>> {
    let fields = object(value, &[])?;
    string(fields.get("name"), &["name".into()], true, 1, 80).map_err(|e| vec![e])
}

/// `replaceScheduleSchema`
fn parse_replace(value: &Value) -> Result<ReplaceInput, Vec<Issue>> {
    let fields = object(value, &[])?;
    let mut issues = Vec::new();
    let base_revision = collect(&mut issues, integer(fields.get("baseRevision"), &["baseRevision".into()], "baseRevision"));
    let blocks = match fields.get("blocks") {
        Some(Value::Array(items)) => {
            let mut blocks = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if let Some(block) = parse_block(item, &["blocks".into(), i.into()], &mut issues) {
                    blocks.push(block);
                }
            }
            Some(blocks)
        }
        _ => {
            issues.push(issue("Expected array", &["blocks".into()]));
            None
        }
    };
    let name = collect(&mut issues, string(fields.get("name"), &["name".into()], true, 1, 80));
    match (base_revision, blocks, name) {
        (Some(base_revision), Some(blocks), Some(name)) if issues.is_empty() => Ok(ReplaceInput { base_revision, blocks, name }),
        _ => Err(issues),
    }
}

/// `scheduleBlockSchema`
fn parse_block(value: &Value, path: &[Value], issues: &mut Vec<Issue>) -> Option<Block> {
    let Some(fields) = value.as_object() else {
        issues.push(issue("Expected object", path));
        return None;
    };
    let id = collect(issues, string(fields.get("id"), &at(path, "id"), false, 1, 80));
    let color = match fields.get("color") {
        None | Some(Value::Null) => Some(None),
        other => collect(issues, string(other, &at(path, "color"), false, 0, 64)).map(Some),
    };
    let day_of_week = collect(issues, integer(fields.get("dayOfWeek"), &at(path, "dayOfWeek"), "dayOfWeek"));
    let end_minute = collect(issues, integer(fields.get("endMinute"), &at(path, "endMinute"), "endMinute"));
    let name = collect(issues, string(fields.get("name"), &at(path, "name"), true, 1, 80));
    let start_minute = collect(issues, integer(fields.get("startMinute"), &at(path, "startMinute"), "startMinute"));
    Some(Block { id: id?, color: color?, day_of_week: day_of_week?, end_minute: end_minute?, name: name?, start_minute: start_minute? })
}

// ---------------------------------------------------------------------------------------
// page.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct SchedulePageProps {
    pub active_schedule_id: Option<i64>,
    pub csrf_token: String,
    pub schedules: Vec<Schedule>,
    pub schedule: Option<ScheduleDocument>,
}

pub struct SchedulePage;

impl Component for SchedulePage {
    type Props = SchedulePageProps;

    fn setup(_: &SchedulePageProps, _: &Handle<Self>) -> Self {
        SchedulePage
    }

    fn render(&self, props: &SchedulePageProps, _: &Handle<Self>) -> Node {
        let schedules: Vec<SidebarSchedule> = props
            .schedules
            .iter()
            .map(|s| SidebarSchedule {
                delete_href: ROUTES.schedules.destroy.href([("scheduleId", s.id.to_string().as_str())]),
                href: schedule_href(s.id),
                id: s.id,
                name: s.name.clone(),
            })
            .collect();
        let workspace = match &props.schedule {
            // Keyed by id: another schedule gets a fresh grid (Remix resets its state when
            // the schedule id changes).
            Some(schedule) => {
                let grid = ScheduleGridProps {
                    csrf_token: props.csrf_token.clone(),
                    download_ics_href: ROUTES.schedules.download_ics.href([("scheduleId", schedule.id.to_string().as_str())]),
                    schedule: schedule.clone(),
                };
                rsx! { <ScheduleGrid key={schedule.id} csrf_token={grid.csrf_token} download_ics_href={grid.download_ics_href} schedule={grid.schedule}/> }
            }
            None => rsx! {
                <div mix={css(&format!("align-items: center; color: {}; display: flex; font-size: 16px; height: 100%; justify-content: center;", text::SECONDARY))}>
                    "Create a schedule to get started."
                </div>
            },
        };
        rsx! {
            <Document title="Timeboxer">
                <div mix={css(&format!("background-color: {}; display: grid; grid-template-columns: 240px minmax(0, 1fr); height: 100vh; overflow: hidden;", surface::LVL0))}>
                    <ScheduleSidebar active_schedule_id={props.active_schedule_id} csrf_token={props.csrf_token.clone()}
                        logout_href={ROUTES.auth.logout.href(())} schedules={schedules}/>
                    <main aria-label="Workspace" mix={css("min-height: 0; overflow: hidden;")}>{workspace}</main>
                </div>
            </Document>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_download_filenames() {
        assert_eq!(download_filename("regular schedule"), "regular-schedule.ics");
        assert_eq!(download_filename("  --Week #1!  "), "week-1.ics");
        assert_eq!(download_filename("***"), "schedule.ics");
    }

    #[test]
    fn reports_issues_with_paths() {
        let issues = parse_replace(&json!({ "baseRevision": 1.5, "blocks": [{ "id": "", "dayOfWeek": 0, "startMinute": 0, "endMinute": 15, "name": " " }] })).unwrap_err();
        let summary: Vec<(String, Value)> = issues.iter().map(|i| (i.message.clone(), json!(i.path))).collect();
        assert_eq!(
            summary,
            [
                ("baseRevision must be an integer.".to_string(), json!(["baseRevision"])),
                ("Expected at least 1 characters".to_string(), json!(["blocks", 0, "id"])),
                ("Expected at least 1 characters".to_string(), json!(["blocks", 0, "name"])),
                ("Expected string".to_string(), json!(["name"])),
            ]
        );
    }
}

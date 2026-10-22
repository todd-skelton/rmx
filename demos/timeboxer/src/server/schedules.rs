//! `actions/schedules/controller.tsx` and `page.tsx`: the JSON schedule API, the ICS
//! download and the schedule page.

use http::{header, HeaderValue, StatusCode};
use rmx::{css, rsx, Component, Handle, Node};
use rmx::fetch::{response, Body, Response};
use rmx::router::{action, Context, Router};
use rmx::data_schema::checks::{max_length, min_length};
use rmx::data_schema::{self as s, Issue, PathSegment, Schema};
use serde_json::{json, Map, Value};

use super::data::{ReplaceInput, Schedule, ScheduleDataError};
use super::ics::create_schedule_ics;
use super::ui::Document;
use super::{current_user, db, json, json_with, render, token};
use crate::grid::{ScheduleGrid, ScheduleGridProps};
use crate::layout::{Block, ScheduleDocument};
use crate::routes::{schedule_href, SchedulesActions, ROUTES};
use crate::sidebar::{ScheduleSidebar, SidebarSchedule};
use crate::theme::{surface, text};

pub fn routes(router: &mut Router) {
    router.map(
        &ROUTES.schedules,
        SchedulesActions {
            index: action(index),
            create: action(create),
            destroy: action(destroy),
            download_ics: action(download_ics),
            show: action(show),
            update: action(update),
        },
    );
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
    let name = match parse_json_request(&mut ctx, create_schedule()).await {
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
    let input = match parse_json_request(&mut ctx, replace_schedule()).await {
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

/// `scheduleNameSchema`: trimmed, 1 to 80 characters.
fn schedule_name() -> impl Schema<Output = String> {
    s::string().transform(|v| v.trim().to_string()).pipe(min_length(1)).pipe(max_length(80))
}

/// `s.number().refine(Number.isInteger, '<name> must be an integer.')`
fn integer(name: &str) -> impl Schema<Output = i64> {
    s::number().refine_with(|n| n.fract() == 0.0, format!("{name} must be an integer.")).transform(|n| n as i64)
}

/// `scheduleBlockSchema`
fn schedule_block() -> impl Schema<Output = Block> {
    s::object(|o| Block {
        id: o.field("id", s::string().pipe(min_length(1)).pipe(max_length(80))),
        color: o.field("color", s::optional(s::nullable(s::string().pipe(max_length(64))))).flatten(),
        day_of_week: o.field("dayOfWeek", integer("dayOfWeek")),
        end_minute: o.field("endMinute", integer("endMinute")),
        name: o.field("name", schedule_name()),
        start_minute: o.field("startMinute", integer("startMinute")),
    })
}

/// `createScheduleSchema`
fn create_schedule() -> impl Schema<Output = String> {
    s::object(|o| o.field("name", schedule_name()))
}

/// `replaceScheduleSchema`
fn replace_schedule() -> impl Schema<Output = ReplaceInput> {
    s::object(|o| ReplaceInput {
        base_revision: o.field("baseRevision", integer("baseRevision")),
        blocks: o.field("blocks", s::array(schedule_block())),
        name: o.field("name", schedule_name()),
    })
}

/// `parseJsonRequest(schema, request)`
async fn parse_json_request<S: Schema>(ctx: &mut Context, schema: S) -> Result<S::Output, Vec<Issue>> {
    let text = ctx.take_body().text().await.unwrap_or_default();
    let value: Value = serde_json::from_str(&text).map_err(|_| vec![Issue::new("Expected a valid JSON request body.", Vec::new())])?;
    s::parse(&schema, value).map_err(|e| e.issues)
}

/// An issue as JSON: `{ message, path? }`.
fn issue_json(issue: &Issue) -> Value {
    let mut out = Map::new();
    out.insert("message".into(), Value::String(issue.message.clone()));
    if !issue.path.is_empty() {
        let path = issue.path.iter().map(|segment| match segment {
            PathSegment::Key(k) => Value::String(k.clone()),
            PathSegment::Index(i) => json!(i),
        });
        out.insert("path".into(), Value::Array(path.collect()));
    }
    Value::Object(out)
}

fn validation_error(issues: Vec<Issue>) -> Response {
    let mut field_errors = Map::new();
    for issue in &issues {
        let Some(PathSegment::Key(field)) = issue.path.first() else { continue };
        if !field_errors.contains_key(field) {
            field_errors.insert(field.clone(), Value::String(field_message(field, &issue.message)));
        }
    }
    let issues: Vec<Value> = issues.iter().map(issue_json).collect();
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
        let input = json!({ "baseRevision": 1.5, "blocks": [{ "id": "", "dayOfWeek": 0, "startMinute": 0, "endMinute": 15, "name": " " }] });
        let issues = s::parse(&replace_schedule(), input).unwrap_err().issues;
        let summary: Vec<(String, Value)> = issues.iter().map(|i| (i.message.clone(), issue_json(i)["path"].clone())).collect();
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

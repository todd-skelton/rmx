//! `schedule-sidebar.tsx` and `new-schedule-action.tsx`: the schedule list with optimistic
//! deletes, and the inline "New schedule" form that creates a schedule through the JSON API
//! and navigates to it.

use std::collections::{HashMap, HashSet};

use rmx::browser::{self, FetchInit, FetchResponse};
use rmx::{css, on, rsx, Component, Handle, Mixin, NavigateOptions, Node, NodeHandle};
use rmx_ui::animation::{animate_entrance, animate_exit, Animate, SpringPreset};
use serde::{Deserialize, Serialize};

use crate::dom_util;
use crate::routes::{schedule_href, ROUTES};
use crate::theme::{self, border, font_size, space, surface, text, Tone, DANGER, FOCUS_RING, RADIUS};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSchedule {
    pub delete_href: String,
    pub href: String,
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSidebarProps {
    pub active_schedule_id: Option<i64>,
    pub csrf_token: String,
    pub logout_href: String,
    pub schedules: Vec<SidebarSchedule>,
}

pub struct ScheduleSidebar {
    deleted: HashSet<i64>,
    deleting: HashSet<i64>,
    errors: HashMap<i64, String>,
}

impl Component for ScheduleSidebar {
    type Props = ScheduleSidebarProps;
    rmx::client_entry!("ScheduleSidebar");

    fn setup(_: &ScheduleSidebarProps, _: &Handle<Self>) -> Self {
        ScheduleSidebar { deleted: HashSet::new(), deleting: HashSet::new(), errors: HashMap::new() }
    }

    fn render(&self, props: &ScheduleSidebarProps, h: &Handle<Self>) -> Node {
        let rows: Vec<Node> = props
            .schedules
            .iter()
            .filter(|s| !self.deleted.contains(&s.id))
            .map(|schedule| {
                let active = Some(schedule.id) == props.active_schedule_id;
                let deleting = self.deleting.contains(&schedule.id);
                let on_delete = ScheduleSidebar::delete_handler(h, schedule.clone(), props.csrf_token.clone(), active);
                sidebar_row(schedule, active, deleting, self.errors.get(&schedule.id).cloned(), on_delete)
            })
            .collect();
        rsx! {
            <aside aria-label="Schedules" mix={css(&format!("background-color: {}; border-right: 1px solid {}; display: grid; grid-template-rows: 56px 32px minmax(0, 1fr); min-height: 0;", surface::LVL0, border::STRONG))}>
                <div mix={css(&format!("align-items: center; display: flex; gap: {}; justify-content: space-between; padding: {} {};", space::SM, space::SM, space::MD))}>
                    <a href="/" mix={css(&format!("color: {}; font-size: {}; font-weight: 700; letter-spacing: {}; text-decoration: none; white-space: nowrap;", text::PRIMARY, font_size::XL, theme::LETTER_SPACING_TIGHT))}>"Timeboxer"</a>
                    <form action={props.logout_href.clone()} method="post" mix={css("margin: 0;")}>
                        <input type="hidden" name="_csrf" value={props.csrf_token.clone()}/>
                        <button type="submit" mix={[theme::button(Tone::Neutral), css(&format!("color: {};", text::SECONDARY))]}>"Logout"</button>
                    </form>
                </div>
                <div mix={css(&format!("min-height: 0; padding-inline: {};", space::MD))}>
                    <NewScheduleAction csrf_token={props.csrf_token.clone()}/>
                </div>
                <div mix={css(&format!("min-height: 0; overflow-y: auto; padding: {} {};", space::XL, space::LG))}>
                    <nav aria-label="Schedule list" mix={css(&format!("display: grid; gap: {};", space::SM))}>{rows}</nav>
                </div>
            </aside>
        }
    }
}

impl ScheduleSidebar {
    fn delete_handler(h: &Handle<Self>, schedule: SidebarSchedule, csrf_token: String, active: bool) -> rmx::EventHandler {
        h.on_event(move |this, h, event| {
            event.prevent_default();
            event.stop_propagation();
            if this.deleting.contains(&schedule.id) {
                return;
            }
            this.deleting.insert(schedule.id);
            this.errors.remove(&schedule.id);
            h.update();
            let (h, schedule, csrf_token) = (h.clone(), schedule.clone(), csrf_token.clone());
            h.clone().spawn(async move {
                let init = FetchInit {
                    method: "DELETE".into(),
                    headers: vec![("Accept".into(), "application/json".into()), ("X-Csrf-Token".into(), csrf_token)],
                    ..Default::default()
                };
                let result = at_least(500.0, browser::fetch(&schedule.delete_href, init)).await;
                let response = match result {
                    Ok(response) if response.ok() => response,
                    other => {
                        let message = match other {
                            Ok(response) => delete_error_message(&response),
                            Err(_) => "Could not delete schedule. Please try again.".into(),
                        };
                        h.with(|this, h| {
                            this.errors.insert(schedule.id, message);
                            this.deleting.remove(&schedule.id);
                            h.update();
                        });
                        return;
                    }
                };
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Deleted {
                    next_schedule_href: Option<String>,
                }
                let next = response.json::<Deleted>().ok().and_then(|d| d.next_schedule_href).unwrap_or_else(|| "/".into());
                if active {
                    rmx::navigate_async(&next, NavigateOptions::default()).await;
                    return;
                }
                h.with(|this, h| {
                    this.deleted.insert(schedule.id);
                    this.deleting.remove(&schedule.id);
                    h.update();
                });
            });
        })
    }
}

/// Await `work` and take at least `ms` in total (`Promise.all([work, sleep(ms)])`).
pub async fn at_least<T>(ms: f64, work: impl std::future::Future<Output = T>) -> T {
    let started = browser::now();
    let value = work.await;
    let left = ms - (browser::now() - started);
    if left > 0.0 {
        browser::sleep(left as u32).await;
    }
    value
}

/// The JSON `error`, else a message for the status.
fn delete_error_message(response: &FetchResponse) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<String>,
    }
    if response.content_type.contains("application/json") {
        if let Some(error) = response.json::<ErrorBody>().ok().and_then(|b| b.error) {
            return error;
        }
    }
    match response.status {
        401 => "Please sign in before deleting this schedule.",
        403 => "Could not verify this request. Please refresh and try again.",
        404 => "Schedule was already deleted.",
        _ => "Could not delete schedule. Please try again.",
    }
    .into()
}

fn sidebar_row(schedule: &SidebarSchedule, active: bool, deleting: bool, error: Option<String>, on_delete: rmx::EventHandler) -> Node {
    let error_id = format!("delete-schedule-{}-error", schedule.id);
    let link = css(&format!(
        "align-items: center; border-radius: {RADIUS}; color: {}; display: grid; gap: {xs}; grid-template-columns: minmax(0, 1fr) auto; min-width: 0;
         padding: {xs} {xs} {xs} {}; text-decoration: none;
         & > button {{ opacity: 0; pointer-events: none; }}
         &:hover {{ background-color: {lvl2}; }} &:hover > button {{ opacity: 1; pointer-events: auto; }}
         &:focus-within {{ background-color: {lvl2}; }} &:focus-within > button {{ opacity: 1; pointer-events: auto; }}
         &[data-active='true'] {{ background-color: {lvl2}; }}
         &:focus-visible {{ outline: 2px solid {FOCUS_RING}; outline-offset: 2px; }}
         &[data-active='true'] > span {{ font-weight: 500; }}
         &[data-deleting='true'] {{ opacity: 0.75; }} &[data-deleting='true'] > button {{ opacity: 1; pointer-events: auto; }}",
        text::PRIMARY,
        space::SM,
        xs = space::XS,
        lvl2 = surface::LVL2,
    ));
    let delete_button = css(&format!(
        "align-items: center; background-color: transparent; border: 0; border-radius: 4px; color: {}; cursor: pointer; display: inline-flex; height: 1.5rem;
         justify-content: center; margin: 0; padding: 0; width: 1.5rem; &:hover {{ color: {DANGER}; }}
         &:focus-visible {{ outline: 2px solid {FOCUS_RING}; outline-offset: 2px; }} &:disabled {{ cursor: default; }}",
        text::SECONDARY
    ));
    let icon = if deleting { spinner_icon(icon_style()) } else { x_icon() };
    rsx! {
        <div key={schedule.id} mix={css(&format!("display: grid; gap: {};", space::XS))}>
            <a aria-current={active.then_some("page")} data-active={active.then_some("true")} data-deleting={deleting.then_some("true")} href={schedule.href.clone()} mix={link}>
                <span mix={css("min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;")}>{schedule.name.clone()}</span>
                <button aria-describedby={error.as_ref().map(|_| error_id.clone())} aria-label={format!("Delete {}", schedule.name)} disabled={deleting} type="button"
                    mix={[delete_button, on("click", on_delete)]}>
                    {icon}
                </button>
            </a>
            {error.map(|e| rsx! { <small id={error_id} role="alert" mix={css(&format!("color: {DANGER}; font-size: {}; grid-column: 1 / -1;", font_size::XS))}>{e}</small> })}
        </div>
    }
}

fn icon_style() -> Mixin {
    css("display: block; height: 1rem; width: 1rem;")
}

fn x_icon() -> Node {
    rsx! {
        <svg aria-hidden="true" viewBox="0 0 16 16" mix={icon_style()}>
            <path d="M4.25 4.25 11.75 11.75M11.75 4.25 4.25 11.75" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.75"/>
        </svg>
    }
}

/// The spinning circle (`timebox-spin` comes from the document).
pub fn spinner_icon(size: Mixin) -> Node {
    rsx! {
        <svg aria-hidden="true" viewBox="0 0 16 16" fill="none" mix={[size, css("animation: timebox-spin 700ms linear infinite;")]}>
            <path d="M8 2.5a5.5 5.5 0 1 1-5.5 5.5" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"/>
        </svg>
    }
}

// ---------------------------------------------------------------------------------------
// new-schedule-action.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Creating,
    Submitting,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewScheduleActionProps {
    pub csrf_token: String,
}

pub struct NewScheduleAction {
    state: State,
    error: Option<String>,
    submitted_name: String,
    button: Option<NodeHandle>,
    input: Option<NodeHandle>,
}

const NAME_ERROR_ID: &str = "new-schedule-name-error";

impl NewScheduleAction {
    fn cancel(&mut self, h: &Handle<Self>) {
        if self.state != State::Creating {
            return;
        }
        self.error = None;
        self.submitted_name.clear();
        self.state = State::Idle;
        h.update();
        h.queue_task(|this, _| {
            if let Some(button) = &this.button {
                button.focus();
            }
        });
    }

    fn select_input(h: &Handle<Self>) {
        h.queue_task(|this, _| {
            if let Some(input) = &this.input {
                input.select();
            }
        });
    }

    fn submit(&mut self, h: &Handle<Self>, name: String, csrf_token: String) {
        let name = name.trim().to_string();
        self.submitted_name = name.clone();
        if name.is_empty() {
            self.error = Some("Name is required.".into());
            h.update();
            NewScheduleAction::select_input(h);
            return;
        }
        self.error = None;
        self.state = State::Submitting;
        h.update();
        let h = h.clone();
        h.clone().spawn(async move {
            let init = FetchInit {
                method: "POST".into(),
                headers: vec![
                    ("Accept".into(), "application/json".into()),
                    ("Content-Type".into(), "application/json".into()),
                    ("X-Csrf-Token".into(), csrf_token),
                ],
                body: Some(serde_json::json!({ "name": name }).to_string()),
                ..Default::default()
            };
            let result = at_least(500.0, browser::fetch(&ROUTES.schedules.create.href(()), init)).await;
            let failed = |message: String| {
                h.with(|this, h| {
                    this.error = Some(message);
                    this.state = State::Creating;
                    h.update();
                    NewScheduleAction::select_input(h);
                });
            };
            let response = match result {
                Ok(response) if response.ok() => response,
                Ok(response) => return failed(create_error_message(&response)),
                Err(_) => return failed("Could not create schedule. Please try again.".into()),
            };
            #[derive(Deserialize)]
            struct Created {
                schedule: Id,
            }
            #[derive(Deserialize)]
            struct Id {
                id: i64,
            }
            let Ok(created) = response.json::<Created>() else { return failed("Could not create schedule. Please try again.".into()) };
            rmx::navigate_async(&schedule_href(created.schedule.id), NavigateOptions::default()).await;
            h.with(|this, h| {
                this.state = State::Idle;
                this.error = None;
                this.submitted_name.clear();
                h.update();
            });
        });
    }
}

/// `fieldErrors.name`, else `error`, else a message for the status.
fn create_error_message(response: &FetchResponse) -> String {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ErrorBody {
        error: Option<String>,
        field_errors: Option<HashMap<String, serde_json::Value>>,
    }
    if response.content_type.contains("application/json") {
        if let Ok(body) = response.json::<ErrorBody>() {
            if let Some(name) = body.field_errors.and_then(|f| f.get("name").and_then(|v| v.as_str().map(str::to_string))) {
                return name;
            }
            if let Some(error) = body.error {
                return error;
            }
        }
    }
    match response.status {
        401 => "Please sign in before creating a schedule.",
        403 => "Could not verify this request. Please refresh and try again.",
        _ => "Could not create schedule. Please try again.",
    }
    .into()
}

impl Component for NewScheduleAction {
    type Props = NewScheduleActionProps;

    fn setup(_: &NewScheduleActionProps, _: &Handle<Self>) -> Self {
        NewScheduleAction { state: State::Idle, error: None, submitted_name: String::new(), button: None, input: None }
    }

    fn render(&self, props: &NewScheduleActionProps, h: &Handle<Self>) -> Node {
        let content = match self.state {
            State::Idle => {
                let open = h.on(|this, h| {
                    this.error = None;
                    this.submitted_name.clear();
                    this.state = State::Creating;
                    h.update();
                    h.queue_task(|this, _| {
                        if let Some(input) = &this.input {
                            input.focus();
                        }
                    });
                });
                rsx! {
                    <button type="button" mix={[theme::button(Tone::Neutral), css("height: 100%; width: 100%;"), h.node_ref(|this, node| this.button = Some(node.clone())), on("click", open)]}>
                        <svg aria-hidden="true" fill="none" viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" mix={css("flex-shrink: 0; height: 1em; width: 1em;")}>
                            <path d="M8 3.25v9.5M3.25 8h9.5" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"/>
                        </svg>
                        "New schedule"
                    </button>
                }
            }
            State::Submitting => rsx! {
                <div aria-live="polite" key="new-schedule-loading" mix={[
                    css(&format!(
                        "align-items: center; color: {}; display: flex; font-size: {}; font-weight: 500; gap: {}; height: 100%; justify-content: center; min-height: 0; padding-inline: {}; width: 100%;",
                        text::SECONDARY, font_size::MD, space::SM, space::MD)),
                    animate_entrance(Animate::spring(SpringPreset::Snappy).opacity(0.0).transform("scale(0.5)")),
                ]}>
                    {spinner_icon(css("width: 1.25em; height: 1.25em;"))}
                    <span>"Creating"</span>
                </div>
            },
            State::Creating => {
                let csrf = props.csrf_token.clone();
                let submit = h.on_event(move |this, h, event| {
                    event.prevent_default();
                    let name = event.form_value("name").unwrap_or_default();
                    this.submit(h, name, csrf.clone());
                });
                let focus_out = h.on_event(|this, h, event| {
                    if !dom_util::focus_left_current(event) {
                        return;
                    }
                    this.cancel(h);
                });
                let key_down = h.on_event(|this, h, event| {
                    if event.key() == "Escape" {
                        event.prevent_default();
                        this.cancel(h);
                    }
                });
                let clear_error = h.on(|this, h| {
                    if this.error.take().is_some() {
                        h.update();
                    }
                });
                let error_id = self.error.as_ref().map(|_| NAME_ERROR_ID);
                rsx! {
                    <form aria-describedby={error_id} autocomplete="off" key="new-schedule-form"
                        mix={[
                            css("display: block;"),
                            animate_exit(Animate::spring(SpringPreset::Snappy).opacity(0.0).transform("scale(0.5)")),
                            on("submit", submit),
                            on("focusout", focus_out),
                            on("keydown", key_down),
                        ]}>
                        <input aria-label="Schedule name" aria-describedby={error_id} aria-invalid={self.error.as_ref().map(|_| true)}
                            autocomplete="off" name="name" placeholder="Schedule name" required type="text" defaultValue={self.submitted_name.clone()}
                            mix={[
                                css(&format!(
                                    "border: 1px solid {}; border-radius: {RADIUS}; box-sizing: border-box; color: {}; font: inherit; font-size: {}; min-height: 28px; padding: 0 {}; width: 100%;
                                     &:focus {{ border-color: {FOCUS_RING}; outline: 2px solid {FOCUS_RING}; outline-offset: 2px; }}",
                                    border::DEFAULT, text::PRIMARY, font_size::XS, space::MD)),
                                h.node_ref(|this, node| this.input = Some(node.clone())),
                                on("input", clear_error),
                            ]}/>
                        {self.error.clone().map(|e| rsx! {
                            <small id={NAME_ERROR_ID} role="alert" mix={css(&format!("color: {DANGER}; display: block; font-size: {}; margin-top: {};", font_size::XS, space::XS))}>{e}</small>
                        })}
                    </form>
                }
            }
        };
        rsx! { <div mix={css("display: grid; height: 100%; min-height: 0; & > * { grid-area: 1 / 1; min-height: 0; }")}>{content}</div> }
    }
}

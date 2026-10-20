//! Components for the frame-navigation demo (an LMS with named frames for courses, the
//! account editor and settings), shared by the server and the browser.

use rmx::{css, link, on, rsx, Children, Component, EventHandler, Handle, Mixin, Node};
use serde::{Deserialize, Serialize};

pub const COURSES_FRAME: &str = "courses";
pub const ACCOUNT_FRAME: &str = "account";
pub const SETTINGS_FRAME: &str = "settings";

// ---------------------------------------------------------------------------------------
// Layout (ui/layout.tsx, ui/nav-link.tsx)
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MainNav {
    #[default]
    Dashboard,
    Courses,
    Calendar,
    Account,
    Settings,
}

#[derive(Clone, Default)]
pub struct LayoutProps {
    pub title: String,
    pub active_nav: Option<MainNav>,
    pub children: Children,
}

pub struct Layout;

const NAV_ITEMS: [(MainNav, &str, &str); 5] = [
    (MainNav::Dashboard, "Dashboard", "/"),
    (MainNav::Courses, "Courses", "/courses"),
    (MainNav::Calendar, "Calendar", "/calendar"),
    (MainNav::Account, "Account", "/account"),
    (MainNav::Settings, "Settings", "/settings"),
];

pub fn head(title: &str) -> Node {
    rsx! {
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1"/>
            <title>{title.to_string()}</title>
            <script type="module" async src="/boot.js"></script>
        </head>
    }
}

fn nav_link_style() -> Mixin {
    css("display: grid; gap: 0.35rem; margin-top: 0.5rem; & a { text-decoration: none; border-radius: 10px; padding: 0.55rem 0.75rem; font-size: 0.95rem; color: #334155; font-weight: 500; } & a[aria-current=\"page\"] { color: #0f172a; background-color: #e2e8f0; font-weight: 600; }")
}

impl Component for Layout {
    type Props = LayoutProps;

    fn setup(_: &LayoutProps, _: &Handle<Self>) -> Self {
        Layout
    }

    fn render(&self, props: &LayoutProps, _: &Handle<Self>) -> Node {
        let nav: Vec<Node> = NAV_ITEMS
            .iter()
            .map(|(id, label, href)| {
                let current = (props.active_nav == Some(*id)).then_some("page");
                rsx! { <a href={*href} aria-current={current}>{*label}</a> }
            })
            .collect();
        rsx! {
            <html lang="en">
                {head(&format!("{} | LMS", props.title))}
                <body mix={css("margin: 0; min-height: 100vh; font-family: Inter, ui-sans-serif, system-ui, sans-serif; color: #0f172a; background-color: #f8fafc;")}>
                    <div mix={css("height: 100vh; display: grid; grid-template-columns: 280px minmax(0, 1fr);")}>
                        <aside mix={css("position: sticky; top: 0; align-self: start; height: 100vh; box-sizing: border-box; border-right: 1px solid #e2e8f0; background-color: #ffffff; padding: 1.25rem 1rem; display: flex; flex-direction: column; gap: 0.75rem;")}>
                            <a href="/" mix={css("font-weight: 700; color: #0f172a; text-decoration: none; font-size: 1.1rem;")}>"Atlas LMS"</a>
                            <p mix={css("margin: 0; color: #64748b; font-size: 0.9rem;")}>"Student workspace"</p>
                            <nav id="main-nav" mix={nav_link_style()}>{nav}</nav>
                            <form method="POST" action="/auth/logout" mix={css("margin-top: auto; padding-top: 1rem;")}>
                                <button type="submit" id="logout" mix={css("width: 100%; border: none; border-radius: 10px; padding: 0.65rem 0.75rem; font-weight: 600; cursor: pointer; background-color: #e2e8f0; color: #0f172a;")}>"Logout"</button>
                            </form>
                        </aside>
                        <main mix={css("padding: 1.5rem 2rem 3rem;")}>
                            <header mix={css("margin-bottom: 1.25rem; padding-bottom: 0.75rem; border-bottom: 1px solid #e2e8f0;")}>
                                <h1 id="page-title" mix={css("margin: 0; font-size: 1.25rem;")}>{props.title.clone()}</h1>
                            </header>
                            {&props.children}
                        </main>
                    </div>
                </body>
            </html>
        }
    }
}

pub fn title_style() -> Mixin {
    css("margin-top: 0; font-size: 1.8rem; color: #0f172a;")
}

pub fn description_style() -> Mixin {
    css("margin-top: 0.5rem; color: #475569; line-height: 1.7; max-width: 65ch;")
}

// ---------------------------------------------------------------------------------------
// Client entries
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StatCard {
    pub label: String,
    pub value: String,
    pub href: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StatGridProps {
    pub cards: Vec<StatCard>,
}

/// `main/public/dashboard-stat-grid.tsx`: cards that act as links via the `link` mixin.
pub struct DashboardStatGrid;

impl Component for DashboardStatGrid {
    type Props = StatGridProps;
    rmx::client_entry!("DashboardStatGrid");

    fn setup(_: &StatGridProps, _: &Handle<Self>) -> Self {
        DashboardStatGrid
    }

    fn render(&self, props: &StatGridProps, _: &Handle<Self>) -> Node {
        let cards: Vec<Node> = props
            .cards
            .iter()
            .map(|card| {
                rsx! {
                    <article class="stat-card" mix={[
                        css("border: 1px solid #e2e8f0; border-radius: 12px; padding: 1rem; background-color: #ffffff; cursor: pointer; transition: border-color 120ms ease, transform 120ms ease; &:hover { border-color: #94a3b8; transform: translateY(-1px); } &:focus-visible { outline: 2px solid #2563eb; outline-offset: 2px; }"),
                        link(card.href.clone()),
                    ]}>
                        <p mix={css("margin: 0; font-size: 0.875rem; color: #64748b;")}>{card.label.clone()}</p>
                        <p mix={css("margin: 0.3rem 0 0; font-size: 1.5rem; font-weight: 700;")}>{card.value.clone()}</p>
                    </article>
                }
            })
            .collect();
        rsx! { <div mix={css("margin-top: 1.25rem; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 0.75rem;")}>{cards}</div> }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CourseFilterProps {
    pub query: String,
}

/// `main/public/course-filter-input.tsx`: submits its form on every keystroke.
pub struct CourseFilterInput;

impl Component for CourseFilterInput {
    type Props = CourseFilterProps;
    rmx::client_entry!("CourseFilterInput");

    fn setup(_: &CourseFilterProps, _: &Handle<Self>) -> Self {
        CourseFilterInput
    }

    fn render(&self, props: &CourseFilterProps, _: &Handle<Self>) -> Node {
        rsx! {
            <input id="course-filter" name="q" type="search" value={props.query.clone()} placeholder="Search by course name"
                mix={[
                    css("flex: 1 1 16rem; min-width: 0; border: 1px solid #cbd5e1; border-radius: 8px; padding: 0.65rem 0.75rem; font: inherit; &:focus { border-color: #2563eb; outline: 2px solid #bfdbfe; outline-offset: 1px; }"),
                    on("input", EventHandler::new(|e| e.request_submit())),
                ]}/>
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod entry;

/// Browser entry.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    entry::start();
}

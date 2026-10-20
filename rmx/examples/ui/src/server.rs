use std::sync::LazyLock;

use http::StatusCode;
use rmx::{css, rsx, Children, Component, Handle, Mixin, Node};
use rmx_fetch::response;
use rmx_middleware::logger::logger;
use rmx_middleware::static_files::{static_files, StaticOptions};
use rmx_render::{Render, ResponseInit};
use rmx_router::routes::{get, Route};
use rmx_router::{Context, Router};
use ui_demo::demos::*;
use ui_demo::{find_demo, Demo, DEMOS};

pub fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44105".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "examples/ui/public".into());
    let addr = format!("{host}:{port}");
    println!("ui demos on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx_serve::listen(&addr, router(&public))).expect("serve");
}

// config/routes.ts
pub struct Routes {
    pub index: Route,
    pub show: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { index: get("/"), show: get("/demo/*filename") });

pub fn router(public: &str) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: false, last_modified: false, index: false, ..Default::default() },
    ));
    router.route(&ROUTES.index, |ctx: Context| async move {
        ctx.render_with(rsx! { <DemoIndexDocument/> }, no_store())
    });
    router.route(&ROUTES.show, |ctx: Context| async move {
        match ctx.param("filename").and_then(find_demo) {
            Some(demo) => ctx.render_with(Node::component::<DemoDocument>(None, DemoDocumentProps { slug: demo.slug }), no_store()),
            None => response::status(StatusCode::NOT_FOUND, "Demo not found"),
        }
    });
    router
}

fn no_store() -> ResponseInit {
    let mut init = ResponseInit::default();
    init.headers.insert(http::header::CACHE_CONTROL, http::HeaderValue::from_static("no-store"));
    init
}

/// The demo component for a slug.
fn demo_node(slug: &str) -> Node {
    match slug {
        "component/runtime/basic" => rsx! { <basic::BasicCounter/> },
        "component/runtime/keyed-list" => rsx! { <keyed_list::KeyedList/> },
        "component/runtime/controlled-uncontrolled-values" => rsx! { <controlled::ControlledValues/> },
        "component/runtime/mixin-lifecycle" => rsx! { <lifecycle::MixinLifecycle/> },
        "component/runtime/readme" => rsx! { <readme::ReadmeExamples/> },
        "cases/draggable" => rsx! { <draggable::DraggableDemo/> },
        "ui/animation" => rsx! { <animation::AnimationGallery/> },
        "ui/accordion" => rsx! { <accordion::AccordionDemo/> },
        "ui/anchor" => rsx! { <anchor::AnchorDemo/> },
        "ui/popover" => rsx! { <popover::PopoverDemo/> },
        "ui/tabs" => rsx! { <tabs::TabsDemo/> },
        "ui/toggle" => rsx! { <toggle::ToggleDemo/> },
        _ => Node::Empty,
    }
}

// app/demo-runner/view.tsx

pub struct DemoIndexDocument;

impl Component for DemoIndexDocument {
    type Props = ();

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        DemoIndexDocument
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let sections: Vec<Node> = sections()
            .into_iter()
            .map(|(title, description, groups)| {
                let groups: Vec<Node> = groups
                    .into_iter()
                    .map(|(module, demos)| {
                        let items: Vec<Node> = demos
                            .iter()
                            .map(|d| rsx! { <li key={d.slug} mix={demo_item_css()}><a href={format!("/demo/{}", d.slug)} mix={demo_link_css()}>{d.title}</a></li> })
                            .collect();
                        rsx! {
                            <section key={module} mix={demo_group_css()}>
                                <h3 mix={demo_group_title_css()}>{module}</h3>
                                <ol mix={demo_list_css()}>{items}</ol>
                            </section>
                        }
                    })
                    .collect();
                rsx! {
                    <section key={title} mix={demo_section_css()}>
                        <header mix={demo_section_header_css()}>
                            <h2 mix={demo_section_title_css()}>{title}</h2>
                            <p mix={demo_section_description_css()}>{description}</p>
                        </header>
                        <div mix={demo_groups_css()}>{groups}</div>
                    </section>
                }
            })
            .collect();
        rsx! {
            <html>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>"rmx UI Demos"</title>
                </head>
                <body mix={body_css()}>
                    <main mix={index_shell_css()}>
                        <header mix={index_header_css()}>
                            <p mix={eyebrow_css()}>"rmx interface"</p>
                            <h1 mix={title_css()}>"Demos"</h1>
                            <p mix={description_css()}>{format!("{} demos across the component runtime and mixins.", DEMOS.len())}</p>
                        </header>
                        <div mix={demo_sections_css()}>{sections}</div>
                    </main>
                </body>
            </html>
        }
    }
}

type Group = (&'static str, Vec<&'static Demo>);

fn sections() -> Vec<(&'static str, &'static str, Vec<Group>)> {
    [
        ("Component runtime", "Rendering, interaction, and mixin demos for rmx."),
        ("UI", "Demos for @remix-run/ui, ported to rmx-ui."),
        ("Cases", "Standalone cases from the Remix ui demos."),
    ]
    .into_iter()
    .map(|(title, description)| {
        let mut groups: Vec<Group> = Vec::new();
        for demo in DEMOS.iter().filter(|d| d.section == title) {
            match groups.iter_mut().find(|(m, _)| *m == demo.module) {
                Some((_, list)) => list.push(demo),
                None => groups.push((demo.module, vec![demo])),
            }
        }
        groups.sort_by_key(|(m, _)| *m);
        (title, description, groups)
    })
    .filter(|(_, _, groups)| !groups.is_empty())
    .collect()
}

#[derive(Clone, Default)]
pub struct DemoDocumentProps {
    pub slug: &'static str,
}

pub struct DemoDocument;

impl Component for DemoDocument {
    type Props = DemoDocumentProps;

    fn setup(_: &DemoDocumentProps, _: &Handle<Self>) -> Self {
        DemoDocument
    }

    fn render(&self, props: &DemoDocumentProps, _: &Handle<Self>) -> Node {
        let Some(demo) = find_demo(props.slug) else { return Node::Empty };
        let stage: Vec<Mixin> = if demo.center { vec![demo_stage_css(), center_demo_stage_css()] } else { vec![demo_stage_css()] };
        let content = demo_node(demo.slug);
        let content = if demo.center { rsx! { <div mix={center_demo_canvas_css()}>{content}</div> } } else { content };
        rsx! {
            <html>
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <script type="module" async src="/boot.js"></script>
                    <title>{format!("{} | rmx Interface Demo", demo.title)}</title>
                </head>
                <body mix={body_css()}>
                    <main mix={stage}>{Children::from(content)}</main>
                </body>
            </html>
        }
    }
}

// app/demo-runner/design.ts as CSS text.
fn body_css() -> Mixin {
    css("margin: 0; min-height: 100vh; color: #151515; background-color: #ffffff; font-family: \"Inter\", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif;")
}
fn index_shell_css() -> Mixin {
    css("width: min(100% - 48px, 1040px); margin-inline: auto; padding-block: 32px;")
}
fn index_header_css() -> Mixin {
    css("display: grid; gap: 4px; margin-bottom: 32px;")
}
fn eyebrow_css() -> Mixin {
    css("margin: 0; color: #6d6d6d; font-size: 12px; font-weight: 500; text-transform: uppercase;")
}
fn title_css() -> Mixin {
    css("margin: 0; font-size: 28px; line-height: 1.25;")
}
fn description_css() -> Mixin {
    css("margin: 0; max-width: 44rem; color: #4f4f4f; font-size: 13px; line-height: 1.45;")
}
fn demo_sections_css() -> Mixin {
    css("display: grid; gap: 40px;")
}
fn demo_section_css() -> Mixin {
    css("display: grid; gap: 16px;")
}
fn demo_section_header_css() -> Mixin {
    css("display: grid; gap: 4px;")
}
fn demo_section_title_css() -> Mixin {
    css("margin: 0; font-size: 24px; font-weight: 600; line-height: 1.25;")
}
fn demo_section_description_css() -> Mixin {
    css("margin: 0; color: #4f4f4f; font-size: 13px; line-height: 1.45;")
}
fn demo_groups_css() -> Mixin {
    css("display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); align-items: start; column-gap: 32px; row-gap: 32px;")
}
fn demo_group_css() -> Mixin {
    css("display: grid; gap: 4px; min-width: 0;")
}
fn demo_group_title_css() -> Mixin {
    css("margin: 0; color: #151515; font-size: 13px; font-weight: 600; line-height: 1.25;")
}
fn demo_list_css() -> Mixin {
    css("display: grid; gap: 2px; padding: 0; margin: 0; list-style: none;")
}
fn demo_item_css() -> Mixin {
    css("display: flex; min-width: 0;")
}
fn demo_link_css() -> Mixin {
    css("display: block; width: 100%; padding-block: 1px; color: #4f4f4f; font-size: 12px; font-weight: 400; line-height: 1.45; text-decoration: none; overflow-wrap: anywhere; &:hover { color: #1A72FF; }")
}
fn demo_stage_css() -> Mixin {
    css("width: 100%; min-height: 100vh; min-width: 0; box-sizing: border-box;")
}
fn center_demo_stage_css() -> Mixin {
    css("display: grid; place-items: center; padding: 32px; background-color: color-mix(in oklab, rgb(246 246 246) 72%, white);")
}
fn center_demo_canvas_css() -> Mixin {
    css("width: fit-content; max-width: min(100%, 44rem); min-height: 18rem; display: grid; place-items: center;")
}

#[cfg(test)]
mod tests;

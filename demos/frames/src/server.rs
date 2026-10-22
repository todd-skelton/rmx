use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frames_demo::scroll_reload::*;
use frames_demo::*;
use http::StatusCode;
use rmx::{css, rsx, Frame, Node};
use rmx::fetch::response;
use rmx::middleware::compression::compression;
use rmx::middleware::form_data::{form_data, FormDataExt};
use rmx::middleware::logger::logger;
use rmx::middleware::static_files::{static_files, ETag, StaticOptions};
use rmx::render::Render;
use frames_demo::routes::{FramesActions, RoutesActions, ROUTES};
use rmx::router::{action, Context, Router};

pub fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44110".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "demos/frames/public".into());
    let router = router(&public);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let addr = format!("{host}:{port}");
    println!("frames demo on http://{addr}");
    rt.block_on(rmx::serve::listen(&addr, router)).expect("serve");
}

pub fn router(public: &str) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(compression());
    router.use_middleware(form_data());
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: ETag::Off, last_modified: false, index: vec![], ..Default::default() },
    ));

    router.map(
        &*ROUTES,
        RoutesActions {
            home: action(|ctx: Context| async move { ctx.render(home_page()) }),
            time: action(|ctx: Context| async move { ctx.render(time_page()) }),
            reload_scope: action(|ctx: Context| async move { ctx.render(reload_scope_page()) }),
            state_search: action(|ctx: Context| async move {
                let query = ctx.url().search_params().get("query").unwrap_or_default().to_string();
                ctx.render(state_search_page(query))
            }),
            client_mounted: action(|ctx: Context| async move { ctx.render(client_mounted_page()) }),
            root_reload_client_entries: action(|ctx: Context| async move {
                delay(1000).await;
                let include_removed = ctx.url().search_params().get("removed") != Some("0");
                ctx.render(root_reload_page(include_removed, clock()))
            }),
            scroll_anchoring: action(|ctx: Context| async move { ctx.render(scroll_anchoring_page()) }),
            scroll_anchoring_detail: action(|ctx: Context| async move { ctx.render(scroll_anchoring_detail_page()) }),
            scroll_restoration: action(|ctx: Context| async move {
                let history = newsletter_history(ctx.url().search_params().get("newsletter"));
                ctx.render(scroll_restoration_page(history))
            }),
            scroll_restoration_detail: action(|ctx: Context| async move { ctx.render(scroll_restoration_detail_page()) }),
            newsletter_signup: action(|ctx: Context| async move {
                let form = ctx.form_data();
                let email = form.get("email").unwrap_or_default();
                let history = newsletter_history(form.get("history"));
                match history {
                    Some(history) if !email.trim().is_empty() => {
                        response::redirect_with(&format!("/scroll-restoration?newsletter={history}"), StatusCode::SEE_OTHER)
                    }
                    _ => response::status(StatusCode::BAD_REQUEST, "Enter an email address and choose a navigation type."),
                }
            }),
        },
    );

    // actions/frames/controller.tsx
    router.map(
        &ROUTES.frames,
        FramesActions {
            sidebar: action(|ctx: Context| async move {
                delay(400).await;
                ctx.render(rsx! {
                    <div>
                        <p mix={lead_style()}>"This content is rendered by " <code>"/frames/sidebar"</code> "."</p>
                        <ul mix={list_style()}>
                            <li>"Streams in after initial HTML"</li>
                            <li>"Can contain client entries"</li>
                            <li>"Can nest frames"</li>
                        </ul>
                    </div>
                })
            }),
            activity: action(|ctx: Context| async move {
                delay(2000).await;
                ctx.render(rsx! {
                    <div>
                        <p mix={lead_style()}>"Rendered by " <code>"/frames/activity"</code> " at " <time>{clock()}</time> "."</p>
                        <Frame src="/frames/activity/detail" fallback={rsx! { <div mix={muted_style()}>"Loading detail\u{2026}"</div> }}/>
                    </div>
                })
            }),
            activity_detail: action(|ctx: Context| async move {
                delay(600).await;
                ctx.render(rsx! {
                    <div>
                        <p mix={[muted_style(), css("margin-top: 0; margin-bottom: 8px;")]}>"Nested frame with a hydrated counter:"</p>
                        <div mix={css("margin-top: 12px;")}>
                            <Frame src="/frames/time" fallback={rsx! { <div mix={muted_style()}>"Loading server time\u{2026}"</div> }}/>
                        </div>
                    </div>
                })
            }),
            client_frame_example: action(|ctx: Context| async move {
                delay(500).await;
                ctx.render(rsx! {
                    <div mix={fragment_style()}>
                        <div mix={fragment_label_style()}>"Server fragment from /frames/client-frame-example"</div>
                        <div mix={clock_value_style()}>{clock()}</div>
                        <div mix={css("margin-top: 8px;")}><Counter initial_count={5} label="Inside mounted frame"/></div>
                        <div mix={css("margin-top: 10px;")}>
                            <div mix={nested_frame_label_style()}>"Nested frame:"</div>
                            <Frame src="/frames/client-frame-example/nested" fallback={rsx! { <div mix={muted_style()}>"Loading nested frame\u{2026}"</div> }}/>
                        </div>
                    </div>
                })
            }),
            client_frame_example_nested: action(|ctx: Context| async move {
                delay(350).await;
                ctx.render(rsx! {
                    <div mix={nested_fragment_style()}>
                        <div mix={fragment_label_style()}>"Nested server fragment"</div>
                        <div mix={css("margin-top: 6px;")}><Counter initial_count={1} label="Nested frame counter"/></div>
                    </div>
                })
            }),
            client_mounted_outer: action(|ctx: Context| async move {
                delay(350).await;
                ctx.render(rsx! {
                    <div mix={fragment_style()}>
                        <div mix={fragment_label_style()}>"Outer server fragment from /frames/client-mounted-outer"</div>
                        <div mix={css("margin-top: 8px;")}><Counter initial_count={2} label="Outer frame counter"/></div>
                        <div mix={css("margin-top: 10px;")}>
                            <div mix={nested_frame_label_style()}>"Nested non-blocking frame:"</div>
                            <Frame src="/frames/client-mounted-nested" fallback={rsx! { <div mix={muted_style()}>"Loading nested non-blocking frame\u{2026}"</div> }}/>
                        </div>
                    </div>
                })
            }),
            client_mounted_nested: action(|ctx: Context| async move {
                delay(2200).await;
                ctx.render(rsx! {
                    <div mix={nested_fragment_style()}>
                        <div mix={fragment_label_style()}>"Nested server fragment"</div>
                        <div mix={clock_value_style()}>{clock()}</div>
                        <div mix={css("margin-top: 6px;")}><Counter initial_count={3} label="Nested frame counter"/></div>
                    </div>
                })
            }),
            time: action(|ctx: Context| async move {
                delay(1200).await;
                ctx.render(rsx! {
                    <div>
                        <div mix={css("display: flex; align-items: center; justify-content: space-between; gap: 12px;")}>
                            <div>
                                <div mix={clock_label_style()}>"Server time"</div>
                                <div class="server-time" mix={large_clock_value_style()}>{clock()}</div>
                            </div>
                            <Counter initial_count={0} label="In a frame"/>
                            <div mix={css("display: flex; flex-direction: column; gap: 12px;")}><ReloadTime/></div>
                        </div>
                    </div>
                })
            }),
            reload_scope: action(|ctx: Context| async move {
                delay(700).await;
                ctx.render(reload_scope_frame("Non-blocking frame server time"))
            }),
            reload_scope_blocking: action(|ctx: Context| async move {
                delay(500).await;
                ctx.render(reload_scope_frame("Blocking frame server time"))
            }),
            root_reload_entry_frame: action(|ctx: Context| async move {
                delay(700).await;
                ctx.render(rsx! {
                    <div class="entry-frame" mix={fragment_style()}>
                        <div mix={fragment_label_style()}>"Frame inside preserved client entry"</div>
                        <div mix={clock_value_style()}>{clock()}</div>
                        <div mix={css("margin-top: 8px;")}><Counter initial_count={10} label="Nested frame"/></div>
                    </div>
                })
            }),
            scroll_anchoring_items: action(|ctx: Context| async move { ctx.render(rsx! { <ScrollAnchoringList loaded_at={clock()}/> }) }),
            scroll_restoration_items: action(|ctx: Context| async move { ctx.render(rsx! { <ScrollRestorationList loaded_at={clock()}/> }) }),
            state_search_results: action(|ctx: Context| async move {
                delay(300).await;
                let query = ctx.url().search_params().get("query").unwrap_or_default().trim().to_string();
                let matches = search_united_states(&query);
                let heading = if query.is_empty() {
                    format!("Showing all states ({})", matches.len())
                } else {
                    format!("Results for \"{query}\" ({})", matches.len())
                };
                let list = if matches.is_empty() {
                    rsx! { <p mix={[muted_style(), css("margin: 0;")]}>"No states matched that query."</p> }
                } else {
                    rsx! {
                        <ul class="state-results" mix={[list_style(), css("display: grid; gap: 4px;")]}>
                            {matches.iter().map(|s| rsx! { <li key={*s}>{*s}</li> }).collect::<Vec<_>>()}
                        </ul>
                    }
                };
                ctx.render(rsx! { <div><p mix={[lead_style(), css("margin-bottom: 10px;")]}>{heading}</p>{list}</div> })
            }),
        },
    );
    router
}

async fn delay(ms: u64) {
    let ms = if std::env::var("RMX_FAST").is_ok() { 10 } else { ms };
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// The current UTC time of day, `HH:MM:SS`.
fn clock() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    format!("{:02}:{:02}:{:02} UTC", secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

fn back_link() -> Node {
    rsx! { <a href="/" mix={link_style()}>"\u{2190} Back"</a> }
}

fn home_page() -> Node {
    let links: Vec<(&str, &str)> = vec![
        ("/time", "Server time demo"),
        ("/reload-scope", "Frame vs top reload demo"),
        ("/root-reload-client-entries", "Root reload client entries"),
        ("/scroll-restoration", "Navigation scroll behavior"),
        ("/scroll-anchoring", "Scroll anchoring restoration"),
        ("/state-search", "Dynamic src search demo"),
        ("/client-mounted", "Client-mounted nested frame demo"),
    ];
    let mut nav = Vec::new();
    for (i, (href, label)) in links.into_iter().enumerate() {
        if i > 0 {
            nav.push(rsx! { " \u{b7} " });
        }
        nav.push(rsx! { <a href={href} mix={link_style()}>{label}</a> });
    }
    rsx! {
        <Document title="Frames + rmx-router demo" max_width="980px">
            <h1 mix={css("margin: 0; letter-spacing: -0.02em;")}>"Full-stack Frames"</h1>
            <p mix={css("margin-top: 8px; color: #b9c6ff;")}>
                "Server routes are handled by " <code>"rmx-router"</code> "; UI is streamed with "
                <code>"rmx"</code> " Frames and client entries."
            </p>
            <p mix={css("margin-top: 0; margin-bottom: 16px;")}>{nav}</p>
            <div mix={css("display: grid; grid-template-columns: 320px 1fr; gap: 16px; align-items: start; margin-top: 24px;")}>
                <aside mix={panel_style()}>
                    <h2 mix={section_heading_style()}>"Sidebar (Frame)"</h2>
                    <Frame src="/frames/sidebar" fallback={rsx! { <div mix={muted_style()}>"Loading sidebar\u{2026}"</div> }}/>
                </aside>
                <main mix={panel_style()}>
                    <h2 mix={section_heading_style()}>"Main"</h2>
                    <p mix={lead_style()}>"The counter below is a client entry."</p>
                    <Counter initial_count={0} label="Clicks"/>
                    <ClientFrameExample/>
                    <div mix={css("height: 16px;")}></div>
                    <h3 mix={css("margin: 0; font-size: 14px; color: #cfd8ff;")}>"Activity (Frame)"</h3>
                    <Frame src="/frames/activity" fallback={rsx! { <div mix={muted_style()}>"Loading activity\u{2026}"</div> }}/>
                </main>
            </div>
        </Document>
    }
}

fn time_page() -> Node {
    rsx! {
        <Document title="Server time" max_width="720px">
            {back_link()}
            <h1 mix={page_heading_style()}>"Server time (Frame reload)"</h1>
            <p mix={lead_style()}>
                "The frame below renders the current server time. Click \u{201c}Refresh\u{201d} to call "
                <code>"frame.reload()"</code> " from a client entry inside the frame."
            </p>
            <div mix={panel_style()}>
                <Frame src="/frames/time" fallback={rsx! { <div mix={muted_style()}>"Loading server time\u{2026}"</div> }}/>
            </div>
        </Document>
    }
}

fn reload_scope_page() -> Node {
    rsx! {
        <Document title="Frame vs top reload">
            {back_link()}
            <h1 mix={page_heading_style()}>"Frame reload vs top reload"</h1>
            <p mix={lead_style()}>
                "Compare blocking and non-blocking child frames while reloading either a child frame or the entire runtime tree."
            </p>
            <div mix={[panel_style(), css("margin-bottom: 16px;")]}>
                <div mix={[clock_label_style(), css("margin-bottom: 8px;")]}>"Root client entry state"</div>
                <div mix={css("display: flex; gap: 12px; align-items: center; flex-wrap: wrap;")}>
                    <Counter initial_count={0} label="Root"/>
                    <ReloadTopFrame/>
                </div>
            </div>
            <div mix={css("margin-bottom: 10px;")}>
                <div mix={clock_label_style()}>"Page server time"</div>
                <div id="page-time" mix={css("font-size: 20px; font-variant-numeric: tabular-nums;")}>{clock()}</div>
            </div>
            <div mix={css("display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px;")}>
                <section id="non-blocking" mix={panel_style()}>
                    <h2 mix={section_heading_style()}>"Non-blocking child frame"</h2>
                    <Frame name="reload-scope-non-blocking" src="/frames/reload-scope"
                        fallback={rsx! { <div mix={muted_style()}>"Loading non-blocking frame\u{2026}"</div> }}/>
                </section>
                <section id="blocking" mix={panel_style()}>
                    <h2 mix={section_heading_style()}>"Blocking child frame"</h2>
                    <Frame name="reload-scope-blocking" src="/frames/reload-scope/blocking"/>
                </section>
            </div>
        </Document>
    }
}

fn reload_scope_frame(label: &'static str) -> Node {
    rsx! {
        <div>
            <div mix={clock_label_style()}>{label}</div>
            <div class="frame-time" mix={[large_clock_value_style(), css("margin-bottom: 10px;")]}>{clock()}</div>
            <ReloadScope/>
        </div>
    }
}

fn state_search_page(initial_query: String) -> Node {
    rsx! {
        <Document title="Dynamic Frame src search">
            {back_link()}
            <h1 mix={page_heading_style()}>"Dynamic " <code>"<Frame src>"</code> " state search"</h1>
            <p mix={lead_style()}>"Submit the form to update the frame " <code>"src"</code> " query params and fetch matching U.S. states."</p>
            <StateSearchPage initial_query={initial_query}/>
        </Document>
    }
}

fn client_mounted_page() -> Node {
    rsx! {
        <Document title="Client-mounted nested frame">
            {back_link()}
            <h1 mix={page_heading_style()}>"Client-mounted nested non-blocking frame"</h1>
            <p mix={lead_style()}>"Mount the outer frame, then watch the nested frame fallback render before its server content streams in."</p>
            <ClientMountedPageExample/>
        </Document>
    }
}

fn newsletter_history(value: Option<&str>) -> Option<String> {
    value.filter(|v| *v == "push" || *v == "replace").map(str::to_string)
}

fn list_end(id: &'static str, heading: &'static str, intro: Option<Node>, link_id: Option<&'static str>, href: &'static str, label: &'static str) -> Node {
    rsx! {
        <section id={id} mix={css("border: 1px solid rgba(255,255,255,0.16); border-radius: 12px; padding: 20px; margin-top: 20px; background: rgba(129,140,248,0.12);")}>
            <h2 mix={css("margin-top: 0; font-size: 20px;")}>{heading}</h2>
            {intro}
            <a id={link_id} href={href} mix={css("color: #ffffff; font-weight: 700; text-decoration: underline;")}>{label}</a>
        </section>
    }
}

// actions/scroll-restoration.tsx
fn scroll_restoration_page(newsletter_history: Option<String>) -> Node {
    let intro = rsx! {
        <p mix={[lead_style(), css("line-height: 1.6;")]}>
            "Note " <code>"window.scrollY"</code> ", open the detail page, and return with the browser Back button."
        </p>
    };
    rsx! {
        <Document title="Navigation scroll behavior" max_width="860px">
            <a href="/" mix={link_style()}>"\u{2190} Back to demos"</a>
            <h1 mix={page_heading_style()}>"Navigation scroll behavior"</h1>
            <p mix={[lead_style(), css("line-height: 1.6;")]}>
                "This mirrors the store issue: a top-level client entry switches between a short detail and a tall collection rendered by a blocking frame."
            </p>
            <StoreScrollReproduction variant="list"/>
            {list_end("scroll-restoration-list-end", "End of the list", Some(intro), Some("scroll-restoration-detail-link"), "/scroll-restoration/detail", "Open the shorter detail page \u{2192}")}
            <NewsletterSignup history={newsletter_history}/>
        </Document>
    }
}

fn scroll_restoration_detail_page() -> Node {
    rsx! {
        <Document title="Navigation scroll behavior" max_width="860px">
            <a href="/" mix={link_style()}>"\u{2190} Back to demos"</a>
            <h1 mix={page_heading_style()}>"Navigation scroll behavior"</h1>
            <p mix={[lead_style(), css("line-height: 1.6;")]}>
                "This mirrors the store issue: a top-level client entry switches between a short detail and a tall collection rendered by a blocking frame."
            </p>
            <StoreScrollReproduction variant="detail"/>
        </Document>
    }
}

// actions/scroll-anchoring.tsx
fn scroll_anchoring_page() -> Node {
    rsx! {
        <Document title="Scroll anchoring restoration" max_width="860px">
            <a href="/" mix={link_style()}>"\u{2190} Back to demos"</a>
            <h1 mix={page_heading_style()}>"Scroll anchoring restoration"</h1>
            <p mix={[lead_style(), css("line-height: 1.6;")]}>
                "This isolates the case where reconciliation inserts content before a preserved client entry and browser scroll anchoring interferes with traversal restoration."
            </p>
            <ScrollAnchoringReproduction variant="list"/>
            {list_end("scroll-anchoring-list-end", "End of the anchoring list", None, None, "/scroll-anchoring/detail", "Open the anchoring detail page \u{2192}")}
        </Document>
    }
}

fn scroll_anchoring_detail_page() -> Node {
    rsx! {
        <Document title="Scroll anchoring restoration" max_width="860px">
            <a href="/" mix={link_style()}>"\u{2190} Back to demos"</a>
            <h1 mix={page_heading_style()}>"Scroll anchoring restoration"</h1>
            <p mix={[lead_style(), css("line-height: 1.6;")]}>"The detail-only client entry below precedes the preserved collection entry."</p>
            <ScrollAnchoringDetail/>
            <ScrollAnchoringReproduction variant="detail"/>
        </Document>
    }
}

// actions/root-reload-client-entries.tsx
fn root_reload_page(include_removed: bool, server_version: String) -> Node {
    let removable = if include_removed {
        rsx! { <RemovableRootReloadEntry server_version={server_version.clone()}/> }
    } else {
        rsx! {
            <section mix={css("border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; padding: 16px; background: rgba(255,255,255,0.03);")}>
                <h2 mix={section_heading_style()}>"Removed entry slot"</h2>
                <p id="removed-entry-replacement" mix={[muted_style(), css("margin-bottom: 0;")]}>"The removable client entry is absent from this server response."</p>
            </section>
        }
    };
    rsx! {
        <Document title="Root reload client entries" max_width="860px">
            {back_link()}
            <h1 mix={page_heading_style()}>"Root reload client entries"</h1>
            <p mix={lead_style()}>
                "Use the buttons below to reload the root document frame with new server props, either keeping all client entries or removing one entry from the next HTML payload."
            </p>
            <RootReloadControls include_removed={include_removed} server_version={server_version.clone()}
                with_removed_href="/root-reload-client-entries?removed=1" without_removed_href="/root-reload-client-entries?removed=0"/>
            <div mix={css("display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 16px; margin-top: 20px;")}>
                <PersistentRootReloadEntry server_version={server_version}/>
                {removable}
            </div>
        </Document>
    }
}

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frames_demo::*;
use rmx::{css, rsx, Frame, Node};
use rmx_middleware::compression::compression;
use rmx_middleware::logger::logger;
use rmx_middleware::static_files::{static_files, StaticOptions};
use rmx_render::Render;
use rmx_router::{Context, Router};

pub fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44110".into());
    let public = std::env::var("PUBLIC_DIR").unwrap_or_else(|_| "examples/frames/public".into());
    let router = router(&public);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let addr = format!("{host}:{port}");
    println!("frames demo on http://{addr}");
    rt.block_on(rmx_serve::listen(&addr, router)).expect("serve");
}

pub fn router(public: &str) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }
    router.use_middleware(compression());
    router.use_middleware(static_files(
        public,
        StaticOptions { cache_control: Some("no-store".into()), etag: false, last_modified: false, index: false, ..Default::default() },
    ));

    router.get("/", |ctx: Context| async move { ctx.render(home_page()) });
    router.get("/time", |ctx: Context| async move { ctx.render(time_page()) });
    router.get("/reload-scope", |ctx: Context| async move { ctx.render(reload_scope_page()) });
    router.get("/state-search", |ctx: Context| async move {
        let query = ctx.url().search_params().get("query").unwrap_or_default().to_string();
        ctx.render(state_search_page(query))
    });
    router.get("/client-mounted", |ctx: Context| async move { ctx.render(client_mounted_page()) });

    router.mount("/frames", |r| {
        r.get("/sidebar", |ctx: Context| async move {
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
        });
        r.get("/activity", |ctx: Context| async move {
            delay(2000).await;
            ctx.render(rsx! {
                <div>
                    <p mix={lead_style()}>"Rendered by " <code>"/frames/activity"</code> " at " <time>{clock()}</time> "."</p>
                    <Frame src="/frames/activity/detail" fallback={rsx! { <div mix={muted_style()}>"Loading detail\u{2026}"</div> }}/>
                </div>
            })
        });
        r.get("/activity/detail", |ctx: Context| async move {
            delay(600).await;
            ctx.render(rsx! {
                <div>
                    <p mix={[muted_style(), css("margin-top: 0; margin-bottom: 8px;")]}>"Nested frame with a hydrated counter:"</p>
                    <div mix={css("margin-top: 12px;")}>
                        <Frame src="/frames/time" fallback={rsx! { <div mix={muted_style()}>"Loading server time\u{2026}"</div> }}/>
                    </div>
                </div>
            })
        });
        r.get("/client-frame-example", |ctx: Context| async move {
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
        });
        r.get("/client-frame-example/nested", |ctx: Context| async move {
            delay(350).await;
            ctx.render(rsx! {
                <div mix={nested_fragment_style()}>
                    <div mix={fragment_label_style()}>"Nested server fragment"</div>
                    <div mix={css("margin-top: 6px;")}><Counter initial_count={1} label="Nested frame counter"/></div>
                </div>
            })
        });
        r.get("/client-mounted-outer", |ctx: Context| async move {
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
        });
        r.get("/client-mounted-nested", |ctx: Context| async move {
            delay(2200).await;
            ctx.render(rsx! {
                <div mix={nested_fragment_style()}>
                    <div mix={fragment_label_style()}>"Nested server fragment"</div>
                    <div mix={clock_value_style()}>{clock()}</div>
                    <div mix={css("margin-top: 6px;")}><Counter initial_count={3} label="Nested frame counter"/></div>
                </div>
            })
        });
        r.get("/time", |ctx: Context| async move {
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
        });
        r.get("/reload-scope", |ctx: Context| async move {
            delay(700).await;
            ctx.render(reload_scope_frame("Non-blocking frame server time"))
        });
        r.get("/reload-scope/blocking", |ctx: Context| async move {
            delay(500).await;
            ctx.render(reload_scope_frame("Blocking frame server time"))
        });
        r.get("/state-search-results", |ctx: Context| async move {
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
        });
    });
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

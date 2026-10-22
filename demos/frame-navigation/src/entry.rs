//! `app/actions/public/entry.tsx`: the browser entry. Frames load through a custom
//! resolver that renders a "Reload required" card for unexpected error responses, and an
//! app error listener replaces the page with a "Something went wrong" card.

use std::rc::Rc;

use js_sys::{Array, Object, Reflect};
use rmx::browser::{self, FetchInit};
use rmx::{css, on, rsx, Component, EventHandler, FrameContent, FrameRequest, FrameResponse, Handle, Mixin, Node, ResolveFuture};
use rmx::ui::animation::{animate_entrance, spring, Animate, SpringPreset};
use wasm_bindgen::JsCast;

use crate::{CourseFilterInput, DashboardStatGrid};

pub fn start() {
    let app = rmx::run(rmx::RunOptions {
        entries: vec![rmx::entry::<DashboardStatGrid>(), rmx::entry::<CourseFilterInput>()],
        resolve_frame: Some(Rc::new(resolve_frame)),
    });
    let handle = app.clone();
    app.on_error(move |error| {
        handle.dispose();
        let message = error.message.clone();
        wasm_bindgen_futures::spawn_local(async move {
            fade_out_body().await;
            let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) else { return };
            let reload = on("click", EventHandler::new(|_| {
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
            }));
            rmx::create_root(&body).render(rsx! {
                <div id="app-error" mix={page_css()}>
                    <ErrorCard eyebrow="Unexpected Error" title="Something went wrong" message={message} animated={true}
                        action={Some(rsx! { <button mix={[reload_button_css(), reload]}>"Reload the page"</button> })}/>
                </div>
            });
        });
    });
}

fn resolve_frame(request: FrameRequest) -> ResolveFuture {
    Box::pin(async move {
        let mut headers = vec![("Accept".to_string(), "text/html".to_string()), ("X-Remix-Frame".to_string(), "true".to_string())];
        if let Some(target) = &request.target {
            headers.push(("X-Remix-Target".to_string(), target.clone()));
        }
        let body = request.form.as_ref().filter(|_| !request.method.eq_ignore_ascii_case("GET")).map(|form| {
            headers.push(("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string()));
            url_encode(form)
        });
        let res = browser::fetch(&request.url, FetchInit { method: request.method.clone(), headers, body, signal: Some(request.signal.clone()) }).await?;

        // A redirected response may contain document UI that is not valid for the requested
        // subframe. The destination opts into subframe rendering by returning the matching
        // target header.
        if res.redirected && request.target.is_some() {
            if let Some(w) = web_sys::window() {
                let _ = w.location().assign(&res.url);
            }
            return std::future::pending().await;
        }

        // A 422 contains expected validation UI.
        if res.status == 422 {
            return Ok(FrameResponse { content: FrameContent::Html(res.text), url: None });
        }

        // Other errors render a custom reload UI.
        if !res.ok() && res.status >= 400 {
            let href = web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default();
            let card = rsx! {
                <ErrorCard eyebrow="Unexpected Error" title="Reload required"
                    message="An unexpected error occurred. Please reload the page to try again."
                    action={Some(rsx! { <a data-rmx-document="" href={href} mix={action_link_css()}>"Reload"</a> })}/>
            };
            return Ok(FrameResponse { content: FrameContent::Node(card), url: None });
        }

        let url = res.redirected.then(|| res.url.clone());
        Ok(FrameResponse { content: FrameContent::Html(res.text), url })
    })
}

/// `application/x-www-form-urlencoded`, as `URLSearchParams` writes it.
fn url_encode(form: &[(String, String)]) -> String {
    let encode = |s: &str| String::from(js_sys::encode_uri_component(s)).replace("%20", "+");
    form.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

async fn fade_out_body() {
    let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) else { return };
    let frame = |opacity: &str, transform: &str| {
        let o = Object::new();
        let _ = Reflect::set(&o, &"opacity".into(), &opacity.into());
        let _ = Reflect::set(&o, &"transform".into(), &transform.into());
        o
    };
    let keyframes = Array::of2(&frame("1", "translateY(0) scale(1)"), &frame("0", "translateY(10px) scale(0.985)"));
    let snappy = spring(SpringPreset::Snappy);
    let options = Object::new();
    let _ = Reflect::set(&options, &"duration".into(), &snappy.duration().into());
    let _ = Reflect::set(&options, &"easing".into(), &snappy.easing().into());
    let animation = Reflect::get(&body, &"animate".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call2(&body, &keyframes, &options).ok());
    if let Some(finished) = animation.and_then(|a| Reflect::get(&a, &"finished".into()).ok()) {
        let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&finished)).await;
    }
    body.set_inner_html("");
}

#[derive(Clone, Default)]
pub struct ErrorCardProps {
    pub eyebrow: &'static str,
    pub title: &'static str,
    pub message: String,
    pub action: Option<Node>,
    pub animated: bool,
}

pub struct ErrorCard;

impl Component for ErrorCard {
    type Props = ErrorCardProps;

    fn setup(_: &ErrorCardProps, _: &Handle<Self>) -> Self {
        ErrorCard
    }

    fn render(&self, props: &ErrorCardProps, _: &Handle<Self>) -> Node {
        let entrance = props
            .animated
            .then(|| animate_entrance(Animate::spring(SpringPreset::Smooth).opacity(0.0).transform("translateY(-14px) scale(0.97)")));
        rsx! {
            <div class="error-card" mix={[Some(card_css()), entrance]}>
                <p mix={css("margin: 0 0 12px; font-size: 12px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; color: #64748b;")}>{props.eyebrow}</p>
                <h1 mix={css("margin: 0 0 12px; font-size: 32px; line-height: 1.1; font-weight: 700;")}>{props.title}</h1>
                <p mix={css("margin: 0; font-size: 16px; line-height: 1.6; color: #475569;")}>{props.message.clone()}</p>
                {props.action.clone()}
            </div>
        }
    }
}

fn page_css() -> Mixin {
    css("min-height: 100vh; display: grid; place-items: center; padding: 32px; background: #f8fafc; color: #0f172a; font-family: ui-sans-serif, system-ui, sans-serif;")
}

fn card_css() -> Mixin {
    css("width: 100%; max-width: 560px; padding: 40px 36px; background: #ffffff; border: 1px solid #e2e8f0; border-radius: 20px; box-shadow: 0 20px 45px rgba(15, 23, 42, 0.08); box-sizing: border-box;")
}

fn reload_button_css() -> Mixin {
    css("margin-top: 24px; padding: 12px 18px; border: none; border-radius: 999px; background: #0f172a; color: #ffffff; font-size: 14px; font-weight: 600; line-height: 1; cursor: pointer; &:hover { background: #1e293b; }")
}

fn action_link_css() -> Mixin {
    css("display: inline-flex; margin-top: 24px; padding: 12px 18px; border-radius: 999px; background: #0f172a; color: #ffffff; font-size: 14px; font-weight: 600; line-height: 1; text-decoration: none; &:hover { background: #1e293b; }")
}

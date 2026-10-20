//! Soft navigation through the Navigation API.
//!
//! Same-origin link clicks, form submissions and back/forward become frame reloads: the
//! browser URL and history update as usual, but instead of loading a new document the
//! target frame (the top frame unless `data-rmx-target` names one) fetches its new content
//! and reconciles it into the page. Without the Navigation API the browser navigates
//! normally, so every page still works.
//!
//! Link and form attributes (as in Remix):
//!
//! - `data-rmx-target="name"` reloads the named frame instead of the top frame.
//! - `data-rmx-src="/url"` loads that URL into the target frame (the address bar still
//!   shows `href`).
//! - `data-rmx-history="replace" | "push"` chooses how the history entry is added.
//! - `data-rmx-document` opts out: the browser does a full document navigation.

use std::rc::Rc;

use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::frames::{self, Submission};
use super::{warn, window};

/// Options for [`navigate`].
#[derive(Clone, Debug, Default)]
pub struct NavigateOptions {
    /// A named frame to load into instead of the top frame.
    pub target: Option<String>,
    /// The URL to load into the target frame (defaults to `href`).
    pub src: Option<String>,
    /// Replace the current history entry instead of pushing one.
    pub replace: bool,
}

fn get(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn call(target: &JsValue, method: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = get(target, method).dyn_into()?;
    let array = js_sys::Array::new();
    for a in args {
        array.push(a);
    }
    f.apply(target, &array)
}

fn object(entries: &[(&str, JsValue)]) -> JsValue {
    let o = Object::new();
    for (k, v) in entries {
        let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o.into()
}

fn opt_str(v: Option<&str>) -> JsValue {
    v.map_or(JsValue::UNDEFINED, JsValue::from_str)
}

/// The `navigation` object, when the browser supports intercepting form and link
/// navigations (`NavigateEvent.sourceElement`).
fn navigation() -> Option<JsValue> {
    let w: JsValue = window().into();
    let nav = get(&w, "navigation");
    if nav.is_undefined() || nav.is_null() {
        return None;
    }
    let event_class = get(&w, "NavigateEvent");
    if event_class.is_undefined() {
        return None;
    }
    let proto = get(&event_class, "prototype");
    Reflect::has(&proto, &JsValue::from_str("sourceElement")).ok()?.then_some(nav)
}

fn state(target: Option<&str>, src: &str) -> JsValue {
    object(&[("$rmx", JsValue::TRUE), ("target", opt_str(target)), ("src", JsValue::from_str(src))])
}

fn is_runtime_state(v: &JsValue) -> bool {
    v.is_object() && Reflect::has(v, &JsValue::from_str("$rmx")).unwrap_or(false)
}

/// Navigate with the runtime: like clicking a link with the matching `data-rmx-*`
/// attributes. Falls back to `location.assign` without the Navigation API.
pub fn navigate(href: &str, options: NavigateOptions) {
    let Some(nav) = navigation() else {
        let location = window().location();
        let _ = if options.replace { location.replace(href) } else { location.assign(href) };
        return;
    };
    let src = options.src.as_deref().unwrap_or(href);
    let opts = object(&[
        ("state", state(options.target.as_deref(), src)),
        ("history", JsValue::from_str(if options.replace { "replace" } else { "push" })),
    ]);
    let _ = call(&nav, "navigate", &[&JsValue::from_str(href), &opts]);
}

pub(crate) fn start() {
    let Some(nav) = navigation() else { return };
    let href = window().location().href().unwrap_or_default();
    let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", state(None, &href))])]);
    let listener = Closure::<dyn FnMut(JsValue)>::new(on_navigate);
    let _ = call(&nav, "addEventListener", &[&JsValue::from_str("navigate"), listener.as_ref()]);
    listener.forget();
}

fn same_origin(url: &str) -> bool {
    let location = window().location();
    let base = location.href().unwrap_or_default();
    web_sys::Url::new_with_base(url, &base).is_ok_and(|u| Some(u.origin()) == location.origin().ok())
}

/// What a navigation should do, read from its source element or history state.
struct Plan {
    target: Option<String>,
    src: String,
    replace: bool,
    submission: Option<Submission>,
}

fn on_navigate(event: JsValue) {
    let info = get(&event, "info");
    if get(&info, "type").as_string().as_deref() == Some("rmx-redirect") {
        // The content for this URL is already in the page; just commit the URL.
        intercept(&event, Closure::once_into_js(|| JsValue::UNDEFINED), false);
        return;
    }
    let can_intercept = get(&event, "canIntercept").as_bool().unwrap_or(false);
    let hash_change = get(&event, "hashChange").as_bool().unwrap_or(false);
    let download = get(&event, "downloadRequest");
    if !can_intercept || hash_change || !(download.is_null() || download.is_undefined()) {
        return;
    }
    let destination = get(&event, "destination");
    let Some(dest) = get(&destination, "url").as_string() else { return };
    if !same_origin(&dest) {
        return;
    }
    let nav_type = get(&event, "navigationType").as_string().unwrap_or_default();
    let Some(plan) = plan(&event, &destination, &dest, &nav_type) else { return };
    if !same_origin(&plan.src) {
        return;
    }

    let top = frames::top();
    let frame = match &plan.target {
        Some(name) => match frames::by_name(name) {
            Some(f) => f,
            None => return, // unknown frame: let the browser load the document
        },
        None => top.clone(),
    };

    let Some(nav) = navigation() else { return };
    let cancelable = get(&event, "cancelable").as_bool().unwrap_or(false);
    if plan.replace && nav_type == "push" && plan.submission.is_none() && cancelable {
        let _ = call(&event, "preventDefault", &[]);
        let opts = object(&[("state", state(plan.target.as_deref(), &plan.src)), ("history", JsValue::from_str("replace"))]);
        let _ = call(&nav, "navigate", &[&JsValue::from_str(&dest), &opts]);
        return;
    }

    let entry_state = state(plan.target.as_deref(), &plan.src);
    let traverse = nav_type == "traverse";
    let handler = Closure::once_into_js(move || -> JsValue {
        wasm_bindgen_futures::future_to_promise(async move {
            if !traverse {
                let _ = call(&nav, "updateCurrentEntry", &[&object(&[("state", entry_state)])]);
            }
            top.set_src(&dest);
            if !Rc::ptr_eq(&frame, &top) {
                frame.set_src(&plan.src);
            }
            match frame.load(plan.submission.unwrap_or_default()).await {
                Ok(Some(redirected)) if Rc::ptr_eq(&frame, &top) => {
                    let opts = object(&[
                        ("state", state(None, &redirected)),
                        ("history", JsValue::from_str("replace")),
                        ("info", object(&[("type", JsValue::from_str("rmx-redirect"))])),
                    ]);
                    let _ = call(&nav, "navigate", &[&JsValue::from_str(&redirected), &opts]);
                }
                Ok(_) => {}
                Err(e) => {
                    warn(&format!("rmx: navigation failed, loading the document: {}", frames::js_error(&e)));
                    let _ = window().location().assign(&dest);
                }
            }
            Ok(JsValue::UNDEFINED)
        })
        .into()
    });
    intercept(&event, handler, true);
}

fn intercept(event: &JsValue, handler: JsValue, scroll: bool) {
    let mut opts = vec![("handler", handler)];
    if !scroll {
        opts.push(("scroll", JsValue::from_str("manual")));
    }
    if let Err(e) = call(event, "intercept", &[&object(&opts)]) {
        warn(&format!("rmx: could not intercept navigation: {}", frames::js_error(&e)));
    }
}

fn plan(event: &JsValue, destination: &JsValue, dest: &str, nav_type: &str) -> Option<Plan> {
    let from_state = || {
        let s = call(destination, "getState", &[]).ok()?;
        is_runtime_state(&s).then(|| Plan {
            target: get(&s, "target").as_string(),
            src: get(&s, "src").as_string().unwrap_or_else(|| dest.to_string()),
            replace: false,
            submission: None,
        })
    };
    if nav_type == "traverse" {
        return from_state();
    }
    let source = get(event, "sourceElement");
    let Some(source) = source.dyn_ref::<web_sys::Element>() else {
        // Programmatic: `navigate()` above, or a script calling navigation.navigate.
        return from_state();
    };

    if let Ok(Some(link)) = source.closest("a, area") {
        if link.has_attribute("data-rmx-document") || link.has_attribute("download") {
            return None;
        }
        return Some(Plan {
            target: link.get_attribute("data-rmx-target"),
            src: link.get_attribute("data-rmx-src").unwrap_or_else(|| dest.to_string()),
            replace: link.get_attribute("data-rmx-history").as_deref() == Some("replace"),
            submission: None,
        });
    }

    // A form submission: the source is the form or its submit button.
    let tag = source.tag_name();
    let submitter = (tag == "BUTTON" || tag == "INPUT").then(|| source.clone());
    let form: web_sys::Element = if tag == "FORM" {
        source.clone()
    } else {
        get(source, "form").dyn_into().ok()?
    };
    let attr = |name: &str, submitter_name: &str| {
        submitter
            .as_ref()
            .filter(|s| s.has_attribute(submitter_name))
            .and_then(|s| s.get_attribute(submitter_name))
            .or_else(|| form.get_attribute(name))
    };
    let has = |name: &str| form.has_attribute(name) || submitter.as_ref().is_some_and(|s| s.has_attribute(name));
    if has("data-rmx-document") {
        return None;
    }
    let method = attr("method", "formmethod").unwrap_or_else(|| "get".into()).to_ascii_lowercase();
    let target_attr = attr("target", "formtarget").unwrap_or_default().to_ascii_lowercase();
    if method == "dialog" || target_attr == "_blank" {
        return None;
    }
    let submission = if method == "get" {
        None
    } else {
        let form_data = get(event, "formData");
        let enctype = attr("enctype", "formenctype").unwrap_or_default().to_ascii_lowercase();
        let (body, content_type) = if form_data.is_null() || form_data.is_undefined() {
            (None, None)
        } else if enctype == "multipart/form-data" {
            (Some(form_data), None)
        } else {
            let params = web_sys::UrlSearchParams::new_with_str_sequence_sequence(&form_data).ok()?;
            (Some(params.into()), Some("application/x-www-form-urlencoded".to_string()))
        };
        Some(Submission { method: Some(method.to_ascii_uppercase()), body, content_type })
    };
    Some(Plan {
        target: attr("data-rmx-target", "data-rmx-target"),
        src: attr("data-rmx-src", "data-rmx-src").unwrap_or_else(|| dest.to_string()),
        replace: attr("data-rmx-history", "data-rmx-history").as_deref() == Some("replace"),
        submission,
    })
}


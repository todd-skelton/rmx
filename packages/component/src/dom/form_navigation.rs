//! Form submissions behind Navigation API events (Remix `createFormNavigationResolver`).
//!
//! A form's `navigate` event fires after the submission, so by then its submit button
//! may have been moved or removed, and `button.form` no longer finds the form. A
//! capture-phase `submit` listener records each submission's form, submitter and the
//! form data the browser built for it (`formdata`), and the resolver matches the
//! `navigate` event's `sourceElement` (the form or its submitter) back to it. Records that
//! no navigation claims expire after a second.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::js::get;
use super::{document, window};

const PENDING_TIMEOUT_MS: i32 = 1000;

type FormDataListener = Closure<dyn Fn(JsValue)>;

/// Data for an intercepted non-GET form submission.
pub struct FormSubmission {
    /// The submitted entries the browser built, when available.
    pub form_data: Option<web_sys::FormData>,
    /// The effective method (`post`), including the submitter's `formmethod`.
    pub method: String,
    /// The effective encoding, including the submitter's `formenctype`.
    pub enc_type: String,
}

struct Pending {
    form: web_sys::HtmlFormElement,
    submitter: Option<web_sys::Element>,
    form_data: RefCell<Option<web_sys::FormData>>,
    timeout: Cell<i32>,
    on_form_data: RefCell<Option<FormDataListener>>,
    on_timeout: RefCell<Option<Closure<dyn Fn()>>>,
}

impl Pending {
    fn remove_form_data_listener(&self) {
        if let Some(listener) = self.on_form_data.borrow().as_ref() {
            let _ = self.form.remove_event_listener_with_callback("formdata", listener.as_ref().unchecked_ref());
        }
    }
}

#[derive(Default)]
struct Inner {
    /// Submissions by form and by submitter, so a detached submitter still finds its form.
    by_element: RefCell<Vec<(web_sys::Element, Rc<Pending>)>>,
    /// Every submission waiting for its `formdata` event, navigation or timeout.
    live: RefCell<Vec<Rc<Pending>>>,
}

impl Inner {
    fn get(&self, element: &web_sys::Element) -> Option<Rc<Pending>> {
        self.by_element.borrow().iter().find(|(e, _)| e == element).map(|(_, p)| p.clone())
    }

    fn set(&self, element: web_sys::Element, pending: Rc<Pending>) {
        let mut map = self.by_element.borrow_mut();
        map.retain(|(e, _)| *e != element);
        map.push((element, pending));
    }

    fn clear(&self, pending: &Rc<Pending>) {
        window().clear_timeout_with_handle(pending.timeout.get());
        pending.remove_form_data_listener();
        self.by_element.borrow_mut().retain(|(_, p)| !Rc::ptr_eq(p, pending));
        // Dropping the record drops its listeners; take them out first so a listener that
        // is running right now is released after it returns.
        let removed: Vec<Rc<Pending>> = {
            let mut live = self.live.borrow_mut();
            let (gone, kept) = live.drain(..).partition(|p| Rc::ptr_eq(p, pending));
            *live = kept;
            gone
        };
        drop(removed);
    }

    fn on_submit(self: &Rc<Self>, event: web_sys::Event) {
        let Some(form) = event.target().and_then(|t| t.dyn_into::<web_sys::HtmlFormElement>().ok()) else { return };
        let submitter = get(&event, "submitter").dyn_into::<web_sys::Element>().ok().filter(is_submitter);
        let pending = Rc::new(Pending {
            form: form.clone(),
            submitter,
            form_data: RefCell::new(None),
            timeout: Cell::new(0),
            on_form_data: RefCell::new(None),
            on_timeout: RefCell::new(None),
        });

        let (inner, weak) = (Rc::downgrade(self), Rc::downgrade(&pending));
        let submit_event = event.clone();
        let on_form_data = FormDataListener::new(move |form_data_event: JsValue| {
            let (Some(inner), Some(pending)) = (inner.upgrade(), weak.upgrade()) else { return };
            // The browser's own `formdata` follows an uncanceled submit, after its dispatch.
            // Snapshots taken in submit handlers or after cancellation are not the submission.
            if submit_event.event_phase() != web_sys::Event::NONE || submit_event.default_prevented() {
                return;
            }
            // A newer submission of the form replaces one whose navigation never came.
            if let Some(existing) = inner.get(pending.form.unchecked_ref()) {
                inner.clear(&existing);
            }
            *pending.form_data.borrow_mut() = get(&form_data_event, "formData").dyn_into().ok();
            inner.set(pending.form.clone().unchecked_into(), pending.clone());
            if let Some(submitter) = &pending.submitter {
                inner.set(submitter.clone(), pending.clone());
            }
            pending.remove_form_data_listener();
        });
        let _ = form.add_event_listener_with_callback("formdata", on_form_data.as_ref().unchecked_ref());
        *pending.on_form_data.borrow_mut() = Some(on_form_data);

        let (inner, weak) = (Rc::downgrade(self), Rc::downgrade(&pending));
        let on_timeout = Closure::<dyn Fn()>::new(move || {
            if let (Some(inner), Some(pending)) = (inner.upgrade(), weak.upgrade()) {
                inner.clear(&pending);
            }
        });
        let handle = window()
            .set_timeout_with_callback_and_timeout_and_arguments_0(on_timeout.as_ref().unchecked_ref(), PENDING_TIMEOUT_MS)
            .unwrap_or(0);
        pending.timeout.set(handle);
        *pending.on_timeout.borrow_mut() = Some(on_timeout);
        self.live.borrow_mut().push(pending);
    }
}

fn is_submitter(element: &web_sys::Element) -> bool {
    matches!(element.tag_name().as_str(), "BUTTON" | "INPUT")
}

/// Tracks form submissions until dropped (Remix `createFormNavigationResolver`).
pub struct FormNavigationResolver {
    inner: Rc<Inner>,
    on_submit: Closure<dyn Fn(web_sys::Event)>,
}

impl Default for FormNavigationResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl FormNavigationResolver {
    /// Start tracking `submit` events on the document.
    pub fn new() -> FormNavigationResolver {
        let inner = Rc::new(Inner::default());
        let weak = Rc::downgrade(&inner);
        let on_submit = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_submit(event);
            }
        });
        let _ = document().add_event_listener_with_callback_and_bool("submit", on_submit.as_ref().unchecked_ref(), true);
        FormNavigationResolver { inner, on_submit }
    }

    /// The form submission behind a `navigate` event, or `None` for other navigations,
    /// `method="dialog"` and `target="_blank"` submissions.
    pub fn resolve(&self, event: &JsValue) -> Option<FormNavigation> {
        let source = get(event, "sourceElement").dyn_into::<web_sys::Element>().ok()?;
        let form = self.source_form(&source)?;
        let pending = self.inner.get(form.unchecked_ref());
        let submitter = pending.as_ref().and_then(|p| p.submitter.clone());
        let overridden = |name: &str| submitter.as_ref().filter(|s| s.has_attribute(name));
        let method = match overridden("formmethod") {
            Some(s) => get(s, "formMethod").as_string().unwrap_or_default(),
            None => form.method(),
        };
        let enc_type = match overridden("formenctype") {
            Some(s) => get(s, "formEnctype").as_string().unwrap_or_default(),
            None => form.enctype(),
        };
        let target = submission_attribute(&form, submitter.as_ref(), "target", Some("formtarget"));
        if method == "dialog" || target.is_some_and(|t| t.eq_ignore_ascii_case("_blank")) {
            return None;
        }
        let submission = if method.eq_ignore_ascii_case("get") {
            if let Some(pending) = &pending {
                self.inner.clear(pending);
            }
            None
        } else {
            Some(PendingSubmission(Rc::new(PendingInner {
                resolver: Rc::downgrade(&self.inner),
                pending,
                event_form_data: get(event, "formData").dyn_into().ok(),
                method,
                enc_type,
            })))
        };
        Some(FormNavigation { form, submitter, submission })
    }

    /// The submitted form: the source itself, or its submitter's form. A submitter detached
    /// before `navigate` fired has no `form`; use the one recorded at submit time.
    fn source_form(&self, source: &web_sys::Element) -> Option<web_sys::HtmlFormElement> {
        if let Some(form) = source.dyn_ref::<web_sys::HtmlFormElement>() {
            return Some(form.clone());
        }
        if !is_submitter(source) {
            return None;
        }
        get(source, "form").dyn_into().ok().or_else(|| self.inner.get(source).map(|p| p.form.clone()))
    }
}

impl Drop for FormNavigationResolver {
    fn drop(&mut self) {
        let _ = document().remove_event_listener_with_callback_and_bool("submit", self.on_submit.as_ref().unchecked_ref(), true);
        let live: Vec<Rc<Pending>> = self.inner.live.borrow().clone();
        for pending in &live {
            self.inner.clear(pending);
        }
    }
}

fn submission_attribute(
    form: &web_sys::HtmlFormElement,
    submitter: Option<&web_sys::Element>,
    name: &str,
    submitter_name: Option<&str>,
) -> Option<String> {
    let submitter_name = submitter_name.unwrap_or(name);
    match submitter.filter(|s| s.has_attribute(submitter_name)) {
        Some(s) => s.get_attribute(submitter_name),
        None => form.get_attribute(name),
    }
}

/// The form and submitter behind a navigation (Remix `FormNavigation`).
pub struct FormNavigation {
    form: web_sys::HtmlFormElement,
    submitter: Option<web_sys::Element>,
    submission: Option<PendingSubmission>,
}

impl FormNavigation {
    /// Whether the submitter or the form has the attribute.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.submitter.as_ref().is_some_and(|s| s.has_attribute(name)) || self.form.has_attribute(name)
    }

    /// A submission attribute, read from the submitter first (as `submitter_name`, when
    /// given), then from the form.
    pub fn get_attribute(&self, name: &str, submitter_name: Option<&str>) -> Option<String> {
        submission_attribute(&self.form, self.submitter.as_ref(), name, submitter_name)
    }

    /// The submission data, for non-GET submissions.
    pub fn submission(&self) -> Option<PendingSubmission> {
        self.submission.clone()
    }
}

struct PendingInner {
    resolver: Weak<Inner>,
    pending: Option<Rc<Pending>>,
    event_form_data: Option<web_sys::FormData>,
    method: String,
    enc_type: String,
}

/// A non-GET submission waiting for its navigation handler (Remix `getSubmission`).
#[derive(Clone)]
pub struct PendingSubmission(Rc<PendingInner>);

impl PendingSubmission {
    /// The submission's data. Releases the submit-time record, without touching a newer
    /// submission of the same form or submitter.
    pub fn get(&self) -> FormSubmission {
        let p = &self.0;
        if let (Some(resolver), Some(pending)) = (p.resolver.upgrade(), &p.pending) {
            resolver.clear(pending);
        }
        let form_data = p.event_form_data.clone().or_else(|| p.pending.as_ref().and_then(|x| x.form_data.borrow().clone()));
        FormSubmission { form_data, method: p.method.clone(), enc_type: p.enc_type.clone() }
    }
}

//! Anchored popovers with focus handling and outside-click dismissal (Remix's
//! `@remix-run/ui/popover`).
//!
//! ```ignore
//! use rmx_ui::anchor::{AnchorOptions, Placement};
//! use rmx_ui::popover;
//!
//! rsx! {
//!     <popover::Context>
//!         <button type="button" mix={[popover::anchor(AnchorOptions::new(Placement::BottomStart).offset(8.0)), popover::focus_on_hide(), on("click", toggle)]}>"View options"</button>
//!         <div mix={popover::surface(popover::SurfaceOptions::new(self.open, close))}>
//!             <button type="button" mix={popover::focus_on_show()}>"Close"</button>
//!         </div>
//!     </popover::Context>
//! }
//! ```
//!
//! The surface is a `popover="manual"` element. It opens and closes with `open`, is
//! positioned against the registered anchor with [`crate::anchor`] while open, locks
//! document scrolling, and asks to hide (`on_hide`) on Escape or a click outside it.
//! Focus moves to the `focus_on_show` element when it opens and back to the
//! `focus_on_hide` element when it closes.

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
use std::rc::Rc;

use rmx::{mixin, on, Children, Component, Element, Handle, HostMixin, Mixin, MixinHandle, Node};

use crate::anchor::AnchorOptions;

/// Why the surface asked to hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideReason {
    EscapeKey,
    OutsideClick,
}

/// A request to hide the surface (Remix's `PopoverHideRequest`).
#[derive(Clone, Debug)]
pub struct HideRequest {
    pub reason: HideReason,
    /// The clicked node, for outside clicks.
    #[cfg(target_arch = "wasm32")]
    pub target: Option<web_sys::Node>,
}

/// Called when the surface asks to hide.
pub type OnHide = Rc<dyn Fn(&HideRequest)>;

/// Options for [`surface`] (Remix's `PopoverSurfaceOptions`).
#[derive(Clone)]
pub struct SurfaceOptions {
    pub open: bool,
    pub on_hide: OnHide,
    /// Whether a click on the anchor counts as an outside click (default `true`).
    pub close_on_anchor_click: bool,
    /// Focus the `focus_on_hide` element when the surface closes (default `true`).
    pub restore_focus_on_hide: bool,
    /// Stop an outside click from reaching its target (default `true`).
    pub stop_outside_click_propagation: bool,
}

impl SurfaceOptions {
    pub fn new(open: bool, on_hide: impl Fn(&HideRequest) + 'static) -> Self {
        SurfaceOptions { open, on_hide: Rc::new(on_hide), close_on_anchor_click: true, restore_focus_on_hide: true, stop_outside_click_propagation: true }
    }

    pub fn close_on_anchor_click(mut self, close: bool) -> Self {
        self.close_on_anchor_click = close;
        self
    }

    pub fn restore_focus_on_hide(mut self, restore: bool) -> Self {
        self.restore_focus_on_hide = restore;
        self
    }

    pub fn stop_outside_click_propagation(mut self, stop: bool) -> Self {
        self.stop_outside_click_propagation = stop;
        self
    }
}

/// Elements registered with the popover (Remix's `PopoverContext`).
#[derive(Default)]
pub struct PopoverState {
    #[cfg(target_arch = "wasm32")]
    inner: RefCell<Registered>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct Registered {
    hide_focus_target: Option<web_sys::HtmlElement>,
    show_focus_target: Option<web_sys::HtmlElement>,
    surface: Option<web_sys::HtmlElement>,
    anchor: Option<(crate::anchor::AnchorTarget, AnchorOptions)>,
}

impl PopoverState {
    /// Anchor the surface to an element or point (what the `anchor` mixin does).
    #[cfg(target_arch = "wasm32")]
    pub fn set_anchor(&self, target: crate::anchor::AnchorTarget, options: AnchorOptions) {
        self.inner.borrow_mut().anchor = Some((target, options));
    }

    /// The registered surface element.
    #[cfg(target_arch = "wasm32")]
    pub fn surface(&self) -> Option<web_sys::HtmlElement> {
        self.inner.borrow().surface.clone()
    }

    #[cfg(target_arch = "wasm32")]
    fn anchor_contains(&self, target: &web_sys::Node) -> bool {
        match &self.inner.borrow().anchor {
            Some((crate::anchor::AnchorTarget::Element(el), _)) => el.contains(Some(target)),
            _ => false,
        }
    }
}

/// Props for [`Context`].
#[derive(Clone, Default)]
pub struct ContextProps {
    pub children: Children,
}

/// The popover provider (Remix's `popover.Context`). Renders its children.
pub struct Context;

impl Component for Context {
    type Props = ContextProps;

    fn setup(_: &ContextProps, h: &Handle<Self>) -> Self {
        h.set_context(Rc::new(PopoverState::default()));
        Context
    }

    fn render(&self, props: &ContextProps, _: &Handle<Self>) -> Node {
        props.children.node().clone()
    }
}

fn state<M: HostMixin>(h: &MixinHandle<M>) -> Rc<PopoverState> {
    h.context::<Rc<PopoverState>>().map(|s| (*s).clone()).expect("popover mixins must be inside <popover::Context>")
}

/// Register the host as the element the surface is positioned against.
pub fn anchor(options: AnchorOptions) -> Mixin {
    mixin::<Anchor>(options)
}

/// The floating surface.
pub fn surface(options: SurfaceOptions) -> Mixin {
    mixin::<Surface>(options)
}

/// Focus the host when the surface opens.
pub fn focus_on_show() -> Mixin {
    mixin::<FocusTarget>(true)
}

/// Focus the host when the surface closes.
pub fn focus_on_hide() -> Mixin {
    mixin::<FocusTarget>(false)
}

pub struct Anchor {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    state: Rc<PopoverState>,
}

impl HostMixin for Anchor {
    type Args = AnchorOptions;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        Anchor { state: state(h) }
    }

    fn render(&mut self, options: &AnchorOptions, _: &mut Element, h: &MixinHandle<Self>) {
        let options = options.clone();
        h.queue_task(move |this, _, node| {
            #[cfg(target_arch = "wasm32")]
            if let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) {
                this.state.set_anchor(crate::anchor::AnchorTarget::Element(el), options);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, node, options);
        });
    }
}

pub struct FocusTarget;

impl HostMixin for FocusTarget {
    type Args = bool;
    const SSR: bool = false;

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        FocusTarget
    }

    fn render(&mut self, show: &bool, _: &mut Element, h: &MixinHandle<Self>) {
        let show = *show;
        let popover = state(h);
        h.queue_task(move |_, _, node| {
            #[cfg(target_arch = "wasm32")]
            if let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) {
                let mut inner = popover.inner.borrow_mut();
                if show {
                    inner.show_focus_target = Some(el);
                } else {
                    inner.hide_focus_target = Some(el);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (node, show, popover);
        });
    }
}

pub struct Surface {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    state: Rc<PopoverState>,
    open: bool,
    options: Option<SurfaceOptions>,
    #[cfg(target_arch = "wasm32")]
    anchored: Option<crate::anchor::Anchored>,
    #[cfg(target_arch = "wasm32")]
    scroll_lock: Option<scroll_lock::ScrollLock>,
}

impl HostMixin for Surface {
    type Args = SurfaceOptions;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        let popover = state(h);
        #[cfg(target_arch = "wasm32")]
        {
            let registered = popover.clone();
            h.queue_task(move |_, h, node| {
                let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(node.element().clone()) else { return };
                registered.inner.borrow_mut().surface = Some(el.clone());
                let registered = registered.clone();
                crate::event::on_abort(&h.signal(), move || {
                    let mut inner = registered.inner.borrow_mut();
                    if inner.surface.as_ref() == Some(&el) {
                        inner.surface = None;
                    }
                });
            });
            h.on_insert(|_, h, event| outside_click::listen(h, &event.node));
            h.on_remove(|this, _| {
                this.anchored = None;
                this.scroll_lock = None;
            });
        }
        Surface {
            state: popover,
            open: false,
            options: None,
            #[cfg(target_arch = "wasm32")]
            anchored: None,
            #[cfg(target_arch = "wasm32")]
            scroll_lock: None,
        }
    }

    fn render(&mut self, options: &SurfaceOptions, host: &mut Element, h: &MixinHandle<Self>) {
        let was_open = self.open;
        self.open = options.open;
        self.options = Some(options.clone());
        let open = options.open;
        // `showPopover` dispatches `beforetoggle` synchronously while this task holds the
        // mixin state, so the task does the open and close work itself.
        h.queue_task(move |this, _, node| {
            #[cfg(target_arch = "wasm32")]
            {
                let el = node.element();
                if open && !was_open {
                    this.opening(el);
                    call(el, "showPopover");
                } else if !open && was_open {
                    call(el, "hidePopover");
                    this.closed();
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, node, was_open, open);
        });

        host.set_attr("popover", "manual");
        host.mix(on(
            "beforetoggle",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                // Toggles started elsewhere (the task above handles its own).
                match new_state(event).as_deref() {
                    Some("open") if this.anchored.is_none() => {
                        if let Some(el) = event.current_element() {
                            this.opening(&el);
                        }
                    }
                    Some("closed") => this.closed(),
                    _ => {}
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (this, event);
            }),
        ));
        host.mix(on(
            "toggle",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                {
                    let inner = this.state.inner.borrow();
                    let restore = this.options.as_ref().is_none_or(|o| o.restore_focus_on_hide);
                    match new_state(event).as_deref() {
                        Some("open") => {
                            if let Some(el) = &inner.show_focus_target {
                                let _ = el.focus();
                            }
                        }
                        Some("closed") if restore => {
                            if let Some(el) = &inner.hide_focus_target {
                                let _ = el.focus();
                            }
                        }
                        _ => {}
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (this, event);
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, _, event| {
                if event.key() != "Escape" {
                    return;
                }
                event.prevent_default();
                if let Some(options) = &this.options {
                    (options.on_hide)(&HideRequest {
                        reason: HideReason::EscapeKey,
                        #[cfg(target_arch = "wasm32")]
                        target: None,
                    });
                }
            }),
        ));
    }
}

#[cfg(target_arch = "wasm32")]
impl Surface {
    /// Position against the anchor and lock scrolling (Remix's `beforetoggle` open branch).
    fn opening(&mut self, el: &web_sys::Element) {
        let anchor = self.state.inner.borrow().anchor.clone();
        let Some((target, options)) = anchor else {
            crate::event::warn("popover::surface() requires a registered anchor before opening");
            return;
        };
        let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(el.clone()) else { return };
        self.anchored = Some(crate::anchor::anchor(&el, target, options));
        if self.scroll_lock.is_none() {
            self.scroll_lock = Some(scroll_lock::lock());
        }
    }

    fn closed(&mut self) {
        self.anchored = None;
        self.scroll_lock = None;
    }
}

#[cfg(target_arch = "wasm32")]
fn call(el: &web_sys::Element, method: &str) {
    if let Ok(f) = js_sys::Reflect::get(el, &method.into()).and_then(wasm_bindgen::JsCast::dyn_into::<js_sys::Function>) {
        let _ = f.call0(el);
    }
}

#[cfg(target_arch = "wasm32")]
fn new_state(event: &rmx::Event) -> Option<String> {
    js_sys::Reflect::get(event.raw(), &"newState".into()).ok()?.as_string()
}

/// A capture-phase document click listener that asks the surface to hide when a click
/// lands outside it (Remix's internal `onOutsideClick` mixin).
#[cfg(target_arch = "wasm32")]
mod outside_click {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use super::*;

    pub(super) fn listen(h: &MixinHandle<Surface>, node: &rmx::NodeHandle) {
        let surface = node.element().clone();
        let handle = h.clone();
        let listener = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
            let target = event.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
            handle.with(|this, _| {
                let Some(options) = this.options.clone() else { return };
                if !this.open {
                    return;
                }
                if let Some(target) = &target {
                    let on_anchor = !options.close_on_anchor_click && this.state.anchor_contains(target);
                    if surface.contains(Some(target)) || on_anchor {
                        return;
                    }
                }
                if options.stop_outside_click_propagation {
                    event.stop_propagation();
                }
                (options.on_hide)(&HideRequest { reason: HideReason::OutsideClick, target: target.clone() });
            });
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_capture(true);
        let signal = h.signal();
        options.set_signal(&signal);
        if let Some(doc) = node.element().owner_document() {
            let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("click", listener.as_ref().unchecked_ref(), &options);
        }
        // Kept alive until the signal removes the listener with the slot.
        crate::event::on_abort(&signal, move || drop(listener));
    }
}

/// Document scroll locking shared by every open surface (Remix's `lockScroll`).
#[cfg(target_arch = "wasm32")]
pub(crate) mod scroll_lock {
    use std::cell::RefCell;

    struct Saved {
        count: usize,
        overflow: String,
        gutter: String,
        scroll: (f64, f64),
    }

    thread_local! {
        static LOCK: RefCell<Option<Saved>> = const { RefCell::new(None) };
    }

    /// Releases its lock on drop; the document scrolls again once every lock is released.
    pub struct ScrollLock(());

    pub fn lock() -> ScrollLock {
        let Some(window) = web_sys::window() else { return ScrollLock(()) };
        let Some(root) = window.document().and_then(|d| d.document_element()) else { return ScrollLock(()) };
        let Ok(root) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(root) else { return ScrollLock(()) };
        LOCK.with(|lock| {
            let mut lock = lock.borrow_mut();
            let saved = lock.get_or_insert_with(|| {
                let style = root.style();
                let inner = window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let client = root.client_width() as f64;
                let scrollbar = if client > 0.0 { (inner - client).max(0.0) } else { 0.0 };
                let gutter = window.get_computed_style(&root).ok().flatten().and_then(|s| s.get_property_value("scrollbar-gutter").ok()).unwrap_or_default();
                let saved = Saved {
                    count: 0,
                    overflow: style.get_property_value("overflow").unwrap_or_default(),
                    gutter: style.get_property_value("scrollbar-gutter").unwrap_or_default(),
                    scroll: (window.scroll_x().unwrap_or(0.0), window.scroll_y().unwrap_or(0.0)),
                };
                let _ = style.set_property("overflow", "hidden");
                if scrollbar > 0.0 && gutter == "auto" {
                    let _ = style.set_property("scrollbar-gutter", "stable");
                }
                saved
            });
            saved.count += 1;
        });
        ScrollLock(())
    }

    impl Drop for ScrollLock {
        fn drop(&mut self) {
            let restore = LOCK.with(|lock| {
                let mut lock = lock.borrow_mut();
                let saved = lock.as_mut()?;
                saved.count = saved.count.saturating_sub(1);
                if saved.count > 0 {
                    return None;
                }
                lock.take()
            });
            let Some(saved) = restore else { return };
            let Some(window) = web_sys::window() else { return };
            if let Some(root) = window.document().and_then(|d| d.document_element()).and_then(|r| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(r).ok()) {
                let style = root.style();
                let _ = style.set_property("overflow", &saved.overflow);
                let _ = style.set_property("scrollbar-gutter", &saved.gutter);
            }
            window.scroll_to_with_x_and_y(saved.scroll.0, saved.scroll.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::{render_to_string, rsx};

    #[test]
    fn surface_renders_as_a_manual_popover_and_browser_only_mixins_are_skipped() {
        let html = render_to_string(&rsx! {
            <Context>
                <button type="button" mix={[anchor(AnchorOptions::default()), focus_on_hide()]}>"Open"</button>
                <div mix={surface(SurfaceOptions::new(false, |_| {}))}><button mix={focus_on_show()}>"Close"</button></div>
            </Context>
        });
        assert_eq!(html, r#"<button type="button">Open</button><div popover="manual"><button>Close</button></div>"#);
    }
}

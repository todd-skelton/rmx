//! The browser side of the animation mixins: Web Animations API calls, the per-element
//! record of running animations, and the FLIP layout state machine.

use js_sys::{Array, Object, Promise, Reflect, WeakMap};
use rsmix::Key;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AnimationPlayState, HtmlElement};

use super::{Animate, Composite, Layout};

thread_local! {
    /// element -> [Animation, [property, ...]] for entrance/exit animations in flight.
    static ANIMATING: WeakMap = WeakMap::new();
    /// parent -> Set of keys whose first entrance was skipped (`initial: false`).
    static SEEN_BY_PARENT: WeakMap = WeakMap::new();
}

/// `background-color` -> `backgroundColor` (WAAPI keyframe names). Custom properties and
/// names without hyphens pass through.
fn camel(property: &str) -> String {
    if property.starts_with("--") || !property.contains('-') {
        return property.to_string();
    }
    let mut out = String::with_capacity(property.len());
    let mut upper = false;
    for c in property.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `backgroundColor` -> `background-color` (CSSOM names).
fn kebab(property: &str) -> String {
    if property.contains('-') {
        return property.to_string();
    }
    let mut out = String::with_capacity(property.len() + 4);
    for c in property.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn keyframe(styles: &[(String, String)]) -> Object {
    let frame = Object::new();
    for (property, value) in styles {
        set(&frame, &camel(property), value.as_str());
    }
    frame
}

fn set(object: &Object, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(object, &key.into(), &value.into());
}

/// WAAPI timing options. (web-sys only binds `KeyframeAnimationOptions` behind its unstable
/// API flag, so this is a plain object.)
fn options(config: &Animate, fill: &str) -> Object {
    let options = Object::new();
    set(&options, "duration", config.duration);
    if let Some(delay) = config.delay {
        set(&options, "delay", delay);
    }
    if let Some(easing) = &config.easing {
        set(&options, "easing", easing.as_str());
    }
    if let Some(composite) = config.composite {
        let composite = match composite {
            Composite::Replace => "replace",
            Composite::Add => "add",
            Composite::Accumulate => "accumulate",
        };
        set(&options, "composite", composite);
    }
    set(&options, "fill", fill);
    options
}

/// `el.animate([from, to], options)`.
fn run(el: &web_sys::Element, from: &Object, to: &Object, options: &Object) -> web_sys::Animation {
    let animate: js_sys::Function = Reflect::get(el, &"animate".into()).expect("Element.animate").unchecked_into();
    animate.call2(el, &Array::of2(from, to), options).expect("Element.animate").unchecked_into()
}

fn is_running(animation: &web_sys::Animation) -> bool {
    animation.play_state() == AnimationPlayState::Running
}

fn current(el: &web_sys::Element) -> Option<(web_sys::Animation, Vec<String>)> {
    let entry = ANIMATING.with(|m| m.get(el));
    let entry: Array = entry.dyn_into().ok()?;
    let animation = entry.get(0).dyn_into().ok()?;
    let properties = entry.get(1).dyn_into::<Array>().ok()?.iter().filter_map(|p| p.as_string()).collect();
    Some((animation, properties))
}

/// Remember the animation running on `el` until it finishes (or another replaces it).
fn track(el: &web_sys::Element, animation: &web_sys::Animation, properties: Vec<String>) {
    let list: Array = properties.iter().map(|p| JsValue::from_str(p)).collect();
    ANIMATING.with(|m| m.set(el, &Array::of2(animation, &list)));
    let (el, animation) = (el.clone(), animation.clone());
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(finished) = animation.finished() {
            let _ = JsFuture::from(finished).await;
        }
        if current(&el).is_some_and(|(a, _)| a == animation) {
            ANIMATING.with(|m| m.delete(&el));
        }
    });
}

fn properties(styles: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (p, _) in styles {
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// Resolve when the animation finishes or is cancelled, or `signal` aborts.
async fn finished_or_abort(animation: &web_sys::Animation, signal: &web_sys::AbortSignal) {
    if signal.aborted() {
        return;
    }
    let aborted = Promise::new(&mut |resolve, _| {
        let _ = signal.add_event_listener_with_callback("abort", &resolve);
    });
    let finished = animation.finished().unwrap_or_else(|_| Promise::resolve(&JsValue::UNDEFINED));
    let _ = JsFuture::from(Promise::race(&Array::of2(&finished, &aborted))).await;
}

fn key_value(key: &Key) -> JsValue {
    match key {
        Key::Int(i) => JsValue::from_f64(*i as f64),
        Key::Str(s) => JsValue::from_str(s),
    }
}

/// `initial: false` skips the first entrance seen for each key under a parent.
fn skip_initial(el: &web_sys::Element, key: Option<&Key>, config: &Animate) -> bool {
    if config.initial {
        return false;
    }
    let (Some(key), Some(parent)) = (key, el.parent_node()) else { return false };
    let seen = SEEN_BY_PARENT.with(|m| {
        m.get(&parent).dyn_into::<js_sys::Set>().unwrap_or_else(|_| {
            let set = js_sys::Set::new(&JsValue::UNDEFINED);
            m.set(&parent, &set);
            set
        })
    });
    let key = key_value(key);
    if seen.has(&key) {
        return false;
    }
    seen.add(&key);
    true
}

pub(super) fn enter(el: &web_sys::Element, key: Option<&Key>, config: &Animate) {
    if current(el).is_some_and(|(a, _)| is_running(&a)) {
        return;
    }
    if skip_initial(el, key, config) {
        return;
    }
    let animation = run(el, &keyframe(&config.styles), &Object::new(), &options(config, "backwards"));
    track(el, &animation, properties(&config.styles));
}

pub(super) async fn exit(el: &web_sys::Element, config: &Animate, signal: &web_sys::AbortSignal) {
    if let Some((animation, _)) = current(el).filter(|(a, _)| is_running(a)) {
        // Mid-entrance: play it backwards instead of jumping to the natural styles.
        let _ = animation.reverse();
        finished_or_abort(&animation, signal).await;
        return;
    }
    let animation = run(el, &Object::new(), &keyframe(&config.styles), &options(config, "forwards"));
    track(el, &animation, properties(&config.styles));
    finished_or_abort(&animation, signal).await;
}

/// A reclaimed element was mid-exit: continue from its current look back to its natural
/// styles instead of reversing the exit.
pub(super) fn retarget(el: &web_sys::Element, config: &Animate) {
    let Some((animation, props)) = current(el).filter(|(a, _)| is_running(a)) else { return };
    // commitStyles can throw when the element is momentarily not rendered; the computed
    // style below still gives a starting point.
    if let Ok(commit) = Reflect::get(&animation, &"commitStyles".into()).and_then(|f| f.dyn_into::<js_sys::Function>()) {
        let _ = commit.call0(&animation);
    }
    animation.cancel();
    let Some(html) = el.dyn_ref::<HtmlElement>() else { return };
    let style = html.style();
    let computed = web_sys::window().and_then(|w| w.get_computed_style(el).ok().flatten());
    let from = Object::new();
    let mut animated = Vec::new();
    for property in &props {
        let css = kebab(property);
        let mut value = style.get_property_value(&css).unwrap_or_default();
        if value.is_empty() {
            value = computed.as_ref().and_then(|c| c.get_property_value(&css).ok()).unwrap_or_default();
        }
        if !value.is_empty() {
            set(&from, &camel(property), value.as_str());
            animated.push(property.clone());
        }
        let _ = style.remove_property(&css);
    }
    let animation = run(el, &from, &Object::new(), &options(config, "none"));
    track(el, &animation, animated);
}

// ---------------------------------------------------------------------------------------
// Layout (FLIP)
// ---------------------------------------------------------------------------------------

const DEFAULT_DURATION: f64 = 200.0;
const DEFAULT_EASING: &str = "ease-out";
const SCALE_PRECISION: f64 = 0.0001;
const TRANSLATE_PRECISION: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
struct Axis {
    min: f64,
    max: f64,
}

#[derive(Clone, Copy, Debug)]
struct LayoutBox {
    x: Axis,
    y: Axis,
}

#[derive(Clone, Copy, Debug)]
struct AxisDelta {
    translate: f64,
    scale: f64,
    origin: f64,
}

fn near(value: f64, target: f64, threshold: f64) -> bool {
    (value - target).abs() <= threshold
}

fn mix(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

fn axis_delta(source: Axis, target: Axis, origin: f64) -> AxisDelta {
    let origin_point = mix(source.min, source.max, origin);
    let source_len = source.max - source.min;
    let target_len = target.max - target.min;
    let mut scale = if source_len != 0.0 { target_len / source_len } else { 1.0 };
    let mut translate = mix(target.min, target.max, origin) - origin_point;
    if near(scale, 1.0, SCALE_PRECISION) || scale.is_nan() {
        scale = 1.0;
    }
    if near(translate, 0.0, TRANSLATE_PRECISION) || translate.is_nan() {
        translate = 0.0;
    }
    AxisDelta { translate, scale, origin }
}

fn rect_box(rect: &web_sys::DomRect) -> LayoutBox {
    LayoutBox { x: Axis { min: rect.left(), max: rect.right() }, y: Axis { min: rect.top(), max: rect.bottom() } }
}

/// The element's box without any projection transform.
fn natural_box(el: &HtmlElement) -> LayoutBox {
    let style = el.style();
    let transform = style.get_property_value("transform").unwrap_or_default();
    let origin = style.get_property_value("transform-origin").unwrap_or_default();
    let _ = style.set_property("transform", "none");
    let _ = style.remove_property("transform-origin");
    let rect = rect_box(&el.get_bounding_client_rect());
    set_or_remove(&style, "transform", &transform);
    set_or_remove(&style, "transform-origin", &origin);
    rect
}

fn set_or_remove(style: &web_sys::CssStyleDeclaration, property: &str, value: &str) {
    if value.is_empty() {
        let _ = style.remove_property(property);
    } else {
        let _ = style.set_property(property, value);
    }
}

fn same_target(a: &LayoutBox, b: &LayoutBox, layout: &Layout) -> bool {
    near(a.x.min, b.x.min, TRANSLATE_PRECISION)
        && near(a.y.min, b.y.min, TRANSLATE_PRECISION)
        && (!layout.size || (near(a.x.max, b.x.max, TRANSLATE_PRECISION) && near(a.y.max, b.y.max, TRANSLATE_PRECISION)))
}

/// The layout mixin's state between `before_update` and `commit`.
#[derive(Default)]
pub(super) struct LayoutState {
    snapshot: Option<LayoutBox>,
    animation: Option<web_sys::Animation>,
    target: Option<LayoutBox>,
    end_time: f64,
}

impl LayoutState {
    fn clear_animation(&mut self) {
        self.animation = None;
        self.target = None;
        self.end_time = 0.0;
    }

    /// Record where the element is now, including a running animation's current offset,
    /// so an interrupted animation continues from what is on screen.
    pub(super) fn before_update(&mut self, el: &web_sys::Element, _layout: &Layout) {
        let Some(el) = el.dyn_ref::<HtmlElement>() else { return };
        self.snapshot = Some(if self.animation.as_ref().is_some_and(is_running) {
            rect_box(&el.get_bounding_client_rect())
        } else {
            natural_box(el)
        });
    }

    /// Compare the new box with the snapshot and play the inverse transform back to none.
    /// `on_finish` runs (with the animation) when it finishes naturally.
    pub(super) fn commit(&mut self, el: &web_sys::Element, layout: Option<&Layout>, on_finish: impl FnOnce(web_sys::Animation) + 'static) {
        let Some(html) = el.dyn_ref::<HtmlElement>() else { return };
        let running = self.animation.clone().filter(is_running);
        let mut latest = None;
        if let (Some(running), Some(target), Some(layout)) = (&running, self.target, layout) {
            let measured = self.measure_target(html, running);
            if same_target(&measured, &target, layout) {
                // Already heading to the right place; let it continue.
                self.snapshot = None;
                return;
            }
            latest = Some(measured);
        }
        if let Some(animation) = self.animation.take() {
            animation.cancel();
        }
        self.clear_animation();
        let style = html.style();
        let _ = style.remove_property("transform");
        let _ = style.remove_property("transform-origin");
        let latest = latest.unwrap_or_else(|| natural_box(html));
        let Some(layout) = layout else {
            self.snapshot = None;
            return;
        };
        let Some(snapshot) = self.snapshot.take() else { return };

        let origin = if layout.size { 0.5 } else { 0.0 };
        let dx = axis_delta(latest.x, snapshot.x, origin);
        let dy = axis_delta(latest.y, snapshot.y, origin);
        let unchanged = near(dx.translate, 0.0, TRANSLATE_PRECISION)
            && near(dy.translate, 0.0, TRANSLATE_PRECISION)
            && (!layout.size || (near(dx.scale, 1.0, SCALE_PRECISION) && near(dy.scale, 1.0, SCALE_PRECISION)));
        if unchanged {
            return;
        }

        let mut invert = String::new();
        if dx.translate != 0.0 || dy.translate != 0.0 {
            invert = format!("translate3d({}px, {}px, 0)", dx.translate, dy.translate);
        }
        if layout.size && (dx.scale != 1.0 || dy.scale != 1.0) {
            if !invert.is_empty() {
                invert.push(' ');
            }
            invert.push_str(&format!("scale({}, {})", dx.scale, dy.scale));
        }
        if invert.is_empty() {
            invert.push_str("none");
        }
        let transform_origin = format!("{}% {}%", dx.origin * 100.0, dy.origin * 100.0);
        let _ = style.set_property("transform", &invert);
        let _ = style.set_property("transform-origin", &transform_origin);

        let duration = layout.duration.unwrap_or(DEFAULT_DURATION);
        let options = Object::new();
        set(&options, "duration", duration);
        set(&options, "easing", layout.easing.as_deref().unwrap_or(DEFAULT_EASING));
        set(&options, "fill", "forwards");
        let from = Object::new();
        set(&from, "transform", invert.as_str());
        set(&from, "transformOrigin", transform_origin.as_str());
        let to = Object::new();
        set(&to, "transform", "none");
        set(&to, "transformOrigin", transform_origin.as_str());
        let active = run(el, &from, &to, &options);
        self.animation = Some(active.clone());
        self.target = Some(latest);
        self.end_time = duration;
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(finished) = active.finished() else { return };
            if JsFuture::from(finished).await.is_ok() {
                on_finish(active);
            }
        });
    }

    /// The animation finished on its own: drop it and its projection styles. It is
    /// cancelled too, because a finished `fill: forwards` animation still holds
    /// `transform` and would make a later `commitStyles` throw.
    pub(super) fn finished(&mut self, active: web_sys::Animation) {
        if self.animation.as_ref() != Some(&active) {
            return;
        }
        active.cancel();
        if let Some(el) = active.effect().and_then(|e| Reflect::get(&e, &"target".into()).ok()).and_then(|t| t.dyn_into::<HtmlElement>().ok()) {
            let style = el.style();
            let _ = style.remove_property("transform");
            let _ = style.remove_property("transform-origin");
        }
        self.clear_animation();
        self.snapshot = None;
    }

    pub(super) fn remove(&mut self) {
        if let Some(animation) = self.animation.take() {
            animation.cancel();
        }
        self.clear_animation();
        self.snapshot = None;
    }

    /// Where a running animation ends: jump to its end, measure, and jump back.
    fn measure_target(&self, el: &HtmlElement, animation: &web_sys::Animation) -> LayoutBox {
        let current_time = animation.current_time();
        let was_running = is_running(animation);
        animation.set_current_time(Some(self.end_time));
        let measured = natural_box(el);
        animation.set_current_time(current_time);
        if was_running && !is_running(animation) {
            let _ = animation.play();
        }
        measured
    }
}

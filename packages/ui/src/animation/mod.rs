//! Entrance, exit and layout animation mixins, springs and tweens (Remix's
//! `@remix-run/ui/animation`).
//!
//! ```ignore
//! use rmx_ui::animation::{animate_entrance, animate_exit, animate_layout, spring_transition, Animate, SpringPreset};
//!
//! rsx! {
//!     <div
//!         style={format!("transition: {}", spring_transition(&["opacity", "transform"], SpringPreset::Snappy))}
//!         mix={[
//!             animate_entrance(Animate::new(120.0).opacity(0.0)),
//!             animate_exit(Animate::new(120.0).opacity(0.0)),
//!             animate_layout(true),
//!         ]}
//!     >"Saved filters"</div>
//! }
//! ```
//!
//! The mixins are browser-only (`{ ssr: false }` in Remix): the server renders the element
//! as if they were absent. Entrance animations run from the given keyframe to the
//! element's natural styles when it is inserted; exit animations keep a removed keyed
//! element in the document (via `persist_node`) while it animates to the keyframe, and a
//! keyed element rendered again before its exit finishes is reclaimed and animates back.
//! Layout animations measure the element before and after each update of its owner and
//! play a FLIP transform between the two boxes.

mod spring;
mod tween;

#[cfg(target_arch = "wasm32")]
mod web;

pub use spring::{spring, spring_transition, Spring, SpringOptions, SpringPreset};
pub use tween::{easings, tween, BezierCurve, Tween, TweenOptions, TweenStep};

/// Whether the user asked for reduced motion (`prefers-reduced-motion: reduce`), read
/// fresh on each call. `false` during server rendering or when `matchMedia` is
/// unavailable, so render functions can call it (Remix's `prefersReducedMotion`):
///
/// ```ignore
/// mix={animate_layout(if prefers_reduced_motion() { LayoutConfig::from(false) } else { Layout::spring(SpringPreset::Snappy).into() })}
/// ```
pub fn prefers_reduced_motion() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Some(window) = web_sys::window() else { return false };
        // Read matchMedia as a property so a missing or replaced one is handled like Remix.
        let Ok(match_media) = js_sys::Reflect::get(&window, &"matchMedia".into()) else { return false };
        let Some(match_media) = match_media.dyn_ref::<js_sys::Function>() else { return false };
        let Ok(list) = match_media.call1(&window, &"(prefers-reduced-motion: reduce)".into()) else { return false };
        js_sys::Reflect::get(&list, &"matches".into()).ok().and_then(|m| m.as_bool()).unwrap_or(false)
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

use rmx::{mixin, Element, HostMixin, Mixin, MixinHandle, NodeHandle};

/// How animation effects combine with the underlying value (WAAPI `composite`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Composite {
    Replace,
    Add,
    Accumulate,
}

/// An entrance or exit animation: one keyframe of styles plus WAAPI timing
/// (Remix's `AnimateMixinConfig`). Entrances animate from the keyframe to the element's
/// natural styles, exits from the natural styles to the keyframe.
#[derive(Clone, Debug, PartialEq)]
pub struct Animate {
    /// Milliseconds.
    pub duration: f64,
    pub easing: Option<String>,
    /// Milliseconds.
    pub delay: Option<f64>,
    pub composite: Option<Composite>,
    /// `false` skips the first entrance of each key under a parent (`initial: false`).
    pub initial: bool,
    /// Style properties for the keyframe, in CSS (`background-color`) or WAAPI
    /// (`backgroundColor`) spelling.
    pub styles: Vec<(String, String)>,
}

impl Animate {
    /// A keyframe animation lasting `duration` milliseconds.
    pub fn new(duration: f64) -> Animate {
        Animate { duration, easing: None, delay: None, composite: None, initial: true, styles: Vec::new() }
    }

    /// Timing from a spring (`...spring('snappy')`): its settling time and `linear()` easing.
    pub fn spring(options: impl Into<SpringOptions>) -> Animate {
        let s = spring(options);
        Animate::new(s.duration()).easing(s.easing())
    }

    pub fn easing(mut self, easing: impl Into<String>) -> Self {
        self.easing = Some(easing.into());
        self
    }

    pub fn delay(mut self, ms: f64) -> Self {
        self.delay = Some(ms);
        self
    }

    pub fn composite(mut self, composite: Composite) -> Self {
        self.composite = Some(composite);
        self
    }

    pub fn initial(mut self, initial: bool) -> Self {
        self.initial = initial;
        self
    }

    /// Add a style property to the keyframe.
    pub fn style(mut self, property: impl Into<String>, value: impl ToString) -> Self {
        self.styles.push((property.into(), value.to_string()));
        self
    }

    pub fn opacity(self, opacity: f64) -> Self {
        self.style("opacity", opacity)
    }

    pub fn transform(self, transform: impl ToString) -> Self {
        self.style("transform", transform)
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn default_enter() -> Animate {
        Animate::new(150.0).easing("ease-out").opacity(0.0)
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn default_exit() -> Animate {
        Animate::new(150.0).easing("ease-in").opacity(0.0)
    }
}

/// The argument of [`animate_entrance`] and [`animate_exit`]: `true` for the default
/// opacity fade, `false` or `None` to disable, or an [`Animate`].
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationConfig {
    Default,
    Off,
    Custom(Animate),
}

impl From<bool> for AnimationConfig {
    fn from(on: bool) -> Self {
        if on {
            AnimationConfig::Default
        } else {
            AnimationConfig::Off
        }
    }
}

impl From<Animate> for AnimationConfig {
    fn from(animate: Animate) -> Self {
        AnimationConfig::Custom(animate)
    }
}

impl From<Option<Animate>> for AnimationConfig {
    fn from(animate: Option<Animate>) -> Self {
        animate.map_or(AnimationConfig::Off, AnimationConfig::Custom)
    }
}

impl AnimationConfig {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn resolve(&self, default: fn() -> Animate) -> Option<Animate> {
        match self {
            AnimationConfig::Default => Some(default()),
            AnimationConfig::Off => None,
            AnimationConfig::Custom(a) => Some(a.clone()),
        }
    }
}

/// Layout animation options (Remix's `LayoutAnimationConfig`).
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Milliseconds; 200 when unset.
    pub duration: Option<f64>,
    /// `ease-out` when unset.
    pub easing: Option<String>,
    /// `false` animates position only, without scale projection.
    pub size: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { duration: None, easing: None, size: true }
    }
}

impl Layout {
    pub fn new() -> Layout {
        Layout::default()
    }

    /// Timing from a spring (`animateLayout({ ...spring('smooth') })`).
    pub fn spring(options: impl Into<SpringOptions>) -> Layout {
        let s = spring(options);
        Layout::new().duration(s.duration()).easing(s.easing())
    }

    pub fn duration(mut self, ms: f64) -> Self {
        self.duration = Some(ms);
        self
    }

    pub fn easing(mut self, easing: impl Into<String>) -> Self {
        self.easing = Some(easing.into());
        self
    }

    pub fn size(mut self, size: bool) -> Self {
        self.size = size;
        self
    }
}

/// The argument of [`animate_layout`]: `true` for the defaults, `false` or `None` to
/// disable, or a [`Layout`].
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutConfig(Option<Layout>);

impl From<bool> for LayoutConfig {
    fn from(on: bool) -> Self {
        LayoutConfig(on.then(Layout::default))
    }
}

impl From<Layout> for LayoutConfig {
    fn from(layout: Layout) -> Self {
        LayoutConfig(Some(layout))
    }
}

impl From<Option<Layout>> for LayoutConfig {
    fn from(layout: Option<Layout>) -> Self {
        LayoutConfig(layout)
    }
}

/// Animate the element from `config`'s keyframe to its natural styles when it is inserted.
pub fn animate_entrance(config: impl Into<AnimationConfig>) -> Mixin {
    mixin::<AnimateEntrance>(config.into())
}

/// Keep the removed element in the document while it animates to `config`'s keyframe.
/// Give the element a key so it can be reclaimed if it is rendered again mid-exit.
pub fn animate_exit(config: impl Into<AnimationConfig>) -> Mixin {
    mixin::<AnimateExit>(config.into())
}

/// Animate changes to the element's position and size between renders (FLIP).
pub fn animate_layout(config: impl Into<LayoutConfig>) -> Mixin {
    mixin::<AnimateLayout>(config.into())
}

#[doc(hidden)]
pub struct AnimateEntrance {
    config: AnimationConfig,
}

impl HostMixin for AnimateEntrance {
    type Args = AnimationConfig;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|this, _, event| {
            #[cfg(target_arch = "wasm32")]
            if let Some(config) = this.config.resolve(Animate::default_enter) {
                web::enter(event.node.element(), event.key.as_ref(), &config);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, event);
        });
        AnimateEntrance { config: AnimationConfig::Default }
    }

    fn render(&mut self, config: &AnimationConfig, _: &mut Element, _: &MixinHandle<Self>) {
        self.config.clone_from(config);
    }
}

#[doc(hidden)]
pub struct AnimateExit {
    config: AnimationConfig,
    node: Option<NodeHandle>,
}

impl HostMixin for AnimateExit {
    type Args = AnimationConfig;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|this, _, event| this.node = Some(event.node.clone()));
        h.on_reclaimed(|this, _, node| {
            this.node = Some(node.clone());
            #[cfg(target_arch = "wasm32")]
            web::retarget(node.element(), &this.config.resolve(Animate::default_enter).unwrap_or_else(Animate::default_enter));
        });
        h.on_before_remove(|this, _, event| {
            #[cfg(target_arch = "wasm32")]
            if let (Some(config), Some(node)) = (this.config.resolve(Animate::default_exit), this.node.clone()) {
                event.persist_node(move |signal| async move { web::exit(node.element(), &config, &signal).await });
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, event);
        });
        AnimateExit { config: AnimationConfig::Default, node: None }
    }

    fn render(&mut self, config: &AnimationConfig, _: &mut Element, _: &MixinHandle<Self>) {
        self.config.clone_from(config);
    }
}

#[doc(hidden)]
pub struct AnimateLayout {
    config: LayoutConfig,
    #[cfg(target_arch = "wasm32")]
    state: web::LayoutState,
}

impl HostMixin for AnimateLayout {
    type Args = LayoutConfig;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            h.on_before_update(|this, _, node| {
                if let Some(layout) = &this.config.0 {
                    this.state.before_update(node.element(), layout);
                }
            });
            h.on_commit(|this, h, node| {
                let h = h.clone();
                this.state.commit(node.element(), this.config.0.as_ref(), move |active| {
                    h.with(|this, _| this.state.finished(active));
                });
            });
            h.on_remove(|this, _| this.state.remove());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = h;
        AnimateLayout {
            config: LayoutConfig(Some(Layout::default())),
            #[cfg(target_arch = "wasm32")]
            state: web::LayoutState::default(),
        }
    }

    fn render(&mut self, config: &LayoutConfig, _: &mut Element, _: &MixinHandle<Self>) {
        self.config.clone_from(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::{render_to_string, rsx};

    #[test]
    fn animation_mixins_do_not_render_on_the_server() {
        let html = render_to_string(&rsx! {
            <div key="a" mix={[
                animate_entrance(Animate::spring(SpringPreset::Snappy).opacity(0.0).transform("scale(0)")),
                animate_exit(true),
                animate_layout(Layout::new().size(false)),
            ]}>"Saved"</div>
        });
        assert_eq!(html, "<div>Saved</div>");
    }

    #[test]
    fn prefers_reduced_motion_is_false_on_the_server() {
        assert!(!prefers_reduced_motion());
    }

    #[test]
    fn configs() {
        assert_eq!(AnimationConfig::from(false), AnimationConfig::Off);
        assert_eq!(AnimationConfig::from(None::<Animate>), AnimationConfig::Off);
        assert_eq!(AnimationConfig::from(true).resolve(Animate::default_exit).unwrap().easing.as_deref(), Some("ease-in"));
        let a = Animate::spring(SpringPreset::Snappy).opacity(0.0).delay(10.0).initial(false);
        assert_eq!(a.duration, 350.0);
        assert!(a.easing.as_deref().is_some_and(|e| e.starts_with("linear(")));
        assert_eq!(a.styles, vec![("opacity".to_string(), "0".to_string())]);
        assert_eq!(LayoutConfig::from(false), LayoutConfig(None));
        assert_eq!(Layout::spring(SpringPreset::Smooth).duration, Some(1050.0));
    }
}

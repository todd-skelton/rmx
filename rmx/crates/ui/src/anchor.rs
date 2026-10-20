//! Position a floating element against an anchor element or a point (Remix's
//! `@remix-run/ui/anchor`).
//!
//! ```ignore
//! use rmx_ui::anchor::{anchor, AnchorOptions, AnchorTarget, Placement};
//!
//! // In the browser, with both elements in the document:
//! let tracking = anchor(&floating, AnchorTarget::Element(button), AnchorOptions::new(Placement::BottomStart).offset(8.0));
//! // ... later: drop(tracking) stops following the anchor.
//! ```
//!
//! [`anchor`] places the element on the preferred side, flips to the opposite side when
//! that fits the viewport better, keeps it inside the viewport (16px padding), limits its
//! size to the room available, and records the chosen side in `data-anchor-placement`.
//! It keeps following scrolling, resizing and size changes until the returned
//! [`Anchored`] is dropped. The geometry is plain Rust; only measuring and writing styles
//! touch the DOM.

// The geometry is driven by the browser-only `anchor` (and the tests).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Side and alignment of the floating element relative to its anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    Top,
    TopStart,
    TopEnd,
    #[default]
    Bottom,
    BottomStart,
    BottomEnd,
    Left,
    LeftStart,
    LeftEnd,
    Right,
    RightStart,
    RightEnd,
}

impl Placement {
    /// The `data-anchor-placement` value (`"bottom-start"` and so on).
    pub fn as_str(self) -> &'static str {
        match self {
            Placement::Top => "top",
            Placement::TopStart => "top-start",
            Placement::TopEnd => "top-end",
            Placement::Bottom => "bottom",
            Placement::BottomStart => "bottom-start",
            Placement::BottomEnd => "bottom-end",
            Placement::Left => "left",
            Placement::LeftStart => "left-start",
            Placement::LeftEnd => "left-end",
            Placement::Right => "right",
            Placement::RightStart => "right-start",
            Placement::RightEnd => "right-end",
        }
    }

    pub fn opposite(self) -> Placement {
        use Placement::*;
        match self {
            Top => Bottom,
            TopStart => BottomStart,
            TopEnd => BottomEnd,
            Bottom => Top,
            BottomStart => TopStart,
            BottomEnd => TopEnd,
            Left => Right,
            LeftStart => RightStart,
            LeftEnd => RightEnd,
            Right => Left,
            RightStart => LeftStart,
            RightEnd => LeftEnd,
        }
    }

    fn is_top(self) -> bool {
        matches!(self, Placement::Top | Placement::TopStart | Placement::TopEnd)
    }

    fn is_bottom(self) -> bool {
        matches!(self, Placement::Bottom | Placement::BottomStart | Placement::BottomEnd)
    }

    fn is_left(self) -> bool {
        matches!(self, Placement::Left | Placement::LeftStart | Placement::LeftEnd)
    }

    fn is_right(self) -> bool {
        matches!(self, Placement::Right | Placement::RightStart | Placement::RightEnd)
    }

    fn is_horizontal(self) -> bool {
        self.is_left() || self.is_right()
    }

    fn is_vertical(self) -> bool {
        self.is_top() || self.is_bottom()
    }
}

/// Viewport coordinates to anchor to instead of an element (a pointer position, say).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorPoint {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Placement and offsets for [`anchor`] (Remix's `AnchorOptions`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorOptions {
    /// Preferred side and alignment; flipped when the opposite side fits better.
    pub placement: Placement,
    /// Align inside the anchor's edge instead of outside it.
    pub inset: bool,
    /// Selector for a descendant of the floating element to align with the anchor.
    pub relative_to: Option<String>,
    /// Pixels along the placement axis.
    pub offset: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl AnchorOptions {
    pub fn new(placement: Placement) -> Self {
        AnchorOptions { placement, ..Default::default() }
    }

    pub fn offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    pub fn offset_xy(mut self, x: f64, y: f64) -> Self {
        self.offset_x = x;
        self.offset_y = y;
        self
    }

    pub fn inset(mut self, inset: bool) -> Self {
        self.inset = inset;
        self
    }

    pub fn relative_to(mut self, selector: impl Into<String>) -> Self {
        self.relative_to = Some(selector.into());
        self
    }
}

const VIEWPORT_PADDING: f64 = 16.0;

/// A rectangle in viewport pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.left + self.width
    }

    fn bottom(&self) -> f64 {
        self.top + self.height
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn changed(&self, other: &Rect) -> bool {
        (self.top - other.top).abs() >= 1.0
            || (self.left - other.left).abs() >= 1.0
            || (self.right() - other.right()).abs() >= 1.0
            || (self.bottom() - other.bottom()).abs() >= 1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// The window's size and scroll position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Viewport {
    pub width: f64,
    pub height: f64,
    pub scroll_x: f64,
    pub scroll_y: f64,
}

impl Viewport {
    fn client_bounds(&self, padding: f64) -> Bounds {
        Bounds { left: padding, top: padding, right: self.width - padding, bottom: self.height - padding }
    }

    fn document_bounds(&self, fixed: bool, padding: f64) -> Bounds {
        if fixed {
            return self.client_bounds(padding);
        }
        Bounds {
            left: self.scroll_x + padding,
            top: self.scroll_y + padding,
            right: self.scroll_x + self.width - padding,
            bottom: self.scroll_y + self.height - padding,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    top: f64,
    left: f64,
}

/// Measurements of the floating element (Remix's `FloatingDimensions`, minus the element).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Dims {
    pub width: f64,
    pub height: f64,
    pub scroll_container_offset_x: f64,
    pub scroll_container_offset_y: f64,
    pub scroll_viewport_width: f64,
    pub scroll_viewport_height: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub relative_width: Option<f64>,
    pub relative_height: Option<f64>,
    pub relative_offset_x: Option<f64>,
    pub relative_offset_y: Option<f64>,
}

impl Dims {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn changed(&self, other: &Dims) -> bool {
        let num = |a: f64, b: f64| (a - b).abs() >= 1.0;
        let opt = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => num(a, b),
            (a, b) => a.is_some() != b.is_some(),
        };
        num(self.width, other.width)
            || num(self.height, other.height)
            || num(self.scroll_viewport_width, other.scroll_viewport_width)
            || num(self.scroll_viewport_height, other.scroll_viewport_height)
            || num(self.scroll_width, other.scroll_width)
            || num(self.scroll_height, other.scroll_height)
            || num(self.scroll_container_offset_x, other.scroll_container_offset_x)
            || num(self.scroll_container_offset_y, other.scroll_container_offset_y)
            || opt(self.relative_width, other.relative_width)
            || opt(self.relative_height, other.relative_height)
            || opt(self.relative_offset_x, other.relative_offset_x)
            || opt(self.relative_offset_y, other.relative_offset_y)
    }

    /// The size used for placement: the `relative_to` element's when there is one.
    fn placement_size(&self, relative: bool) -> (f64, f64) {
        let w = match self.relative_width {
            Some(w) if relative && w != 0.0 => w,
            _ => self.width,
        };
        let h = match self.relative_height {
            Some(h) if relative && h != 0.0 => h,
            _ => self.height,
        };
        (w, h)
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn constrain_to_axis(mut position: f64, size: f64, min: f64, max: f64) -> f64 {
    let end = position + size;
    if end > max {
        position -= end - max;
    }
    if position < min {
        position = min;
    }
    position
}

fn calculate_position(placement: Placement, a: Rect, fw: f64, fh: f64, inset: bool) -> Pos {
    use Placement::*;
    let mut top = a.bottom();
    let mut left = a.left + (a.width - fw) / 2.0;
    match placement {
        Bottom => {}
        BottomStart => left = a.left,
        BottomEnd => left = a.right() - fw,
        Top => top = a.top - fh,
        TopStart => {
            top = a.top - fh;
            left = a.left;
        }
        TopEnd => {
            top = a.top - fh;
            left = a.right() - fw;
        }
        Left => {
            top = a.top + (a.height - fh) / 2.0;
            left = a.left - fw;
        }
        LeftStart => {
            top = a.top;
            left = a.left - fw;
        }
        LeftEnd => {
            top = a.bottom() - fh;
            left = a.left - fw;
        }
        Right => {
            top = a.top + (a.height - fh) / 2.0;
            left = a.right();
        }
        RightStart => {
            top = a.top;
            left = a.right();
        }
        RightEnd => {
            top = a.bottom() - fh;
            left = a.right();
        }
    }
    if inset {
        if placement.is_bottom() {
            top = a.bottom() - fh;
        } else if placement.is_top() {
            top = a.top;
        } else if placement.is_left() {
            left = a.left;
        } else if placement.is_right() {
            left = a.right() - fw;
        }
    }
    Pos { top, left }
}

fn apply_offset(mut p: Pos, placement: Placement, options: &AnchorOptions) -> Pos {
    let offset = options.offset;
    if placement.is_top() {
        p.top -= offset;
    } else if placement.is_bottom() {
        p.top += offset;
    } else if placement.is_left() {
        p.left -= offset;
    } else if placement.is_right() {
        p.left += offset;
    }
    p.left += options.offset_x;
    p.top += options.offset_y;
    p
}

/// Where the floating element would land for `placement`, in viewport coordinates.
fn floating_bounds(placement: Placement, a: Rect, d: &Dims, options: &AnchorOptions) -> Bounds {
    let relative = options.relative_to.is_some();
    let (pw, ph) = d.placement_size(relative);
    let mut p = apply_offset(calculate_position(placement, a, pw, ph, options.inset), placement, options);
    if let Some(x) = d.relative_offset_x {
        p.left -= x;
    }
    if let Some(y) = d.relative_offset_y {
        p.top -= y;
    }
    Bounds { left: p.left, top: p.top, right: p.left + d.width, bottom: p.top + d.height }
}

struct Score {
    overflow: f64,
    total: usize,
}

impl Score {
    fn perfect(&self) -> bool {
        self.total == 4
    }
}

fn placement_score(placement: Placement, a: Rect, d: &Dims, options: &AnchorOptions, vp: &Viewport) -> Score {
    let viewport = vp.client_bounds(VIEWPORT_PADDING);
    let b = floating_bounds(placement, a, d, options);
    let total = [b.left >= viewport.left, b.right <= viewport.right, b.top >= viewport.top, b.bottom <= viewport.bottom]
        .into_iter()
        .filter(|ok| *ok)
        .count();
    let overflow = (viewport.left - b.left).max(0.0)
        + (b.right - viewport.right).max(0.0)
        + (viewport.top - b.top).max(0.0)
        + (b.bottom - viewport.bottom).max(0.0);
    Score { overflow, total }
}

/// The preferred placement, or its opposite when that fits the viewport better.
pub(crate) fn flipped_placement(a: Rect, d: &Dims, options: &AnchorOptions, anchor_in_flow: bool, vp: &Viewport) -> Placement {
    let placement = options.placement;
    let (_, ph) = d.placement_size(options.relative_to.is_some());
    if placement.is_top() && anchor_in_flow && vp.scroll_y > 0.0 && a.top - ph < 0.0 {
        return placement;
    }
    let original = placement_score(placement, a, d, options, vp);
    if original.perfect() {
        return placement;
    }
    let opposite = placement.opposite();
    let flipped = placement_score(opposite, a, d, options, vp);
    if flipped.perfect() || flipped.total > original.total || (flipped.total == original.total && flipped.overflow < original.overflow) {
        return opposite;
    }
    placement
}

fn available_width(placement: Placement, left: f64, width: f64, viewport: &Bounds) -> f64 {
    if placement.is_left() {
        left + width - viewport.left
    } else if placement.is_right() {
        viewport.right - left
    } else {
        viewport.right - viewport.left
    }
}

fn available_height(placement: Placement, top: f64, height: f64, viewport: &Bounds) -> f64 {
    if placement.is_top() {
        top + height - viewport.top
    } else if placement.is_bottom() {
        viewport.bottom - top
    } else {
        viewport.bottom - viewport.top
    }
}

fn document_position(placement: Placement, a: Rect, d: &Dims, options: &AnchorOptions, fixed: bool, vp: &Viewport) -> Pos {
    let (pw, ph) = d.placement_size(options.relative_to.is_some());
    let mut p = calculate_position(placement, a, pw, ph, options.inset);
    if !fixed {
        p.top += vp.scroll_y;
        p.left += vp.scroll_x;
    }
    p = apply_offset(p, placement, options);
    p.left -= d.scroll_container_offset_x;
    p.top -= d.scroll_container_offset_y;
    if let Some(x) = d.relative_offset_x {
        p.left -= x;
    }
    if let Some(y) = d.relative_offset_y {
        p.top -= y;
    }
    p
}

/// Position on one axis plus the scroll offset of the floating element's scroll container
/// that lines its `relative_to` element up with the anchor (Remix's
/// `solveRelativeAlignmentAxis`).
fn solve_relative_axis(current_scroll: f64, desired: f64, natural: f64, visible: f64, min: f64, max: f64) -> (f64, f64) {
    let max_scroll = (natural - visible).max(0.0);
    let mut scroll = clamp(current_scroll, 0.0, max_scroll);
    if max_scroll == 0.0 {
        return (clamp(desired, min, max), scroll);
    }
    let exact_min = (min - desired).max(0.0);
    let exact_max = max_scroll.min(max - desired);
    if exact_min <= exact_max {
        scroll = clamp(scroll, exact_min, exact_max);
    } else if desired > max {
        scroll = 0.0;
    } else if desired + max_scroll < min {
        scroll = max_scroll;
    }
    (clamp(desired + scroll, min, max), scroll)
}

/// The result of laying out the floating element once.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub placement: Placement,
    pub top: f64,
    pub left: f64,
    /// Scroll offsets for the scroll container, when `relative_to` alignment set them.
    pub scroll_top: Option<f64>,
    pub scroll_left: Option<f64>,
}

/// Size limits for the floating element: `(max_width, max_height)`, `None` when it fits.
pub(crate) fn size_limits(a: Rect, natural: &Dims, options: &AnchorOptions, fixed: bool, anchor_in_flow: bool, vp: &Viewport) -> (Placement, Option<f64>, Option<f64>) {
    let placement = flipped_placement(a, natural, options, anchor_in_flow, vp);
    let viewport = vp.document_bounds(fixed, VIEWPORT_PADDING);
    let p = document_position(placement, a, natural, options, fixed, vp);
    let w = available_width(placement, p.left, natural.width, &viewport).max(0.0);
    let h = available_height(placement, p.top, natural.height, &viewport).max(0.0);
    let limit = |avail: f64, size: f64| (avail < size).then(|| avail.floor());
    (placement, limit(w, natural.width), limit(h, natural.height))
}

/// Final position once the size limits are applied and `dims` remeasured.
pub(crate) fn final_position(
    placement: Placement,
    a: Rect,
    d: &Dims,
    scroll: (f64, f64),
    options: &AnchorOptions,
    fixed: bool,
    vp: &Viewport,
) -> Layout {
    let viewport = vp.document_bounds(fixed, VIEWPORT_PADDING);
    let mut p = document_position(placement, a, d, options, fixed, vp);
    let (min_x, min_y, max_x, max_y) = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    let x_range = (min_x, min_x.max(max_x - d.width));
    let y_range = (min_y, min_y.max(max_y - d.height));
    let relative = options.relative_to.is_some();
    let (scroll_top, scroll_left) = scroll;
    let mut layout = Layout { placement, ..Default::default() };
    let solve_y = |p: &mut Pos, layout: &mut Layout| {
        let (pos, s) = solve_relative_axis(scroll_top, p.top, d.scroll_height, d.scroll_viewport_height, y_range.0, y_range.1);
        layout.scroll_top = Some(s);
        p.top = pos;
    };
    let solve_x = |p: &mut Pos, layout: &mut Layout| {
        let (pos, s) = solve_relative_axis(scroll_left, p.left, d.scroll_width, d.scroll_viewport_width, x_range.0, x_range.1);
        layout.scroll_left = Some(s);
        p.left = pos;
    };

    if placement.is_horizontal() {
        if relative && d.relative_offset_y.is_some() {
            solve_y(&mut p, &mut layout);
        } else {
            p.top = constrain_to_axis(p.top, d.height, min_y, max_y);
        }
        if relative {
            if d.relative_offset_x.is_some() {
                solve_x(&mut p, &mut layout);
            } else {
                p.left = clamp(p.left, x_range.0, x_range.1);
            }
        }
    } else if placement.is_vertical() {
        if relative && d.relative_offset_x.is_some() {
            solve_x(&mut p, &mut layout);
        } else {
            p.left = constrain_to_axis(p.left, d.width, min_x, max_x);
        }
        if !fixed && vp.scroll_y > 0.0 && p.top < viewport.top {
            p.top = viewport.top;
        }
        if relative {
            if d.relative_offset_y.is_some() {
                solve_y(&mut p, &mut layout);
            } else {
                p.top = clamp(p.top, y_range.0, y_range.1);
            }
        }
    }

    // An anchor scrolled out of view pins the element to that edge of the viewport.
    if placement.is_horizontal() {
        if a.right() <= 0.0 {
            p.left = min_x;
        } else if a.left >= vp.width {
            p.left = max_x - d.width;
        }
    } else if placement.is_vertical() {
        if a.bottom() <= 0.0 {
            p.top = min_y;
        } else if a.top >= vp.height {
            p.top = max_y - d.height;
        }
    }
    layout.top = p.top;
    layout.left = p.left;
    layout
}

#[cfg(target_arch = "wasm32")]
pub use dom::{anchor, Anchored, AnchorTarget};

#[cfg(target_arch = "wasm32")]
mod dom {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use super::*;

    /// What the floating element is positioned against.
    #[derive(Clone, Debug)]
    pub enum AnchorTarget {
        Element(web_sys::HtmlElement),
        Point(AnchorPoint),
    }

    fn window() -> web_sys::Window {
        web_sys::window().expect("window")
    }

    fn viewport() -> Viewport {
        let w = window();
        let num = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        Viewport {
            width: num(w.inner_width()),
            height: num(w.inner_height()),
            scroll_x: w.scroll_x().unwrap_or(0.0),
            scroll_y: w.scroll_y().unwrap_or(0.0),
        }
    }

    fn rect_of(el: &web_sys::Element) -> Rect {
        let r = el.get_bounding_client_rect();
        Rect { left: r.left(), top: r.top(), width: r.width(), height: r.height() }
    }

    fn anchor_rect(target: &AnchorTarget) -> Rect {
        match target {
            AnchorTarget::Element(el) => rect_of(el),
            AnchorTarget::Point(p) => Rect { left: p.x, top: p.y, width: p.width, height: p.height },
        }
    }

    fn computed(el: &web_sys::Element, property: &str) -> String {
        window().get_computed_style(el).ok().flatten().and_then(|s| s.get_property_value(property).ok()).unwrap_or_default()
    }

    fn scrollable(value: &str) -> bool {
        matches!(value, "auto" | "scroll" | "overlay")
    }

    fn relative_scroll_container(floating: &web_sys::HtmlElement, relative: &web_sys::HtmlElement) -> web_sys::HtmlElement {
        let mut current = relative.parent_element();
        while let Some(el) = current {
            if el == **floating {
                break;
            }
            if scrollable(&computed(&el, "overflow-x")) || scrollable(&computed(&el, "overflow-y")) {
                if let Ok(html) = el.clone().dyn_into::<web_sys::HtmlElement>() {
                    return html;
                }
            }
            current = el.parent_element();
        }
        floating.clone()
    }

    fn read_dims(floating: &web_sys::HtmlElement, relative_to: Option<&str>) -> (Dims, web_sys::HtmlElement) {
        let relative = relative_to
            .and_then(|s| floating.query_selector(s).ok().flatten())
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
        let container = match &relative {
            Some(rel) => relative_scroll_container(floating, rel),
            None => floating.clone(),
        };
        let mut d = Dims {
            width: floating.offset_width() as f64,
            height: floating.offset_height() as f64,
            scroll_viewport_width: container.offset_width() as f64,
            scroll_viewport_height: container.offset_height() as f64,
            scroll_width: (container.scroll_width().max(container.offset_width())) as f64,
            scroll_height: (container.scroll_height().max(container.offset_height())) as f64,
            ..Default::default()
        };
        if let Some(rel) = &relative {
            let f = rect_of(floating);
            let c = rect_of(&container);
            let r = rect_of(rel);
            d.scroll_container_offset_x = c.left - f.left;
            d.scroll_container_offset_y = c.top - f.top;
            d.relative_offset_x = Some(r.left - c.left + container.scroll_left() as f64);
            d.relative_offset_y = Some(r.top - c.top + container.scroll_top() as f64);
            d.relative_width = Some(rel.offset_width() as f64);
            d.relative_height = Some(rel.offset_height() as f64);
        }
        (d, container)
    }

    /// Measure, laying the element out off screen first if it is not displayed.
    fn dims(floating: &web_sys::HtmlElement, relative_to: Option<&str>, ignore_max_size: bool) -> (Dims, web_sys::HtmlElement) {
        let style = floating.style();
        let get = |p: &str| style.get_property_value(p).unwrap_or_default();
        let (position, left, display, max_w, max_h) = (get("position"), get("left"), get("display"), get("max-width"), get("max-height"));
        if ignore_max_size {
            let _ = style.set_property("max-width", "");
            let _ = style.set_property("max-height", "");
        }
        let temporary = floating.offset_width() == 0;
        if temporary {
            let _ = style.set_property("position", "absolute");
            let _ = style.set_property("left", "-9999px");
            let _ = style.set_property("display", "block");
        }
        let result = read_dims(floating, relative_to);
        if temporary {
            let _ = style.set_property("position", &position);
            let _ = style.set_property("left", &left);
            let _ = style.set_property("display", &display);
        }
        if ignore_max_size {
            let _ = style.set_property("max-width", &max_w);
            let _ = style.set_property("max-height", &max_h);
        }
        result
    }

    struct Tracker {
        floating: web_sys::HtmlElement,
        target: AnchorTarget,
        options: AnchorOptions,
        fixed: bool,
        anchor_in_flow: bool,
        last: Option<(Rect, Dims)>,
    }

    impl Tracker {
        fn update(&mut self, anchor: Option<Rect>) {
            let a = anchor.unwrap_or_else(|| anchor_rect(&self.target));
            let relative_to = self.options.relative_to.as_deref();
            let vp = viewport();
            let (natural, _) = dims(&self.floating, relative_to, true);
            let (placement, max_w, max_h) = size_limits(a, &natural, &self.options, self.fixed, self.anchor_in_flow, &vp);
            let _ = self.floating.set_attribute("data-anchor-placement", placement.as_str());
            let style = self.floating.style();
            let px = |v: Option<f64>| v.map(|v| format!("{v}px")).unwrap_or_default();
            let _ = style.set_property("max-width", &px(max_w));
            let _ = style.set_property("max-height", &px(max_h));
            let (d, container) = dims(&self.floating, relative_to, false);
            let scroll = (container.scroll_top() as f64, container.scroll_left() as f64);
            let layout = final_position(placement, a, &d, scroll, &self.options, self.fixed, &vp);
            if let Some(top) = layout.scroll_top {
                container.set_scroll_top(top as i32);
            }
            if let Some(left) = layout.scroll_left {
                container.set_scroll_left(left as i32);
            }
            let _ = style.set_property("position", if self.fixed { "fixed" } else { "absolute" });
            let _ = style.set_property("inset", "auto");
            let _ = style.set_property("top", &format!("{}px", layout.top));
            let _ = style.set_property("left", &format!("{}px", layout.left));
            self.last = Some((a, d));
        }

        fn poll(&mut self) {
            let a = anchor_rect(&self.target);
            let (d, _) = dims(&self.floating, self.options.relative_to.as_deref(), false);
            let changed = match &self.last {
                Some((la, ld)) => a.changed(la) || d.changed(ld),
                None => true,
            };
            if changed {
                self.update(Some(a));
            }
        }
    }

    type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

    /// Keeps a floating element positioned against its anchor; dropping it stops tracking
    /// and leaves the last styles in place.
    pub struct Anchored {
        frame: Rc<std::cell::Cell<i32>>,
        frame_loop: FrameLoop,
        on_change: Closure<dyn FnMut()>,
    }

    impl Drop for Anchored {
        fn drop(&mut self) {
            let w = window();
            let _ = w.cancel_animation_frame(self.frame.get());
            self.frame_loop.borrow_mut().take();
            let f = self.on_change.as_ref().unchecked_ref();
            let _ = w.remove_event_listener_with_callback("scroll", f);
            let _ = w.remove_event_listener_with_callback("resize", f);
        }
    }

    /// Position `floating` against `target` now and keep it there until the returned
    /// [`Anchored`] is dropped.
    pub fn anchor(floating: &web_sys::HtmlElement, target: AnchorTarget, options: AnchorOptions) -> Anchored {
        let anchor_in_flow = match &target {
            AnchorTarget::Element(el) => computed(el, "position") != "fixed",
            AnchorTarget::Point(_) => false,
        };
        let fixed = floating.has_attribute("popover") || !anchor_in_flow;
        let tracker = Rc::new(RefCell::new(Tracker { floating: floating.clone(), target, options, fixed, anchor_in_flow, last: None }));
        tracker.borrow_mut().update(None);

        let frame = Rc::new(std::cell::Cell::new(0));
        let frame_loop: FrameLoop = Rc::default();
        let (polled, next, slot) = (tracker.clone(), frame.clone(), Rc::downgrade(&frame_loop));
        *frame_loop.borrow_mut() = Some(Closure::new(move || {
            polled.borrow_mut().poll();
            if let Some(slot) = slot.upgrade() {
                if let Some(f) = slot.borrow().as_ref() {
                    next.set(window().request_animation_frame(f.as_ref().unchecked_ref()).unwrap_or(0));
                }
            }
        }));
        if let Some(f) = frame_loop.borrow().as_ref() {
            frame.set(window().request_animation_frame(f.as_ref().unchecked_ref()).unwrap_or(0));
        }
        let on_change = Closure::<dyn FnMut()>::new(move || tracker.borrow_mut().update(None));
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(true);
        let w = window();
        let f = on_change.as_ref().unchecked_ref();
        let _ = w.add_event_listener_with_callback_and_add_event_listener_options("scroll", f, &options);
        let _ = w.add_event_listener_with_callback_and_add_event_listener_options("resize", f, &options);
        Anchored { frame, frame_loop, on_change }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Viewport {
        Viewport { width: 800.0, height: 600.0, scroll_x: 0.0, scroll_y: 0.0 }
    }

    fn dims(w: f64, h: f64) -> Dims {
        Dims { width: w, height: h, scroll_viewport_width: w, scroll_viewport_height: h, scroll_width: w, scroll_height: h, ..Default::default() }
    }

    const BUTTON: Rect = Rect { left: 100.0, top: 100.0, width: 80.0, height: 30.0 };

    #[test]
    fn places_below_with_offset() {
        let options = AnchorOptions::new(Placement::BottomStart).offset(8.0);
        let d = dims(200.0, 100.0);
        let (placement, w, h) = size_limits(BUTTON, &d, &options, true, true, &vp());
        assert_eq!((placement, w, h), (Placement::BottomStart, None, None));
        let layout = final_position(placement, BUTTON, &d, (0.0, 0.0), &options, true, &vp());
        assert_eq!((layout.top, layout.left), (138.0, 100.0));
    }

    #[test]
    fn centers_and_aligns_ends() {
        let d = dims(100.0, 50.0);
        let at = |p| {
            let l = final_position(p, BUTTON, &d, (0.0, 0.0), &AnchorOptions::new(p), true, &vp());
            (l.top, l.left)
        };
        assert_eq!(at(Placement::Bottom), (130.0, 90.0));
        assert_eq!(at(Placement::TopEnd), (50.0, 80.0));
        assert_eq!(at(Placement::RightStart), (100.0, 180.0));
        // The main axis is not clamped (flipping handles that), so Left can start at 0.
        assert_eq!(at(Placement::Left), (90.0, 0.0));
    }

    #[test]
    fn flips_when_the_opposite_side_fits() {
        let near_bottom = Rect { top: 540.0, ..BUTTON };
        let options = AnchorOptions::new(Placement::Bottom);
        assert_eq!(flipped_placement(near_bottom, &dims(100.0, 100.0), &options, true, &vp()), Placement::Top);
        assert_eq!(flipped_placement(BUTTON, &dims(100.0, 100.0), &options, true, &vp()), Placement::Bottom);
    }

    #[test]
    fn limits_height_to_the_room_available() {
        let d = dims(100.0, 1000.0);
        let (placement, _, h) = size_limits(BUTTON, &d, &AnchorOptions::new(Placement::Bottom), true, true, &vp());
        assert_eq!(placement, Placement::Bottom);
        assert_eq!(h, Some(454.0), "600 - 16 padding - 130 anchor bottom");
    }

    #[test]
    fn inset_aligns_inside_the_edge() {
        let d = dims(100.0, 50.0);
        let options = AnchorOptions::new(Placement::Left).inset(true);
        let l = final_position(Placement::Left, BUTTON, &d, (0.0, 0.0), &options, true, &vp());
        assert_eq!(l.left, 100.0);
    }

    #[test]
    fn relative_alignment_scrolls_the_container() {
        // Clamped without a scroll range.
        assert_eq!(solve_relative_axis(0.0, -50.0, 100.0, 100.0, 16.0, 500.0), (16.0, 0.0));
        // With room to scroll, scrolling keeps the desired position exactly.
        assert_eq!(solve_relative_axis(0.0, -50.0, 400.0, 100.0, 16.0, 500.0), (16.0, 66.0));
    }

    #[test]
    fn anchor_out_of_view_pins_to_the_edge() {
        let above = Rect { top: -100.0, ..BUTTON };
        let d = dims(100.0, 50.0);
        let l = final_position(Placement::Bottom, above, &d, (0.0, 0.0), &AnchorOptions::default(), true, &vp());
        assert_eq!(l.top, 16.0);
    }
}

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

use std::fmt;
use std::sync::Arc;

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

/// A distance for [`AnchorOptions`]: pixels, or computed from the floating element's
/// rendered size each time it is positioned (Remix's `AnchorOffsetValue`, a number or a
/// function of the floating element).
#[derive(Clone)]
pub enum Offset {
    Px(f64),
    Sized(Arc<dyn Fn(FloatingSize) -> f64 + Send + Sync>),
}

impl Offset {
    /// An offset from the floating element's size: `Offset::sized(|f| f.height / 10.0)`.
    pub fn sized(f: impl Fn(FloatingSize) -> f64 + Send + Sync + 'static) -> Offset {
        Offset::Sized(Arc::new(f))
    }

    pub fn resolve(&self, size: FloatingSize) -> f64 {
        match self {
            Offset::Px(px) => *px,
            Offset::Sized(f) => f(size),
        }
    }
}

impl Default for Offset {
    fn default() -> Offset {
        Offset::Px(0.0)
    }
}

impl From<f64> for Offset {
    fn from(px: f64) -> Offset {
        Offset::Px(px)
    }
}

impl fmt::Debug for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Offset::Px(px) => write!(f, "Px({px})"),
            Offset::Sized(_) => f.write_str("Sized(..)"),
        }
    }
}

/// Pixel offsets compare by value, sized offsets by identity.
impl PartialEq for Offset {
    fn eq(&self, other: &Offset) -> bool {
        match (self, other) {
            (Offset::Px(a), Offset::Px(b)) => a == b,
            (Offset::Sized(a), Offset::Sized(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// The floating element's rendered size (`offsetWidth` and `offsetHeight`), passed to
/// [`Offset::sized`] functions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatingSize {
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
    /// Distance along the placement axis.
    pub offset: Offset,
    /// Horizontal adjustment after placement.
    pub offset_x: Offset,
    /// Vertical adjustment after placement.
    pub offset_y: Offset,
}

impl AnchorOptions {
    pub fn new(placement: Placement) -> Self {
        AnchorOptions { placement, ..Default::default() }
    }

    pub fn offset(mut self, offset: impl Into<Offset>) -> Self {
        self.offset = offset.into();
        self
    }

    pub fn offset_x(mut self, offset: impl Into<Offset>) -> Self {
        self.offset_x = offset.into();
        self
    }

    pub fn offset_y(mut self, offset: impl Into<Offset>) -> Self {
        self.offset_y = offset.into();
        self
    }

    pub fn offset_xy(self, x: impl Into<Offset>, y: impl Into<Offset>) -> Self {
        self.offset_x(x).offset_y(y)
    }

    pub fn inset(mut self, inset: bool) -> Self {
        self.inset = inset;
        self
    }

    pub fn relative_to(mut self, selector: impl Into<String>) -> Self {
        self.relative_to = Some(selector.into());
        self
    }

    /// The options with offsets resolved against the floating element's current size.
    pub(crate) fn geometry(&self, size: FloatingSize) -> Geometry {
        Geometry {
            placement: self.placement,
            inset: self.inset,
            relative: self.relative_to.is_some(),
            offset: self.offset.resolve(size),
            offset_x: self.offset_x.resolve(size),
            offset_y: self.offset_y.resolve(size),
        }
    }
}

/// [`AnchorOptions`] for one positioning pass, offsets in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Geometry {
    pub placement: Placement,
    pub inset: bool,
    pub relative: bool,
    pub offset: f64,
    pub offset_x: f64,
    pub offset_y: f64,
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

fn apply_offset(mut p: Pos, placement: Placement, g: &Geometry) -> Pos {
    let offset = g.offset;
    if placement.is_top() {
        p.top -= offset;
    } else if placement.is_bottom() {
        p.top += offset;
    } else if placement.is_left() {
        p.left -= offset;
    } else if placement.is_right() {
        p.left += offset;
    }
    p.left += g.offset_x;
    p.top += g.offset_y;
    p
}

/// Where the floating element would land for `placement`, in viewport coordinates.
fn floating_bounds(placement: Placement, a: Rect, d: &Dims, g: &Geometry) -> Bounds {
    let relative = g.relative;
    let (pw, ph) = d.placement_size(relative);
    let mut p = apply_offset(calculate_position(placement, a, pw, ph, g.inset), placement, g);
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

fn placement_score(placement: Placement, a: Rect, d: &Dims, g: &Geometry, vp: &Viewport) -> Score {
    let viewport = vp.client_bounds(VIEWPORT_PADDING);
    let b = floating_bounds(placement, a, d, g);
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
pub(crate) fn flipped_placement(a: Rect, d: &Dims, g: &Geometry, anchor_in_flow: bool, vp: &Viewport) -> Placement {
    let placement = g.placement;
    let (_, ph) = d.placement_size(g.relative);
    if placement.is_top() && anchor_in_flow && vp.scroll_y > 0.0 && a.top - ph < 0.0 {
        return placement;
    }
    let original = placement_score(placement, a, d, g, vp);
    if original.perfect() {
        return placement;
    }
    let opposite = placement.opposite();
    let flipped = placement_score(opposite, a, d, g, vp);
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

fn document_position(placement: Placement, a: Rect, d: &Dims, g: &Geometry, fixed: bool, vp: &Viewport) -> Pos {
    let (pw, ph) = d.placement_size(g.relative);
    let mut p = calculate_position(placement, a, pw, ph, g.inset);
    if !fixed {
        p.top += vp.scroll_y;
        p.left += vp.scroll_x;
    }
    p = apply_offset(p, placement, g);
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
pub(crate) fn size_limits(a: Rect, natural: &Dims, g: &Geometry, fixed: bool, anchor_in_flow: bool, vp: &Viewport) -> (Placement, Option<f64>, Option<f64>) {
    let placement = flipped_placement(a, natural, g, anchor_in_flow, vp);
    let viewport = vp.document_bounds(fixed, VIEWPORT_PADDING);
    let p = document_position(placement, a, natural, g, fixed, vp);
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
    g: &Geometry,
    fixed: bool,
    vp: &Viewport,
) -> Layout {
    let viewport = vp.document_bounds(fixed, VIEWPORT_PADDING);
    let mut p = document_position(placement, a, d, g, fixed, vp);
    let (min_x, min_y, max_x, max_y) = (viewport.left, viewport.top, viewport.right, viewport.bottom);
    let x_range = (min_x, min_x.max(max_x - d.width));
    let y_range = (min_y, min_y.max(max_y - d.height));
    let relative = g.relative;
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

/// Whether the floating element uses `position: fixed` (it is a popover, or the anchor is
/// fixed or a point) rather than `absolute`.
pub(crate) fn uses_fixed_position(popover: bool, anchor_in_flow: bool) -> bool {
    popover || !anchor_in_flow
}

/// What the DOM reports for one element: its bounding rect, `offsetWidth`/`offsetHeight`,
/// scroll size and scroll position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Measured {
    pub rect: Rect,
    pub offset_width: f64,
    pub offset_height: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub scroll_left: f64,
    pub scroll_top: f64,
}

/// The floating element's dimensions (Remix's `getFloatingDimensions`) from measurements
/// of it, its scroll container (the floating element itself unless `relative` sits in a
/// scrollable descendant) and the `relative_to` element.
pub(crate) fn dims_from(floating: &Measured, container: &Measured, relative: Option<&Measured>) -> Dims {
    let mut d = Dims {
        width: floating.offset_width,
        height: floating.offset_height,
        scroll_viewport_width: container.offset_width,
        scroll_viewport_height: container.offset_height,
        scroll_width: container.scroll_width.max(container.offset_width),
        scroll_height: container.scroll_height.max(container.offset_height),
        ..Default::default()
    };
    if let Some(r) = relative {
        let (f, c) = (floating.rect, container.rect);
        d.scroll_container_offset_x = c.left - f.left;
        d.scroll_container_offset_y = c.top - f.top;
        d.relative_offset_x = Some(r.rect.left - c.left + container.scroll_left);
        d.relative_offset_y = Some(r.rect.top - c.top + container.scroll_top);
        d.relative_width = Some(r.offset_width);
        d.relative_height = Some(r.offset_height);
    }
    d
}

/// Whether a polled frame must reposition: the anchor or the floating element moved or
/// resized by a pixel or more since the last pass.
pub(crate) fn needs_update(last: Option<&(Rect, Dims)>, anchor: &Rect, dims: &Dims) -> bool {
    match last {
        Some((la, ld)) => anchor.changed(la) || dims.changed(ld),
        None => true,
    }
}

/// One positioning pass.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pass {
    pub layout: Layout,
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
    /// The measurements after the size limits were applied.
    pub dims: Dims,
}

/// Remix's `updatePosition` over measurements: choose the side and size limits from the
/// floating element's natural size, then `remeasure(placement, max_width, max_height)`
/// applies them and returns the new dimensions and the scroll container's
/// `(scroll_top, scroll_left)`, and the final position is solved from those. Offsets
/// resolve against `size`, the element's rendered size before the pass.
#[allow(clippy::too_many_arguments)]
pub(crate) fn position_pass(
    a: Rect,
    natural: &Dims,
    size: FloatingSize,
    options: &AnchorOptions,
    fixed: bool,
    anchor_in_flow: bool,
    vp: &Viewport,
    remeasure: impl FnOnce(Placement, Option<f64>, Option<f64>) -> (Dims, (f64, f64)),
) -> Pass {
    let g = options.geometry(size);
    let (placement, max_width, max_height) = size_limits(a, natural, &g, fixed, anchor_in_flow, vp);
    let (dims, scroll) = remeasure(placement, max_width, max_height);
    let layout = final_position(placement, a, &dims, scroll, &g, fixed, vp);
    Pass { layout, max_width, max_height, dims }
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
        /// Shared so the owner can move the point; tracking follows it on the next frame.
        Point(Rc<std::cell::Cell<AnchorPoint>>),
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
            AnchorTarget::Point(p) => {
                let p = p.get();
                Rect { left: p.x, top: p.y, width: p.width, height: p.height }
            }
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

    fn measure(el: &web_sys::HtmlElement) -> Measured {
        Measured {
            rect: rect_of(el),
            offset_width: el.offset_width() as f64,
            offset_height: el.offset_height() as f64,
            scroll_width: el.scroll_width() as f64,
            scroll_height: el.scroll_height() as f64,
            scroll_left: el.scroll_left() as f64,
            scroll_top: el.scroll_top() as f64,
        }
    }

    fn read_dims(floating: &web_sys::HtmlElement, relative_to: Option<&str>) -> (Dims, web_sys::HtmlElement) {
        let relative = relative_to
            .and_then(|s| floating.query_selector(s).ok().flatten())
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
        let container = match &relative {
            Some(rel) => relative_scroll_container(floating, rel),
            None => floating.clone(),
        };
        let d = dims_from(&measure(floating), &measure(&container), relative.as_ref().map(measure).as_ref());
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
            let size = FloatingSize { width: self.floating.offset_width() as f64, height: self.floating.offset_height() as f64 };
            let floating = &self.floating;
            let mut container = None;
            let pass = position_pass(a, &natural, size, &self.options, self.fixed, self.anchor_in_flow, &vp, |placement, max_w, max_h| {
                let _ = floating.set_attribute("data-anchor-placement", placement.as_str());
                let style = floating.style();
                let px = |v: Option<f64>| v.map(|v| format!("{v}px")).unwrap_or_default();
                let _ = style.set_property("max-width", &px(max_w));
                let _ = style.set_property("max-height", &px(max_h));
                let (d, c) = dims(floating, relative_to, false);
                let scroll = (c.scroll_top() as f64, c.scroll_left() as f64);
                container = Some(c);
                (d, scroll)
            });
            if let Some(container) = container {
                if let Some(top) = pass.layout.scroll_top {
                    container.set_scroll_top(top as i32);
                }
                if let Some(left) = pass.layout.scroll_left {
                    container.set_scroll_left(left as i32);
                }
            }
            let style = self.floating.style();
            let _ = style.set_property("position", if self.fixed { "fixed" } else { "absolute" });
            let _ = style.set_property("inset", "auto");
            let _ = style.set_property("top", &format!("{}px", pass.layout.top));
            let _ = style.set_property("left", &format!("{}px", pass.layout.left));
            self.last = Some((a, pass.dims));
        }

        fn poll(&mut self) {
            let a = anchor_rect(&self.target);
            let (d, _) = dims(&self.floating, self.options.relative_to.as_deref(), false);
            if needs_update(self.last.as_ref(), &a, &d) {
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
        let fixed = uses_fixed_position(floating.has_attribute("popover"), anchor_in_flow);
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
    //! Remix's anchor.test.ts, one test per case, against the same mocked layout: each
    //! element has a bounding rect and scroll size, the floating element's inline
    //! `max-width`/`max-height` shrink its rect and offset size, and the window is 800x600.
    //! The DOM half of `anchor` only measures and writes styles; `Scene` stands in for it.
    //! Not ported natively: "cancels its animation frame polling during cleanup" (dropping
    //! [`Anchored`] cancels the frame; it needs a browser).

    use super::*;

    const VP: Viewport = Viewport { width: 800.0, height: 600.0, scroll_x: 0.0, scroll_y: 0.0 };

    /// `mockLayout`'s rect: position, size and an optional larger scroll size.
    #[derive(Clone, Copy, Debug, Default)]
    struct Mock {
        top: f64,
        left: f64,
        width: f64,
        height: f64,
        scroll_height: Option<f64>,
    }

    fn mock(top: f64, left: f64, width: f64, height: f64) -> Mock {
        Mock { top, left, width, height, scroll_height: None }
    }

    fn rect(m: Mock) -> Rect {
        Rect { left: m.left, top: m.top, width: m.width, height: m.height }
    }

    fn measure(m: Mock, max_width: Option<f64>, max_height: Option<f64>, scroll_top: f64) -> Measured {
        let width = max_width.map_or(m.width, |max| m.width.min(max));
        let height = max_height.map_or(m.height, |max| m.height.min(max));
        Measured {
            rect: Rect { left: m.left, top: m.top, width, height },
            offset_width: width,
            offset_height: height,
            scroll_width: m.width,
            scroll_height: m.scroll_height.unwrap_or(m.height),
            scroll_left: 0.0,
            scroll_top,
        }
    }

    /// A floating element, optionally with a `relative_to` element inside it and a
    /// scrollable element between the two.
    #[derive(Default)]
    struct Scene {
        floating: Mock,
        scroller: Option<Mock>,
        relative: Option<Mock>,
        max_width: Option<f64>,
        max_height: Option<f64>,
        scroll_top: f64,
        placement: Option<Placement>,
    }

    impl Scene {
        fn new(floating: Mock) -> Scene {
            Scene { floating, ..Default::default() }
        }

        fn dims(&self, ignore_max_size: bool) -> Dims {
            let (max_w, max_h) = if ignore_max_size { (None, None) } else { (self.max_width, self.max_height) };
            let own_scroll = if self.scroller.is_none() { self.scroll_top } else { 0.0 };
            let floating = measure(self.floating, max_w, max_h, own_scroll);
            let container = self.scroller.map_or(floating, |s| measure(s, None, None, self.scroll_top));
            // Scrolling the container moves the relative element's rect up, as in a browser.
            let relative = self.relative.map(|r| measure(Mock { top: r.top - self.scroll_top, ..r }, None, None, 0.0));
            dims_from(&floating, &container, relative.as_ref())
        }

        /// One `updatePosition`: returns `(top, left)` and keeps the size limits, scroll
        /// position and placement it wrote.
        fn position(&mut self, anchor: Rect, options: &AnchorOptions, fixed: bool, anchor_in_flow: bool) -> (f64, f64) {
            let natural = self.dims(true);
            let current = measure(self.floating, self.max_width, self.max_height, 0.0);
            let size = FloatingSize { width: current.offset_width, height: current.offset_height };
            let pass = position_pass(anchor, &natural, size, options, fixed, anchor_in_flow, &VP, |placement, max_w, max_h| {
                self.placement = Some(placement);
                self.max_width = max_w;
                self.max_height = max_h;
                (self.dims(false), (self.scroll_top, 0.0))
            });
            if let Some(top) = pass.layout.scroll_top {
                self.scroll_top = top;
            }
            (pass.layout.top, pass.layout.left)
        }

        /// Position against an element anchor in the document flow (`position: absolute`).
        fn against(&mut self, anchor: Mock, options: &AnchorOptions) -> (f64, f64) {
            self.position(rect(anchor), options, uses_fixed_position(false, true), true)
        }

        /// Position against a point (`position: fixed`).
        fn at_point(&mut self, x: f64, y: f64, options: &AnchorOptions) -> (f64, f64) {
            self.position(Rect { left: x, top: y, width: 0.0, height: 0.0 }, options, uses_fixed_position(false, false), false)
        }
    }

    const BUTTON: Mock = Mock { top: 40.0, left: 200.0, width: 80.0, height: 28.0, scroll_height: None };

    fn options(placement: Placement) -> AnchorOptions {
        AnchorOptions::new(placement)
    }

    #[test]
    fn positions_a_floating_element_below_its_anchor_by_default() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert!(!uses_fixed_position(false, true), "position: absolute");
        assert_eq!(scene.against(BUTTON, &AnchorOptions::default()), (68.0, 180.0));
    }

    #[test]
    fn supports_aligned_placements_and_offset() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart).offset(8.0)), (76.0, 200.0));
    }

    #[test]
    fn positions_a_floating_element_from_coordinates() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert!(uses_fixed_position(false, false), "position: fixed");
        assert_eq!(scene.at_point(200.0, 40.0, &options(Placement::BottomStart)), (40.0, 200.0));
    }

    #[test]
    fn supports_horizontal_and_vertical_offsets_independently() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        let options = options(Placement::BottomStart)
            .offset(8.0)
            .offset_x(Offset::sized(|f| f.width / 10.0))
            .offset_y(Offset::sized(|f| -f.height / 20.0));
        assert_eq!(scene.against(BUTTON, &options), (72.0, 212.0));
    }

    #[test]
    fn flips_when_the_requested_placement_would_overflow_the_viewport() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert_eq!(scene.against(mock(560.0, 200.0, 80.0, 28.0), &options(Placement::Bottom)), (480.0, 180.0));
        assert_eq!(scene.placement, Some(Placement::Top));
    }

    #[test]
    fn exposes_the_final_horizontal_placement_when_right_placement_flips_to_the_left() {
        let mut scene = Scene::new(mock(0.0, 0.0, 180.0, 120.0));
        assert_eq!(scene.against(mock(40.0, 760.0, 24.0, 32.0), &options(Placement::RightStart)), (40.0, 580.0));
        assert_eq!(scene.placement.map(Placement::as_str), Some("left-start"));
    }

    #[test]
    fn supports_inset_positioning() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart).inset(true)), (40.0, 200.0));
    }

    #[test]
    fn supports_positioning_relative_to_an_inner_element() {
        let mut scene = Scene { relative: Some(mock(10.0, 20.0, 100.0, 40.0)), ..Scene::new(mock(0.0, 0.0, 200.0, 100.0)) };
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart).relative_to(".inner")), (58.0, 180.0));
    }

    #[test]
    fn supports_inset_left_positioning_relative_to_a_selected_inner_element() {
        let mut scene = Scene { relative: Some(mock(10.0, 20.0, 100.0, 40.0)), ..Scene::new(mock(0.0, 0.0, 200.0, 100.0)) };
        let options = options(Placement::Left).inset(true).relative_to("[role=\"option\"][aria-selected=\"true\"]");
        assert_eq!(scene.against(BUTTON, &options), (24.0, 180.0));
    }

    #[test]
    fn preserves_relative_alignment_by_scrolling_oversized_inset_popovers_when_possible() {
        let mut scene = Scene {
            scroller: Some(Mock { top: 4.0, left: 4.0, width: 192.0, height: 560.0, scroll_height: Some(800.0) }),
            relative: Some(mock(404.0, 24.0, 100.0, 40.0)),
            ..Scene::new(mock(0.0, 0.0, 200.0, 800.0))
        };
        let options = options(Placement::Left).inset(true).relative_to("[role=\"option\"][aria-selected=\"true\"]");
        assert_eq!(scene.against(mock(200.0, 200.0, 80.0, 28.0), &options), (16.0, 176.0));
        assert_eq!(scene.max_height, Some(568.0));
        assert_eq!(scene.scroll_top, 226.0);
    }

    #[test]
    fn repositions_when_the_floating_dimensions_change_during_animation_frame_polling() {
        let anchor = mock(500.0, 200.0, 80.0, 28.0);
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 40.0));
        assert_eq!(scene.against(anchor, &options(Placement::Bottom)), (528.0, 180.0));
        let last = (rect(anchor), scene.dims(false));

        scene.floating = mock(0.0, 0.0, 120.0, 80.0);
        assert!(needs_update(Some(&last), &rect(anchor), &scene.dims(false)));
        assert_eq!(scene.against(anchor, &options(Placement::Bottom)), (420.0, 180.0));
    }

    #[test]
    fn repositions_when_a_coordinate_target_changes_during_animation_frame_polling() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 80.0));
        assert_eq!(scene.at_point(200.0, 40.0, &options(Placement::BottomStart)), (40.0, 200.0));
        let last = (Rect { left: 200.0, top: 40.0, ..Rect::default() }, scene.dims(false));

        let moved = Rect { left: 240.0, top: 90.0, ..Rect::default() };
        assert!(needs_update(Some(&last), &moved, &scene.dims(false)));
        assert_eq!(scene.at_point(240.0, 90.0, &options(Placement::BottomStart)), (90.0, 240.0));
    }

    #[test]
    fn shrinks_oversized_bottom_popovers_to_the_remaining_viewport_height() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 800.0));
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart)), (68.0, 200.0));
        assert_eq!(scene.max_height, Some(516.0));
    }

    #[test]
    fn keeps_oversized_popovers_constrained_across_repeated_attachments() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 800.0));
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart)), (68.0, 200.0));
        assert_eq!(scene.max_height, Some(516.0));
        // Attaching again starts from the inline max-height the first attachment left.
        assert_eq!(scene.against(BUTTON, &options(Placement::BottomStart)), (68.0, 200.0));
        assert_eq!(scene.max_height, Some(516.0));
    }

    #[test]
    fn prefers_the_side_with_less_overflow_for_oversized_popovers() {
        let mut scene = Scene::new(mock(0.0, 0.0, 120.0, 700.0));
        assert_eq!(scene.against(mock(500.0, 200.0, 80.0, 28.0), &options(Placement::Bottom)), (16.0, 180.0));
        assert_eq!(scene.max_height, Some(484.0));
    }

    // rmx-only

    #[test]
    fn stays_aligned_when_repositioned_after_scrolling() {
        let mut scene = Scene {
            scroller: Some(Mock { top: 4.0, left: 4.0, width: 192.0, height: 560.0, scroll_height: Some(800.0) }),
            relative: Some(mock(404.0, 24.0, 100.0, 40.0)),
            ..Scene::new(mock(0.0, 0.0, 200.0, 800.0))
        };
        let options = options(Placement::Left).inset(true).relative_to("[aria-selected=\"true\"]");
        let anchor = mock(200.0, 200.0, 80.0, 28.0);
        assert_eq!(scene.against(anchor, &options), (16.0, 176.0));
        assert_eq!(scene.against(anchor, &options), (16.0, 176.0));
        assert_eq!(scene.scroll_top, 226.0);
    }

    #[test]
    fn polling_ignores_sub_pixel_changes() {
        let a = rect(BUTTON);
        let d = Scene::new(mock(0.0, 0.0, 120.0, 80.0)).dims(false);
        assert!(needs_update(None, &a, &d));
        let nudged = Rect { top: a.top + 0.5, ..a };
        assert!(!needs_update(Some(&(a, d)), &nudged, &d));
    }

    #[test]
    fn centers_and_aligns_ends() {
        let at = |p| Scene::new(mock(0.0, 0.0, 100.0, 50.0)).against(mock(100.0, 100.0, 80.0, 30.0), &options(p));
        assert_eq!(at(Placement::TopEnd), (50.0, 80.0));
        assert_eq!(at(Placement::RightStart), (100.0, 180.0));
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
        let mut scene = Scene::new(mock(0.0, 0.0, 100.0, 50.0));
        assert_eq!(scene.position(Rect { left: 100.0, top: -100.0, width: 80.0, height: 30.0 }, &AnchorOptions::default(), true, true).0, 16.0);
    }

    #[test]
    fn sized_offsets_compare_by_identity() {
        let offset = Offset::sized(|f| f.width);
        assert_eq!(offset, offset.clone());
        assert_ne!(offset, Offset::sized(|f| f.width));
        assert_eq!(Offset::from(8.0), Offset::Px(8.0));
        assert_eq!(offset.resolve(FloatingSize { width: 3.0, height: 0.0 }), 3.0);
    }
}

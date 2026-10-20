//! Pointer intent toward a submenu (Remix's `lib/hover-aim.ts`). When the pointer leaves a
//! submenu trigger heading for the open child surface, pointer events on the items it
//! crosses on the way are ignored until it reaches the surface, leaves the corridor
//! between its start point and the surface, stalls, or takes too long.

// The geometry is used by the browser tracker and unit tested natively.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

const PADDING: f64 = 8.0;
const MAX_DURATION_MS: i32 = 300;
const STALL_DURATION_MS: i32 = 120;
const MIN_PROGRESS: f64 = 0.5;
const MAX_STALLED_UPDATES: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    fn is_empty(&self) -> bool {
        self.left == self.right || self.top == self.bottom
    }

    fn inflate(&self, by: f64) -> Rect {
        Rect { left: self.left - by, top: self.top - by, right: self.right + by, bottom: self.bottom + by }
    }

    fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x <= self.right && p.y >= self.top && p.y <= self.bottom
    }

    fn center(&self) -> Point {
        Point { x: self.left + (self.right - self.left) / 2.0, y: self.top + (self.bottom - self.top) / 2.0 }
    }

    fn distance_to(&self, p: Point) -> f64 {
        let dx = if p.x < self.left { self.left - p.x } else if p.x > self.right { p.x - self.right } else { 0.0 };
        let dy = if p.y < self.top { self.top - p.y } else if p.y > self.bottom { p.y - self.bottom } else { 0.0 };
        dx.hypot(dy)
    }
}

/// Whether `point` is on the target's side of the source's center, along the axis the
/// target is mostly offset on.
pub(crate) fn is_heading_toward(source: &Rect, target: &Rect, point: Point) -> bool {
    let (s, t) = (source.center(), target.center());
    let (dx, dy) = (t.x - s.x, t.y - s.y);
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 { point.x >= s.x } else { point.x <= s.x }
    } else if dy >= 0.0 {
        point.y >= s.y
    } else {
        point.y <= s.y
    }
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// The convex hull of the start point and the rectangle's corners (monotone chain).
pub(crate) fn corridor(start: Point, rect: &Rect) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    for p in [
        start,
        Point { x: rect.left, y: rect.top },
        Point { x: rect.right, y: rect.top },
        Point { x: rect.right, y: rect.bottom },
        Point { x: rect.left, y: rect.bottom },
    ] {
        if !points.contains(&p) {
            points.push(p);
        }
    }
    if points.len() <= 2 {
        return points;
    }
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let half = |iter: &mut dyn Iterator<Item = Point>| {
        let mut hull: Vec<Point> = Vec::new();
        for p in iter {
            while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
        hull
    };
    let mut lower = half(&mut points.iter().copied());
    let upper = half(&mut points.iter().rev().copied());
    lower.extend(upper);
    lower
}

fn on_segment(p: Point, a: Point, b: Point) -> bool {
    cross(a, b, p).abs() <= 0.001 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Even-odd point-in-polygon test; points on an edge are inside.
pub(crate) fn point_in_polygon(p: Point, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (pi, pj) = (polygon[i], polygon[j]);
        if on_segment(p, pj, pi) {
            return true;
        }
        if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(target_arch = "wasm32")]
pub(crate) use dom::HoverAim;

#[cfg(target_arch = "wasm32")]
mod dom {
    use std::cell::RefCell;
    use std::rc::{Rc, Weak};

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use super::*;

    struct Session {
        id: u32,
        target: web_sys::HtmlElement,
        actual: Rect,
        polygon: Vec<Point>,
        started_at: f64,
        last_distance: f64,
        last_point: Point,
        stalled_updates: u32,
        stall_generation: u32,
    }

    #[derive(Default)]
    struct Inner {
        last_pointer: Option<Point>,
        session: Option<Session>,
        next_id: u32,
        tracking: Option<Closure<dyn Fn(web_sys::Event)>>,
        tracking_active: bool,
        processed: Option<(web_sys::Event, bool)>,
    }

    /// One menu's hover-aim tracker (Remix's `createHoverAim()`).
    #[derive(Clone, Default)]
    pub(crate) struct HoverAim(Rc<RefCell<Inner>>);

    fn num(event: &web_sys::Event, key: &str) -> f64 {
        js_sys::Reflect::get(event, &key.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
    }

    fn point(event: &web_sys::Event) -> Point {
        Point { x: num(event, "clientX"), y: num(event, "clientY") }
    }

    fn is_mouse(event: &web_sys::Event) -> bool {
        match js_sys::Reflect::get(event, &"pointerType".into()).ok().and_then(|v| v.as_string()) {
            None => true,
            Some(t) => t.is_empty() || t == "mouse",
        }
    }

    fn rect(el: &web_sys::Element) -> Rect {
        let r = el.get_bounding_client_rect();
        Rect { left: r.left(), top: r.top(), right: r.right(), bottom: r.bottom() }
    }

    fn as_node(target: Option<web_sys::EventTarget>) -> Option<web_sys::Node> {
        target.and_then(|t| t.dyn_into::<web_sys::Node>().ok())
    }

    fn set_timeout(ms: i32, f: impl FnOnce() + 'static) {
        // Every timer fires (stale ones do nothing), so the closure is always freed.
        let f = Closure::once_into_js(f);
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(f.unchecked_ref(), ms);
        }
    }

    impl HoverAim {
        fn clear(&self, inner: &mut Inner) {
            inner.session = None;
            if inner.tracking_active {
                inner.tracking_active = false;
                if let (Some(listener), Some(doc)) = (&inner.tracking, web_sys::window().and_then(|w| w.document())) {
                    let _ = doc.remove_event_listener_with_callback_and_bool("pointermove", listener.as_ref().unchecked_ref(), true);
                }
            }
        }

        fn start_tracking(&self, inner: &mut Inner) {
            if inner.tracking_active {
                return;
            }
            let weak: Weak<RefCell<Inner>> = Rc::downgrade(&self.0);
            let listener = inner.tracking.get_or_insert_with(|| {
                Closure::new(move |event: web_sys::Event| {
                    if let Some(inner) = weak.upgrade() {
                        HoverAim(inner).accepts(&event);
                    }
                })
            });
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let _ = doc.add_event_listener_with_callback_and_bool("pointermove", listener.as_ref().unchecked_ref(), true);
                inner.tracking_active = true;
            }
        }

        fn schedule_stall(&self, session: &mut Session) {
            session.stall_generation += 1;
            let (id, generation) = (session.id, session.stall_generation);
            let weak = Rc::downgrade(&self.0);
            set_timeout(STALL_DURATION_MS, move || {
                let Some(inner) = weak.upgrade() else { return };
                let aim = HoverAim(inner);
                let mut inner = aim.0.borrow_mut();
                if inner.session.as_ref().is_some_and(|s| s.id == id && s.stall_generation == generation) {
                    aim.clear(&mut inner);
                }
            });
        }

        /// Start aiming from `source` toward `target` as the pointer leaves `source`.
        /// Returns whether the pointer is heading for the target.
        pub(crate) fn start(&self, source: &web_sys::HtmlElement, target: &web_sys::HtmlElement, event: &web_sys::Event) -> bool {
            let mut inner = self.0.borrow_mut();
            self.clear(&mut inner);
            if !is_mouse(event) || !source.is_connected() || !target.is_connected() {
                return false;
            }
            let start = point(event);
            let related = as_node(js_sys::Reflect::get(event, &"relatedTarget".into()).ok().and_then(|v| v.dyn_into().ok()));
            if related.is_some_and(|n| target.contains(Some(&n))) {
                inner.last_pointer = Some(start);
                return true;
            }
            let (source_rect, actual) = (rect(source), rect(target));
            if source_rect.is_empty() || actual.is_empty() || actual.contains(start) || !is_heading_toward(&source_rect, &actual, start) {
                return false;
            }
            inner.last_pointer = Some(start);
            inner.next_id += 1;
            let mut session = Session {
                id: inner.next_id,
                target: target.clone(),
                actual,
                polygon: corridor(start, &actual.inflate(PADDING)),
                started_at: js_sys::Date::now(),
                last_distance: actual.distance_to(start),
                last_point: start,
                stalled_updates: 0,
                stall_generation: 0,
            };
            self.start_tracking(&mut inner);
            let (id, weak) = (session.id, Rc::downgrade(&self.0));
            set_timeout(MAX_DURATION_MS, move || {
                let Some(inner) = weak.upgrade() else { return };
                let aim = HoverAim(inner);
                let mut inner = aim.0.borrow_mut();
                if inner.session.as_ref().is_some_and(|s| s.id == id) {
                    aim.clear(&mut inner);
                }
            });
            self.schedule_stall(&mut session);
            inner.session = Some(session);
            true
        }

        /// Whether a pointer event should act on what it is over (false while aiming).
        pub(crate) fn accepts(&self, event: &web_sys::Event) -> bool {
            let Ok(mut inner) = self.0.try_borrow_mut() else { return true };
            if let Some((seen, result)) = &inner.processed {
                if seen == event {
                    return *result;
                }
            }
            let result = self.evaluate(&mut inner, event);
            inner.processed = Some((event.clone(), result));
            result
        }

        fn evaluate(&self, inner: &mut Inner, event: &web_sys::Event) -> bool {
            if is_mouse(event) {
                inner.last_pointer = Some(point(event));
            }
            let Some(mut session) = inner.session.take() else { return true };
            if !is_mouse(event) || event.type_() != "pointermove" || !session.target.is_connected() {
                self.clear(inner);
                return true;
            }
            let p = point(event);
            let entered = session.actual.contains(p) || as_node(event.target()).is_some_and(|n| session.target.contains(Some(&n)));
            if entered || !point_in_polygon(p, &session.polygon) {
                self.clear(inner);
                return true;
            }
            if (p.x - session.last_point.x).hypot(p.y - session.last_point.y) > 0.0 {
                self.schedule_stall(&mut session);
            }
            let distance = session.actual.distance_to(p);
            if session.last_distance - distance >= MIN_PROGRESS {
                session.stalled_updates = 0;
            } else {
                session.stalled_updates += 1;
            }
            session.last_point = p;
            session.last_distance = distance;
            if session.stalled_updates >= MAX_STALLED_UPDATES || js_sys::Date::now() - session.started_at >= MAX_DURATION_MS as f64 {
                self.clear(inner);
                return true;
            }
            inner.session = Some(session);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn corridor_is_the_hull_of_the_start_point_and_the_target() {
        let target = Rect { left: 100.0, top: 0.0, right: 200.0, bottom: 100.0 };
        let hull = corridor(pt(0.0, 50.0), &target);
        assert_eq!(hull.len(), 5, "{hull:?}");
        assert!(point_in_polygon(pt(50.0, 50.0), &hull));
        assert!(point_in_polygon(pt(150.0, 50.0), &hull));
        assert!(!point_in_polygon(pt(50.0, 5.0), &hull), "outside the wedge toward the target");
        assert!(point_in_polygon(pt(0.0, 50.0), &hull), "the start point is on the edge");
        // A start point inside the rectangle adds nothing to the hull.
        assert_eq!(corridor(pt(150.0, 50.0), &target).len(), 4);
    }

    #[test]
    fn heading_toward_uses_the_dominant_axis() {
        let source = Rect { left: 0.0, top: 0.0, right: 100.0, bottom: 20.0 };
        let right = Rect { left: 110.0, top: 0.0, right: 200.0, bottom: 100.0 };
        assert!(is_heading_toward(&source, &right, pt(99.0, 10.0)));
        assert!(!is_heading_toward(&source, &right, pt(10.0, 10.0)));
        let below = Rect { left: 0.0, top: 200.0, right: 100.0, bottom: 300.0 };
        assert!(is_heading_toward(&source, &below, pt(50.0, 19.0)));
        assert!(!is_heading_toward(&source, &below, pt(50.0, 1.0)));
    }

    #[test]
    fn rect_distance_and_padding() {
        let r = Rect { left: 10.0, top: 10.0, right: 20.0, bottom: 20.0 };
        assert_eq!(r.distance_to(pt(15.0, 15.0)), 0.0);
        assert_eq!(r.distance_to(pt(0.0, 15.0)), 10.0);
        assert_eq!(r.distance_to(pt(23.0, 24.0)), 5.0);
        assert!(r.inflate(PADDING).contains(pt(3.0, 3.0)));
        assert!(Rect { left: 1.0, top: 0.0, right: 1.0, bottom: 5.0 }.is_empty());
    }
}

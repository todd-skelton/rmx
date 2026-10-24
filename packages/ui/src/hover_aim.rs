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

/// Where a pointer event is and what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointerInput {
    pub point: Point,
    /// A mouse (or unknown) pointer; touch and pen never aim.
    pub mouse: bool,
}

/// The pointer leaving a submenu trigger, for [`Core::start`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Leave {
    pub source: Rect,
    pub target: Rect,
    /// Both elements are in the document.
    pub connected: bool,
    /// The leave event; without one the last pointer position seen is the start point.
    pub event: Option<PointerInput>,
    /// The pointer left straight into the target.
    pub related_in_target: bool,
}

/// A pointer event checked by [`Core::accepts`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Move {
    pub pointer: PointerInput,
    /// `pointermove` (anything else ends aiming).
    pub is_move: bool,
    /// The session's target is still in the document.
    pub target_connected: bool,
    /// The event's target is inside the session's target element.
    pub in_target: bool,
}

struct Session<T> {
    payload: T,
    actual: Rect,
    polygon: Vec<Point>,
    started_at: f64,
    last_distance: f64,
    last_point: Point,
    stalled_updates: u32,
    /// When the stall timeout fires unless the pointer moves first.
    stall_at: f64,
}

/// Remix's `createHoverAim` session logic with the clock passed in (milliseconds), so the
/// DOM tracker only measures and schedules timers. `T` is carried with the session (the
/// target element and `onExpire` in the browser) and handed back when the session expires.
pub(crate) struct Core<T> {
    last_pointer: Option<Point>,
    session: Option<Session<T>>,
}

impl<T> Default for Core<T> {
    fn default() -> Self {
        Core { last_pointer: None, session: None }
    }
}

impl<T> Core<T> {
    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }

    /// The deadlines a running session needs timers for: `(stall, max duration)`.
    pub fn deadlines(&self) -> Option<(f64, f64)> {
        self.session.as_ref().map(|s| (s.stall_at, s.started_at + MAX_DURATION_MS as f64))
    }

    /// Start aiming as the pointer leaves the source. Returns whether the pointer is heading
    /// for the target.
    pub fn start(&mut self, leave: Leave, payload: T, now: f64) -> bool {
        let Leave { source, target, connected, event, related_in_target } = leave;
        self.session = None;
        if event.is_some_and(|e| !e.mouse) || !connected {
            return false;
        }
        let Some(start) = event.map(|e| e.point).or(self.last_pointer) else { return false };
        if related_in_target {
            self.last_pointer = Some(start);
            return true;
        }
        if source.is_empty() || target.is_empty() || target.contains(start) || !is_heading_toward(&source, &target, start) {
            return false;
        }
        self.last_pointer = Some(start);
        self.session = Some(Session {
            payload,
            actual: target,
            polygon: corridor(start, &target.inflate(PADDING)),
            started_at: now,
            last_distance: target.distance_to(start),
            last_point: start,
            stalled_updates: 0,
            stall_at: now + STALL_DURATION_MS as f64,
        });
        true
    }

    /// Whether a pointer event should act on what it is over: false while the pointer is
    /// still on its way through the corridor to the target.
    pub fn accepts(&mut self, m: Move, now: f64) -> bool {
        if m.pointer.mouse {
            self.last_pointer = Some(m.pointer.point);
        }
        let Some(session) = self.session.as_mut() else { return true };
        let p = m.pointer.point;
        if !m.pointer.mouse || !m.is_move || !m.target_connected || m.in_target || session.actual.contains(p) || !point_in_polygon(p, &session.polygon) {
            self.session = None;
            return true;
        }
        if (p.x - session.last_point.x).hypot(p.y - session.last_point.y) > 0.0 {
            session.stall_at = now + STALL_DURATION_MS as f64;
        }
        let distance = session.actual.distance_to(p);
        if session.last_distance - distance >= MIN_PROGRESS {
            session.stalled_updates = 0;
        } else {
            session.stalled_updates += 1;
        }
        session.last_point = p;
        session.last_distance = distance;
        if session.stalled_updates >= MAX_STALLED_UPDATES || now - session.started_at >= MAX_DURATION_MS as f64 {
            self.session = None;
            return true;
        }
        false
    }

    /// Run the stall and duration timeouts due at `now`: a session past either deadline is
    /// cleared and its payload returned so the caller can run `onExpire`.
    pub fn expire(&mut self, now: f64) -> Option<T> {
        let (stall, duration) = self.deadlines()?;
        if now < stall && now < duration {
            return None;
        }
        self.session.take().map(|s| s.payload)
    }
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

    struct Payload {
        target: web_sys::HtmlElement,
        on_expire: Option<Box<dyn FnOnce()>>,
    }

    #[derive(Default)]
    struct Inner {
        core: Core<Payload>,
        tracking: Option<Closure<dyn Fn(web_sys::Event)>>,
        tracking_active: bool,
        processed: Option<(web_sys::Event, bool)>,
        /// The deadlines timers are already scheduled for.
        scheduled: Option<(f64, f64)>,
    }

    /// One menu's hover-aim tracker (Remix's `createHoverAim()`).
    #[derive(Clone, Default)]
    pub(crate) struct HoverAim(Rc<RefCell<Inner>>);

    fn num(event: &web_sys::Event, key: &str) -> f64 {
        js_sys::Reflect::get(event, &key.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
    }

    fn pointer(event: &web_sys::Event) -> PointerInput {
        let mouse = match js_sys::Reflect::get(event, &"pointerType".into()).ok().and_then(|v| v.as_string()) {
            None => true,
            Some(t) => t.is_empty() || t == "mouse",
        };
        PointerInput { point: Point { x: num(event, "clientX"), y: num(event, "clientY") }, mouse }
    }

    fn rect(el: &web_sys::Element) -> Rect {
        let r = el.get_bounding_client_rect();
        Rect { left: r.left(), top: r.top(), right: r.right(), bottom: r.bottom() }
    }

    fn as_node(target: Option<web_sys::EventTarget>) -> Option<web_sys::Node> {
        target.and_then(|t| t.dyn_into::<web_sys::Node>().ok())
    }

    fn now() -> f64 {
        js_sys::Date::now()
    }

    fn set_timeout(ms: f64, f: impl FnOnce() + 'static) {
        // Every timer fires (stale ones find nothing due), so the closure is always freed.
        let f = Closure::once_into_js(f);
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(f.unchecked_ref(), ms.max(0.0).ceil() as i32);
        }
    }

    impl HoverAim {
        /// Stop tracking document moves once no session is running, and schedule timers for
        /// new deadlines.
        fn sync(&self, inner: &mut Inner) {
            let deadlines = inner.core.deadlines();
            if deadlines.is_none() && inner.tracking_active {
                inner.tracking_active = false;
                if let (Some(listener), Some(doc)) = (&inner.tracking, web_sys::window().and_then(|w| w.document())) {
                    let _ = doc.remove_event_listener_with_callback_and_bool("pointermove", listener.as_ref().unchecked_ref(), true);
                }
            }
            if deadlines.is_some() && !inner.tracking_active {
                self.start_tracking(inner);
            }
            if deadlines != inner.scheduled {
                if let Some((stall, duration)) = deadlines {
                    let (stall_changed, duration_changed) = match inner.scheduled {
                        Some((s, d)) => (s != stall, d != duration),
                        None => (true, true),
                    };
                    for (changed, at) in [(stall_changed, stall), (duration_changed, duration)] {
                        if changed {
                            let weak = Rc::downgrade(&self.0);
                            set_timeout(at - now(), move || {
                                if let Some(inner) = weak.upgrade() {
                                    HoverAim(inner).expire();
                                }
                            });
                        }
                    }
                }
                inner.scheduled = deadlines;
            }
        }

        fn expire(&self) {
            let expired = {
                let Ok(mut inner) = self.0.try_borrow_mut() else { return };
                let expired = inner.core.expire(now());
                self.sync(&mut inner);
                expired
            };
            if let Some(on_expire) = expired.and_then(|p| p.on_expire) {
                on_expire();
            }
        }

        fn start_tracking(&self, inner: &mut Inner) {
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

        /// Start aiming from `source` toward `target` as the pointer leaves `source` (from the
        /// last pointer position when `event` is `None`). `on_expire` runs if the pointer
        /// stalls or takes too long. Returns whether the pointer is heading for the target.
        pub(crate) fn start(&self, source: &web_sys::HtmlElement, target: &web_sys::HtmlElement, event: Option<&web_sys::Event>, on_expire: Option<Box<dyn FnOnce()>>) -> bool {
            let mut inner = self.0.borrow_mut();
            let connected = source.is_connected() && target.is_connected();
            let related = event.and_then(|e| as_node(js_sys::Reflect::get(e, &"relatedTarget".into()).ok().and_then(|v| v.dyn_into().ok())));
            let related_in_target = related.is_some_and(|n| target.contains(Some(&n)));
            let payload = Payload { target: target.clone(), on_expire };
            let leave = Leave { source: rect(source), target: rect(target), connected, event: event.map(pointer), related_in_target };
            let started = inner.core.start(leave, payload, now());
            self.sync(&mut inner);
            started
        }

        /// Whether a pointer event should act on what it is over (false while aiming).
        pub(crate) fn accepts(&self, event: &web_sys::Event) -> bool {
            let Ok(mut inner) = self.0.try_borrow_mut() else { return true };
            if let Some((seen, result)) = &inner.processed {
                if seen == event {
                    return *result;
                }
            }
            let (target_connected, in_target) = match inner.core.session.as_ref() {
                Some(s) => (s.payload.target.is_connected(), as_node(event.target()).is_some_and(|n| s.payload.target.contains(Some(&n)))),
                None => (true, false),
            };
            let m = Move { pointer: pointer(event), is_move: event.type_() == "pointermove", target_connected, in_target };
            let result = inner.core.accepts(m, now());
            self.sync(&mut inner);
            inner.processed = Some((event.clone(), result));
            result
        }
    }
}

#[cfg(test)]
mod tests {
    //! Remix's hover-aim.test.ts, one test per case, against [`Core`] with the same
    //! layout (source at 0,40 and target at 100,40, both 80x80) and a fake clock.

    use super::*;

    const fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    const SOURCE: Rect = Rect { left: 0.0, top: 40.0, right: 80.0, bottom: 120.0 };
    const TARGET: Rect = Rect { left: 100.0, top: 40.0, right: 180.0, bottom: 120.0 };

    fn mouse(x: f64, y: f64) -> PointerInput {
        PointerInput { point: pt(x, y), mouse: true }
    }

    fn leave(source: Rect, target: Rect, event: Option<PointerInput>, related_in_target: bool) -> Leave {
        Leave { source, target, connected: true, event, related_in_target }
    }

    fn mv(x: f64, y: f64) -> Move {
        Move { pointer: mouse(x, y), is_move: true, target_connected: true, in_target: false }
    }

    /// A tracker started from a pointerleave at 40,80 at time 0. The payload counts
    /// expirations.
    fn started() -> Core<&'static str> {
        let mut aim = Core::default();
        assert!(aim.start(leave(SOURCE, TARGET, Some(mouse(40.0, 80.0)), false), "session", 0.0));
        aim
    }

    #[test]
    fn starts_from_an_explicit_pointer_event() {
        let mut aim = started();
        assert!(!aim.accepts(mv(80.0, 80.0), 0.0));
    }

    #[test]
    fn suppresses_pointer_moves_while_the_pointer_stays_in_the_corridor_and_keeps_progressing() {
        let mut aim = started();
        assert!(!aim.accepts(mv(60.0, 80.0), 0.0));
        assert!(!aim.accepts(mv(80.0, 80.0), 0.0));
    }

    #[test]
    fn stops_suppressing_when_the_pointer_enters_the_target() {
        let mut aim = started();
        assert!(aim.accepts(mv(110.0, 80.0), 0.0));
        assert!(aim.accepts(mv(120.0, 80.0), 0.0));
    }

    #[test]
    fn stops_suppressing_when_the_pointer_exits_the_corridor() {
        let mut aim = started();
        assert!(aim.accepts(mv(70.0, 20.0), 0.0));
    }

    #[test]
    fn stops_suppressing_when_progress_stalls_inside_the_corridor() {
        let mut aim = started();
        assert!(!aim.accepts(mv(80.0, 80.0), 0.0));
        assert!(!aim.accepts(mv(80.0, 90.0), 0.0));
        assert!(aim.accepts(mv(80.0, 100.0), 0.0));
    }

    #[test]
    fn stops_suppressing_after_pointer_movement_stalls() {
        let mut aim = started();
        assert!(!aim.accepts(mv(80.0, 80.0), 0.0));
        assert_eq!(aim.expire(121.0), Some("session"));
        assert!(aim.accepts(mv(90.0, 80.0), 121.0));
    }

    #[test]
    fn calls_on_expire_when_the_stall_timeout_clears_the_session() {
        let mut aim = started();
        assert_eq!(aim.expire(119.0), None, "not yet");
        assert_eq!(aim.expire(121.0), Some("session"), "the payload carries onExpire");
        assert!(!aim.is_active());
    }

    #[test]
    fn replaces_the_previous_session_when_start_is_called_again() {
        let mut aim = started();
        let second_source = Rect { left: 0.0, top: 160.0, right: 80.0, bottom: 240.0 };
        let second_target = Rect { left: 120.0, top: 160.0, right: 200.0, bottom: 240.0 };
        assert!(aim.start(leave(second_source, second_target, Some(mouse(60.0, 200.0)), false), "second", 0.0));
        assert!(!aim.accepts(mv(100.0, 200.0), 0.0));
        assert_eq!(aim.expire(500.0), Some("second"));
    }

    #[test]
    fn self_cancels_when_the_target_disconnects() {
        let mut aim = started();
        assert!(aim.accepts(Move { target_connected: false, ..mv(80.0, 80.0) }, 0.0));
    }

    #[test]
    fn does_not_start_when_the_leave_direction_heads_away_from_the_target() {
        let mut aim = Core::default();
        assert!(!aim.start(leave(SOURCE, TARGET, Some(mouse(10.0, 80.0)), false), (), 0.0));
    }

    #[test]
    fn treats_direct_entry_into_the_target_as_a_successful_handoff() {
        let mut aim = Core::default();
        assert!(aim.start(leave(SOURCE, TARGET, Some(mouse(110.0, 80.0)), true), (), 0.0));
        assert!(aim.accepts(mv(120.0, 80.0), 0.0));
    }

    #[test]
    fn tracks_document_pointer_moves_after_start() {
        // The document listener runs `accepts` on every move while a session is running.
        let mut aim = started();
        aim.accepts(mv(70.0, 20.0), 0.0);
        assert!(aim.accepts(mv(80.0, 80.0), 0.0));
    }

    // rsmix-only

    #[test]
    fn starts_from_the_last_pointer_without_an_event() {
        let mut aim = Core::default();
        assert!(!aim.start(leave(SOURCE, TARGET, None, false), (), 0.0), "no pointer seen yet");
        aim.accepts(mv(40.0, 80.0), 0.0);
        assert!(aim.start(leave(SOURCE, TARGET, None, false), (), 0.0));
        assert!(!aim.accepts(mv(60.0, 80.0), 0.0));
    }

    #[test]
    fn ends_after_the_maximum_duration_and_ignores_touch() {
        let mut aim = started();
        assert!(!aim.accepts(mv(60.0, 80.0), 100.0));
        assert!(!aim.accepts(mv(70.0, 80.0), 200.0));
        assert!(aim.accepts(mv(80.0, 80.0), 300.0));
        let mut touch = Core::default();
        assert!(!touch.start(leave(SOURCE, TARGET, Some(PointerInput { point: pt(40.0, 80.0), mouse: false }), false), (), 0.0));
    }

    #[test]
    fn movement_pushes_the_stall_deadline_back() {
        let mut aim = started();
        assert!(!aim.accepts(mv(60.0, 80.0), 100.0));
        assert_eq!(aim.expire(121.0), None);
        assert_eq!(aim.deadlines(), Some((220.0, 300.0)));
    }

    #[test]
    fn corridor_is_the_hull_of_the_start_point_and_the_target() {
        let target = Rect { left: 100.0, top: 0.0, right: 200.0, bottom: 100.0 };
        let hull = corridor(pt(0.0, 50.0), &target);
        assert_eq!(hull.len(), 5, "{hull:?}");
        assert!(point_in_polygon(pt(50.0, 50.0), &hull));
        assert!(!point_in_polygon(pt(50.0, 5.0), &hull), "outside the wedge toward the target");
        assert!(point_in_polygon(pt(0.0, 50.0), &hull), "the start point is on the edge");
        assert_eq!(corridor(pt(150.0, 50.0), &target).len(), 4);
    }

    #[test]
    fn heading_toward_uses_the_dominant_axis() {
        let source = Rect { left: 0.0, top: 0.0, right: 100.0, bottom: 20.0 };
        let below = Rect { left: 0.0, top: 200.0, right: 100.0, bottom: 300.0 };
        assert!(is_heading_toward(&source, &below, pt(50.0, 19.0)));
        assert!(!is_heading_toward(&source, &below, pt(50.0, 1.0)));
    }
}

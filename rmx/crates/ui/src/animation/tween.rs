//! Cubic-bezier tweens (Remix's `tween` and `easings`).
//!
//! Remix's `tween` is a generator: `next()` once to start, then `next(timestamp)` per
//! frame. [`Tween::step`] is that per-frame call; the first step fixes the start time.

/// Cubic-bezier control points, as in CSS `cubic-bezier(x1, y1, x2, y2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BezierCurve {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// Common CSS timing functions as control points (`easings`).
pub mod easings {
    use super::BezierCurve;

    pub const LINEAR: BezierCurve = BezierCurve { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 };
    pub const EASE: BezierCurve = BezierCurve { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1.0 };
    pub const EASE_IN: BezierCurve = BezierCurve { x1: 0.42, y1: 0.0, x2: 1.0, y2: 1.0 };
    pub const EASE_OUT: BezierCurve = BezierCurve { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 };
    pub const EASE_IN_OUT: BezierCurve = BezierCurve { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 };
}

/// What a tween animates (`TweenOptions`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TweenOptions {
    pub from: f64,
    pub to: f64,
    /// Milliseconds.
    pub duration: f64,
    pub curve: BezierCurve,
}

/// One frame of a tween.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TweenStep {
    pub value: f64,
    /// The tween reached `to`; stop requesting frames.
    pub done: bool,
}

/// Interpolates a number over time along a cubic-bezier curve.
#[derive(Clone, Debug)]
pub struct Tween {
    options: TweenOptions,
    start: Option<f64>,
    value: f64,
}

/// Create a tween. Its [`value`](Tween::value) starts at `from`.
pub fn tween(options: TweenOptions) -> Tween {
    Tween { options, start: None, value: options.from }
}

impl Tween {
    /// The latest value (`from` before the first step).
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Advance to a frame timestamp (`requestAnimationFrame`'s argument, in ms).
    pub fn step(&mut self, timestamp: f64) -> TweenStep {
        let TweenOptions { from, to, duration, curve } = self.options;
        let start = *self.start.get_or_insert(timestamp);
        let linear = ((timestamp - start) / duration).min(1.0);
        if linear >= 1.0 {
            self.value = to;
            return TweenStep { value: to, done: true };
        }
        let t = solve_x(curve.x1, curve.x2, linear);
        self.value = from + (to - from) * bezier(t, curve.y1, curve.y2);
        TweenStep { value: self.value, done: false }
    }
}

/// The curve parameter `t` whose x is `target` (Newton-Raphson, at most 8 steps).
fn solve_x(x1: f64, x2: f64, target: f64) -> f64 {
    let mut t = target;
    for _ in 0..8 {
        let slope = bezier_derivative(t, x1, x2);
        if slope.abs() < 1e-6 {
            break;
        }
        let error = bezier(t, x1, x2) - target;
        if error.abs() < 1e-6 {
            break;
        }
        t -= error / slope;
    }
    t.clamp(0.0, 1.0)
}

/// B(t) = 3(1-t)²t·p1 + 3(1-t)t²·p2 + t³ with endpoints (0,0) and (1,1).
fn bezier(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn bezier_derivative(t: f64, p1: f64, p2: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_from_and_finishes_at_to() {
        let mut tw = tween(TweenOptions { from: 0.0, to: 100.0, duration: 300.0, curve: easings::EASE_OUT });
        assert_eq!(tw.value(), 0.0);
        assert_eq!(tw.step(1000.0), TweenStep { value: 0.0, done: false });
        let mid = tw.step(1150.0);
        assert!(!mid.done && mid.value > 50.0 && mid.value < 100.0, "{mid:?}");
        assert_eq!(tw.step(1300.0), TweenStep { value: 100.0, done: true });
    }

    #[test]
    fn linear_curve_is_linear() {
        let mut tw = tween(TweenOptions { from: 10.0, to: 20.0, duration: 100.0, curve: easings::LINEAR });
        tw.step(0.0);
        assert!((tw.step(25.0).value - 12.5).abs() < 1e-6);
        assert!((tw.step(50.0).value - 15.0).abs() < 1e-6);
    }
}

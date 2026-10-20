//! Spring physics (Remix's `spring`), based on SwiftUI's spring math with mass 1:
//!
//! - stiffness = (2π / duration)²
//! - damping ratio ζ = 1 - bounce (bounce ≥ 0), 1 / (1 + bounce) (bounce < 0)
//!
//! A [`Spring`] can be iterated for per-frame progress (0 to 1), formatted for a CSS
//! transition (`format!("transform {spring}")`), or used as WAAPI timing through its
//! [`duration`](Spring::duration) and [`easing`](Spring::easing).

use std::f64::consts::PI;
use std::fmt;

/// The named springs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpringPreset {
    /// Overdamped, no overshoot (bounce -0.3, 400 ms).
    Smooth,
    /// Quick, no overshoot (bounce 0, 200 ms). The default.
    Snappy,
    /// Underdamped bounce (bounce 0.3, 400 ms).
    Bouncy,
}

impl SpringPreset {
    /// The preset's duration (ms) and bounce, as in `spring.presets`.
    pub const fn defaults(self) -> SpringOptions {
        match self {
            SpringPreset::Smooth => SpringOptions { duration: 400.0, bounce: -0.3, velocity: 0.0 },
            SpringPreset::Snappy => SpringOptions { duration: 200.0, bounce: 0.0, velocity: 0.0 },
            SpringPreset::Bouncy => SpringOptions { duration: 400.0, bounce: 0.3, velocity: 0.0 },
        }
    }

    /// The preset with another perceptual duration (`spring('bouncy', { duration })`).
    pub const fn duration(self, ms: f64) -> SpringOptions {
        self.defaults().duration(ms)
    }

    /// The preset with an initial velocity (`spring('snappy', { velocity })`).
    pub const fn velocity(self, units_per_second: f64) -> SpringOptions {
        self.defaults().velocity(units_per_second)
    }
}

/// Spring parameters (`SpringOptions`). `Default` is a 300 ms critically damped spring;
/// [`spring()`] with no options in Remix means [`SpringPreset::Snappy`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringOptions {
    /// Perceptual duration in milliseconds; sets the stiffness.
    pub duration: f64,
    /// From overdamped (`< 0`, down to -1) through critical (`0`) to bouncy (`> 0`, up
    /// to 0.95).
    pub bounce: f64,
    /// Initial velocity in units per second.
    pub velocity: f64,
}

impl Default for SpringOptions {
    fn default() -> Self {
        SpringOptions { duration: 300.0, bounce: 0.0, velocity: 0.0 }
    }
}

impl SpringOptions {
    pub const fn duration(mut self, ms: f64) -> Self {
        self.duration = ms;
        self
    }

    pub const fn bounce(mut self, bounce: f64) -> Self {
        self.bounce = bounce;
        self
    }

    pub const fn velocity(mut self, units_per_second: f64) -> Self {
        self.velocity = units_per_second;
        self
    }
}

impl From<SpringPreset> for SpringOptions {
    fn from(preset: SpringPreset) -> Self {
        preset.defaults()
    }
}

// Rest thresholds for deciding when the spring has settled.
const REST_SPEED: f64 = 0.01;
const REST_DELTA: f64 = 0.005;
const MAX_SETTLING_TIME: f64 = 20_000.0;
const FRAME_MS: f64 = 1000.0 / 60.0;

#[derive(Clone, Copy, Debug)]
enum Curve {
    Underdamped { zeta: f64, omega0: f64, omega_d: f64, velocity: f64 },
    Overdamped { s1: f64, s2: f64, a: f64, b: f64 },
    Critical { omega0: f64, velocity: f64 },
}

impl Curve {
    fn new(options: SpringOptions) -> Curve {
        // Natural frequency ω₀ = 2π / duration, per millisecond.
        let omega0 = 2.0 * PI / (options.duration / 1000.0) / 1000.0;
        let bounce = options.bounce.clamp(-1.0, 0.95);
        let zeta = if bounce >= 0.0 { 1.0 - bounce } else { 1.0 / (1.0 + bounce) };
        let velocity = -options.velocity / 1000.0; // per ms, negated for the spring equation
        if zeta < 1.0 {
            let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
            Curve::Underdamped { zeta, omega0, omega_d, velocity }
        } else if zeta > 1.0 {
            let root = (zeta * zeta - 1.0).sqrt();
            let s1 = omega0 * (-zeta + root);
            let s2 = omega0 * (-zeta - root);
            let a = (s2 + velocity) / (s2 - s1);
            Curve::Overdamped { s1, s2, a, b: 1.0 - a }
        } else {
            Curve::Critical { omega0, velocity }
        }
    }

    /// Progress (0 at the start, 1 at the target) after `t` milliseconds.
    fn position(&self, t: f64) -> f64 {
        match *self {
            Curve::Underdamped { zeta, omega0, omega_d, velocity } => {
                let envelope = (-zeta * omega0 * t).exp();
                1.0 - envelope * ((velocity + zeta * omega0) / omega_d * (omega_d * t).sin() + (omega_d * t).cos())
            }
            Curve::Overdamped { s1, s2, a, b } => 1.0 - a * (s1 * t).exp() - b * (s2 * t).exp(),
            Curve::Critical { omega0, velocity } => 1.0 - (-omega0 * t).exp() * (1.0 + (velocity + omega0) * t),
        }
    }

    /// Velocity in units per second, by numerical differentiation.
    fn velocity_at(&self, t: f64) -> f64 {
        const SAMPLE: f64 = 0.5;
        if t < SAMPLE {
            (self.position(SAMPLE) - self.position(0.0)) / SAMPLE * 1000.0
        } else {
            (self.position(t) - self.position(t - SAMPLE)) / SAMPLE * 1000.0
        }
    }

    fn settling_time(&self) -> f64 {
        let mut t = 0.0;
        while t < MAX_SETTLING_TIME {
            if self.velocity_at(t).abs() <= REST_SPEED && (1.0 - self.position(t)).abs() <= REST_DELTA {
                return t;
            }
            t += 50.0;
        }
        MAX_SETTLING_TIME
    }
}

/// A spring: an iterator of per-frame progress values that also carries its CSS timing
/// (Remix's `SpringIterator`).
#[derive(Clone, Debug)]
pub struct Spring {
    curve: Curve,
    settling_time: f64,
    duration: f64,
    easing: String,
    t: f64,
    done: bool,
}

/// Create a spring from a preset or explicit options: `spring(SpringPreset::Bouncy)`,
/// `spring(SpringOptions { duration: 500.0, bounce: 0.2, velocity: 0.0 })`.
pub fn spring(options: impl Into<SpringOptions>) -> Spring {
    let curve = Curve::new(options.into());
    let settling_time = curve.settling_time();
    let easing = linear_easing(&curve, settling_time);
    Spring { curve, settling_time, duration: js_round(settling_time), easing, t: 0.0, done: false }
}

/// CSS transition entries for each property (`spring.transition`):
/// `spring_transition(&["opacity", "transform"], SpringPreset::Snappy)`.
pub fn spring_transition(properties: &[&str], options: impl Into<SpringOptions>) -> String {
    let s = spring(options);
    properties.iter().map(|p| format!("{p} {s}")).collect::<Vec<_>>().join(", ")
}

impl Spring {
    /// The time the spring settles, in whole milliseconds (the WAAPI/CSS duration).
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// A CSS `linear(...)` easing function that traces the spring.
    pub fn easing(&self) -> &str {
        &self.easing
    }

    /// Progress after `t` milliseconds (bouncy springs overshoot 1).
    pub fn position(&self, t: f64) -> f64 {
        self.curve.position(t)
    }
}

/// `"{duration}ms linear(...)"`, for CSS transitions.
impl fmt::Display for Spring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms {}", self.duration, self.easing)
    }
}

/// One value per 60 fps frame until the spring settles, then exactly `1.0`.
impl Iterator for Spring {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        if self.done {
            return None;
        }
        if self.t < self.settling_time {
            let value = self.curve.position(self.t);
            self.t += FRAME_MS;
            return Some(value);
        }
        self.done = true;
        Some(1.0)
    }
}

/// JavaScript's `Math.round` (halves round toward +∞).
pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Format a number the way JavaScript's `String(number)` does for these magnitudes.
pub(crate) fn js_number(x: f64) -> String {
    if x == 0.0 {
        "0".into() // also -0
    } else {
        x.to_string()
    }
}

fn linear_easing(curve: &Curve, settling_time: f64) -> String {
    let points = adaptive_sample(curve, settling_time);
    let last = points.len() - 1;
    let parts: Vec<String> = points
        .iter()
        .enumerate()
        .map(|(i, &(t, value))| {
            let value = if i == last { 1.0 } else { js_round(value * 10000.0) / 10000.0 };
            if i == 0 || i == last {
                js_number(value)
            } else {
                let percent = js_round(t / settling_time * 1000.0) / 10.0;
                format!("{} {}%", js_number(value), js_number(percent))
            }
        })
        .collect();
    format!("linear({})", parts.join(", "))
}

/// Sample densely where the curve bends and sparsely where it is straight.
fn adaptive_sample(curve: &Curve, duration: f64) -> Vec<(f64, f64)> {
    struct Sampler<'a> {
        curve: &'a Curve,
        points: Vec<(f64, f64)>,
    }

    impl Sampler<'_> {
        fn add(&mut self, t: f64, v: f64) {
            if self.points.last().is_none_or(|&(last, _)| last < t) {
                self.points.push((t, v));
            }
        }

        fn subdivide(&mut self, (t0, v0): (f64, f64), (t1, v1): (f64, f64), depth: u32) {
            const TOLERANCE: f64 = 0.002;
            const MIN_SEGMENT: f64 = 8.0;
            if depth > 12 {
                self.add(t0, v0);
                return;
            }
            let t_mid = (t0 + t1) / 2.0;
            let v_mid = self.curve.position(t_mid);
            let error = (v_mid - (v0 + v1) / 2.0).abs();
            if error > TOLERANCE && t1 - t0 > MIN_SEGMENT {
                self.subdivide((t0, v0), (t_mid, v_mid), depth + 1);
                self.subdivide((t_mid, v_mid), (t1, v1), depth + 1);
            } else {
                self.add(t0, v0);
            }
        }
    }

    let mut s = Sampler { curve, points: Vec::new() };
    s.subdivide((0.0, curve.position(0.0)), (duration, curve.position(duration)), 0);
    s.add(duration, curve.position(duration));
    s.points
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from Remix's spring.test.ts, plus exact output checks against spring.ts.

    #[test]
    fn interface_duration_easing_and_display() {
        let s = spring(SpringPreset::Snappy);
        assert!(s.duration() > 0.0 && s.duration() < 20000.0);
        assert!(s.easing().starts_with("linear(") && s.easing().ends_with(')'));
        let css = format!("transform {s}");
        assert!(css.starts_with(&format!("transform {}ms linear(", s.duration())), "{css}");
    }

    #[test]
    fn presets() {
        for preset in [SpringPreset::Bouncy, SpringPreset::Snappy, SpringPreset::Smooth] {
            assert!(spring(preset).duration() > 0.0);
        }
        assert!(spring(SpringPreset::Bouncy.duration(1000.0)).duration() > spring(SpringPreset::Bouncy).duration());
        assert_eq!(SpringPreset::Bouncy.defaults(), SpringOptions { duration: 400.0, bounce: 0.3, velocity: 0.0 });
    }

    #[test]
    fn custom_options() {
        let short = spring(SpringOptions::default().duration(100.0));
        let long = spring(SpringOptions::default().duration(500.0));
        assert!(long.duration() > short.duration());
        assert!(spring(SpringOptions::default().bounce(0.5)).duration() > 0.0);
        assert!(spring(SpringOptions::default().velocity(5.0)).duration() > 0.0);
    }

    #[test]
    fn physics_invariants() {
        let mut s = spring(SpringPreset::Snappy);
        assert!(s.next().unwrap().abs() < 0.05);
        assert_eq!(spring(SpringPreset::Snappy).last(), Some(1.0));
        assert!(spring(SpringOptions::default().bounce(0.5)).fold(0.0, f64::max) > 1.0);
        assert!(spring(SpringOptions::default().bounce(0.0)).all(|v| v <= 1.001));
        assert!(spring(SpringOptions::default().bounce(-0.5)).all(|v| v <= 1.001));
        let low = spring(SpringOptions::default().bounce(0.1));
        let high = spring(SpringOptions::default().bounce(0.7));
        assert!(high.duration() > low.duration());
        let mut still = spring(SpringOptions::default());
        let mut moving = spring(SpringOptions::default().velocity(10.0));
        still.next();
        moving.next();
        assert!(moving.next().unwrap() > still.next().unwrap());
    }

    #[test]
    fn transition_helper() {
        let one = spring_transition(&["transform"], SpringPreset::Bouncy);
        assert!(one.starts_with("transform 550ms linear("), "{one}");
        let two = spring_transition(&["transform", "opacity"], SpringPreset::Snappy);
        assert!(two.starts_with("transform 350ms linear(") && two.contains("), opacity 350ms linear("), "{two}");
    }

    #[test]
    fn css_output_matches_remix() {
        // `String(spring(...))` from Remix's spring.ts, run in Node 24.
        assert_eq!(spring(SpringPreset::Snappy).to_string(), "350ms linear(0, 0.0132 1.6%, 0.0471 3.1%, 0.0949 4.7%, 0.1514 6.3%, 0.3993 12.5%, 0.5123 15.6%, 0.6104 18.8%, 0.6927 21.9%, 0.7601 25%, 0.8142 28.1%, 0.8572 31.3%, 0.8909 34.4%, 0.9171 37.5%, 0.9372 40.6%, 0.9527 43.8%, 0.9734 50%, 0.9852 56.3%, 0.9918 62.5%, 0.9976 75%, 1)");
        assert_eq!(spring(SpringPreset::Bouncy).to_string(), "550ms linear(0, 0.0022 0.8%, 0.0086 1.6%, 0.0321 3.1%, 0.0676 4.7%, 0.1124 6.3%, 0.1643 7.8%, 0.2211 9.4%, 0.3425 12.5%, 0.4654 15.6%, 0.5818 18.8%, 0.6868 21.9%, 0.7777 25%, 0.8534 28.1%, 0.9141 31.3%, 0.9609 34.4%, 0.9953 37.5%, 1.0192 40.6%, 1.0344 43.8%, 1.0427 46.9%, 1.0458 50%, 1.0421 56.3%, 1.0319 62.5%, 1.0114 75%, 1.0006 87.5%, 1)");
        assert_eq!(spring(SpringPreset::Smooth).to_string(), "1050ms linear(0, 0.0074 0.8%, 0.0262 1.6%, 0.0528 2.3%, 0.0843 3.1%, 0.2282 6.3%, 0.2994 7.8%, 0.3663 9.4%, 0.4279 10.9%, 0.4842 12.5%, 0.5352 14.1%, 0.5814 15.6%, 0.6231 17.2%, 0.6607 18.8%, 0.725 21.9%, 0.7772 25%, 0.8195 28.1%, 0.8537 31.3%, 0.8815 34.4%, 0.904 37.5%, 0.937 43.8%, 0.9586 50%, 0.9728 56.3%, 0.9822 62.5%, 0.9923 75%, 1)");
        assert_eq!(spring(SpringOptions { duration: 600.0, bounce: 0.2, velocity: 0.0 }).to_string(), "900ms linear(0, 0.0026 0.8%, 0.01 1.6%, 0.037 3.1%, 0.0769 4.7%, 0.1263 6.3%, 0.1821 7.8%, 0.242 9.4%, 0.3663 12.5%, 0.4874 15.6%, 0.5982 18.8%, 0.6949 21.9%, 0.7763 25%, 0.8424 28.1%, 0.8945 31.3%, 0.9343 34.4%, 0.9637 37.5%, 0.9845 40.6%, 0.9985 43.8%, 1.0124 50%, 1)");
        let frames: Vec<f64> = spring(SpringPreset::Bouncy).collect();
        assert_eq!(frames.len(), 34);
        assert!((frames[1] - 0.03027069117183745).abs() < 1e-12);
        assert!((frames[5] - 0.44699379935701256).abs() < 1e-12);
    }
}

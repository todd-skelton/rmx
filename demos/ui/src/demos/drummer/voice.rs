//! `voice-looper.ts`: decaying envelopes for the equalizer. Each hit sets a voice to its
//! amplitude and it halves every `half_life_ms` until it falls below an epsilon.

const EPSILON: f64 = 0.001;
const HALF_LIFE_MS: f64 = 220.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Voice {
    pub value: f64,
    /// When the envelope last advanced; `None` once it has decayed.
    last_ms: Option<f64>,
}

impl Voice {
    pub fn trigger(&mut self, amplitude: f64, now_ms: f64) {
        self.value = amplitude;
        self.last_ms = Some(now_ms);
    }

    /// Decay to `now_ms`; whether the voice is still audible.
    pub fn advance(&mut self, now_ms: f64) -> bool {
        let Some(last) = self.last_ms else { return false };
        self.value *= 0.5f64.powf((now_ms - last).max(0.0) / HALF_LIFE_MS);
        self.last_ms = Some(now_ms);
        if self.value <= EPSILON {
            self.value = 0.0;
            self.last_ms = None;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_halves_every_half_life_and_stops_near_zero() {
        let mut voice = Voice::default();
        assert!(!voice.advance(0.0));
        voice.trigger(1.0, 1000.0);
        assert!(voice.advance(1000.0 + HALF_LIFE_MS));
        assert!((voice.value - 0.5).abs() < 1e-9);
        assert!(voice.advance(1000.0 + 2.0 * HALF_LIFE_MS));
        assert!((voice.value - 0.25).abs() < 1e-9);
        assert!(!voice.advance(1000.0 + 20.0 * HALF_LIFE_MS));
        assert_eq!(voice.value, 0.0);
    }
}

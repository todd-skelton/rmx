//! `drummer.ts`: a Web Audio drum machine that schedules a "boom bap" pattern ahead of
//! the audio clock and reports every hit and state change through a typed event target.

use std::cell::Cell;
use std::rc::Rc;

use rmx::TypedEventTarget;

/// What happened (Remix's `DrummerEventMap` keys). Every event but `Change` is followed by
/// a `Change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumEvent {
    Kick,
    Snare,
    Hat,
    Play,
    Stop,
    TempoChange,
    Change,
}

/// The drum machine. Clones share one machine, so it can go in context.
#[derive(Clone)]
pub struct Drummer(Rc<Inner>);

struct Inner {
    bpm: Cell<u32>,
    playing: Cell<bool>,
    current_16th: Cell<u32>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    next_note_time: Cell<f64>,
    events: TypedEventTarget<DrumEvent>,
    #[cfg(target_arch = "wasm32")]
    audio: std::cell::RefCell<Option<audio::Audio>>,
    #[cfg(target_arch = "wasm32")]
    interval: std::cell::RefCell<Option<Interval>>,
}

/// A running `setInterval` and the closure it calls.
#[cfg(target_arch = "wasm32")]
type Interval = (i32, wasm_bindgen::closure::Closure<dyn FnMut()>);

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const LOOKAHEAD_MS: i32 = 25;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const SCHEDULE_AHEAD_S: f64 = 0.1;

impl Drummer {
    pub fn new(bpm: u32) -> Self {
        Drummer(Rc::new(Inner {
            bpm: Cell::new(bpm),
            playing: Cell::new(false),
            current_16th: Cell::new(0),
            next_note_time: Cell::new(0.0),
            events: TypedEventTarget::new(),
            #[cfg(target_arch = "wasm32")]
            audio: Default::default(),
            #[cfg(target_arch = "wasm32")]
            interval: Default::default(),
        }))
    }

    pub fn events(&self) -> &TypedEventTarget<DrumEvent> {
        &self.0.events
    }

    pub fn is_playing(&self) -> bool {
        self.0.playing.get()
    }

    pub fn bpm(&self) -> u32 {
        self.0.bpm.get()
    }

    fn emit(&self, event: DrumEvent) {
        self.0.events.dispatch(&event);
        self.0.events.dispatch(&DrumEvent::Change);
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play(None);
        }
    }

    /// Clamp to 30..=300 BPM (a zero keeps the current tempo).
    pub fn set_tempo(&self, bpm: i64) {
        let bpm = if bpm == 0 { self.bpm() as i64 } else { bpm };
        self.0.bpm.set(bpm.clamp(30, 300) as u32);
        self.emit(DrumEvent::TempoChange);
    }

    /// Start (or keep) playing, optionally at a new tempo. Browsers only start audio after
    /// a user gesture, so call this from an event handler.
    pub fn play(&self, bpm: Option<u32>) {
        #[cfg(target_arch = "wasm32")]
        {
            let this = self.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(resume) = this.ensure_context() else { return };
                if let Some(bpm) = bpm {
                    this.set_tempo(bpm as i64);
                }
                let _ = wasm_bindgen_futures::JsFuture::from(resume).await;
                if this.0.playing.replace(true) {
                    return;
                }
                if let Some(audio) = this.0.audio.borrow().as_ref() {
                    this.0.next_note_time.set(audio.ctx.current_time());
                }
                // current_16th is kept so a restart picks the groove back up.
                this.clear_interval();
                let tick = this.clone();
                let closure = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || tick.scheduler());
                let window = web_sys::window().expect("window");
                use wasm_bindgen::JsCast;
                let id = window
                    .set_interval_with_callback_and_timeout_and_arguments_0(closure.as_ref().unchecked_ref(), LOOKAHEAD_MS)
                    .unwrap_or(0);
                *this.0.interval.borrow_mut() = Some((id, closure));
                this.emit(DrumEvent::Play);
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = bpm;
    }

    pub fn stop(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(now) = self.0.audio.borrow().as_ref().map(|a| a.ctx.current_time()) else { return };
            self.clear_interval();
            self.0.playing.set(false);
            self.0.current_16th.set(0);
            self.0.next_note_time.set(now);
            self.emit(DrumEvent::Stop);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_interval(&self) {
        if let Some((id, _closure)) = self.0.interval.borrow_mut().take() {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(id);
            }
        }
    }

    /// Create the audio graph on first use; the `resume()` promise.
    #[cfg(target_arch = "wasm32")]
    fn ensure_context(&self) -> Option<js_sys::Promise> {
        let mut audio = self.0.audio.borrow_mut();
        if audio.is_none() {
            *audio = audio::Audio::new();
        }
        audio.as_ref()?.ctx.resume().ok()
    }

    fn seconds_per_16th(&self) -> f64 {
        60.0 / self.bpm().max(1) as f64 / 4.0
    }

    /// The pattern over 16 steps: kick on 0 and 10, snare on 4 and 12, hats on the eighths
    /// plus 7 and 9.
    fn hits(step: u32) -> impl Iterator<Item = DrumEvent> {
        [
            (step == 0 || step == 10).then_some(DrumEvent::Kick),
            (step == 4 || step == 12).then_some(DrumEvent::Snare),
            step.is_multiple_of(2).then_some(DrumEvent::Hat),
            (step == 7 || step == 9).then_some(DrumEvent::Hat),
        ]
        .into_iter()
        .flatten()
    }

    #[cfg(target_arch = "wasm32")]
    fn scheduler(&self) {
        loop {
            let Some(now) = self.0.audio.borrow().as_ref().map(|a| a.ctx.current_time()) else { return };
            let time = self.0.next_note_time.get();
            if time >= now + SCHEDULE_AHEAD_S {
                return;
            }
            for hit in Self::hits(self.0.current_16th.get()) {
                if let Some(audio) = self.0.audio.borrow().as_ref() {
                    audio.play(hit, time);
                }
                self.emit(hit);
            }
            self.0.next_note_time.set(time + self.seconds_per_16th());
            self.0.current_16th.set((self.0.current_16th.get() + 1) % 16);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod audio {
    use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode, BiquadFilterType, GainNode, OscillatorType};

    use super::DrumEvent;

    fn play_buffer(source: &AudioBufferSourceNode, time: f64, duration: f64) -> Result<(), wasm_bindgen::JsValue> {
        let source: &AudioScheduledSourceNode = source.as_ref();
        source.start_with_when(time)?;
        source.stop_with_when(time + duration)
    }

    pub(super) struct Audio {
        pub ctx: AudioContext,
        master: GainNode,
        noise: AudioBuffer,
    }

    impl Audio {
        pub fn new() -> Option<Audio> {
            let ctx = AudioContext::new().ok()?;
            let master = ctx.create_gain().ok()?;
            master.gain().set_value(0.8);
            master.connect_with_audio_node(&ctx.destination()).ok()?;
            // One second of white noise for the snare and hats.
            let rate = ctx.sample_rate();
            let noise = ctx.create_buffer(1, rate as u32, rate).ok()?;
            let data: Vec<f32> = (0..rate as usize).map(|_| (js_sys::Math::random() * 2.0 - 1.0) as f32).collect();
            noise.copy_to_channel(&data, 0).ok()?;
            Some(Audio { ctx, master, noise })
        }

        pub fn play(&self, hit: DrumEvent, time: f64) {
            let _ = match hit {
                DrumEvent::Kick => self.kick(time),
                DrumEvent::Snare => self.snare(time),
                DrumEvent::Hat => self.hat(time),
                _ => Ok(()),
            };
        }

        fn gain(&self, start: f32, end: f32, time: f64, duration: f64) -> Result<GainNode, wasm_bindgen::JsValue> {
            let gain = self.ctx.create_gain()?;
            gain.gain().set_value_at_time(start, time)?;
            gain.gain().exponential_ramp_to_value_at_time(end, time + duration)?;
            Ok(gain)
        }

        fn kick(&self, time: f64) -> Result<(), wasm_bindgen::JsValue> {
            let osc = self.ctx.create_oscillator()?;
            osc.set_type(OscillatorType::Sine);
            osc.frequency().set_value_at_time(150.0, time)?;
            osc.frequency().exponential_ramp_to_value_at_time(50.0, time + 0.1)?;
            let gain = self.gain(1.0, 0.001, time, 0.15)?;
            osc.connect_with_audio_node(&gain)?.connect_with_audio_node(&self.master)?;
            osc.start_with_when(time)?;
            osc.stop_with_when(time + 0.2)
        }

        fn snare(&self, time: f64) -> Result<(), wasm_bindgen::JsValue> {
            let noise = self.ctx.create_buffer_source()?;
            noise.set_buffer(Some(&self.noise));
            let band = self.ctx.create_biquad_filter()?;
            band.set_type(BiquadFilterType::Bandpass);
            band.frequency().set_value(1800.0);
            let noise_gain = self.gain(1.0, 0.01, time, 0.2)?;
            noise.connect_with_audio_node(&band)?.connect_with_audio_node(&noise_gain)?.connect_with_audio_node(&self.master)?;
            play_buffer(&noise, time, 0.2)?;
            let osc = self.ctx.create_oscillator()?;
            osc.set_type(OscillatorType::Triangle);
            osc.frequency().set_value_at_time(200.0, time)?;
            let osc_gain = self.gain(0.6, 0.01, time, 0.12)?;
            osc.connect_with_audio_node(&osc_gain)?.connect_with_audio_node(&self.master)?;
            osc.start_with_when(time)?;
            osc.stop_with_when(time + 0.15)
        }

        fn hat(&self, time: f64) -> Result<(), wasm_bindgen::JsValue> {
            let noise = self.ctx.create_buffer_source()?;
            noise.set_buffer(Some(&self.noise));
            let hp = self.ctx.create_biquad_filter()?;
            hp.set_type(BiquadFilterType::Highpass);
            hp.frequency().set_value(7000.0);
            let gain = self.gain(0.5, 0.001, time, 0.04)?;
            noise.connect_with_audio_node(&hp)?.connect_with_audio_node(&gain)?.connect_with_audio_node(&self.master)?;
            play_buffer(&noise, time, 0.05)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn the_pattern_is_boom_bap() {
        let steps: Vec<Vec<DrumEvent>> = (0..16).map(|s| Drummer::hits(s).collect()).collect();
        assert_eq!(steps[0], [DrumEvent::Kick, DrumEvent::Hat]);
        assert_eq!(steps[4], [DrumEvent::Snare, DrumEvent::Hat]);
        assert_eq!(steps[7], [DrumEvent::Hat]);
        assert_eq!(steps[10], [DrumEvent::Kick, DrumEvent::Hat]);
        assert!(steps[11].is_empty());
        let hats = steps.iter().flatten().filter(|e| **e == DrumEvent::Hat).count();
        assert_eq!(hats, 10);
    }

    #[test]
    fn tempo_is_clamped_and_reported() {
        let drummer = Drummer::new(80);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        drummer.events().add_listener(move |e| s.borrow_mut().push(*e));
        drummer.set_tempo(500);
        assert_eq!(drummer.bpm(), 300);
        drummer.set_tempo(0);
        assert_eq!(drummer.bpm(), 300);
        drummer.set_tempo(-4);
        assert_eq!(drummer.bpm(), 30);
        assert_eq!(seen.borrow().len(), 6);
        assert_eq!(seen.borrow()[..2], [DrumEvent::TempoChange, DrumEvent::Change]);
        assert!((drummer.seconds_per_16th() - 0.5).abs() < 1e-9);
    }
}

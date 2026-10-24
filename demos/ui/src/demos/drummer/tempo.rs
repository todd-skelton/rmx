//! `tempo-interaction.tsx`: tap tempo. Four or more taps (pointer, Enter or Space) within
//! four seconds dispatch a `my:tempo` event from the host with the average BPM as `bpm`.

use rsmix::{mixin, on, Element, Event, HostMixin, Mixin, MixinHandle};

pub const TEMPO_EVENT: &str = "my:tempo";
const WINDOW_MS: f64 = 4000.0;

/// `mix={tempo_events()}`
pub fn tempo_events() -> Mixin {
    mixin::<TempoEvents>(())
}

/// The BPM of a `my:tempo` event.
pub fn tempo_bpm(event: &Event) -> Option<u32> {
    event.prop_f64("bpm").map(|bpm| bpm as u32)
}

/// The average BPM of the intervals between taps (milliseconds), once there are four.
pub fn average_bpm(taps: &[f64]) -> Option<u32> {
    if taps.len() < 4 {
        return None;
    }
    let bpms: Vec<f64> = taps.windows(2).map(|w| 60000.0 / (w[1] - w[0])).collect();
    Some((bpms.iter().sum::<f64>() / bpms.len() as f64).round() as u32)
}

#[derive(Default)]
pub struct TempoEvents {
    taps: Vec<f64>,
    /// Bumped on every tap so only the last tap's reset timer clears the taps.
    generation: u32,
}

impl TempoEvents {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn tap(&mut self, event: &Event, h: &MixinHandle<Self>) {
        #[cfg(target_arch = "wasm32")]
        {
            let now = js_sys::Date::now();
            self.taps.push(now);
            self.taps.retain(|tap| now - tap < WINDOW_MS);
            if let (Some(bpm), Some(target)) = (average_bpm(&self.taps), event.current_element()) {
                let init = web_sys::EventInit::new();
                if let Ok(tempo) = web_sys::Event::new_with_event_init_dict(TEMPO_EVENT, &init) {
                    let _ = js_sys::Reflect::set(&tempo, &"bpm".into(), &(bpm as f64).into());
                    let _ = target.dispatch_event(&tempo);
                }
            }
            self.generation += 1;
            let (generation, h) = (self.generation, h.clone());
            let reset = wasm_bindgen::closure::Closure::once_into_js(move || {
                h.with(|this, _| {
                    if this.generation == generation {
                        this.taps.clear();
                    }
                });
            });
            if let Some(window) = web_sys::window() {
                use wasm_bindgen::JsCast;
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(reset.unchecked_ref(), WINDOW_MS as i32);
            }
        }
    }
}

impl HostMixin for TempoEvents {
    type Args = ();

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        TempoEvents::default()
    }

    fn render(&mut self, _: &(), host: &mut Element, h: &MixinHandle<Self>) {
        host.mix(on("pointerdown", h.on_event(|this, h, event| this.tap(event, h))));
        host.mix(on(
            "keydown",
            h.on_event(|this, h, event| {
                if !event.prop_bool("repeat") && matches!(event.key().as_str(), "Enter" | " ") {
                    this.tap(event, h);
                }
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_taps_half_a_second_apart_are_120_bpm() {
        assert_eq!(average_bpm(&[0.0, 500.0, 1000.0]), None);
        assert_eq!(average_bpm(&[0.0, 500.0, 1000.0, 1500.0]), Some(120));
        assert_eq!(average_bpm(&[0.0, 600.0, 1200.0, 1800.0, 2400.0]), Some(100));
    }
}

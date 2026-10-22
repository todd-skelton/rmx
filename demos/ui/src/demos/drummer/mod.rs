//! `demos/ui/cases/drummer`: a Web Audio drum machine. The `Drummer` model lives in
//! context; the equalizer and controls subscribe to its events and re-render themselves.
//! Space toggles playback and ArrowUp/ArrowDown change the tempo anywhere on the page.

// Playback, the equalizer loop and tap tempo only run in the browser.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod components;
mod model;
mod tempo;
mod voice;

use rmx::{on, rsx, Component, Handle, Mixin, Node, NodeHandle};

use components::*;
pub use model::{DrumEvent, Drummer};
use tempo::{tempo_bpm, tempo_events, TEMPO_EVENT};
use voice::Voice;

fn drummer<C: Component>(h: &Handle<C>) -> Drummer {
    h.context::<Drummer>().map(|d| (*d).clone()).expect("inside <DrummerApp>")
}

/// Run `f` on a primary pointer press or a non-repeated Enter or Space (Remix's
/// `onPressDown` mixin).
fn press_down<C: Component>(h: &Handle<C>, f: impl Fn(&mut C, &Handle<C>) + Clone + 'static) -> Vec<Mixin> {
    let on_key = f.clone();
    vec![
        on(
            "pointerdown",
            h.on_event(move |this, h, event| {
                if event.prop_bool("isPrimary") {
                    f(this, h);
                }
            }),
        ),
        on(
            "keydown",
            h.on_event(move |this, h, event| {
                if matches!(event.key().as_str(), "Enter" | " ") && !event.prop_bool("repeat") {
                    event.prevent_default();
                    on_key(this, h);
                }
            }),
        ),
    ]
}

fn mixes(first: Mixin, rest: Vec<Mixin>) -> Vec<Mixin> {
    let mut all = vec![first];
    all.extend(rest);
    all
}

#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
fn focus(node: &Option<NodeHandle>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(Ok(el)) = node.as_ref().map(|n| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(n.element().clone())) {
        let _ = el.focus();
    }
}

/// The app (Remix's `App`): provides the drummer and the page-wide keyboard shortcuts.
pub struct DrummerApp {
    #[cfg(target_arch = "wasm32")]
    _keydown: Option<wasm_bindgen::closure::Closure<dyn Fn(web_sys::Event)>>,
}

impl Component for DrummerApp {
    type Props = ();
    rmx::client_entry!("DrummerApp");

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let drummer = Drummer::new(80);
        h.set_context(drummer.clone());
        #[cfg(target_arch = "wasm32")]
        h.queue_task(move |this, h| {
            use wasm_bindgen::JsCast;
            let keydown = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
                let key = js_sys::Reflect::get(&event, &"key".into()).ok().and_then(|k| k.as_string()).unwrap_or_default();
                match key.as_str() {
                    " " => drummer.toggle(),
                    "ArrowUp" => drummer.set_tempo(drummer.bpm() as i64 + 1),
                    "ArrowDown" => drummer.set_tempo(drummer.bpm() as i64 - 1),
                    _ => {}
                }
            });
            let options = web_sys::AddEventListenerOptions::new();
            options.set_signal(&h.signal());
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let _ = doc.add_event_listener_with_callback_and_add_event_listener_options("keydown", keydown.as_ref().unchecked_ref(), &options);
            }
            this._keydown = Some(keydown);
        });
        DrummerApp {
            #[cfg(target_arch = "wasm32")]
            _keydown: None,
        }
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <Layout>
                <Equalizer/>
                <DrumControls/>
            </Layout>
        }
    }
}

/// Bars that jump on each hit and decay (Remix's `Equalizer`).
pub struct Equalizer {
    /// Kick, snare, hat.
    voices: [Voice; 3],
    looping: bool,
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

impl Equalizer {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn trigger(&mut self, voice: usize, h: &Handle<Self>) {
        #[cfg(target_arch = "wasm32")]
        {
            self.voices[voice].trigger(1.0, now_ms());
            if !self.looping {
                self.looping = true;
                Self::request_frame(h.clone());
                h.update();
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn request_frame(h: Handle<Self>) {
        use wasm_bindgen::JsCast;
        let tick = wasm_bindgen::closure::Closure::once_into_js(move |now: f64| {
            h.with(|this, h| {
                let mut active = false;
                for voice in &mut this.voices {
                    active |= voice.advance(now);
                }
                if active {
                    h.update();
                    Self::request_frame(h.clone());
                } else {
                    this.looping = false;
                }
            });
        });
        if let Some(window) = web_sys::window() {
            let _ = window.request_animation_frame(tick.unchecked_ref());
        }
    }
}

impl Component for Equalizer {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.listen(drummer(h).events(), |this, h, event| match event {
            DrumEvent::Kick => this.trigger(0, h),
            DrumEvent::Snare => this.trigger(1, h),
            DrumEvent::Hat => this.trigger(2, h),
            _ => {}
        });
        Equalizer { voices: Default::default(), looping: false }
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let [kick, snare, hat] = self.voices.map(|v| v.value);
        let volumes = [0.4, 0.8, 0.3, 0.1].map(|v| v * kick).into_iter().chain([0.4, 1.0, 0.7].map(|v| v * snare)).chain([0.1, 0.8].map(|v| v * hat));
        let bars: Vec<Node> = volumes.map(|volume| rsx! { <EqualizerBar volume={volume}/> }).collect();
        rsx! { <EqualizerLayout>{bars}</EqualizerLayout> }
    }
}

/// Tap tempo, the tempo display, play and stop (Remix's `DrumControls`).
pub struct DrumControls {
    drummer: Drummer,
    play: Option<NodeHandle>,
    stop: Option<NodeHandle>,
    focus_stop_when_playing: bool,
}

impl Component for DrumControls {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        let drummer = drummer(h);
        h.listen(drummer.events(), |this, h, event| {
            if *event != DrumEvent::Change {
                return;
            }
            h.update();
            if this.focus_stop_when_playing && this.drummer.is_playing() {
                this.focus_stop_when_playing = false;
                h.queue_task(|this, _| focus(&this.stop));
            }
        });
        DrumControls { drummer, play: None, stop: None, focus_stop_when_playing: false }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let playing = self.drummer.is_playing();
        let set_tempo = on(
            TEMPO_EVENT,
            h.on_event(|this, _, event| {
                if let Some(bpm) = tempo_bpm(event) {
                    this.drummer.play(Some(bpm));
                }
            }),
        );
        let play = on(
            "click",
            h.on(|this, _| {
                // Playing starts once the audio context resumes; focus Stop after that.
                this.focus_stop_when_playing = true;
                this.drummer.play(None);
            }),
        );
        let stop = press_down(h, |this, h| {
            // Stop reports synchronously, while this component is busy handling the event.
            this.drummer.stop();
            h.update();
            h.queue_task(|this, _| focus(&this.play));
        });
        rsx! {
            <ControlGroup>
                <button id="set-tempo" mix={[button_css(), tempo_events(), set_tempo]}>"SET TEMPO"</button>
                <TempoDisplay/>
                <button id="play" disabled={playing} mix={[button_css(), h.node_ref(|this, node| this.play = Some(node.clone())), play]}>"PLAY"</button>
                <button id="stop" disabled={!playing} mix={mixes(button_css(), mixes(h.node_ref(|this, node| this.stop = Some(node.clone())), stop))}>"STOP"</button>
            </ControlGroup>
        }
    }
}

/// The BPM readout with up and down buttons (Remix's `TempoDisplay`).
pub struct TempoDisplay {
    drummer: Drummer,
}

impl Component for TempoDisplay {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        TempoDisplay { drummer: drummer(h) }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let up = press_down(h, |this, _| this.drummer.set_tempo(this.drummer.bpm() as i64 + 1));
        let down = press_down(h, |this, _| this.drummer.set_tempo(this.drummer.bpm() as i64 - 1));
        rsx! {
            <TempoLayout>
                <BpmDisplay bpm={self.drummer.bpm()}/>
                <TempoButtons>
                    <button id="tempo-up" mix={mixes(tempo_button_css(), up)}>{triangle(true)}</button>
                    <button id="tempo-down" mix={mixes(tempo_button_css(), down)}>{triangle(false)}</button>
                </TempoButtons>
            </TempoLayout>
        }
    }
}

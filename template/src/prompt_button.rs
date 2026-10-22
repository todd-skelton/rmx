//! `app/actions/public/prompt-button.tsx`: copies a prompt to the clipboard, confirms it,
//! then fades back. This component hydrates on its own; the rest of the page stays static HTML.

use rmx::{css, on, rsx, Component, Handle, Mixin, Node};
use serde::{Deserialize, Serialize};

const FADE_MS: u32 = 180;
const HOLD_MS: u32 = 1200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyState {
    Idle,
    Copied,
    Failed,
    Resetting,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PromptButtonProps {
    pub text: String,
}

pub struct PromptButton {
    state: CopyState,
}

impl PromptButton {
    /// Set the state and wait for the update to reach the DOM (`await handle.update()`).
    async fn show(h: &Handle<Self>, state: CopyState) {
        h.with(|this, _| this.state = state);
        h.updated().await;
    }
}

impl Component for PromptButton {
    type Props = PromptButtonProps;
    rmx::client_entry!("PromptButton");

    fn setup(_: &PromptButtonProps, _: &Handle<Self>) -> Self {
        PromptButton { state: CopyState::Idle }
    }

    fn render(&self, props: &PromptButtonProps, h: &Handle<Self>) -> Node {
        let prompt_label = format!("\u{201C}{}\u{201D}", props.text);
        let state = self.state;
        let label = match state {
            CopyState::Copied | CopyState::Resetting => "Copied to clipboard".to_string(),
            CopyState::Failed => "Copy failed".to_string(),
            CopyState::Idle => prompt_label.clone(),
        };
        let idle = state == CopyState::Idle;
        let text = props.text.clone();
        let copy = h.on_event(move |_, h, event| {
            let (h, signal, text) = (h.clone(), event.signal(), text.clone());
            h.clone().spawn(async move {
                let copied = write_clipboard(&text).await;
                if copied && signal.aborted() {
                    return;
                }
                Self::show(&h, if copied { CopyState::Copied } else { CopyState::Failed }).await;
                rmx::browser::sleep(HOLD_MS).await;
                if signal.aborted() {
                    return;
                }
                Self::show(&h, CopyState::Resetting).await;
                rmx::browser::sleep(FADE_MS).await;
                if signal.aborted() {
                    return;
                }
                Self::show(&h, CopyState::Idle).await;
            });
        });
        rsx! {
            <button type="button" mix={[button_style(), on("click", copy)]}
                style={(!idle).then_some("background: var(--surface-4); color: var(--brand-blue)")}>
                <span aria-hidden="true" mix={icon_slot_style()}>{copy_icon()}</span>
                <span mix={label_slot_style()} style={if state == CopyState::Resetting { "opacity: 0" } else { "opacity: 1" }}>
                    <span aria-hidden={idle.then_some("true")} mix={status_label_style()}
                        style={if idle { "visibility: hidden" } else { "visibility: visible" }}>
                        {label}
                    </span>
                    <span aria-hidden={(!idle).then_some("true")} style={if idle { "visibility: visible" } else { "visibility: hidden" }}>
                        {prompt_label}
                    </span>
                </span>
            </button>
        }
    }
}

/// `navigator.clipboard.writeText(text)`; false when the browser refuses.
async fn write_clipboard(text: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else { return false };
        let promise = window.navigator().clipboard().write_text(text);
        wasm_bindgen_futures::JsFuture::from(promise).await.is_ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = text;
        false
    }
}

pub fn copy_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 14 16.5" fill="none">
            <path d="M0.75 9.188L0.75 4.083C0.75 2.242 2.242 0.75 4.083 0.75L9.188 0.75M5.75 15.75L11.375 15.75C12.41 15.75 13.25 14.91 13.25 13.875L13.25 5.75C13.25 4.714 12.41 3.875 11.375 3.875L5.75 3.875C4.714 3.875 3.875 4.714 3.875 5.75L3.875 13.875C3.875 14.91 4.714 15.75 5.75 15.75Z"
                stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
    }
}

fn button_style() -> Mixin {
    css("appearance: none; font: inherit; text-align: left; cursor: pointer; width: 100%; display: flex; gap: 16px; align-items: center; padding: 16px; border: 0; border-radius: 12px; color: var(--text-primary); background: transparent; transition: background-color 150ms ease, color 150ms ease; &:hover, &:focus-visible { background: var(--surface-4); color: var(--brand-blue); outline: none; }")
}

fn label_slot_style() -> Mixin {
    css("align-items: center; display: flex; font-size: 14px; flex: 1 1 0; line-height: 1.5; min-width: 0; position: relative; transition: opacity 180ms ease;")
}

fn status_label_style() -> Mixin {
    css("align-items: center; display: flex; inset: 0; position: absolute;")
}

fn icon_slot_style() -> Mixin {
    css("flex: 0 0 24px; width: 24px; display: flex; align-items: center; justify-content: center; & svg { width: 20px; height: 20px; display: block; transform: rotate(180deg); }")
}

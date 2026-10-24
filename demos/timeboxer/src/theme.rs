//! `ui/public/design.ts` and `ui/public/button.ts`: design tokens and the button mixin.

use rsmix::{css, Mixin};

pub mod space {
    pub const XS: &str = "4px";
    pub const SM: &str = "8px";
    pub const MD: &str = "12px";
    pub const LG: &str = "16px";
    pub const XL: &str = "24px";
}

pub const RADIUS: &str = "8px";

pub mod font_size {
    pub const XS: &str = "12px";
    pub const SM: &str = "13px";
    pub const MD: &str = "14px";
    pub const XL: &str = "20px";
    pub const XXL: &str = "28px";
}

pub mod surface {
    pub const LVL0: &str = "#ffffff";
    pub const LVL1: &str = "#f8f8f8";
    pub const LVL2: &str = "#f5f5f5";
}

pub mod text {
    pub const PRIMARY: &str = "#151515";
    pub const SECONDARY: &str = "#4f4f4f";
    pub const MUTED: &str = "#6d6d6d";
}

pub mod border {
    pub const SUBTLE: &str = "#e7e7e7";
    pub const DEFAULT: &str = "#d1d1d1";
    pub const STRONG: &str = "#b0b0b0";
}

pub const DANGER: &str = "#FF3000";
pub const FOCUS_RING: &str = "#1A72FF";
pub const LETTER_SPACING_TIGHT: &str = "-0.03em";
pub const LETTER_SPACING_WIDE: &str = "0.08em";
pub const LINE_HEIGHT_TIGHT: &str = "1.25";
pub const LINE_HEIGHT_RELAXED: &str = "1.65";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Primary,
    Ghost,
}

const BASE: &str = r#"--rmx-button-shadow: 0 0 0 0 rgba(0, 0, 0, 0);
  --rmx-button-focus-shadow: 0 0 0 1px light-dark(#3573F6, #6eaaff), var(--rmx-button-shadow), 0 0 0 4px light-dark(rgba(53, 115, 246, 0.1), rgba(110, 170, 255, 0.18)), 0 6px 32px 4px light-dark(rgba(53, 115, 246, 0.08), rgba(110, 170, 255, 0.14)), inset 0 0 8px 1px light-dark(rgba(53, 115, 246, 0.05), rgba(110, 170, 255, 0.1));
  appearance: none; margin: 0; box-sizing: border-box; cursor: revert; display: inline-flex; align-items: center; justify-content: center;
  flex-direction: row; gap: 4px; padding-block: 0; width: max-content; max-width: 100%; border-radius: 999px;
  font-family: "Inter Variable", Inter, ui-sans-serif, system-ui, sans-serif; font-style: normal; font-weight: 500;
  font-feature-settings: "cv01" on, "ss01" on; letter-spacing: 0; white-space: nowrap; text-decoration: none;
  user-select: none; -webkit-user-select: none; vertical-align: top; box-shadow: var(--rmx-button-shadow);
  &:focus-visible { outline: 0; box-shadow: var(--rmx-button-focus-shadow); }
  &:disabled, &[aria-disabled="true"] { cursor: not-allowed; opacity: 0.55; }"#;

const MEDIUM: &str = "height: 26px; min-height: 26px; padding-inline: 12px; font-size: 12px; line-height: 17px; --rmx-button-neutral-shadow-alpha: 0.03;";

const NEUTRAL: &str = r#"background: light-dark(#FCFCFC, #232323); border: 1px solid light-dark(rgba(0, 0, 0, 0.08), rgba(255, 255, 255, 0.14));
  --rmx-button-shadow: 0 -2px 0 -2px rgba(0, 0, 0, 0.06), 0 0 0 1px light-dark(rgba(255, 255, 255, 0.75), rgba(255, 255, 255, 0.08)), 0 1px 0 light-dark(#FFFFFF, rgba(255, 255, 255, 0.04)), 0 2px 4px -1px rgb(0 0 0 / var(--rmx-button-neutral-shadow-alpha)), inset 0 2px 0 light-dark(#FFFFFF, rgba(255, 255, 255, 0.08)), inset 0 -1px 0 rgba(0, 0, 0, 0.05);
  color: light-dark(#101010, #ececec); text-shadow: 0 1px 0 light-dark(#FFFFFF, rgb(0 0 0 / 0.35));
  &:hover:not(:disabled):not([aria-disabled="true"]) { background: light-dark(#FFFFFF, #272727); border-color: light-dark(rgba(0, 0, 0, 0.1), rgba(255, 255, 255, 0.18));
    --rmx-button-shadow: 0 -2px 0 -2px rgba(0, 0, 0, 0.06), 0 0 0 1px light-dark(rgba(255, 255, 255, 0.8), rgba(255, 255, 255, 0.1)), 0 1px 0 light-dark(#FFFFFF, rgba(255, 255, 255, 0.05)), 0 3px 6px -2px rgb(0 0 0 / calc(var(--rmx-button-neutral-shadow-alpha) + 0.03)), inset 0 2px 0 light-dark(#FFFFFF, rgba(255, 255, 255, 0.1)), inset 0 -1px 0 rgba(0, 0, 0, 0.05); }
  &:active:not(:disabled):not([aria-disabled="true"]), &[aria-pressed="true"]:not(:disabled):not([aria-disabled="true"]) { background: light-dark(#F7F7F7, #1f1f1f); border-color: light-dark(rgba(0, 0, 0, 0.12), rgba(255, 255, 255, 0.2));
    --rmx-button-shadow: 0 0 0 1px light-dark(rgba(255, 255, 255, 0.65), rgba(255, 255, 255, 0.08)), inset 0 1px 2px rgba(0, 0, 0, 0.12), inset 0 -1px 0 light-dark(rgba(255, 255, 255, 0.8), rgba(255, 255, 255, 0.05)); text-shadow: none; }"#;

const PRIMARY: &str = r#"background: radial-gradient(50% 50% at 50% 0%, light-dark(rgba(255, 255, 255, 0.2), rgba(255, 255, 255, 0.65)) 0%, rgba(255, 255, 255, 0) 100%), radial-gradient(49.59% 37.11% at 50.41% 101.56%, light-dark(rgba(255, 255, 255, 0.15), rgba(255, 255, 255, 0.35)) 0%, rgba(255, 255, 255, 0) 66.46%), light-dark(#101010, #ececec);
  border: 0;
  --rmx-button-shadow: 0 16px 16px -8px rgba(0, 0, 0, 0.1), 0 8px 8px -4px rgba(0, 0, 0, 0.1), 0 4px 4px -2px rgba(0, 0, 0, 0.1), 0 2px 2px -1px rgba(0, 0, 0, 0.1), inset 0 0 4px 2px light-dark(#101010, rgba(255, 255, 255, 0.45)), inset 0 0 4px 2px rgba(255, 255, 255, 0.1), inset 0 0 12px -6px rgba(255, 255, 255, 0.75);
  color: light-dark(#FFFFFF, #151515); text-shadow: 0 1px 1px light-dark(#000000, rgb(255 255 255 / 0.45));
  &:hover:not(:disabled):not([aria-disabled="true"]) { background: radial-gradient(50% 50% at 50% 0%, light-dark(rgba(255, 255, 255, 0.24), rgba(255, 255, 255, 0.72)) 0%, rgba(255, 255, 255, 0) 100%), radial-gradient(49.59% 37.11% at 50.41% 101.56%, light-dark(rgba(255, 255, 255, 0.18), rgba(255, 255, 255, 0.42)) 0%, rgba(255, 255, 255, 0) 66.46%), light-dark(#171717, #ffffff); }
  &:active:not(:disabled):not([aria-disabled="true"]) { transform: translateY(1px); }"#;

const GHOST: &str = r#"background: transparent; border: 1px solid transparent; --rmx-button-shadow: 0 0 0 0 rgba(0, 0, 0, 0); color: light-dark(#101010, #ececec); text-shadow: none;
  &:hover:not(:disabled):not([aria-disabled="true"]) { background: light-dark(rgba(16, 16, 16, 0.05), rgba(236, 236, 236, 0.1)); }"#;

/// `button({ tone })`: base, medium size and tone styles.
pub fn button(tone: Tone) -> Mixin {
    let tone = match tone {
        Tone::Neutral => NEUTRAL,
        Tone::Primary => PRIMARY,
        Tone::Ghost => GHOST,
    };
    vec![css(BASE), css(MEDIUM), css(tone)].into()
}

/// The spinner keyframes. `css()` scopes rules to a class, where `@keyframes` cannot nest,
/// so the document adds this once.
pub const KEYFRAMES: &str = "@keyframes timebox-spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }";

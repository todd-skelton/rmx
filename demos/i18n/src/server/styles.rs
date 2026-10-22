//! `ui/styles.ts`, converted mechanically from the Remix style objects.

use rmx::{css, Mixin};

pub fn body() -> Mixin {
    css("min-width: 320px; min-height: 100vh; margin: 0; background: light-dark(#f3f5f7, #0d0d10); color: light-dark(#1b1b1f, #e7e7ea); color-scheme: light dark; font-family: 'Inter', ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; font-size: 16px; line-height: 1.6; -webkit-font-smoothing: antialiased; box-sizing: border-box; & *, & *::before, & *::after { box-sizing: inherit; } & ::selection { background: #ffdf5f; color: #1b1b1f; }")
}

pub fn page_wrapper() -> Mixin {
    css("width: calc(100% - 48px); max-width: 1120px; min-height: calc(100vh - 48px); margin: 24px auto; padding: 0 52px 52px; border-radius: 16px; background: light-dark(#ffffff, #0d0d10); @media (max-width: 900px) { width: 100%; min-height: 100vh; margin: 0; padding: 0 24px 40px; border-radius: 0; }")
}

pub fn header() -> Mixin {
    css("display: flex; justify-content: space-between; align-items: center; min-height: 64px; padding: 16px 0; border-block-end: 1px solid light-dark(#e5e7eb, #2a2a30); flex-wrap: wrap; gap: 16px;")
}

pub fn brand_group() -> Mixin {
    css("display: flex; align-items: center; gap: 10px; border-radius: 4px; color: light-dark(#1b1b1f, #e7e7ea); text-decoration: none; &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 4px; }")
}

pub fn logo() -> Mixin {
    css("display: inline-block; font-size: 24px; line-height: 1;")
}

pub fn brand_title() -> Mixin {
    css("margin: 0; font-size: 16px; font-weight: 700; letter-spacing: -0.02em;")
}

pub fn switcher_form() -> Mixin {
    css("display: flex; align-items: center; gap: 8px; flex-wrap: wrap;")
}

pub fn select() -> Mixin {
    css("height: 36px; padding-block: 0; padding-inline-start: 12px; padding-inline-end: 32px; border: 1px solid light-dark(#d1d5db, #3f3f46); border-radius: 8px; color: light-dark(#1b1b1f, #e7e7ea); background-color: light-dark(#ffffff, #16161a); font-family: inherit; font-size: 14px; cursor: pointer; &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; }")
}

pub fn button() -> Mixin {
    css("height: 36px; padding: 0 16px; border: 1px solid light-dark(#1b1b1f, #e7e7ea); border-radius: 8px; color: light-dark(#ffffff, #0d0d10); background-color: light-dark(#1b1b1f, #e7e7ea); font-family: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color 150ms ease-in-out; &:hover { background-color: light-dark(#3d3d41, #cbcbce); } &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; } @media (prefers-reduced-motion: reduce) { transition: none; }")
}

pub fn clear_preference_button() -> Mixin {
    css("min-height: 36px; padding: 0 4px; border: 0; border-radius: 4px; color: light-dark(#0578be, #2dacf9); background-color: transparent; font-family: inherit; font-size: 13px; font-weight: 500; cursor: pointer; white-space: nowrap; &:hover { color: light-dark(#022f4b, #63c2fb); text-decoration: underline; } &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; }")
}

pub fn hero() -> Mixin {
    css("max-width: 800px; padding: 64px 0 40px;")
}

pub fn eyebrow_badge() -> Mixin {
    css("margin-bottom: 16px; color: light-dark(#eb2358, #ff5d7a); font-size: 12px; font-weight: 700; line-height: 1; letter-spacing: 0.06em; text-transform: uppercase;")
}

pub fn hero_heading() -> Mixin {
    css("max-width: 760px; margin: 0 0 16px; color: light-dark(#1b1b1f, #e7e7ea); font-size: clamp(36px, 7vw, 48px); font-weight: 700; line-height: 1.08; letter-spacing: -0.025em;")
}

pub fn hero_description() -> Mixin {
    css("max-width: 720px; margin: 0 0 24px; color: light-dark(#6b7280, #9aa0aa); font-size: 18px; line-height: 1.6;")
}

pub fn hero_welcome() -> Mixin {
    css("display: inline-flex; align-items: center; max-width: 100%; gap: 8px; padding: 8px 12px; border-radius: 8px; color: light-dark(#1b1b1f, #e7e7ea); background-color: light-dark(#f7f7f8, #16161a); font-size: 14px; font-weight: 500;")
}

pub fn quick_switch_container() -> Mixin {
    css("display: flex; align-items: center; gap: 4px; margin: 0 0 52px; flex-wrap: wrap;")
}

pub fn quick_switch_label() -> Mixin {
    css("margin-inline-end: 4px; color: light-dark(#6b7280, #9aa0aa); font-size: 13px; font-weight: 500;")
}

pub fn quick_switch_pill() -> Mixin {
    css("display: inline-flex; align-items: center; min-height: 32px; padding: 4px 10px; border-radius: 8px; color: light-dark(#0578be, #2dacf9); font-size: 13px; font-weight: 500; text-decoration: none; transition: background-color 150ms ease-in-out, color 150ms ease-in-out; &:hover { color: light-dark(#022f4b, #63c2fb); background-color: light-dark(rgb(0 0 0 / 5%), #2a2a30); } &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; } &[aria-current=\"page\"] { color: light-dark(#1b1b1f, #e7e7ea); background-color: light-dark(rgb(0 0 0 / 10%), #2a2a30); font-weight: 700; } @media (prefers-reduced-motion: reduce) { transition: none; }")
}

pub fn grid() -> Mixin {
    css("display: grid; grid-template-columns: repeat(auto-fit, minmax(min(300px, 100%), 1fr)); align-items: start; gap: 4px; margin-bottom: 52px;")
}

pub fn card() -> Mixin {
    css("min-width: 0; padding: 24px; border-radius: 12px; background-color: light-dark(#f7f7f8, #16161a);")
}

pub fn card_header() -> Mixin {
    css("display: flex; align-items: center; gap: 12px; margin-bottom: 16px;")
}

pub fn card_icon() -> Mixin {
    css("font-size: 20px; line-height: 1;")
}

pub fn card_title() -> Mixin {
    css("margin: 0; color: light-dark(#1b1b1f, #e7e7ea); font-size: 16px; font-weight: 600; line-height: 1.2; letter-spacing: -0.02em;")
}

pub fn card_description() -> Mixin {
    css("margin: 0 0 20px; color: light-dark(#6b7280, #9aa0aa); font-size: 14px; line-height: 1.5;")
}

pub fn step_list() -> Mixin {
    css("display: flex; flex-direction: column; gap: 4px; margin: 0 0 20px; padding: 0; list-style: none;")
}

pub fn step_item() -> Mixin {
    css("display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; padding: 8px 12px; border-radius: 8px; color: light-dark(#6b7280, #9aa0aa); background-color: light-dark(#ffffff, #0d0d10); font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace; font-size: 12px; &[data-active=\"true\"] { color: light-dark(#166534, #86efac); background-color: light-dark(rgb(34 197 94 / 14%), rgb(34 197 94 / 18%)); font-weight: 600; }")
}

pub fn active_badge() -> Mixin {
    css("flex-shrink: 0; padding: 2px 7px; border-radius: 9999px; color: light-dark(#166534, #86efac); background-color: light-dark(rgb(34 197 94 / 14%), rgb(34 197 94 / 18%)); font-family: 'Inter', ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; font-size: 10px; font-weight: 700; line-height: 1.5; letter-spacing: 0.06em; text-transform: uppercase;")
}

pub fn demo_rows() -> Mixin {
    css("display: flex; flex-direction: column; gap: 4px;")
}

pub fn demo_row() -> Mixin {
    css("display: flex; justify-content: space-between; align-items: center; min-height: 40px; padding: 8px 12px; border-radius: 8px; gap: 8px 16px; color: light-dark(#1b1b1f, #e7e7ea); background-color: light-dark(#ffffff, #0d0d10); font-size: 14px; flex-wrap: wrap;")
}

pub fn demo_label() -> Mixin {
    css("color: light-dark(#6b7280, #9aa0aa); font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace; font-size: 12px; font-weight: 400; overflow-wrap: anywhere;")
}

pub fn demo_value() -> Mixin {
    css("color: light-dark(#1b1b1f, #e7e7ea); font-size: 13px; font-weight: 600; overflow-wrap: anywhere; text-align: end;")
}

pub fn sr_only() -> Mixin {
    css("position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border-width: 0;")
}

pub fn footer() -> Mixin {
    css("padding: 28px 0 0; border-block-start: 1px solid light-dark(#e5e7eb, #2a2a30); color: light-dark(#6b7280, #9aa0aa); font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace; font-size: 12px;")
}

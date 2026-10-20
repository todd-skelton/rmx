//! `packages/ui/src/demos/menu.demo.tsx`: headless menu behavior with minimal local styles.
//! Below it, the View menu (checkbox item, disabled item, Zoom submenu) and the File
//! context menu are the fixtures from `menu.test.tsx`, so the browser checks can cover
//! submenus and context menus.

use rmx::{css, rsx, Component, Handle, Mixin, Node};
use rmx_ui::menu::{self, ItemOptions, SubmenuTriggerOptions};

pub struct MenuDemo {
    selected: String,
    word_wrap: bool,
}

impl Component for MenuDemo {
    type Props = ();
    rmx::client_entry!("MenuDemo");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        MenuDemo { selected: "none".into(), word_wrap: false }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let on_select = menu::on_menu_select(h.on_event(|this, h, event| {
            if let Some(item) = menu::menu_select(event) {
                if item.name == "wordWrap" {
                    this.word_wrap = item.checked == Some(true);
                }
                this.selected = match &item.value {
                    Some(value) => format!("{}:{value}", item.name),
                    None => item.name,
                };
                h.update();
            }
        }));
        rsx! {
            <div mix={[demo_css(), on_select]}>
                <menu::Context label="Project actions">
                    <button id="actions" mix={[trigger_css(), menu::trigger()]} type="button">"Actions"</button>
                    <div mix={[surface_css(), menu::popover()]}>
                        <div mix={[list_css(), menu::list()]}>
                            <div mix={[item_css(), menu::item(ItemOptions::new("rename"))]}>"Rename"</div>
                            <div mix={[item_css(), menu::item(ItemOptions::new("duplicate"))]}>"Duplicate"</div>
                            <div mix={[item_css(), menu::item(ItemOptions::new("archive").disabled(true))]}>"Archive"</div>
                        </div>
                    </div>
                </menu::Context>

                <menu::Context>
                    <button id="view" mix={[trigger_css(), menu::trigger()]} type="button">"View"</button>
                    <div mix={[surface_css(), menu::popover()]}>
                        <div mix={[list_css(), menu::list()]}>
                            <div mix={[item_css(), menu::item(ItemOptions::new("wordWrap").checkbox(self.word_wrap))]}>"Word wrap"</div>
                            <div mix={[item_css(), menu::item(ItemOptions::new("minimap").disabled(true))]}>"Minimap"</div>
                            <div role="separator" mix={separator_css()}></div>
                            <menu::Context label="Zoom">
                                <div id="zoom" mix={[item_css(), menu::submenu_trigger(SubmenuTriggerOptions::default())]}>"Zoom"</div>
                                <div mix={[surface_css(), menu::popover()]}>
                                    <div mix={[list_css(), menu::list()]}>
                                        <div mix={[item_css(), menu::item(ItemOptions::new("zoomIn").value("zoom-in"))]}>"Zoom in"</div>
                                        <div mix={[item_css(), menu::item(ItemOptions::new("zoomOut").value("zoom-out"))]}>"Zoom out"</div>
                                    </div>
                                </div>
                            </menu::Context>
                        </div>
                    </div>
                </menu::Context>

                <menu::Context label="File actions">
                    <div id="context-target" tabindex="0" mix={[context_target_css(), menu::context_trigger()]}>"File.txt"</div>
                    <div mix={[surface_css(), menu::popover()]}>
                        <div mix={[list_css(), menu::list()]}>
                            <div mix={[item_css(), menu::item(ItemOptions::new("open"))]}>"Open"</div>
                            <div mix={[item_css(), menu::item(ItemOptions::new("delete"))]}>"Delete"</div>
                        </div>
                    </div>
                </menu::Context>

                <p mix={value_css()}>{format!("selected={}", self.selected)}</p>
            </div>
        }
    }
}

fn demo_css() -> Mixin {
    css("display: grid; justify-items: start; gap: 8px; width: min(100%, 16rem);")
}

fn trigger_css() -> Mixin {
    css("appearance: none; height: 30px; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; background: light-dark(#ffffff, #1a1a1a); color: light-dark(#151515, #ececec); font: 600 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 0 10px; \
         &:focus-visible { outline: 2px solid light-dark(#3573f6, #6eaaff); outline-offset: 2px; }")
}

fn surface_css() -> Mixin {
    css("box-sizing: border-box; min-width: 10rem; margin: 0; border: 1px solid light-dark(#d1d1d1, #444444); border-radius: 6px; background: light-dark(#ffffff, #1a1a1a); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12); padding: 4px; &:popover-open { display: block; }")
}

fn list_css() -> Mixin {
    css("display: grid; gap: 2px; outline: 0;")
}

fn item_css() -> Mixin {
    css("border-radius: 4px; color: light-dark(#151515, #ececec); font: 500 13px/18px \"Inter Variable\", Inter, ui-sans-serif, system-ui, sans-serif; letter-spacing: 0; padding: 6px 8px; \
         &[data-highlighted=\"true\"] { background: light-dark(#eeeeee, #2c2c2c); } \
         &[data-menu-flash=\"true\"] { background: light-dark(#101010, #ececec); color: light-dark(#ffffff, #151515); } \
         &[aria-disabled=\"true\"] { color: light-dark(#9a9a9a, #666666); } \
         &[aria-checked=\"true\"]::before { content: \"\\2713  \"; }")
}

fn separator_css() -> Mixin {
    css("height: 1px; margin: 4px 0; background: light-dark(#e2e2e2, #333333);")
}

fn context_target_css() -> Mixin {
    css("padding: 16px 24px; border: 1px dashed light-dark(#b5b5b5, #555555); border-radius: 6px; font: 500 13px/18px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;")
}

fn value_css() -> Mixin {
    css("margin: 0; color: light-dark(#6d6d6d, #b3b3b3); font: 500 12px/16px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;")
}

//! `packages/component/src/runtime/demos/controlled-uncontrolled-values.demo.tsx`

use rsmix::{css, on, rsx, Component, Handle, Mixin, Node};

pub struct ControlledValues {
    controlled_text: String,
    controlled_checked: bool,
    controlled_choice: String,
    uncontrolled_text_snapshot: String,
    uncontrolled_checked_snapshot: bool,
    render_count: u32,
    uncontrolled_version: u32,
}

impl ControlledValues {
    fn rerender(&mut self, h: &Handle<Self>) {
        self.render_count += 1;
        h.update();
    }
}

/// `JSON.stringify` for a string.
fn json(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Component for ControlledValues {
    type Props = ();
    rsmix::client_entry!("ControlledValues");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        ControlledValues {
            controlled_text: "hello".into(),
            controlled_checked: true,
            controlled_choice: "alpha".into(),
            uncontrolled_text_snapshot: "type to update this".into(),
            uncontrolled_checked_snapshot: true,
            render_count: 0,
            uncontrolled_version: 0,
        }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <main mix={page_style()}>
                <h1>"Controlled vs Uncontrolled Values"</h1>

                <p>"Render count: " <strong id="render-count">{self.render_count}</strong></p>

                <div mix={css("display: flex; gap: 10px; margin-bottom: 18px;")}>
                    <button id="force" mix={[on("click", h.on(|this, h| this.rerender(h)))]}>"Force Re-render"</button>
                    <button id="reset" mix={[on("click", h.on(|this, h| {
                        this.controlled_text = "hello".into();
                        this.controlled_checked = true;
                        this.controlled_choice = "alpha".into();
                        this.rerender(h);
                    }))]}>"Reset Controlled"</button>
                    <button id="remount" mix={[on("click", h.on(|this, h| {
                        this.uncontrolled_version += 1;
                        this.uncontrolled_text_snapshot = "type to update this".into();
                        this.uncontrolled_checked_snapshot = true;
                        this.rerender(h);
                    }))]}>"Remount Uncontrolled"</button>
                </div>

                <section id="controlled" mix={[section_style(), css("margin-bottom: 12px;")]}>
                    <h2>"Controlled"</h2>
                    <p>
                        "These values come from component state. The text input allows everything except digits, "
                        "and invalid input does not call update."
                    </p>

                    <label mix={field_style()}>
                        "Text:"
                        <input id="controlled-text" value={self.controlled_text.clone()} mix={[inline_control_style(), on("input", h.on_event(|this, h, event| {
                            let next = event.target_value();
                            if next.chars().any(|c| c.is_ascii_digit()) {
                                return;
                            }
                            this.controlled_text = next;
                            this.rerender(h);
                        }))]}/>
                    </label>

                    <label mix={field_style()}>
                        <input id="controlled-checked" type="checkbox" checked={self.controlled_checked} mix={[on("change", h.on_event(|this, h, event| {
                            this.controlled_checked = event.target_checked();
                            this.rerender(h);
                        }))]}/>
                        " Checked"
                    </label>

                    <label mix={field_style()}>
                        "Choice:"
                        <select id="controlled-choice" value={self.controlled_choice.clone()} mix={[inline_control_style(), on("change", h.on_event(|this, h, event| {
                            this.controlled_choice = event.target_value();
                            this.rerender(h);
                        }))]}>
                            <option value="alpha">"Alpha"</option>
                            <option value="beta">"Beta"</option>
                            <option value="gamma">"Gamma"</option>
                        </select>
                    </label>

                    <div id="controlled-snapshot">
                        "State snapshot: text=" <code>{json(&self.controlled_text)}</code> ", checked="
                        <code>{self.controlled_checked.to_string()}</code> ", choice="
                        <code>{json(&self.controlled_choice)}</code>
                    </div>
                </section>

                <section id="uncontrolled" key={format!("uncontrolled-{}", self.uncontrolled_version)} mix={section_style()}>
                    <h2>"Uncontrolled"</h2>
                    <p>
                        "These initialize from " <code>"defaultValue/defaultChecked"</code>
                        " once and then keep their own DOM state."
                    </p>

                    <label mix={field_style()}>
                        "Text:"
                        <input id="uncontrolled-text" defaultValue="type to update this" mix={[inline_control_style(), on("input", h.on_event(|this, h, event| {
                            this.uncontrolled_text_snapshot = event.target_value();
                            this.rerender(h);
                        }))]}/>
                    </label>

                    <label mix={field_style()}>
                        <input id="uncontrolled-checked" type="checkbox" defaultChecked={true} mix={[on("change", h.on_event(|this, h, event| {
                            this.uncontrolled_checked_snapshot = event.target_checked();
                            this.rerender(h);
                        }))]}/>
                        " Checked"
                    </label>

                    <div id="uncontrolled-snapshot">
                        "Last DOM snapshot: text=" <code>{json(&self.uncontrolled_text_snapshot)}</code> ", checked="
                        <code>{self.uncontrolled_checked_snapshot.to_string()}</code>
                    </div>
                </section>
            </main>
        }
    }
}

fn page_style() -> Mixin {
    css("font-family: system-ui, sans-serif; max-width: 860px; margin: 24px auto; padding: 0 16px; line-height: 1.45;")
}

fn section_style() -> Mixin {
    css("border: 1px solid #d0d7de; border-radius: 8px; padding: 12px;")
}

fn field_style() -> Mixin {
    css("display: block; margin-bottom: 8px;")
}

fn inline_control_style() -> Mixin {
    css("margin-left: 8px;")
}

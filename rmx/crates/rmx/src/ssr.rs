use crate::node::{AttrValue, Element, Node};

/// Render a node tree to an HTML string.
///
/// Client entries are wrapped in `<!--rmx:h:N-->` / `<!--/rmx:h-->` markers and their props
/// are written to a `<script type="application/json" id="rmx-data">` before `</body>`, the
/// same scheme Remix 3 uses.
pub fn render_to_string(node: &Node) -> String {
    let mut ssr = Ssr { out: String::with_capacity(16 * 1024), entries: Vec::new(), in_entry: false };
    ssr.write(node);
    if !ssr.entries.is_empty() {
        let mut script = String::from(r#"<script type="application/json" id="rmx-data">["#);
        for (i, (name, props)) in ssr.entries.iter().enumerate() {
            if i > 0 {
                script.push(',');
            }
            script.push_str(r#"{"n":""#);
            script.push_str(name);
            script.push_str(r#"","p":"#);
            // JSON is safe inside <script> once "<" cannot start a closing tag.
            script.push_str(&props.replace('<', "\\u003c"));
            script.push('}');
        }
        script.push_str("]</script>");
        match ssr.out.rfind("</body>") {
            Some(pos) => ssr.out.insert_str(pos, &script),
            None => ssr.out.push_str(&script),
        }
    }
    ssr.out
}

pub(crate) struct Ssr {
    out: String,
    entries: Vec<(&'static str, String)>,
    in_entry: bool,
}

impl Ssr {
    pub(crate) fn write(&mut self, node: &Node) {
        match node {
            Node::Element(el) => self.element(el),
            Node::Text(text) => escape_into(&mut self.out, text, false),
            Node::Fragment(nodes) => nodes.iter().for_each(|n| self.write(n)),
            Node::Component(c) => c.inner.ssr(self),
            Node::Shared(n) => self.write(n),
            Node::Raw(html) => self.out.push_str(html),
            Node::Empty => {}
        }
    }

    /// Entries nested inside another entry hydrate as ordinary children of the outer one.
    pub(crate) fn client_entry(&mut self, name: &'static str, props: impl FnOnce() -> String, node: &Node) {
        if self.in_entry {
            return self.write(node);
        }
        let id = self.entries.len();
        self.out.push_str("<!--rmx:h:");
        self.out.push_str(&id.to_string());
        self.out.push_str("-->");
        self.in_entry = true;
        self.write(node);
        self.in_entry = false;
        self.out.push_str("<!--/rmx:h-->");
        self.entries.push((name, props()));
    }

    fn element(&mut self, el: &Element) {
        let out = &mut self.out;
        out.push('<');
        out.push_str(el.tag);
        for (name, value) in &el.attrs {
            match value {
                AttrValue::Str(v) => {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    escape_into(out, v, true);
                    out.push('"');
                }
                AttrValue::Bool(true) => {
                    out.push(' ');
                    out.push_str(name);
                }
                AttrValue::Bool(false) | AttrValue::Absent => {}
            }
        }
        out.push('>');
        if is_void(el.tag) {
            return;
        }
        for child in &el.children {
            self.write(child);
        }
        self.out.push_str("</");
        self.out.push_str(el.tag);
        self.out.push('>');
    }
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr"
    )
}

fn escape_into(out: &mut String, s: &str, attr: bool) {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' if attr => "&quot;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_and_attributes() {
        let node = Node::element(
            "a",
            None,
            vec![("title", AttrValue::Str("say \"hi\" & <go>".into())), ("hidden", AttrValue::Bool(true))],
            vec![],
            vec![Node::Text("1 < 2 & 3".into())],
        );
        assert_eq!(
            render_to_string(&node),
            r#"<a title="say &quot;hi&quot; &amp; &lt;go&gt;" hidden>1 &lt; 2 &amp; 3</a>"#
        );
    }
}

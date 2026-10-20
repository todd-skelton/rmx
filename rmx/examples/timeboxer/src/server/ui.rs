//! `ui/document.tsx` and `ui/layout.tsx`.

use rmx::{css, rsx, Children, Component, Handle, Node};

use crate::routes::ROUTES;
use crate::theme::{self, border, font_size, space, surface, text};

/// The client entry script and its module preloads (Remix's `scriptEntry`).
#[derive(Clone, Debug, Default)]
pub struct PageScripts {
    pub href: Option<String>,
    pub preloads: Vec<String>,
}

#[derive(Clone, Default)]
pub struct ScriptsProviderProps {
    pub scripts: PageScripts,
    pub children: Children,
}

/// Makes the request's [`PageScripts`] available to [`Document`].
pub struct ScriptsProvider;

impl Component for ScriptsProvider {
    type Props = ScriptsProviderProps;

    fn setup(props: &ScriptsProviderProps, h: &Handle<Self>) -> Self {
        h.set_context(props.scripts.clone());
        ScriptsProvider
    }

    fn render(&self, props: &ScriptsProviderProps, _: &Handle<Self>) -> Node {
        rsx! { <>{&props.children}</> }
    }
}

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: Option<String>,
    pub children: Children,
}

pub struct Document {
    scripts: PageScripts,
}

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, h: &Handle<Self>) -> Self {
        Document { scripts: h.context::<PageScripts>().map(|s| (*s).clone()).unwrap_or_default() }
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        let title = props.title.clone().unwrap_or_else(|| "Timeboxer".into());
        let preloads: Vec<Node> = self.scripts.preloads.iter().map(|href| rsx! { <link key={href.clone()} rel="modulepreload" href={href.clone()}/> }).collect();
        let script = self.scripts.href.clone().map(|src| rsx! { <script type="module" src={src}></script> });
        rsx! {
            <html lang="en">
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <meta name="color-scheme" content="light"/>
                    <title>{title}</title>
                    {preloads}
                    {script}
                    <style>{theme::KEYFRAMES}</style>
                </head>
                <body>{&props.children}</body>
            </html>
        }
    }
}

#[derive(Clone, Default)]
pub struct LayoutProps {
    pub title: Option<String>,
    pub children: Children,
}

pub struct Layout;

impl Component for Layout {
    type Props = LayoutProps;

    fn setup(_: &LayoutProps, _: &Handle<Self>) -> Self {
        Layout
    }

    fn render(&self, props: &LayoutProps, _: &Handle<Self>) -> Node {
        let home = ROUTES.home.href(());
        let header = css(&format!("border-bottom: 1px solid {}; background-color: {};", border::SUBTLE, surface::LVL0));
        let nav = css(&format!(
            "align-items: center; display: flex; justify-content: space-between; margin: 0 auto; max-width: 960px; min-height: 64px; padding: 0 {};",
            space::XL
        ));
        let brand = css(&format!("color: {}; font-size: {}; font-weight: 600; text-decoration: none;", text::PRIMARY, font_size::MD));
        let links = css(&format!("display: flex; gap: {};", space::MD));
        let link = css(&format!(
            "color: {}; font-size: {}; text-decoration: none; &:hover {{ color: {}; }}",
            text::SECONDARY,
            font_size::SM,
            text::PRIMARY
        ));
        rsx! {
            <Document title={props.title.clone()}>
                <header mix={header}>
                    <nav mix={nav}>
                        <a href={home.clone()} mix={brand}>"Timeboxer"</a>
                        <div mix={links}>
                            <a href={home} mix={link.clone()}>"Home"</a>
                            <a href={ROUTES.auth.index.href(())} mix={link}>"Account"</a>
                        </div>
                    </nav>
                </header>
                <main>{&props.children}</main>
            </Document>
        }
    }
}

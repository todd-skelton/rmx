//! `ui/document.ts`, `ui/home-page.ts`, `ui/icons.ts` and `ui/styles.ts`.

use rmx_html_template::{html, html_raw, SafeHtml};

/// What controllers render (`HtmlDocument` in `middleware/render.ts`).
pub struct HtmlDocument {
    pub title: String,
    pub content: SafeHtml,
}

/// The stylesheet. Remix interpolates it as a plain string, which escapes the quotes in its
/// font names; it is trusted, so it goes in as SafeHtml here.
pub fn styles() -> SafeHtml {
    SafeHtml::from_trusted(include_str!("styles.css"))
}

pub fn create_document(title: &str, content: &SafeHtml) -> SafeHtml {
    let styles = styles();
    html!(
        r#"<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{title}</title>
    <style>
{styles}
    </style>
  </head>
  <body>
    {content}
  </body>
</html>"#
    )
}

pub fn home_page() -> SafeHtml {
    html!(
        r#"<h1><span class="brand">UNPKG</span></h1>
<div class="home-content">
  <p>Browse the contents of any npm package by entering its name in the URL.</p>
  <p>
    For example, visit <code>/lodash</code> to browse the latest version of lodash, or
    <code>/react@18</code> to browse React version 18.
  </p>
  <p>
    You can also browse specific files by adding the file path, like
    <code>/lodash/package.json</code>.
  </p>

  <div class="examples">
    <h2>Try these packages:</h2>
    <ul>
      <li><a href="/@remix-run/cookie">@remix-run/cookie</a> - scoped package</li>
      <li><a href="/react">react</a> - UI library</li>
      <li><a href="/express">express</a> - web framework</li>
      <li><a href="/typescript@5">typescript@5</a> - specific major version</li>
    </ul>
  </div>
</div>"#
    )
}

pub fn file_icon() -> SafeHtml {
    html_raw!(r##"<svg class="icon-file" viewBox="0 0 16 16" fill="none" stroke="#57606a" stroke-width="1.5"><path d="M3 1.5h6.5L13 5v9.5H3z" /><path d="M9.5 1.5V5H13" /></svg>"##)
}

pub fn directory_icon() -> SafeHtml {
    html_raw!(r##"<svg class="icon-dir" viewBox="0 0 16 16" fill="none" stroke="#57606a" stroke-width="1.5"><path d="M1.5 3.5h4l1.5 2h7.5v8h-13z" /></svg>"##)
}

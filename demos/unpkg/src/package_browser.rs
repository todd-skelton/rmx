//! `actions/package-browser.ts`: directory listings, file views and error pages.

use http::StatusCode;
use rmx::fetch::Response;
use rmx_html_template::{html, SafeHtml};
use rmx::render::{Renderer, ResponseInit};

use crate::format_bytes::format_bytes;
use crate::mime_type::{detect_file_mime_type, image_mime_type, is_text_content};
use crate::npm::{FileKind, PackageFile};
use crate::ui::{directory_icon, file_icon, HtmlDocument};

pub fn render_directory_listing(render: &Renderer<HtmlDocument>, package_name: &str, version: &str, dir_path: &str, files: &[PackageFile]) -> Response {
    let title = if dir_path.is_empty() { format!("{package_name}@{version}") } else { format!("{package_name}@{version}/{dir_path}") };
    let breadcrumb = breadcrumb(package_name, version, dir_path);

    let parent_row = (!dir_path.is_empty()).then(|| {
        let href = format!("/{package_name}@{version}{}", parent_path(dir_path));
        let icon = directory_icon();
        html!(
            r#"<tr>
  <td class="icon"><a href="{href}">{icon}</a></td>
  <td class="name"><a href="{href}">..</a></td>
  <td class="size"><a href="{href}">-</a></td>
  <td class="type"><a href="{href}">directory</a></td>
</tr>"#
        )
    });

    let file_rows: Vec<SafeHtml> = files
        .iter()
        .map(|file| {
            let href = format!("/{package_name}@{version}/{}", file.path);
            let dir = file.kind == FileKind::Directory;
            let icon = if dir { directory_icon() } else { file_icon() };
            let display_name = if dir { format!("{}/", file.name) } else { file.name.clone() };
            let mime_type = if dir { "directory" } else { detect_file_mime_type(&file.name) };
            let size = format_bytes(file.size);
            html!(
                r#"<tr>
  <td class="icon"><a href="{href}">{icon}</a></td>
  <td class="name"><a href="{href}">{display_name}</a></td>
  <td class="size"><a href="{href}">{size}</a></td>
  <td class="type"><a href="{href}">{mime_type}</a></td>
</tr>"#
            )
        })
        .collect();

    let content = html!(
        r#"<h1>{package_name}</h1>
{breadcrumb}
<p class="package-info">Version: {version}</p>
<div class="file-browser">
  <table>
    <thead>
      <tr>
        <th class="icon"></th>
        <th>Name</th>
        <th class="size">Size</th>
        <th class="type">Type</th>
      </tr>
    </thead>
    <tbody>
      {parent_row} {file_rows}
    </tbody>
  </table>
</div>"#
    );
    render.render(HtmlDocument { title, content })
}

pub fn render_file_content(render: &Renderer<HtmlDocument>, package_name: &str, version: &str, file_path: &str, file: &PackageFile, data: &[u8]) -> Response {
    let title = format!("{package_name}@{version}/{file_path}");
    let breadcrumb = breadcrumb(package_name, version, file_path);
    let size = format_bytes(file.size);
    let body = if let Some(mime_type) = image_mime_type(&file.name) {
        let base64 = rmx::crypto::base64_encode(data);
        let name = &file.name;
        html!(r#"<div class="file-content"><img src="data:{mime_type};base64,{base64}" alt="{name}" /></div>"#)
    } else if is_text_content(&file.name, data) {
        let text = String::from_utf8_lossy(data);
        html!(r#"<div class="file-content"><pre>{text}</pre></div>"#)
    } else {
        html!(
            r#"<div class="info">
  <p>This file cannot be displayed. It may be a binary file.</p>
  <p>File size: {size}</p>
</div>"#
        )
    };
    let name = &file.name;
    let content = html!(
        r#"<h1>{name}</h1>
{breadcrumb}
<p class="package-info">Size: {size}</p>
{body}"#
    );
    render.render(HtmlDocument { title, content })
}

pub fn render_error(render: &Renderer<HtmlDocument>, title: &str, message: &str) -> Response {
    let content = html!(
        r#"<h1>{title}</h1>
<div class="error">
  <p>{message}</p>
</div>
<p style="margin-top: 1rem;">
  <a href="/">Back to home</a>
</p>"#
    );
    render.render_with(HtmlDocument { title: title.to_string(), content }, ResponseInit::status(StatusCode::NOT_FOUND))
}

fn breadcrumb(package_name: &str, version: &str, dir_path: &str) -> SafeHtml {
    let mut parts = vec![("Home".to_string(), "/".to_string()), (format!("{package_name}@{version}"), format!("/{package_name}@{version}"))];
    if !dir_path.is_empty() {
        let mut current = String::new();
        for part in dir_path.split('/') {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(part);
            parts.push((part.to_string(), format!("/{package_name}@{version}/{current}")));
        }
    }
    let last = parts.len() - 1;
    let links: Vec<SafeHtml> = parts
        .iter()
        .enumerate()
        .map(|(i, (name, href))| {
            let link = if i == last { html!("<span>{name}</span>") } else { html!(r#"<a href="{href}">{name}</a>"#) };
            if i == 0 { link } else { html!(" / {link}") }
        })
        .collect();
    html!(r#"<nav class="breadcrumb">{links}</nav>"#)
}

fn parent_path(dir_path: &str) -> String {
    match dir_path.rsplit_once('/') {
        Some((parent, _)) => format!("/{parent}"),
        None => String::new(),
    }
}

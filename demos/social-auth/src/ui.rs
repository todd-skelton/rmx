//! Shared UI (ui/*.tsx, ui/styles.ts, ui/design-system.ts).

use rmx::{css, rsx, Children, Component, Handle, Mixin, Node};

// Design tokens and styles ----------------------------------------------------------------

const SANS: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif";
const BORDER_SUBTLE: &str = "1px solid #d1d5db";

pub fn page_reset() -> Mixin {
    css("& * { margin: 0; padding: 0; box-sizing: border-box; } & button, & input, & textarea, & select { font: inherit; } & h1, & h2, & h3, & h4, & h5, & h6 { font-weight: 600; line-height: 1.2; } & a { color: inherit; }")
}

pub fn page() -> Mixin {
    css(&format!(
        "margin: 0; width: 100vw; min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 1.5rem; background: linear-gradient(to bottom right, #eff6ff, #e0e7ff); font-family: {SANS};"
    ))
}

pub fn card() -> Mixin {
    css("width: 100%; max-width: 28rem; padding: 2rem; background-color: #ffffff; border-radius: 0.5rem; box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);")
}

pub fn form() -> Mixin {
    css("display: flex; flex-direction: column; gap: 1rem;")
}

pub fn field_icon() -> Mixin {
    css("position: absolute; left: 0.75rem; top: 50%; transform: translateY(-50%); width: 1.25rem; height: 1.25rem; color: #9ca3af;")
}

pub fn helper_link() -> Mixin {
    css("background: none; border: none; color: #2563eb; cursor: pointer; padding: 0; font-size: 0.875rem; text-decoration: none; &:hover { text-decoration: underline; }")
}

pub fn submit_button() -> Mixin {
    css("width: 100%; display: block; padding: 0.625rem; background-color: #2563eb; color: #ffffff; border: none; border-radius: 0.5rem; cursor: pointer; transition: background-color 0.2s; font-size: 1rem; font-weight: 500; text-decoration: none; text-align: center; &:link, &:visited { color: #ffffff; text-decoration: none; } &:hover { background-color: #1d4ed8; }")
}

pub fn secondary_button() -> Mixin {
    css("width: 100%; display: block; padding: 0.625rem; background-color: #ffffff; color: #374151; border: 1px solid #d1d5db; border-radius: 0.5rem; cursor: pointer; font-size: 1rem; font-weight: 500; text-decoration: none; text-align: center; transition: background-color 0.2s; &:link, &:visited { color: #374151; text-decoration: none; } &:hover { background-color: #f9fafb; }")
}

pub fn form_options() -> Mixin {
    css("display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; font-size: 0.875rem;")
}

pub fn remember_me() -> Mixin {
    css("display: flex; align-items: center; cursor: pointer; color: #4b5563;")
}

pub fn remember_checkbox() -> Mixin {
    css("margin-right: 0.5rem; cursor: pointer;")
}

pub fn divider() -> Mixin {
    css("margin: 1.5rem 0; display: flex; align-items: center;")
}

pub fn divider_line() -> Mixin {
    css(&format!("flex: 1; border-top: {BORDER_SUBTLE};"))
}

pub fn divider_text() -> Mixin {
    css("padding: 0 1rem; font-size: 0.875rem; color: #6b7280;")
}

pub fn social_buttons() -> Mixin {
    css("display: flex; flex-direction: column; gap: 0.75rem;")
}

pub fn social_button() -> Mixin {
    css("width: 100%; padding: 0.625rem 1rem; border: 1px solid #d1d5db; border-radius: 0.5rem; background-color: #ffffff; cursor: pointer; transition: background-color 0.2s; display: flex; align-items: center; justify-content: center; gap: 0.75rem; font-size: 1rem; text-decoration: none; &:hover { background-color: #f9fafb; }")
}

pub fn social_button_disabled() -> Mixin {
    css("opacity: 0.55; cursor: not-allowed; &:hover { background-color: #ffffff; }")
}

pub fn social_icon() -> Mixin {
    css("width: 1.25rem; height: 1.25rem;")
}

pub fn social_button_label() -> Mixin {
    css("color: #374151; font-weight: 500;")
}

pub fn footer_text() -> Mixin {
    css("margin-top: 1.5rem; text-align: center; font-size: 0.875rem; color: #4b5563;")
}

pub fn info_panel() -> Mixin {
    css("padding: 1rem; border-radius: 0.5rem; background-color: #f9fafb; color: #4b5563; font-size: 0.875rem;")
}

pub fn button_row() -> Mixin {
    css("display: flex; flex-direction: column; gap: 0.75rem; margin-top: 1rem;")
}

pub fn profile_header() -> Mixin {
    css("display: flex; align-items: center; gap: 1rem; margin-bottom: 1.5rem;")
}

pub fn profile_avatar() -> Mixin {
    css("width: 72px; height: 72px; border-radius: 999px; object-fit: cover; border: 1px solid #d1d5db; background-color: #f9fafb; display: block;")
}

pub fn profile_fallback_avatar() -> Mixin {
    css("width: 72px; height: 72px; border-radius: 999px; background-color: #2563eb; color: #ffffff; display: flex; align-items: center; justify-content: center; font-size: 1.5rem; font-weight: 600;")
}

pub fn profile_name() -> Mixin {
    css("color: #111827; font-size: 1.375rem; font-weight: 600;")
}

pub fn profile_meta() -> Mixin {
    css("color: #4b5563; font-size: 0.875rem; margin-top: 0.5rem;")
}

pub fn data_dump() -> Mixin {
    css("margin-top: 1rem; padding: 1rem; border-radius: 0.5rem; background-color: #0f172a; color: #e2e8f0; overflow-x: auto; font-size: 0.85rem; line-height: 1.5;")
}

// Document (ui/document.tsx) --------------------------------------------------------------

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: String,
    pub children: Children,
}

pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        rsx! {
            <html lang="en">
                <head>
                    <meta charset="UTF-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
                    <link rel="icon" href="/favicon.svg" type="image/svg+xml"/>
                    <title>{props.title.clone()}</title>
                </head>
                <body mix={[page_reset(), page()]}>{&props.children}</body>
            </html>
        }
    }
}

// AuthCard (ui/auth-card.tsx) -------------------------------------------------------------

#[derive(Clone, Default)]
pub struct AuthCardProps {
    pub title: String,
    pub subtitle: Option<String>,
    pub footer: Option<Node>,
    pub children: Children,
}

pub struct AuthCard;

impl Component for AuthCard {
    type Props = AuthCardProps;

    fn setup(_: &AuthCardProps, _: &Handle<Self>) -> Self {
        AuthCard
    }

    fn render(&self, p: &AuthCardProps, _: &Handle<Self>) -> Node {
        rsx! {
            <div mix={card()}>
                <div mix={css("margin-bottom: 2rem; text-align: center;")}>
                    <h1 mix={css("margin-bottom: 0.5rem; font-size: 1.5rem; font-weight: 600; color: #111827;")}>{p.title.clone()}</h1>
                    {p.subtitle.clone().map(|s| rsx! { <p mix={css("color: #4b5563; font-size: 1rem;")}>{s}</p> })}
                </div>
                {&p.children}
                {p.footer.clone()}
            </div>
        }
    }
}

// Notice (ui/notice.tsx) ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Error,
    Success,
}

#[derive(Clone, Default)]
pub struct NoticeProps {
    pub tone: Tone,
    pub children: Children,
}

pub struct Notice;

impl Component for Notice {
    type Props = NoticeProps;

    fn setup(_: &NoticeProps, _: &Handle<Self>) -> Self {
        Notice
    }

    fn render(&self, p: &NoticeProps, _: &Handle<Self>) -> Node {
        let tone = match p.tone {
            Tone::Error => css("background-color: #fee2e2; color: #991b1b; border: 1px solid #fecaca;"),
            Tone::Success => css("background-color: #dcfce7; color: #166534; border: 1px solid #bbf7d0;"),
        };
        let class = match p.tone {
            Tone::Error => "notice notice-error",
            Tone::Success => "notice notice-success",
        };
        rsx! {
            <div class={class} role="status" mix={[css("padding: 0.75rem; border-radius: 0.5rem; margin-bottom: 1rem; font-size: 0.875rem;"), tone]}>{&p.children}</div>
        }
    }
}

/// `{error ? <Notice tone="error">{error}</Notice> : null}`
pub fn notice(tone: Tone, message: Option<&str>) -> Option<Node> {
    let message = message?.to_string();
    Some(rsx! { <Notice tone={tone}>{message}</Notice> })
}

// TextField (ui/form-field.tsx) -----------------------------------------------------------

#[derive(Clone, Default)]
pub struct TextFieldProps {
    pub id: &'static str,
    pub name: &'static str,
    pub input_type: &'static str,
    pub label: &'static str,
    pub placeholder: Option<&'static str>,
    pub auto_complete: Option<&'static str>,
    pub default_value: Option<String>,
    pub required: bool,
    pub icon: Option<Node>,
}

pub struct TextField;

impl Component for TextField {
    type Props = TextFieldProps;

    fn setup(_: &TextFieldProps, _: &Handle<Self>) -> Self {
        TextField
    }

    fn render(&self, p: &TextFieldProps, _: &Handle<Self>) -> Node {
        rsx! {
            <div mix={css("display: flex; flex-direction: column;")}>
                <label for={p.id} mix={css("display: block; margin-bottom: 0.5rem; font-size: 0.875rem; color: #374151; font-weight: 500;")}>{p.label}</label>
                <div mix={css("position: relative;")}>
                    {p.icon.clone()}
                    <input id={p.id} name={p.name} type={p.input_type} placeholder={p.placeholder} autocomplete={p.auto_complete}
                        defaultValue={p.default_value.clone()} required={p.required}
                        mix={css("width: 100%; padding: 0.5rem 1rem 0.5rem 2.5rem; border: 1px solid #d1d5db; border-radius: 0.5rem; outline: none; transition: all 0.2s; font-size: 1rem; background-color: #ffffff; &:focus { border-color: transparent; box-shadow: 0 0 0 2px #2563eb; }")}/>
                </div>
            </div>
        }
    }
}

/// The "Already have an account? Sign in" line under auth cards (actions/auth/footer.tsx).
pub fn footer(prefix: &str, href: &str, label: &str) -> Node {
    rsx! { <p mix={footer_text()}>{prefix.to_string()}" "<a href={href.to_string()} mix={helper_link()}>{label.to_string()}</a></p> }
}

// Icons (ui/icons.tsx) --------------------------------------------------------------------

fn outline_icon(d: &'static str, mix: Mixin) -> Node {
    rsx! {
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" mix={mix}>
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d={d}/>
        </svg>
    }
}

pub fn user_icon(mix: Mixin) -> Node {
    outline_icon("M16 14a4 4 0 10-8 0m8 0a4 4 0 018 0m-8 0a8 8 0 00-8 0m8 0v1m0-1a8 8 0 018 0M12 12a4 4 0 100-8 4 4 0 000 8z", mix)
}

pub fn email_icon(mix: Mixin) -> Node {
    outline_icon("M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z", mix)
}

pub fn password_icon(mix: Mixin) -> Node {
    outline_icon("M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z", mix)
}

pub fn google_icon(mix: Mixin) -> Node {
    rsx! {
        <svg viewBox="0 0 24 24" mix={mix}>
            <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
            <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
            <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
            <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
        </svg>
    }
}

pub fn github_icon(mix: Mixin) -> Node {
    rsx! {
        <svg viewBox="0 0 24 24" fill="currentColor" mix={mix}>
            <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
        </svg>
    }
}

pub fn x_icon(mix: Mixin) -> Node {
    rsx! {
        <svg viewBox="0 0 24 24" fill="currentColor" mix={mix}>
            <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
        </svg>
    }
}

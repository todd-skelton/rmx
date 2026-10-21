//! `app/actions/home-page.tsx` and `app/actions/document.tsx`: the route-owned starter UI.

use rmx::assets::ScriptEntry;
use rmx::{css, rsx, Mixin, Node};

use crate::prompt_button::PromptButtonProps;
use crate::prompt_button::PromptButton;

const FONT_STACK: &str = "'JetBrains Mono', ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace";

/// The prompts the "Coding with AI?" card offers to copy.
pub const PROMPTS: [&str; 4] = [
    "I want to build a simple headless Shopify store, what does Remix have available to help scaffold this?",
    "Add a sqlite database with a users table and scaffold a signup flow",
    "Make a copy to clipboard component that confirms to the user it was copied then resets after a few seconds",
    "Add compression middleware",
];

/// `<Document>`: the HTML shell with the client entry script and its module preloads.
pub fn document(script: Option<&ScriptEntry>, title: Option<&str>, head: Node, children: Node) -> Node {
    let title = title.unwrap_or("rmx App").to_string();
    let preloads: Vec<Node> = script
        .map(|s| s.preloads.iter().map(|href| rsx! { <link key={href.clone()} rel="modulepreload" href={href.clone()}/> }).collect())
        .unwrap_or_default();
    let entry = script.map(|s| rsx! { <script type="module" src={s.href.clone()}></script> });
    rsx! {
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="color-scheme" content="light dark"/>
                <link rel="icon" type="image/svg+xml" href="/favicon.svg"/>
                <title>{title}</title>
                {head}
                {preloads}
                {entry}
            </head>
            <body mix={css("margin: 0;")}>{children}</body>
        </html>
    }
}

/// `<HomePage>`
pub fn home_page(script: Option<&ScriptEntry>) -> Node {
    let main = rsx! {
        <main mix={css(concat!(
            // Light-mode design tokens (default), then dark-mode overrides.
            "--surface-0: #dee2e6; --surface-3: #f0f4f7; --surface-4: #f7fbff; --text-primary: #313539; --text-tertiary: #94989c; --brand-blue: #2dacf9; ",
            "@media (prefers-color-scheme: dark) { --surface-0: #1e2226; --surface-3: #313539; --surface-4: #363a3e; --text-primary: #dee2e6; --text-tertiary: #94989c; } ",
            "& *, & *::before, & *::after { box-sizing: border-box; } ",
            "margin: 0; padding: 48px 24px; min-height: 100vh; background: var(--surface-0); color: var(--text-primary); font-size: 14px; line-height: 1.5; ",
            "-webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; display: flex; align-items: center; justify-content: center;"
        ))} style={format!("font-family: {FONT_STACK}")}>
            <div mix={css("width: 100%; max-width: 820px; display: flex; flex-direction: column; align-items: center; gap: 72px;")}>
                {masthead()}
                {columns()}
                {footer()}
            </div>
        </main>
    };
    document(script, None, home_head(), main)
}

fn home_head() -> Node {
    rsx! {
        <>
            <link rel="preconnect" href="https://fonts.googleapis.com"/>
            <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous"/>
            <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&display=swap"/>
        </>
    }
}

fn masthead() -> Node {
    rsx! {
        <section aria-label="Welcome" mix={css("display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 36px; width: 100%;")}>
            <p mix={css("margin: 0; font-weight: 700; font-size: 14px; line-height: 1.33; text-transform: uppercase; letter-spacing: 0.1em; color: var(--text-primary); text-align: center;")}>
                "Welcome to"
            </p>
            {remix_wordmark_hero()}
        </section>
    }
}

fn columns() -> Node {
    rsx! {
        <section aria-label="Getting started"
            mix={css("display: flex; gap: 16px; align-items: stretch; justify-content: center; width: auto; @media (max-width: 720px) { flex-direction: column; width: 100%; }")}>
            {get_started_card()}
            {coding_with_ai_card()}
        </section>
    }
}

fn get_started_card() -> Node {
    rsx! {
        <div mix={[card_style(), css("flex: 0 0 auto;")]}>
            <h2 mix={card_header_style()}>"Get started"</h2>
            <ul mix={css("list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; align-items: stretch; width: 100%;")}>
                <li>{card_link("https://api.remix.run", atom_icon(), "Remix API")}</li>
                <li>{card_link("https://discord.gg/xwx7mMzVkA", discord_face_icon(), "Join Discord")}</li>
            </ul>
        </div>
    }
}

fn coding_with_ai_card() -> Node {
    let prompts: Vec<Node> = PROMPTS
        .iter()
        .map(|text| Node::component::<PromptButton>(None, PromptButtonProps { text: text.to_string() }))
        .collect();
    rsx! {
        <div mix={[card_style(), css("width: 540px; justify-content: center; @media (max-width: 720px) { width: 100%; }")]}>
            <h2 mix={card_header_style()}>"Coding with AI?"</h2>
            <div mix={css("width: 100%; display: flex; flex-direction: column; align-items: stretch;")}>
                <p mix={css("margin: 0; padding: 0 16px 16px; font-size: 14px; line-height: 1.67; color: var(--text-primary);")}>
                    "Navigate to this project folder using your preferred AI-powered tool, and try copying any of these prompts into the agent chat:"
                </p>
                {prompts}
            </div>
        </div>
    }
}

fn card_link(href: &str, icon: Node, label: &str) -> Node {
    rsx! {
        <a href={href.to_string()}
            mix={css("display: flex; gap: 16px; align-items: center; padding: 16px; border-radius: 12px; color: var(--text-primary); text-decoration: none; background: transparent; transition: background-color 150ms ease, color 150ms ease; &:hover, &:focus-visible { background: var(--surface-4); color: var(--brand-blue); outline: none; }")}>
            {icon_slot(icon)}
            <span mix={css("font-size: 14px; line-height: 1.5; white-space: nowrap;")}>{label.to_string()}</span>
        </a>
    }
}

fn icon_slot(children: Node) -> Node {
    rsx! {
        <span aria-hidden="true"
            mix={css("flex: 0 0 24px; width: 24px; display: flex; align-items: center; justify-content: flex-start; & svg { width: 20px; height: 20px; display: block; }")}>
            {children}
        </span>
    }
}

fn footer() -> Node {
    rsx! {
        <footer mix={css("display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;")}>
            <div mix={css("display: flex; gap: 24px; align-items: center; justify-content: center;")}>
                {footer_wordmark()}
                <nav aria-label="Remix social links"
                    mix={css("display: flex; gap: 16px; align-items: center; justify-content: flex-end; color: var(--text-tertiary); & a { width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center; color: inherit; transition: color 150ms ease; } & a:hover, & a:focus-visible { color: var(--text-primary); outline: none; } & svg { width: 100%; height: 100%; display: block; }")}>
                    <a href="https://github.com/remix-run/remix" aria-label="GitHub">{github_icon()}</a>
                    <a href="https://x.com/remix_run" aria-label="X">{x_icon()}</a>
                    <a href="https://www.youtube.com/@Remix-Run" aria-label="YouTube">{youtube_icon()}</a>
                    <a href="https://discord.gg/xwx7mMzVkA" aria-label="Discord">{discord_icon()}</a>
                </nav>
            </div>
            <div mix={css("display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; font-size: 10px; line-height: 1.6; letter-spacing: 0.05em; color: var(--text-tertiary); text-align: center; & p { margin: 0; white-space: nowrap; }")}>
                <p>"DOCS AND EXAMPLES LICENSED UNDER MIT"</p>
                <p>"\u{a9}2026 SHOPIFY, INC."</p>
            </div>
        </footer>
    }
}

fn card_style() -> Mixin {
    css("background: var(--surface-3); border-radius: 20px; padding: 32px 16px 16px; display: flex; flex-direction: column; align-items: center; gap: 32px;")
}

fn card_header_style() -> Mixin {
    css("margin: 0; width: 100%; padding: 0 16px; display: flex; align-items: flex-start; font-size: 14px; font-weight: 700; line-height: 1.5; text-transform: uppercase; letter-spacing: 0.1em; color: var(--text-primary);")
}

// ----- SVG icons -----
// Inline so the page is fully self-contained with no external icon assets.

fn atom_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 17.5 17.5" fill="none">
          <path
            d="M8.75 8.825V8.75M16.219 16.219C14.639 17.798 10.014 15.735 5.89 11.61C1.765 7.485 -0.299 2.861 1.281 1.281C2.861 -0.299 7.485 1.765 11.61 5.89C15.735 10.014 17.798 14.639 16.219 16.219ZM1.281 16.219C-0.299 14.639 1.765 10.014 5.89 5.89C10.014 1.765 14.639 -0.299 16.219 1.281C17.799 2.861 15.735 7.485 11.61 11.61C7.486 15.735 2.861 17.798 1.281 16.219Z"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
        </svg>
    }
}

fn discord_face_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 25.5 19.4" fill="none">
          <path
            d="M15.068 0.911C16.442 1.137 17.766 1.53 19.04 2.061L19.583 2.297L19.776 2.384L19.894 2.559C22.358 6.185 23.587 10.304 23.152 15.044L23.122 15.375L22.855 15.575C21.187 16.827 19.305 17.756 17.311 18.368L16.805 18.524L16.489 18.099C16.029 17.479 15.626 16.834 15.282 16.146H15.281C13.127 16.699 10.864 16.698 8.711 16.141L8.709 16.146C8.365 16.834 7.962 17.479 7.502 18.099L7.188 18.52L6.685 18.37C4.68 17.775 2.818 16.823 1.139 15.578L0.869 15.378L0.838 15.044C0.463 10.957 1.238 6.816 4.074 2.582L4.19 2.409L4.378 2.32C5.807 1.645 7.336 1.169 8.903 0.911L9.414 0.827L9.674 1.276C9.804 1.501 9.927 1.734 10.043 1.972C11.339 1.817 12.645 1.817 13.941 1.972C14.054 1.735 14.175 1.496 14.291 1.287L14.548 0.825L15.068 0.911ZM8.494 9.064C7.954 9.064 7.364 9.601 7.364 10.44C7.364 11.269 7.946 11.796 8.494 11.796C9.055 11.796 9.625 11.275 9.641 10.432C9.637 9.588 9.06 9.064 8.494 9.064ZM15.478 9.064C14.938 9.064 14.35 9.601 14.35 10.44C14.35 11.269 14.93 11.796 15.478 11.796C16.021 11.796 16.593 11.292 16.607 10.427C16.621 9.59 16.047 9.064 15.478 9.064Z"
            stroke="currentColor"
            stroke-width="1.5"
          />
        </svg>
    }
}

fn github_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 20 19.67" fill="none">
          <path
            fill-rule="evenodd"
            clip-rule="evenodd"
            d="M10.008 0C4.474 0 0 4.507 0 10.083C0 14.54 2.867 18.312 6.843 19.648C7.341 19.748 7.523 19.431 7.523 19.164C7.523 18.93 7.506 18.129 7.506 17.294C4.722 17.895 4.142 16.092 4.142 16.092C3.695 14.924 3.032 14.623 3.032 14.623C2.121 14.006 3.099 14.006 3.099 14.006C4.109 14.072 4.64 15.041 4.64 15.041C5.534 16.576 6.976 16.142 7.556 15.875C7.639 15.224 7.904 14.773 8.186 14.523C5.965 14.289 3.629 13.421 3.629 9.548C3.629 8.447 4.026 7.545 4.656 6.844C4.557 6.594 4.209 5.559 4.756 4.173C4.756 4.173 5.601 3.906 7.506 5.208C8.322 4.987 9.163 4.875 10.008 4.874C10.853 4.874 11.715 4.991 12.51 5.208C14.416 3.906 15.261 4.173 15.261 4.173C15.808 5.559 15.46 6.594 15.36 6.844C16.007 7.545 16.388 8.447 16.388 9.548C16.388 13.421 14.051 14.273 11.814 14.523C12.179 14.84 12.494 15.441 12.494 16.393C12.494 17.745 12.477 18.83 12.477 19.164C12.477 19.431 12.66 19.748 13.157 19.648C17.133 18.312 20 14.54 20 10.083C20.016 4.507 15.526 0 10.008 0Z"
            fill="currentColor"
          />
        </svg>
    }
}

fn x_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 18.33 16.57" fill="none">
          <path
            d="M14.439 0H17.25L11.108 7.02L18.333 16.572H12.676L8.245 10.778L3.175 16.572H0.362L6.931 9.063L0 0H5.801L9.806 5.295L14.439 0ZM13.452 14.889H15.01L4.955 1.594H3.283L13.452 14.889Z"
            fill="currentColor"
          />
        </svg>
    }
}

fn youtube_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 20 14.07" fill="none">
          <path
            d="M19.801 3.035C19.801 3.035 19.605 1.656 19.004 1.051C18.242 0.254 17.391 0.25 17 0.203C14.203 0 10.004 0 10.004 0H9.996C9.996 0 5.797 0 3 0.203C2.609 0.25 1.758 0.254 0.996 1.051C0.395 1.656 0.203 3.035 0.203 3.035C0.203 3.035 0 4.656 0 6.273V7.789C0 9.406 0.199 11.027 0.199 11.027C0.199 11.027 0.395 12.406 0.992 13.012C1.754 13.809 2.754 13.781 3.199 13.867C4.801 14.02 10 14.066 10 14.066C10 14.066 14.203 14.059 17 13.859C17.391 13.813 18.242 13.809 19.004 13.012C19.605 12.406 19.801 11.027 19.801 11.027C19.801 11.027 20 9.41 20 7.789V6.273C20 4.656 19.801 3.035 19.801 3.035ZM7.934 9.629V4.008L13.336 6.828L7.934 9.629Z"
            fill="currentColor"
          />
        </svg>
    }
}

fn discord_icon() -> Node {
    rsx! {
        <svg viewBox="0 0 20 15.24" fill="none">
          <path
            d="M16.931 1.264C15.656 0.679 14.289 0.248 12.86 0.001C12.834 -0.004 12.808 0.008 12.795 0.032C12.619 0.345 12.424 0.753 12.288 1.073C10.75 0.843 9.221 0.843 7.715 1.073C7.579 0.745 7.377 0.345 7.2 0.032C7.187 0.009 7.161 -0.003 7.135 0.001C5.707 0.247 4.34 0.678 3.064 1.264C3.053 1.268 3.044 1.276 3.037 1.287C0.445 5.16 -0.266 8.939 0.083 12.67C0.084 12.689 0.094 12.706 0.109 12.717C1.819 13.973 3.476 14.736 5.103 15.242C5.129 15.25 5.156 15.24 5.173 15.219C5.558 14.693 5.901 14.139 6.195 13.557C6.212 13.523 6.195 13.482 6.16 13.469C5.616 13.262 5.098 13.011 4.6 12.725C4.56 12.702 4.557 12.646 4.593 12.619C4.698 12.54 4.803 12.458 4.903 12.376C4.921 12.361 4.947 12.358 4.968 12.367C8.241 13.862 11.785 13.862 15.019 12.367C15.04 12.357 15.066 12.36 15.085 12.375C15.185 12.458 15.29 12.54 15.395 12.619C15.431 12.646 15.429 12.702 15.39 12.725C14.891 13.016 14.374 13.262 13.829 13.468C13.793 13.481 13.778 13.523 13.795 13.557C14.095 14.139 14.438 14.692 14.816 15.218C14.831 15.24 14.86 15.25 14.886 15.242C16.52 14.736 18.177 13.973 19.888 12.717C19.903 12.706 19.912 12.689 19.914 12.671C20.331 8.357 19.215 4.61 16.957 1.287C16.951 1.276 16.942 1.268 16.931 1.264ZM6.683 10.398C5.698 10.398 4.886 9.493 4.886 8.382C4.886 7.271 5.682 6.367 6.683 6.367C7.692 6.367 8.497 7.279 8.481 8.382C8.481 9.493 7.685 10.398 6.683 10.398ZM13.329 10.398C12.343 10.398 11.532 9.493 11.532 8.382C11.532 7.271 12.328 6.367 13.329 6.367C14.338 6.367 15.142 7.279 15.126 8.382C15.126 9.493 14.338 10.398 13.329 10.398Z"
            stroke="currentColor"
            stroke-width="1.5"
          />
        </svg>
    }
}

/// The same wordmark in both color schemes: the text paths use `currentColor`.
fn footer_wordmark() -> Node {
    rsx! {
        <span role="img" aria-label="Remix"
            mix={css("display: block; height: 8px; width: calc(8px * 163 / 16); color: var(--text-primary); opacity: 0.55; & svg { display: block; width: 100%; height: 100%; }")}>
            <svg viewBox="0 0 163 16" fill="currentColor" aria-hidden="true">
              <path d="M11.5566 11.5024C11.9535 11.5025 12.2424 11.8811 12.1396 12.2661L11.1846 15.8481H0.0673828L1.22656 11.5024H11.5566ZM30.1533 0.0180664V0.019043C34.3663 0.0191833 37.2765 1.9102 36.6543 4.24268L36.2324 5.82178C35.6099 8.15428 31.6907 10.0454 27.4775 10.0454H27.0469L35.8965 15.8481H21.6875L14.5332 10.3257C14.247 10.1423 13.9147 10.0454 13.5752 10.0454H1.61523L2.74219 5.8208H23.6904C24.4776 5.82071 25.2104 5.4677 25.3271 5.03174C25.4436 4.59555 24.8992 4.2417 24.1113 4.2417H3.16406L4.29102 0.0180664H30.1533Z" />
              <path d="M113.897 15.9271L118.132 0.124207H129.313L125.052 15.9271H113.897Z" />
              <path d="M71.7284 0.124207H107.931C112.785 0.124207 116.142 2.29324 115.419 4.9787L112.475 15.9271H101.32L102.844 10.2722L103.722 7.04445L104.057 5.805C104.264 5.00452 103.257 4.33316 101.785 4.33316H98.6089C98.5831 4.53973 98.5831 4.74631 98.5056 4.9787L95.5877 15.9271H84.4069L85.9304 10.2722L86.8083 7.04445L87.144 5.805C87.3506 5.00452 86.3436 4.33316 84.8717 4.33316H81.7731L78.6487 15.9271H67.4937L71.7284 0.124207Z" />
              <path d="M145.926 2.73926L149.765 0.219727H162.734L150.971 7.93848L158.611 15.8135H145.642L143.047 13.1387L138.971 15.8135H126.002L138.002 7.93848L130.513 0.219727H143.482L145.926 2.73926Z" />
              <path d="M70.4294 0.124146L69.319 4.33313H48.6296L48.2175 5.9054H48.2233L48.2224 5.90833H68.8796L67.7692 10.1427H47.0856L47.0603 10.2726C46.8284 11.0727 47.8351 11.7177 49.3063 11.7179H67.3308L66.194 15.9269H43.1608C38.3069 15.9267 34.95 13.7581 35.6726 11.0988L37.2995 4.97864C37.3359 4.84353 37.384 4.71053 37.4392 4.57825L37.4372 4.57922L38.6042 0.124146H70.4294Z" />
            </svg>
        </span>
    }
}

/// The bracket characters keep their colors; the letterforms use `currentColor`.
fn remix_wordmark_hero() -> Node {
    rsx! {
        <svg
          role="img"
          aria-label="Remix"
          viewBox="0 0 820 73"
          mix={css("width: 100%; height: auto; display: block; color: var(--text-primary);")}
        >
          <path d="M53.1347 52.3526H37.3303L32.0532 72.1341H47.8519L53.1347 52.3526Z" fill="#FFDF5F" />
          <path d="M44.2314 26.4861L39.1011 45.7148H54.9079L60.0431 26.4861H44.2314Z" fill="#FFDF5F" />
          <path
            d="M61.9637 19.2947L67.0981 0.0684052H51.2799L46.1504 19.2947H61.9637Z"
            fill="#FFDF5F"
          />
          <path d="M37.3365 52.3526H21.5321L16.2559 72.1341H32.0537L37.3365 52.3526Z" fill="#80E464" />
          <path d="M46.1656 19.2947L51.3 0.0684052H35.4818L30.3523 19.2947H46.1656Z" fill="#80E464" />
          <path d="M28.4333 26.4861L23.303 45.7148H39.1098L44.245 26.4861H28.4333Z" fill="#80E464" />
          <path d="M21.54 52.3526H5.73559L0.458496 72.1341H16.2572L21.54 52.3526Z" fill="#20AAFF" />
          <path d="M12.6359 26.4861L7.50635 45.7148H23.3124L28.4476 26.4861H12.6359Z" fill="#20AAFF" />
          <path
            d="M30.3683 19.2947L35.5035 0.0684052H19.6844L14.5549 19.2947H30.3683Z"
            fill="#20AAFF"
          />
          <path d="M68.7538 52.3526H52.9495L47.6724 72.1341H63.471L68.7538 52.3526Z" fill="#FF65DB" />
          <path d="M59.8513 26.4861L54.7209 45.7148H70.527L75.6622 26.4861H59.8513Z" fill="#FF65DB" />
          <path d="M77.583 19.2947L82.7174 0.0684052H66.8991L61.7688 19.2947H77.583Z" fill="#FF65DB" />
          <path d="M84.5487 52.3526H68.7443L63.468 72.1341H79.2659L84.5487 52.3526Z" fill="#FF5148" />
          <path d="M75.6455 26.4861L70.5151 45.7148H86.322L91.4572 26.4861H75.6455Z" fill="#FF5148" />
          <path d="M93.3778 19.2947L98.5122 0.0684052H82.694L77.5645 19.2947H93.3778Z" fill="#FF5148" />
          <path
            fill-rule="evenodd"
            clip-rule="evenodd"
            d="M216.245 0.0681458L216.243 0.0700989V0.072052C235.424 0.072052 248.674 8.67951 245.842 19.2986L243.923 26.489C241.089 37.1082 223.245 45.7166 204.064 45.7166H202.103L242.393 72.1355H177.706L145.137 46.9959C143.833 46.1598 142.319 45.7156 140.772 45.7156H86.3262L91.4561 26.4871H186.821C190.406 26.4871 193.744 24.877 194.274 22.8914H194.276C194.806 20.9057 192.328 19.2958 188.741 19.2957H93.375L98.5039 0.0681458H216.245ZM131.582 52.3523C133.389 52.3523 134.705 54.0738 134.237 55.8269L129.888 72.1336H79.2783L84.5557 52.3523H131.582Z"
            fill="currentColor"
          />
          <path
            d="M597.362 72.4945L616.641 0.550888H667.543L648.146 72.4945H597.362Z"
            fill="currentColor"
          />
          <path
            d="M405.385 0.550934H570.197C592.298 0.550934 607.58 10.4256 604.288 22.6513L590.887 72.4946H540.103L547.039 46.75L551.036 32.0557L552.564 26.413C553.504 22.7688 548.92 19.7124 542.219 19.7124H527.76C527.642 20.6528 527.642 21.5933 527.29 22.6513L514.006 72.4946H463.105L470.04 46.75L474.037 32.0557L475.566 26.413C476.506 22.7688 471.921 19.7124 465.221 19.7124H451.114L436.89 72.4946H386.106L405.385 0.550934Z"
            fill="currentColor"
          />
          <path
            fill-rule="evenodd"
            clip-rule="evenodd"
            d="M743.176 12.4551L760.652 0.986328H819.692L766.143 36.1279L800.923 71.9785H741.884L730.069 59.8008L711.513 71.9785H652.474L707.104 36.1279L673.009 0.986328H732.049L743.176 12.4551Z"
            fill="currentColor"
          />
          <path
            fill-rule="evenodd"
            clip-rule="evenodd"
            d="M399.467 0.551117L394.413 19.7122H312.242C312.196 19.7122 312.151 19.7139 312.105 19.7142H300.223L298.349 26.8685H298.375L298.371 26.8831H392.415L387.36 46.1624H293.198L293.081 46.7503C292.023 50.3944 296.607 53.3332 303.307 53.3333H385.361L380.189 72.4945H275.33C253.23 72.4945 237.948 62.6201 241.239 50.5121L248.644 22.6517C248.809 22.0384 249.02 21.4319 249.27 20.8314L254.585 0.551117H399.467Z"
            fill="currentColor"
          />
        </svg>
    }
}

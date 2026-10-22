//! `i18n/config.ts` plus the parts of i18next the demo relies on: dotted keys, `{{name}}`
//! interpolation, plural suffixes from the language's plural rules with a `_zero` override,
//! and an English fallback.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `supportedLanguages`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Es,
    Fr,
    Ja,
    Ar,
}

impl Language {
    pub const ALL: [Language; 5] = [Language::En, Language::Es, Language::Fr, Language::Ja, Language::Ar];

    /// `fallbackLanguage`.
    pub const FALLBACK: Language = Language::En;

    /// `isSupportedLanguage`: an exact, case-sensitive match on a code.
    pub fn parse(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code() == code)
    }

    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Es => "es",
            Language::Fr => "fr",
            Language::Ja => "ja",
            Language::Ar => "ar",
        }
    }

    /// `languageNames`.
    pub fn name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Es => "Español",
            Language::Fr => "Français",
            Language::Ja => "日本語",
            Language::Ar => "العربية",
        }
    }

    /// i18next's `dir(locale)`.
    pub fn direction(self) -> &'static str {
        if self == Language::Ar { "rtl" } else { "ltr" }
    }
}

/// `detectionSources`: where the request's language came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetectionSource {
    Path,
    Cookie,
    #[default]
    Browser,
}

impl DetectionSource {
    pub const ALL: [DetectionSource; 3] = [DetectionSource::Path, DetectionSource::Cookie, DetectionSource::Browser];

    pub fn as_str(self) -> &'static str {
        match self {
            DetectionSource::Path => "path",
            DetectionSource::Cookie => "cookie",
            DetectionSource::Browser => "browser",
        }
    }
}

/// A translator fixed to one language (i18next's `getFixedT`), over a JSON resource table.
#[derive(Clone, Copy)]
pub struct Translator<'a> {
    language: Language,
    table: &'a Value,
    fallback: Option<&'a Value>,
}

impl<'a> Translator<'a> {
    /// `fallback` is the fallback language's table, or `None` for `fallbackLng: false`.
    pub fn new(language: Language, table: &'a Value, fallback: Option<&'a Value>) -> Translator<'a> {
        Translator { language, table, fallback }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// `t.get(path)`: the raw value at a dotted path, such as a whole section.
    pub fn get(&self, path: &str) -> Option<&'a Value> {
        lookup(self.table, path)
    }

    /// `t(key)`
    pub fn t(&self, key: &str) -> String {
        self.translate(key, None, &[])
    }

    /// `t(key, { name: value, .. })`
    pub fn t_with(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.translate(key, None, vars)
    }

    /// `t(key, { count })`
    pub fn t_count(&self, key: &str, count: u64) -> String {
        self.translate(key, Some(count), &[])
    }

    fn translate(&self, key: &str, count: Option<u64>, vars: &[(&str, &str)]) -> String {
        let tables = [Some((self.language, self.table)), self.fallback.map(|t| (Language::FALLBACK, t))];
        let found = tables.into_iter().flatten().find_map(|(language, table)| {
            let mut keys = Vec::with_capacity(3);
            if let Some(n) = count {
                if n == 0 {
                    keys.push(format!("{key}_zero"));
                }
                keys.push(format!("{key}_{}", crate::format::plural_category(language.code(), n)));
            }
            keys.push(key.to_string());
            keys.iter().find_map(|k| lookup(table, k)?.as_str())
        });
        let Some(template) = found else { return key.to_string() };
        let count = count.map(|n| n.to_string());
        let vars = count.as_deref().map(|c| ("count", c)).into_iter().chain(vars.iter().copied());
        interpolate(template, &vars.collect::<Vec<_>>())
    }
}

fn lookup<'a>(table: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(table, |value, segment| value.get(segment))
}

/// Replace each `{{name}}` with its value. Unknown names are left as written.
fn interpolate(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start + 2..].find("}}") else { break };
        let name = rest[start + 2..start + 2 + len].trim();
        out.push_str(&rest[..start]);
        match vars.iter().find(|(k, _)| *k == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[start..start + len + 4]),
        }
        rest = &rest[start + len + 4..];
    }
    out.push_str(rest);
    out
}

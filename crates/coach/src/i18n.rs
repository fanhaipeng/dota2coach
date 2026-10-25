//! Spoken languages: message catalogs that turn a rule's message key into a
//! sentence.
//!
//! Rules say *what* to tell the player (`#{ msg: "low_hp" }`); catalogs say
//! *how* in each language. A catalog is a TOML file named after its language
//! code, and catalogs are layered like the rules themselves:
//!
//! ```text
//! rules/lang/<code>.toml               shared messages
//! rules/roles/<role>/lang/<code>.toml  messages for that role pack
//! rules/heroes/<hero>/lang/<code>.toml messages for that hero pack
//! ```
//!
//! Tables flatten to dotted keys, so `[rune] water = "Water"` is `rune.water`.
//! Templates use `{name}` placeholders filled from the rule's `args`. The
//! optional `[_meta]` table describes the language. As with any TOML table,
//! top-level messages must come before it:
//!
//! ```toml
//! [_meta]
//! name = "English"      # shown in logs
//! voice = "en-US"       # BCP 47 tag used to pick a speech voice
//! list_separator = ", " # between list items
//! list_last = " and "   # before the last list item
//! ```
//!
//! Adding a language is adding `<code>.toml` files; no code changes. A key
//! missing from the chosen language falls back to English.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use tracing::warn;

/// The language every catalog falls back to.
pub const FALLBACK_LANG: &str = "en";

const META: &str = "_meta";

/// A value to fill a `{placeholder}` with.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// Spoken as it is: numbers, hero names, anything already in words.
    Text(String),
    /// Message keys, each translated, then joined into a spoken list
    /// ("Power and Bounty", "能量符和赏金符").
    Keys(Vec<String>),
}

/// Describes a language, from a catalog's `[_meta]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangMeta {
    pub code: String,
    pub name: String,
    /// BCP 47 tag for choosing a voice, for example `zh-CN`.
    pub voice: String,
    pub list_separator: String,
    pub list_last: String,
}

impl LangMeta {
    fn defaults(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            name: code.to_owned(),
            voice: code.to_owned(),
            list_separator: ", ".to_owned(),
            list_last: " and ".to_owned(),
        }
    }
}

/// The messages of one language, with English underneath.
#[derive(Debug, Clone)]
pub struct Catalog {
    meta: LangMeta,
    messages: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

impl Catalog {
    /// Loads `lang` from `<dir>/lang/<lang>.toml` for each dir in `dirs`, in
    /// order, so later (more specific) packs override earlier ones. The shared
    /// catalog in the first dir must exist; pack catalogs are optional.
    pub fn load(lang: &str, dirs: &[PathBuf]) -> anyhow::Result<Self> {
        let Some(root) = dirs.first() else {
            bail!("no rules folder to load languages from");
        };
        let shared = lang_file(root, lang);
        if !shared.is_file() {
            bail!(
                "no messages for language `{lang}` (expected {}); available: {}",
                shared.display(),
                available(root).join(", ")
            );
        }

        let mut meta = LangMeta::defaults(lang);
        let messages = load_layers(lang, dirs, Some(&mut meta))?;
        let fallback = if lang == FALLBACK_LANG {
            HashMap::new()
        } else {
            let fallback = load_layers(FALLBACK_LANG, dirs, None)?;
            let mut missing: Vec<_> = fallback
                .keys()
                .filter(|k| !messages.contains_key(*k))
                .map(String::as_str)
                .collect();
            if !missing.is_empty() {
                missing.sort();
                warn!(
                    lang,
                    missing = missing.join(", "),
                    "untranslated messages will be spoken in English"
                );
            }
            fallback
        };
        Ok(Self {
            meta,
            messages,
            fallback,
        })
    }

    pub fn meta(&self) -> &LangMeta {
        &self.meta
    }

    /// The sentence for `key` with `args` filled in, or `None` if no catalog
    /// has the key.
    pub fn render(&self, key: &str, args: &BTreeMap<String, Arg>) -> Option<String> {
        let template = self.lookup(key)?;
        Some(fill(template, |name| {
            args.get(name).map(|arg| match arg {
                Arg::Text(text) => text.clone(),
                Arg::Keys(keys) => self.join(keys),
            })
        }))
    }

    fn lookup(&self, key: &str) -> Option<&str> {
        self.messages
            .get(key)
            .or_else(|| self.fallback.get(key))
            .map(String::as_str)
    }

    fn join(&self, keys: &[String]) -> String {
        let words: Vec<&str> = keys.iter().map(|k| self.lookup(k).unwrap_or(k)).collect();
        match words.as_slice() {
            [] => String::new(),
            [only] => (*only).to_owned(),
            [init @ .., last] => format!(
                "{}{}{last}",
                init.join(&self.meta.list_separator),
                self.meta.list_last
            ),
        }
    }
}

/// Language codes with a shared catalog under `root`, for example `["en", "zh"]`.
pub fn available(root: &Path) -> Vec<String> {
    let mut codes: Vec<String> = std::fs::read_dir(root.join("lang"))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .collect();
    codes.sort();
    codes
}

fn lang_file(dir: &Path, lang: &str) -> PathBuf {
    dir.join("lang").join(format!("{lang}.toml"))
}

fn load_layers(
    lang: &str,
    dirs: &[PathBuf],
    mut meta: Option<&mut LangMeta>,
) -> anyhow::Result<HashMap<String, String>> {
    let mut messages = HashMap::new();
    for dir in dirs {
        let path = lang_file(dir, lang);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut table: toml::Table = text
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(toml::Value::Table(m)) = table.remove(META)
            && let Some(meta) = meta.as_deref_mut()
        {
            apply_meta(meta, &m);
        }
        flatten("", &table, &mut messages).with_context(|| format!("in {}", path.display()))?;
    }
    Ok(messages)
}

fn apply_meta(meta: &mut LangMeta, table: &toml::Table) {
    // Messages written after `[_meta]` land inside it in TOML; say so instead
    // of losing them.
    for key in table.keys() {
        if !["name", "voice", "list_separator", "list_last"].contains(&key.as_str()) {
            warn!(
                key,
                "unknown `_meta` setting; put messages before `[_meta]` or under their own table"
            );
        }
    }
    let set = |field: &mut String, key: &str| {
        if let Some(value) = table.get(key).and_then(toml::Value::as_str) {
            *field = value.to_owned();
        }
    };
    set(&mut meta.name, "name");
    set(&mut meta.voice, "voice");
    set(&mut meta.list_separator, "list_separator");
    set(&mut meta.list_last, "list_last");
}

fn flatten(
    prefix: &str,
    table: &toml::Table,
    out: &mut HashMap<String, String>,
) -> anyhow::Result<()> {
    for (key, value) in table {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            toml::Value::Table(inner) => flatten(&key, inner, out)?,
            _ => bail!("`{key}` should be a string or a table"),
        }
    }
    Ok(())
}

/// Replaces each `{name}` in `template`. Unknown placeholders are left as they
/// are, so a typo is heard rather than silently dropped.
fn fill(template: &str, mut value: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match value(name) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::tests::TempRules;

    fn args(pairs: &[(&str, Arg)]) -> BTreeMap<String, Arg> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn keys(list: &[&str]) -> Arg {
        Arg::Keys(list.iter().map(|k| (*k).to_owned()).collect())
    }

    fn sample(name: &str) -> TempRules {
        let rules = TempRules::new(name);
        rules.write(
            "lang/en.toml",
            r#"
            gold = "You have {gold} gold."
            runes = "{runes} runes soon."
            only_en = "English only."
            [rune]
            power = "Power"
            bounty = "Bounty"
            water = "Water"
            "#,
        );
        rules.write(
            "lang/zh.toml",
            r#"
            [_meta]
            name = "中文"
            voice = "zh-CN"
            list_separator = "、"
            list_last = "和"
            [rune]
            power = "能量符"
            bounty = "赏金符"
            water = "水符"
            gold = "你有{gold}金。"
            "#,
        );
        rules.write("heroes/axe/lang/zh.toml", "runes = \"{runes}快刷新了。\"");
        rules
    }

    #[test]
    fn fills_placeholders_and_joins_lists() {
        let rules = sample("fills_placeholders_and_joins_lists");
        let en = Catalog::load("en", std::slice::from_ref(&rules.0)).unwrap();
        assert_eq!(
            en.render("gold", &args(&[("gold", Arg::Text("2400".into()))])),
            Some("You have 2400 gold.".into())
        );
        assert_eq!(
            en.render(
                "runes",
                &args(&[("runes", keys(&["rune.water", "rune.power", "rune.bounty"]))])
            ),
            Some("Water, Power and Bounty runes soon.".into())
        );
        assert_eq!(en.render("missing", &BTreeMap::new()), None);
    }

    #[test]
    fn layers_override_and_fall_back_to_english() {
        let rules = sample("layers_override_and_fall_back_to_english");
        let dirs = [rules.0.clone(), rules.0.join("heroes/axe")];
        let zh = Catalog::load("zh", &dirs).unwrap();
        assert_eq!(zh.meta().voice, "zh-CN");
        assert_eq!(
            zh.render(
                "runes",
                &args(&[("runes", keys(&["rune.power", "rune.bounty"]))])
            ),
            Some("能量符和赏金符快刷新了。".into())
        );
        // `gold` is under [rune] in this zh file by mistake, so English is used.
        assert_eq!(
            zh.render("gold", &args(&[("gold", Arg::Text("1".into()))])),
            Some("You have 1 gold.".into())
        );
        assert_eq!(
            zh.render("only_en", &BTreeMap::new()),
            Some("English only.".into())
        );
    }

    #[test]
    fn unknown_language_lists_the_available_ones() {
        let rules = sample("unknown_language_lists_the_available_ones");
        let err = Catalog::load("fr", std::slice::from_ref(&rules.0)).unwrap_err();
        assert!(err.to_string().contains("available: en, zh"), "{err}");
    }

    #[test]
    fn unknown_placeholders_are_kept() {
        assert_eq!(fill("a {x} b {y}", |_| None), "a {x} b {y}");
        assert_eq!(fill("{x}{x}", |_| Some("1".into())), "11");
        assert_eq!(fill("open { brace", |_| Some("1".into())), "open { brace");
    }
}

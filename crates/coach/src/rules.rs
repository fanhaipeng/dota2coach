//! The rule engine: coaching rules written as small Rhai scripts.
//!
//! Rules live outside the compiled binary so they can be tuned without
//! recompiling. They are loaded in layers, from general to specific:
//!
//! ```text
//! rules/universal/*.rhai        every game
//! rules/roles/<role>/*.rhai     for example roles/pos1, roles/pos3
//! rules/heroes/<hero>/*.rhai    for example heroes/axe
//! ```
//!
//! Each file defines `fn check(s)`, where `s` is the [`GameSnapshot`] as a map.
//! It returns `()` for nothing to say, or a map (or an array of maps):
//!
//! ```text
//! #{ say: "Buy a TP scroll.", priority: 2, cooldown: 90 }
//! ```
//!
//! `priority` defaults to 1 and `cooldown` (game seconds) to 60. A rule's id is
//! its layer and file stem, for example `universal/no_tp`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rhai::{AST, Dynamic, Engine, Map, Scope};
use tracing::{debug, info, warn};

use crate::advice::{Advice, Advisor};
use crate::snapshot::GameSnapshot;

const DEFAULT_PRIORITY: i64 = 1;
const DEFAULT_COOLDOWN: i64 = 60;

/// Which rule packs to load on top of the universal ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packs {
    pub role: Option<String>,
    /// Short hero name, for example `axe`.
    pub hero: Option<String>,
}

struct Rule {
    id: String,
    ast: AST,
}

/// An [`Advisor`] backed by Rhai rule files.
pub struct RuleEngine {
    engine: Engine,
    root: PathBuf,
    packs: Packs,
    rules: Vec<Rule>,
    broken: HashSet<String>,
}

impl RuleEngine {
    /// Loads the universal rules plus the role and hero packs in `packs`.
    /// A pack folder that doesn't exist is skipped; a rule that doesn't compile
    /// is an error, so mistakes show up at startup rather than mid-game.
    pub fn load(root: impl Into<PathBuf>, packs: Packs) -> anyhow::Result<Self> {
        let mut engine = Engine::new();
        // Guard against a rule with an accidental infinite loop.
        engine.set_max_operations(100_000);
        engine.set_max_call_levels(32);

        let mut this = Self {
            engine,
            root: root.into(),
            packs: Packs::default(),
            rules: Vec::new(),
            broken: HashSet::new(),
        };
        this.reload(packs)?;
        Ok(this)
    }

    /// Reloads all rule files, for example when a new game starts with a
    /// different hero.
    pub fn reload(&mut self, packs: Packs) -> anyhow::Result<()> {
        let mut layers = vec![("universal".to_owned(), self.root.join("universal"))];
        if let Some(role) = &packs.role {
            layers.push((format!("roles/{role}"), self.root.join("roles").join(role)));
        }
        if let Some(hero) = &packs.hero {
            layers.push((
                format!("heroes/{hero}"),
                self.root.join("heroes").join(hero),
            ));
        }

        let mut rules = Vec::new();
        for (layer, dir) in layers {
            if !dir.is_dir() {
                info!(layer, "no rule pack folder; skipping");
                continue;
            }
            for path in rule_files(&dir)? {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let id = format!("{layer}/{stem}");
                let ast = self
                    .engine
                    .compile_file(path.clone())
                    .with_context(|| format!("compiling rule {}", path.display()))?;
                if !ast.iter_functions().any(|f| f.name == "check") {
                    anyhow::bail!("rule {} has no `fn check(s)`", path.display());
                }
                rules.push(Rule { id, ast });
            }
        }
        info!(
            count = rules.len(),
            role = packs.role.as_deref().unwrap_or("-"),
            hero = packs.hero.as_deref().unwrap_or("-"),
            "loaded rules"
        );
        self.rules = rules;
        self.packs = packs;
        self.broken.clear();
        Ok(())
    }

    pub fn packs(&self) -> &Packs {
        &self.packs
    }

    pub fn rule_ids(&self) -> impl Iterator<Item = &str> {
        self.rules.iter().map(|r| r.id.as_str())
    }
}

impl Advisor for RuleEngine {
    fn name(&self) -> &str {
        "rules"
    }

    fn advise(&mut self, snapshot: &GameSnapshot) -> Vec<Advice> {
        let input = match rhai::serde::to_dynamic(snapshot) {
            Ok(input) => input,
            Err(err) => {
                warn!(%err, "couldn't hand the snapshot to the rules");
                return Vec::new();
            }
        };

        let mut out = Vec::new();
        for rule in &self.rules {
            if self.broken.contains(&rule.id) {
                continue;
            }
            let result = self.engine.call_fn::<Dynamic>(
                &mut Scope::new(),
                &rule.ast,
                "check",
                (input.clone(),),
            );
            match result {
                Ok(value) => collect(&rule.id, value, &mut out),
                Err(err) => {
                    // Disable the rule for this game instead of logging it on
                    // every payload.
                    warn!(rule = rule.id, %err, "rule failed; disabling it until reload");
                    self.broken.insert(rule.id.clone());
                }
            }
        }
        out
    }
}

fn rule_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "rhai"))
        .collect();
    files.sort();
    Ok(files)
}

fn collect(rule_id: &str, value: Dynamic, out: &mut Vec<Advice>) {
    if value.is_unit() {
        return;
    }
    if value.is_array() {
        for item in value.into_array().unwrap_or_default() {
            collect(rule_id, item, out);
        }
        return;
    }
    let Some(map) = value.try_cast::<Map>() else {
        warn!(
            rule = rule_id,
            "check() should return (), a map or an array of maps"
        );
        return;
    };
    let Some(message) = map
        .get("say")
        .and_then(|v| v.clone().into_immutable_string().ok())
    else {
        warn!(rule = rule_id, "returned a map without a `say` string");
        return;
    };
    let int = |key: &str, default: i64| {
        map.get(key)
            .and_then(|v| v.as_int().ok())
            .unwrap_or(default)
    };
    debug!(rule = rule_id, %message, "rule fired");
    out.push(Advice {
        rule_id: rule_id.to_owned(),
        message: message.to_string(),
        priority: int("priority", DEFAULT_PRIORITY),
        cooldown_secs: int("cooldown", DEFAULT_COOLDOWN),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch rules folder that is removed when dropped.
    pub(crate) struct TempRules(pub PathBuf);

    impl TempRules {
        pub fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dota2coach-rules-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn write(&self, rel: &str, body: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
    }

    impl Drop for TempRules {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn snap() -> GameSnapshot {
        GameSnapshot {
            in_game: true,
            clock_time: 600,
            hero: "axe".into(),
            alive: true,
            health_percent: 20,
            gold: 500,
            items: vec!["item_blink".into()],
            ..Default::default()
        }
    }

    #[test]
    fn loads_layers_and_fires_rules() {
        let rules = TempRules::new("layers");
        rules.write(
            "universal/low_hp.rhai",
            r#"fn check(s) { if s.health_percent < 30 { #{ say: "Low HP", priority: 3 } } }"#,
        );
        rules.write(
            "roles/pos3/blink.rhai",
            r#"fn check(s) { if s.items.contains("item_blink") { #{ say: "Blink is ready" } } }"#,
        );
        rules.write(
            "heroes/axe/both.rhai",
            r#"fn check(s) { [#{ say: "one", cooldown: 5 }, #{ say: "two" }] }"#,
        );
        rules.write(
            "heroes/lion/never.rhai",
            r#"fn check(s) { #{ say: "wrong hero" } }"#,
        );

        let mut engine = RuleEngine::load(
            &rules.0,
            Packs {
                role: Some("pos3".into()),
                hero: Some("axe".into()),
            },
        )
        .unwrap();
        let mut said: Vec<_> = engine
            .advise(&snap())
            .into_iter()
            .map(|a| (a.rule_id, a.message, a.priority, a.cooldown_secs))
            .collect();
        said.sort();
        assert_eq!(
            said,
            vec![
                ("heroes/axe/both".into(), "one".into(), 1, 5),
                ("heroes/axe/both".into(), "two".into(), 1, 60),
                ("roles/pos3/blink".into(), "Blink is ready".into(), 1, 60),
                ("universal/low_hp".into(), "Low HP".into(), 3, 60),
            ]
        );
    }

    #[test]
    fn a_compile_error_fails_loading() {
        let rules = TempRules::new("compile");
        rules.write("universal/bad.rhai", "fn check(s) { if }");
        assert!(RuleEngine::load(&rules.0, Packs::default()).is_err());
    }

    #[test]
    fn a_rule_that_errors_at_runtime_is_disabled() {
        let rules = TempRules::new("runtime");
        rules.write("universal/oops.rhai", "fn check(s) { s.no_such_field + 1 }");
        rules.write("universal/fine.rhai", r#"fn check(s) { #{ say: "fine" } }"#);
        let mut engine = RuleEngine::load(&rules.0, Packs::default()).unwrap();
        assert_eq!(engine.advise(&snap()).len(), 1);
        assert_eq!(engine.advise(&snap()).len(), 1);
        assert!(engine.broken.contains("universal/oops"));
    }
}

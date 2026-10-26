//! Post-game review, part one: the facts of a recorded game, worked out
//! without any LLM.
//!
//! [`analyze`] turns the game's snapshots and what the coach said into a
//! [`Report`]: deaths with their context, the farm curve against role targets,
//! item timings, time spent sitting on gold, and the advice given. The report
//! renders as text in the chosen language, or serializes to JSON for an LLM to
//! explain later.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::i18n::{Arg, Catalog};
use crate::role::{Lane, lane_at};
use crate::snapshot::GameSnapshot;

/// Gold above this, carried while alive, counts as unspent.
pub const BANKED_GOLD: i64 = 2000;
/// Advice spoken this many seconds before a death counts as a warning.
const WARNING_WINDOW: i64 = 30;
/// Minutes the farm curve is sampled at.
const FARM_MINUTES: [i64; 5] = [5, 10, 15, 20, 30];

/// Bought items that are used up, so their timing says little.
const CONSUMABLES: &[&str] = &[
    "item_tango",
    "item_flask",
    "item_clarity",
    "item_enchanted_mango",
    "item_faerie_fire",
    "item_tpscroll",
    "item_ward_observer",
    "item_ward_sentry",
    "item_ward_dispenser",
    "item_smoke_of_deceit",
    "item_dust",
    "item_blood_grenade",
    "item_aegis",
    "item_cheese",
];

/// Last-hit targets at 5 and 10 minutes, matching the role packs'
/// `last_hits.rhai`.
fn last_hit_target(role: &str, minute: i64) -> Option<i64> {
    match (role, minute) {
        ("pos1", 5) => Some(30),
        ("pos1", 10) => Some(65),
        ("pos2", 5) => Some(25),
        ("pos2", 10) => Some(55),
        ("pos3", 5) => Some(15),
        ("pos3", 10) => Some(35),
        _ => None,
    }
}

/// One thing the coach said during the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Said {
    pub clock: i64,
    pub rule_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub match_id: String,
    pub hero: String,
    pub team: String,
    /// `pos1` to `pos5`, or empty if it was never known.
    pub role: String,
    /// The game clock at the last snapshot.
    pub duration: i64,
    /// `Some(true)` for a win, `None` if the recording stops before the end.
    pub won: Option<bool>,
    pub level: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub denies: i64,
    pub gpm: i64,
    pub xpm: i64,
    pub farm: Vec<FarmPoint>,
    pub death_list: Vec<Death>,
    /// Items held at the end, with when each was first carried.
    pub items: Vec<ItemTiming>,
    pub banked: Banked,
    pub advice: Vec<Said>,
    /// The hero at the start of each game minute, for the coach's write-up.
    pub timeline: Vec<Minute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FarmPoint {
    pub minute: i64,
    pub last_hits: i64,
    pub gpm: i64,
    pub target_last_hits: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Death {
    pub clock: i64,
    /// Where the hero was just before dying, from its own team's view.
    pub lane: Option<&'static str>,
    pub gold_carried: i64,
    pub gold_lost: i64,
    pub had_tp: bool,
    /// Advice the coach gave in the 30 seconds before.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemTiming {
    pub clock: i64,
    pub item: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Minute {
    pub minute: i64,
    pub level: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub denies: i64,
    pub gold: i64,
    pub gpm: i64,
    pub xpm: i64,
    pub alive: bool,
    pub lane: Option<&'static str>,
}

/// Time spent alive with more than [`BANKED_GOLD`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Banked {
    pub total_secs: i64,
    pub longest_secs: i64,
    pub longest_from: i64,
}

/// Works out the report from a game's snapshots, in order, and the advice
/// spoken during it. Snapshots from before the horn are only used for
/// context. Returns `None` for an empty game.
pub fn analyze(snapshots: &[GameSnapshot], said: &[Said]) -> Option<Report> {
    let last = snapshots
        .iter()
        .rev()
        .find(|s| s.in_game || s.clock_time > 0)?;

    let mut farm = Vec::new();
    for minute in FARM_MINUTES {
        let at = minute * 60;
        if let Some(s) = snapshots.iter().find(|s| s.clock_time >= at) {
            farm.push(FarmPoint {
                minute,
                last_hits: s.last_hits,
                gpm: s.gpm,
                target_last_hits: last_hit_target(&last.role, minute),
            });
        }
    }

    let mut deaths = Vec::new();
    let mut prev: Option<&GameSnapshot> = None;
    for s in snapshots {
        if let Some(p) = prev
            && p.alive
            && !s.alive
            && s.in_game
        {
            deaths.push(Death {
                clock: s.clock_time,
                lane: lane_name(p),
                gold_carried: p.gold,
                gold_lost: (p.gold - s.gold).max(0),
                had_tp: p.has_item("item_tpscroll"),
                warnings: said
                    .iter()
                    .filter(|a| a.clock <= s.clock_time && a.clock >= s.clock_time - WARNING_WINDOW)
                    .map(|a| a.text.clone())
                    .collect(),
            });
        }
        prev = Some(s);
    }

    let mut first_seen: HashMap<&str, i64> = HashMap::new();
    for s in snapshots.iter().filter(|s| s.clock_time > 0) {
        for item in &s.items {
            first_seen.entry(item.as_str()).or_insert(s.clock_time);
        }
    }
    let mut items: Vec<ItemTiming> = last
        .items
        .iter()
        .filter(|i| !CONSUMABLES.contains(&i.as_str()) && !i.starts_with("item_recipe"))
        .filter_map(|i| {
            Some(ItemTiming {
                clock: *first_seen.get(i.as_str())?,
                item: i.clone(),
            })
        })
        .collect();
    items.sort_by_key(|i| (i.clock, i.item.clone()));
    items.dedup();

    Some(Report {
        match_id: last.match_id.clone(),
        hero: last.hero.clone(),
        team: last.team.clone(),
        role: last.role.clone(),
        duration: last.clock_time,
        won: match last.win_team.as_str() {
            "radiant" | "dire" if !last.team.is_empty() => Some(last.win_team == last.team),
            _ => None,
        },
        level: last.level,
        kills: last.kills,
        deaths: last.deaths,
        assists: last.assists,
        last_hits: last.last_hits,
        denies: last.denies,
        gpm: last.gpm,
        xpm: last.xpm,
        farm,
        death_list: deaths,
        items,
        banked: banked(snapshots),
        advice: said.to_vec(),
        timeline: timeline(snapshots),
    })
}

fn lane_name(s: &GameSnapshot) -> Option<&'static str> {
    lane_at(&s.team, s.xpos, s.ypos).map(|lane| match lane {
        Lane::Safe => "safe",
        Lane::Mid => "mid",
        Lane::Off => "off",
    })
}

fn timeline(snapshots: &[GameSnapshot]) -> Vec<Minute> {
    let mut out: Vec<Minute> = Vec::new();
    for s in snapshots.iter().filter(|s| s.in_game && s.clock_time >= 0) {
        let minute = s.clock_time / 60;
        if out.last().is_some_and(|m| m.minute >= minute) {
            continue;
        }
        out.push(Minute {
            minute,
            level: s.level,
            kills: s.kills,
            deaths: s.deaths,
            assists: s.assists,
            last_hits: s.last_hits,
            denies: s.denies,
            gold: s.gold,
            gpm: s.gpm,
            xpm: s.xpm,
            alive: s.alive,
            lane: lane_name(s),
        });
    }
    out
}

fn banked(snapshots: &[GameSnapshot]) -> Banked {
    let mut out = Banked::default();
    let mut run_start: Option<i64> = None;
    let mut prev_clock: Option<i64> = None;
    let close = |out: &mut Banked, start: i64, end: i64| {
        let len = end - start;
        out.total_secs += len;
        if len > out.longest_secs {
            out.longest_secs = len;
            out.longest_from = start;
        }
    };
    for s in snapshots.iter().filter(|s| s.in_game && s.clock_time > 0) {
        let banking = s.alive && s.gold > BANKED_GOLD;
        match (banking, run_start) {
            (true, None) => run_start = Some(s.clock_time),
            (false, Some(start)) => {
                close(&mut out, start, s.clock_time);
                run_start = None;
            }
            _ => {}
        }
        prev_clock = Some(s.clock_time);
    }
    if let (Some(start), Some(end)) = (run_start, prev_clock) {
        close(&mut out, start, end);
    }
    out
}

/// `754` -> `12:34`.
pub fn format_clock(secs: i64) -> String {
    let sign = if secs < 0 { "-" } else { "" };
    let secs = secs.abs();
    format!("{sign}{}:{:02}", secs / 60, secs % 60)
}

impl Report {
    /// The report as plain text in the catalog's language.
    pub fn render(&self, catalog: &Catalog) -> String {
        let t = |key: &str, args: &[(&str, String)]| {
            let args: BTreeMap<String, Arg> = args
                .iter()
                .map(|(k, v)| ((*k).to_owned(), Arg::Text(v.clone())))
                .collect();
            catalog
                .render(&format!("review.{key}"), &args)
                .unwrap_or_else(|| format!("review.{key}"))
        };
        let mut out = Vec::new();

        let role = if self.role.is_empty() {
            t("role_unknown", &[])
        } else {
            self.role.clone()
        };
        let result = match self.won {
            Some(true) => t("won", &[]),
            Some(false) => t("lost", &[]),
            None => t("result_unknown", &[]),
        };
        out.push(t(
            "title",
            &[
                ("match", self.match_id.clone()),
                ("hero", self.hero.clone()),
                ("role", role),
                ("duration", format_clock(self.duration)),
                ("result", result),
            ],
        ));
        out.push(t(
            "final",
            &[
                ("level", self.level.to_string()),
                ("kills", self.kills.to_string()),
                ("deaths", self.deaths.to_string()),
                ("assists", self.assists.to_string()),
                ("last_hits", self.last_hits.to_string()),
                ("gpm", self.gpm.to_string()),
            ],
        ));

        out.push(String::new());
        out.push(t("farm_header", &[]));
        for p in &self.farm {
            let target = p
                .target_last_hits
                .map(|target| {
                    let mark = if p.last_hits * 10 < target * 7 {
                        " ⚠"
                    } else {
                        ""
                    };
                    format!(
                        "{}{mark}",
                        t("farm_target", &[("target", target.to_string())])
                    )
                })
                .unwrap_or_default();
            out.push(t(
                "farm_row",
                &[
                    ("time", format!("{}:00", p.minute)),
                    ("last_hits", p.last_hits.to_string()),
                    ("gpm", p.gpm.to_string()),
                    ("target", target),
                ],
            ));
        }

        out.push(String::new());
        out.push(t(
            "deaths_header",
            &[("count", self.death_list.len().to_string())],
        ));
        if self.death_list.is_empty() {
            out.push(t("no_deaths", &[]));
        }
        for d in &self.death_list {
            let place = t(&format!("lane_{}", d.lane.unwrap_or("none")), &[]);
            let tp = if d.had_tp {
                String::new()
            } else {
                t("no_tp", &[])
            };
            let warned = if d.warnings.is_empty() {
                String::new()
            } else {
                t("warned", &[("advice", d.warnings.join(" / "))])
            };
            out.push(t(
                "death_row",
                &[
                    ("time", format_clock(d.clock)),
                    ("place", place),
                    ("lost", d.gold_lost.to_string()),
                    ("carried", d.gold_carried.to_string()),
                    ("tp", tp),
                    ("warned", warned),
                ],
            ));
        }

        out.push(String::new());
        out.push(t("items_header", &[]));
        for i in &self.items {
            out.push(t(
                "item_row",
                &[
                    ("time", format_clock(i.clock)),
                    ("item", i.item.trim_start_matches("item_").replace('_', " ")),
                ],
            ));
        }

        out.push(String::new());
        out.push(t("gold_header", &[]));
        if self.banked.total_secs == 0 {
            out.push(t("gold_ok", &[("gold", BANKED_GOLD.to_string())]));
        } else {
            out.push(t(
                "gold_banked",
                &[
                    ("gold", BANKED_GOLD.to_string()),
                    ("total", format_clock(self.banked.total_secs)),
                    ("longest", format_clock(self.banked.longest_secs)),
                    ("from", format_clock(self.banked.longest_from)),
                ],
            ));
        }

        out.push(String::new());
        out.push(t(
            "advice_header",
            &[("count", self.advice.len().to_string())],
        ));
        let mut counts: Vec<(usize, &str)> = Vec::new();
        for a in &self.advice {
            match counts.iter_mut().find(|(_, text)| *text == a.text) {
                Some((n, _)) => *n += 1,
                None => counts.push((1, &a.text)),
            }
        }
        counts.sort_by_key(|c| std::cmp::Reverse(c.0));
        for (n, text) in counts.iter().take(5) {
            out.push(t(
                "advice_row",
                &[("count", n.to_string()), ("text", (*text).to_owned())],
            ));
        }
        out.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(t: i64) -> GameSnapshot {
        GameSnapshot {
            match_id: "42".into(),
            in_game: true,
            clock_time: t,
            team: "radiant".into(),
            role: "pos1".into(),
            hero: "slardar".into(),
            alive: true,
            xpos: 4800,
            ypos: -6200,
            last_hits: t / 12,
            gpm: 400,
            gold: 600,
            items: vec!["item_tango".into(), "item_tpscroll".into()],
            ..Default::default()
        }
    }

    #[test]
    fn works_out_the_facts_of_a_game() {
        let mut game: Vec<GameSnapshot> = (-30..=700).map(snap).collect();
        // Banks 2500 gold from 3:00 to 4:00.
        for s in &mut game[210..270] {
            s.gold = 2500;
        }
        // Buys a Blink at 6:40 and keeps it.
        for s in &mut game[430..] {
            s.items.push("item_blink".into());
        }
        // Dies at 8:20 carrying 1800 gold without a TP, losing 500.
        for s in &mut game[525..530] {
            s.gold = 1800;
            s.items.retain(|i| i != "item_tpscroll");
        }
        for s in &mut game[530..560] {
            s.alive = false;
            s.gold = 1300;
            s.deaths = 1;
        }
        game.last_mut().unwrap().win_team = "radiant".into();
        let said = [
            Said {
                clock: 480,
                rule_id: "universal/low_hp".into(),
                text: "Low health. Back off.".into(),
            },
            Said {
                clock: 100,
                rule_id: "universal/runes".into(),
                text: "Water runes in 15 seconds.".into(),
            },
        ];

        let r = analyze(&game, &said).unwrap();
        assert_eq!(
            (r.match_id.as_str(), r.duration, r.won),
            ("42", 700, Some(true))
        );
        assert_eq!(
            r.farm,
            vec![
                FarmPoint {
                    minute: 5,
                    last_hits: 25,
                    gpm: 400,
                    target_last_hits: Some(30)
                },
                FarmPoint {
                    minute: 10,
                    last_hits: 50,
                    gpm: 400,
                    target_last_hits: Some(65)
                },
            ]
        );
        assert_eq!(
            r.death_list,
            vec![Death {
                clock: 500,
                lane: Some("safe"),
                gold_carried: 1800,
                gold_lost: 500,
                had_tp: false,
                warnings: vec!["Low health. Back off.".into()],
            }]
        );
        assert_eq!(
            r.items,
            vec![ItemTiming {
                clock: 400,
                item: "item_blink".into()
            }]
        );
        assert_eq!(
            r.banked,
            Banked {
                total_secs: 60,
                longest_secs: 60,
                longest_from: 180
            }
        );
        // One row per minute from 0:00 to 11:00.
        assert_eq!(r.timeline.len(), 12);
        assert_eq!(r.timeline[11].minute, 11);
        assert_eq!(
            (r.timeline[5].last_hits, r.timeline[5].lane),
            (25, Some("safe"))
        );
    }

    #[test]
    fn an_empty_game_has_no_report() {
        assert!(analyze(&[], &[]).is_none());
    }
}

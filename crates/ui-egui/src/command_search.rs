//! Search (Alt+Q): find a ribbon command by name and run it.
//!
//! One matcher serves the search box (`dialogs::command_search`) and the programmatic
//! `ui.search` command, so agents see exactly what a person typing the same text sees. The
//! catalog is the engine's command registry; only commands placed on the ribbon or a menu are
//! offered, as the search box offers ribbon actions. The search never lists itself.

use gridcraft_engine::{CommandInfo, Session};
use serde_json::{Value as Json, json};

/// Ids that open or are the search itself. They are never listed or run from the search.
pub const SELF_IDS: &[&str] = &["ui.search", "dialog:commandSearch", "commandSearch"];

/// How many matches the search box shows.
pub const MAX_RESULTS: usize = 50;

/// How many recently used commands each workbook remembers.
pub const MAX_RECENT: usize = 10;

/// Whether the search may offer `c`: enabled, on the ribbon or a menu, and not the search itself.
fn searchable(c: &CommandInfo) -> bool {
    c.enabled && !c.menu.is_empty() && !SELF_IDS.contains(&c.id)
}

/// Lower rank is a better match; `None` is no match. Every word of the query must appear in the
/// label, ribbon path, id or shortcut. A query that matches none of them as words may still match
/// the label as a subsequence ("frzpn" finds "Freeze Panes").
fn rank(c: &CommandInfo, query: &str) -> Option<u8> {
    let label = c.label.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let q = words.join(" ");
    if label.starts_with(&q) {
        return Some(0);
    }
    if label.split(|ch: char| !ch.is_alphanumeric()).any(|w| w.starts_with(&q)) {
        return Some(1);
    }
    if label.contains(&q) {
        return Some(2);
    }
    if words.iter().all(|w| label.contains(w)) {
        return Some(3);
    }
    let hay = format!("{label} {} {} {}", c.menu.join(" ").to_lowercase(), c.id.to_lowercase(), c.shortcut.unwrap_or("").to_lowercase());
    if words.iter().all(|w| hay.contains(w)) {
        return Some(4);
    }
    // Short queries would match nearly everything as a subsequence.
    let letters: Vec<char> = q.chars().filter(|ch| !ch.is_whitespace()).collect();
    let mut chars = label.chars();
    (letters.len() >= 3 && letters.iter().all(|ch| chars.any(|l| l == *ch))).then_some(5)
}

/// The commands matching `query`, best first (ties keep catalog order), at most [`MAX_RESULTS`].
pub fn search(session: &Session, query: &str) -> Vec<CommandInfo> {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(u8, CommandInfo)> = session.commands().into_iter().filter(searchable).filter_map(|c| rank(&c, &q).map(|r| (r, c))).collect();
    hits.sort_by_key(|(r, _)| *r);
    hits.into_iter().map(|(_, c)| c).take(MAX_RESULTS).collect()
}

/// The recently used commands in `recent` (most recent first) that can still run here.
pub fn recent(session: &Session, recent: &[String]) -> Vec<CommandInfo> {
    let all = session.commands();
    recent.iter().filter_map(|id| all.iter().find(|c| c.id == id.as_str() && searchable(c)).cloned()).collect()
}

/// Notes `id` as the most recently used command in `list`: moved to the front, no duplicates,
/// at most [`MAX_RECENT`]. The search's own ids are never recorded.
pub fn remember(list: &mut Vec<String>, id: &str) {
    if SELF_IDS.contains(&id) {
        return;
    }
    list.retain(|r| r != id);
    list.insert(0, id.to_string());
    list.truncate(MAX_RECENT);
}

/// What `ui.search` returns for one command.
pub fn to_json(c: &CommandInfo) -> Json {
    json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }

    #[test]
    fn substring_word_and_subsequence_matches_rank_in_that_order() {
        let s = session();
        let hits = search(&s, "bold");
        assert_eq!(hits.first().map(|c| c.id), Some("home.bold"));
        assert!(search(&s, "freeze").iter().any(|c| c.id.contains("freeze")));
        assert!(search(&s, "frzpn").iter().any(|c| c.label.to_lowercase().contains("freeze")));
        assert!(search(&s, "").is_empty());
        assert!(search(&s, "zzzzqqqq").is_empty());
    }

    #[test]
    fn primitives_and_the_search_itself_are_never_offered() {
        let s = session();
        assert!(!search(&s, "enter cell").iter().any(|c| c.id == "cell.set"));
        for q in ["search", "ui.search", "command"] {
            assert!(!search(&s, q).iter().any(|c| SELF_IDS.contains(&c.id)), "{q}");
        }
    }

    #[test]
    fn remember_moves_to_front_dedupes_caps_and_skips_itself() {
        let mut list = Vec::new();
        for i in 0..15 {
            remember(&mut list, &format!("c{i}"));
        }
        remember(&mut list, "c10");
        remember(&mut list, "ui.search");
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list.first().map(String::as_str), Some("c10"));
        assert_eq!(list.iter().filter(|r| *r == "c10").count(), 1);
        assert!(!list.iter().any(|r| r == "ui.search"));
    }
}

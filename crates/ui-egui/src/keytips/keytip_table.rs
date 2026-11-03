//! The keytip sequence table: every Excel-style Alt access sequence and what it does.
//!
//! The matcher in [`super::KeyTips::process`] and the badge painter in `ribbon.rs` both read
//! [`SEQUENCES`], so a sequence is added in exactly one place: `keytips.json`.
//!
//! `keytips.json` is the human-edited source of truth (grouped, commented, one line per
//! sequence; `pending` reserves sequences for commands that do not exist yet). `generated.rs`
//! is produced from it by `cargo xtask keytips` — edit the JSON, not the generated file.

pub(crate) use super::Action;

mod generated;
pub use generated::SEQUENCES;

/// What a fully-matched sequence does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Switch to this ribbon tab and keep the prefix: the tab's own badges take over.
    Tab(&'static str),
    /// Keep the prefix: this sequence is a group-level badge; deeper sequences finish it.
    Continue,
    /// Complete the sequence: run the action and clear the prefix.
    Finish(Action),
}

/// Look up a complete sequence.
pub fn lookup(sequence: &str) -> Option<Entry> {
    SEQUENCES.iter().find(|(seq, _)| *seq == sequence).map(|(_, entry)| *entry)
}

/// True if `prefix` is a strict prefix of any known sequence (badges wait for more keys).
pub fn has_deeper(prefix: &str) -> bool {
    SEQUENCES.iter().any(|(seq, _)| seq.len() > prefix.len() && seq.starts_with(prefix))
}

/// The sequence that switches to `tab_name`, if that tab has one (may be a multi-letter unit
/// like Draw's "JI").
pub fn tab_sequence(tab_name: &str) -> Option<&'static str> {
    SEQUENCES.iter().find_map(|(seq, entry)| match entry {
        Entry::Tab(name) if *name == tab_name => Some(*seq),
        _ => None,
    })
}

/// True if `seq` is a group level: an explicit Continue entry, or an implicit level that only
/// exists because deeper sequences start with it (like "J" before Draw's "JI").
pub fn is_group_level(seq: &str) -> bool {
    matches!(lookup(seq), Some(Entry::Continue)) || (lookup(seq).is_none() && has_deeper(seq))
}

fn matches_target(action: &Action, target: &str) -> bool {
    match action {
        Action::Command(id) | Action::CommandWith(id, _) | Action::Pending(id) | Action::Control(id) => *id == target,
        Action::Dialog(name) | Action::DialogWith(name, _) => format!("dialog:{name}") == target,
        Action::Tab(_) => false,
    }
}

/// All sequences that finish by running `target` (a command id or `dialog:<name>`).
pub fn sequences_for(target: &str) -> Vec<&'static str> {
    SEQUENCES
        .iter()
        .filter_map(|(seq, entry)| match entry {
            Entry::Finish(action) if matches_target(action, target) => Some(*seq),
            _ => None,
        })
        .collect()
}

/// Group-level letters for a menu anchor: sequences one key past `prefix` that continue toward
/// any of `targets` (the menu's items) — the badge shown on the button that opens the menu.
pub fn group_extension(prefix: &str, targets: &[&str]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for target in targets {
        for seq in sequences_for(target) {
            if seq.len() <= prefix.len() + 1 || !seq.starts_with(prefix) {
                continue;
            }
            // The intermediate one key past the prefix is the group badge, if it is a
            // group-level sequence (explicit or implicit) rather than the finish itself.
            let candidate = &seq[..prefix.len() + 1];
            if is_group_level(candidate) && !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

/// The badge for a menu item: its finish sequence exactly one key below the current prefix.
pub fn item_letter(prefix: &str, target: &str) -> Option<&'static str> {
    sequences_for(target).into_iter().find(|seq| seq.len() == prefix.len() + 1 && seq.starts_with(prefix))
}

/// Badge sequences for a command control: every sequence for `target` that continues from the
/// current prefix (the painter strips the prefix, so the remaining letters show).
pub fn control_badges(prefix: &str, target: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return Vec::new(); // empty prefix shows only tab badges
    }
    sequences_for(target).into_iter().filter(|seq| seq.len() > prefix.len() && seq.starts_with(prefix)).collect()
}

/// The badge for a single control: the rest of its finish sequence when it has exactly one, or
/// the common group letter when several sequences land on the same control (menu anchors like
/// Borders, whose HBA..HBW variants all share "HB").
pub fn control_badge(prefix: &str, target: &str) -> Option<&'static str> {
    if prefix.is_empty() {
        return None; // empty prefix shows only tab badges
    }
    let seqs: Vec<&'static str> = sequences_for(target).into_iter().filter(|seq| seq.len() > prefix.len() && seq.starts_with(prefix)).collect();
    match seqs.as_slice() {
        [] => None,
        [single] => Some(single),
        many => {
            let common = |a: &str, b: &str| a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
            let lcp = many.iter().skip(1).fold(many[0].len(), |acc, s| acc.min(common(many[0], s)));
            (lcp > prefix.len()).then_some(&many[0][..lcp])
        }
    }
}

/// The single badge for a menu anchor: the direct finish one key past the prefix, or the
/// group-level candidate that prefixes the most of `targets`' sequences. Anchors get one
/// badge, so overlapping groups (e.g. Pictures' "NP" vs SmartArt's "NM") resolve to the
/// dominant one.
pub fn anchor_badge(prefix: &str, targets: &[&str]) -> Option<&'static str> {
    if prefix.is_empty() {
        return None; // empty prefix shows only tab badges
    }
    let all: Vec<&'static str> = targets.iter().flat_map(|t| sequences_for(t)).filter(|s| s.len() > prefix.len() && s.starts_with(prefix)).collect();
    if all.is_empty() {
        return None;
    }
    let depth = prefix.len() + 1;
    let mut candidates: Vec<&'static str> = all.iter().filter(|s| s.len() == depth).copied().collect();
    if candidates.is_empty() {
        for s in &all {
            let c = &s[..depth.min(s.len())];
            if is_group_level(c) && !candidates.contains(&c) {
                candidates.push(c);
            }
        }
    }
    let mut best: Option<(&'static str, usize)> = None;
    for c in candidates {
        let score = all.iter().filter(|s| s.starts_with(c)).count();
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((c, score));
        }
    }
    best.map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_letters_exist_for_standard_tabs() {
        for (tab, letter) in [
            ("Home", "H"),
            ("Insert", "N"),
            ("Page Layout", "P"),
            ("Formulas", "M"),
            ("Data", "A"),
            ("Review", "R"),
            ("View", "W"),
            ("Draw", "JI"), // two-letter unit, shown as one badge
        ] {
            assert_eq!(tab_sequence(tab), Some(letter), "tab {tab}");
        }
        assert_eq!(tab_sequence("Automate"), None);
    }

    #[test]
    fn sequences_for_finds_command_and_dialog_targets() {
        assert_eq!(sequences_for("home.alignRight"), vec!["HAR"]);
        assert_eq!(sequences_for("dialog:pasteSpecial"), vec!["HVS", "ES"]);
        assert_eq!(sequences_for("home.autofitColumnWidth"), vec!["HOI", "OCA"]);
    }

    #[test]
    fn implicit_levels_wait_for_deeper_sequences() {
        assert!(has_deeper("J"), "JI is reachable only through the implicit J level");
        assert!(is_group_level("J"), "implicit levels are group levels");
        assert!(is_group_level("HV"), "explicit Continue levels are group levels");
        assert!(!is_group_level("H"), "a terminal tab is not a group level");
        assert!(!is_group_level("HVS"), "a finish is not a group level");
        assert!(!has_deeper("X"));
    }

    #[test]
    fn group_extension_paints_only_continue_levels() {
        assert_eq!(group_extension("H", &["dialog:pasteSpecial"]), vec!["HV"]);
        assert!(group_extension("", &["dialog:pasteSpecial"]).is_empty(), "tab letters paint on tabs, not buttons");
        assert!(group_extension("E", &["dialog:pasteSpecial"]).is_empty(), "the finish letter paints on the menu item");
    }

    #[test]
    fn item_letter_is_one_level_deep() {
        assert_eq!(item_letter("HV", "dialog:pasteSpecial"), Some("HVS"));
        assert_eq!(item_letter("HV", "edit.paste"), Some("HVP"));
        assert_eq!(item_letter("H", "dialog:pasteSpecial"), None, "the item badge waits for its group letter");
        assert_eq!(item_letter("HO", "home.autofitColumnWidth"), Some("HOI"));
    }

    #[test]
    fn control_badges_require_a_known_prefix() {
        assert!(control_badges("", "home.alignRight").is_empty(), "empty prefix shows only tab badges");
        assert_eq!(control_badges("HA", "home.alignRight"), vec!["HAR"]);
        assert!(control_badges("HZ", "home.alignRight").is_empty(), "unknown prefixes show nothing");
    }

    #[test]
    fn control_badge_shows_the_rest_or_the_group_letter() {
        assert_eq!(control_badge("H", "home.bold"), Some("H1"), "a unique sequence paints its full rest");
        assert_eq!(control_badge("H", "home.fontColor"), Some("HFC"));
        assert_eq!(control_badge("H", "home.borders"), Some("HB"), "many variants collapse to the group letter");
        assert!(control_badge("", "home.bold").is_none(), "empty prefix shows only tab badges");
        assert!(control_badge("HZ", "home.bold").is_none(), "unknown prefixes show nothing");
        assert!(control_badge("H", "view.zoom").is_none(), "diverging sequences at the next letter show nothing");
    }

    #[test]
    fn anchor_badge_resolves_to_the_dominant_group() {
        assert_eq!(anchor_badge("H", &["edit.paste", "edit.pasteSpecial", "dialog:pasteSpecial"]), Some("HV"));
        assert_eq!(anchor_badge("P", &["pageLayout.orientation"]), Some("PO"), "a direct finish paints itself");
        assert_eq!(anchor_badge("N", &["insert.picture"]), Some("NP"), "NP dominates the lone NM (SmartArt) entry");
        assert_eq!(anchor_badge("H", &["table.style"]), Some("HT"));
        assert!(anchor_badge("", &["edit.paste"]).is_none(), "empty prefix shows only tab badges");
        assert!(anchor_badge("H", &["view.zoom"]).is_none(), "no sequences under this prefix");
    }
}

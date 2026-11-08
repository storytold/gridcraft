//! The ExcelScript conformance report, generated from the checked-in names list
//! (`data/excelscript-api.txt`, extracted mechanically from Microsoft's public reference index)
//! and the members GridCraft implements (`data/implemented.txt`). Names only: no summaries,
//! signatures or other Microsoft text are stored. `cargo xtask office-scripts-coverage` writes
//! `docs/office-scripts-coverage.md`; the test here fails when the checked-in report is stale.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Every class and member of the public ExcelScript reference (names only).
pub const API: &str = include_str!("../data/excelscript-api.txt");
/// The members GridCraft's shim and bridge back with a real engine operation.
pub const IMPLEMENTED: &str = include_str!("../data/implemented.txt");
/// The checked-in report.
pub const REPORT: &str = include_str!("../../../docs/office-scripts-coverage.md");

/// One member of the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member<'a> {
    pub class: &'a str,
    pub name: &'a str,
    /// `method` for interface/class members, `field` for enum values.
    pub kind: MemberKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Method,
    Field,
}

/// A member's status in the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Implemented,
    NotYet,
    NotApplicable,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Implemented => "implemented",
            Status::NotYet => "not-yet",
            Status::NotApplicable => "not-applicable",
        }
    }
}

/// Exact coverage numbers, for the report and the PR body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Numbers {
    pub classes: usize,
    pub interface_types: usize,
    pub classes_with_implemented: usize,
    pub methods: usize,
    pub methods_implemented: usize,
    pub fields: usize,
}

impl Numbers {
    pub fn percent_methods(&self) -> f64 {
        if self.methods == 0 { 0.0 } else { (self.methods_implemented as f64) * 100.0 / (self.methods as f64) }
    }
}

/// The parsed reference and implemented sets.
#[derive(Clone, Debug)]
pub struct Coverage {
    pub members: Vec<Member<'static>>,
    /// Every type in the reference, including those with no members.
    pub types: Vec<&'static str>,
    pub implemented: Vec<&'static str>,
}

impl Coverage {
    /// Reads the checked-in lists. Entries before a `#`, blank lines and duplicate members are
    /// ignored; an implemented name the reference does not have is a hard error, so the two
    /// files can never drift apart silently.
    pub fn parse(api: &'static str, implemented: &'static str) -> Result<Coverage, String> {
        let mut members = Vec::new();
        let mut types = Vec::new();
        for line in api.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(class), Some(kind), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
                return Err(format!("bad reference line: {line}"));
            };
            let kind = match kind {
                "method" => MemberKind::Method,
                "field" => MemberKind::Field,
                "type" => {
                    types.push(class);
                    continue;
                }
                other => return Err(format!("bad member kind `{other}`")),
            };
            members.push(Member { class, name, kind });
        }
        members.sort_by(|a, b| a.class.cmp(b.class).then_with(|| a.name.cmp(b.name)));
        members.dedup_by(|a, b| a.class == b.class && a.name == b.name);
        // Only methods can be implemented: an enum value in the list would pass a name check and
        // then be counted as not-applicable, drifting silently.
        let names: std::collections::HashSet<String> =
            members.iter().filter(|m| m.kind == MemberKind::Method).map(|m| format!("{}.{}", m.class, m.name)).collect();
        let mut impls = Vec::new();
        for line in implemented.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !names.contains(line) {
                return Err(format!("`{line}` is not a method in the ExcelScript reference list"));
            }
            impls.push(line);
        }
        impls.sort_unstable();
        impls.dedup();
        types.sort_unstable();
        types.dedup();
        Ok(Coverage { members, types, implemented: impls })
    }

    fn implemented_set(&self) -> std::collections::HashSet<&str> {
        self.implemented.iter().copied().collect()
    }

    pub fn status(&self, member: &Member<'_>, implemented: &std::collections::HashSet<&str>) -> Status {
        if member.kind == MemberKind::Field {
            return Status::NotApplicable;
        }
        if implemented.contains(format!("{}.{}", member.class, member.name).as_str()) { Status::Implemented } else { Status::NotYet }
    }

    pub fn numbers(&self) -> Numbers {
        let implemented = self.implemented_set();
        let mut n = Numbers::default();
        let mut classes: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
        for m in &self.members {
            match m.kind {
                MemberKind::Field => n.fields += 1,
                MemberKind::Method => {
                    n.methods += 1;
                    let entry = classes.entry(m.class).or_default();
                    entry.0 = true;
                    if self.status(m, &implemented) == Status::Implemented {
                        n.methods_implemented += 1;
                        entry.1 = true;
                    }
                }
            }
        }
        n.classes = self.types.len();
        n.interface_types = classes.values().filter(|(has_methods, _)| *has_methods).count();
        n.classes_with_implemented = classes.values().filter(|(_, done)| *done).count();
        n
    }

    /// The Markdown report. Deterministic: the same lists always produce the same bytes.
    pub fn report(&self) -> String {
        let implemented = self.implemented_set();
        let n = self.numbers();
        let mut out = String::new();
        out.push_str("# Office Scripts coverage\n\n");
        out.push_str("> Generated by `cargo xtask office-scripts-coverage` from `crates/script/data/excelscript-api.txt`\n");
        out.push_str("> (names extracted from Microsoft's public ExcelScript reference index, names only) and\n");
        out.push_str("> `crates/script/data/implemented.txt`. Do not edit by hand; run the command.\n\n");
        let _ = writeln!(
            out,
            "**{}/{} interface members implemented ({:.1}%)**, **{}/{} interface types with any implemented \
             member** ({} types including enums). {} enum values are `not-applicable` (they are type values, \
             not operations). Every `not-yet` member of an interface object throws a typed `notSupported` error \
             naming the class and member.\n",
            n.methods_implemented,
            n.methods,
            n.percent_methods(),
            n.classes_with_implemented,
            n.interface_types,
            n.classes,
            n.fields
        );
        out.push_str(
            "Status meanings: **implemented** = a real engine operation backs it; **not-yet** = in the reference, refused with a typed error; **not-applicable** = an enum value, not an operation. Members of the plain data objects the bridge returns (for example `DataValidationRule`) are read as data: a member the object does not carry reads as `undefined`, not as a thrown error.\n\n",
        );
        out.push_str("| Measure | Value |\n|---|---|\n");
        let _ = writeln!(out, "| Types in the reference | {} |", n.classes);
        let _ = writeln!(out, "| Interface types (classes) | {} |", n.interface_types);
        let _ = writeln!(out, "| Interface types with implemented members | {} |", n.classes_with_implemented);
        let _ = writeln!(out, "| Interface members | {} |", n.methods);
        let _ = writeln!(out, "| Interface members implemented | {} |", n.methods_implemented);
        let _ = writeln!(out, "| Enum values (not-applicable) | {} |", n.fields);
        out.push('\n');
        let mut by_class: BTreeMap<&str, Vec<&Member<'_>>> = BTreeMap::new();
        for m in &self.members {
            by_class.entry(m.class).or_default().push(m);
        }
        for (class, members) in by_class {
            let done = members.iter().filter(|m| self.status(m, &implemented) == Status::Implemented).count();
            let methods = members.iter().filter(|m| m.kind == MemberKind::Method).count();
            let _ = writeln!(out, "## {class} ({done}/{methods})");
            if members.iter().all(|m| m.kind == MemberKind::Field) {
                let _ = writeln!(out, "_Enum values: not-applicable._\n");
                continue;
            }
            out.push_str("| Member | Kind | Status |\n|---|---|---|\n");
            for m in members {
                let kind = if m.kind == MemberKind::Method { "method" } else { "field" };
                let _ = writeln!(out, "| `{}` | {kind} | {} |", m.name, self.status(m, &implemented).as_str());
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checked_in_report_is_current() {
        let coverage = Coverage::parse(API, IMPLEMENTED).unwrap();
        assert_eq!(coverage.report(), REPORT, "run `cargo xtask office-scripts-coverage`");
    }

    #[test]
    fn every_implemented_name_is_in_the_reference() {
        let coverage = Coverage::parse(API, IMPLEMENTED).unwrap();
        assert!(!coverage.members.is_empty());
        // Every listed name is a method the reference has, and it counts as implemented.
        assert_eq!(coverage.numbers().methods_implemented, coverage.implemented.len());
    }

    #[test]
    fn an_enum_value_in_the_implemented_list_is_an_error() {
        // A field (enum value) cannot be implemented: it is data, not an operation.
        assert!(Coverage::parse("Mode\tfield\tvalue\n", "Mode.value\n").is_err());
    }

    #[test]
    fn an_unknown_implemented_name_is_an_error() {
        assert!(Coverage::parse("Workbook\tmethod\tgetName\n", "Workbook.getNope\n").is_err());
    }
}

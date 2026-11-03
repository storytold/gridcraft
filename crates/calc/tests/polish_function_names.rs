use gridcraft_calc::is_known_function;
use gridcraft_formula::FormulaLocale;
use std::collections::HashSet;

#[test]
fn every_polish_name_belongs_to_a_known_function() {
    let names = FormulaLocale::Pl.aliases();
    assert!(names.len() > 400, "{}", names.len());
    let mut seen = HashSet::new();
    for (en, pl) in names {
        assert!(is_known_function(en), "{en} is not a GridCraft function");
        assert_ne!(en, pl, "{en} needs no alias");
        // A Polish name that is another function's English name would call the wrong function.
        assert!(!is_known_function(pl), "{pl} ({en}) shadows an English function");
        assert!(seen.insert(pl.to_uppercase()), "{pl} is listed twice");
    }
}

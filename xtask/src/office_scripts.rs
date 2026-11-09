//! `cargo xtask office-scripts-coverage`: write or check `docs/office-scripts-coverage.md` from
//! the checked-in ExcelScript names list and the implemented-member list.

use std::path::Path;

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let coverage = gridcraft_script::coverage::Coverage::parse(gridcraft_script::coverage::API, gridcraft_script::coverage::IMPLEMENTED)?;
    let report = coverage.report();
    let path = root.join("docs/office-scripts-coverage.md");
    if args.contains(&"--check") {
        let current = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if current != report {
            return Err(format!("{} is stale; run `cargo xtask office-scripts-coverage`", path.display()));
        }
        println!("{}: current ({} bytes)", path.display(), report.len());
        return Ok(());
    }
    std::fs::write(&path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    let n = coverage.numbers();
    println!(
        "{}/{} interface members implemented; {}/{} interface types; {} enum values not-applicable -> {}",
        n.methods_implemented,
        n.methods,
        n.classes_with_implemented,
        n.interface_types,
        n.fields,
        path.display()
    );
    Ok(())
}

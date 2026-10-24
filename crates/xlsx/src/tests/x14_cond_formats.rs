//! Conditional formats kept only in the Excel 2010 extension list (`x14:conditionalFormatting`,
//! formulas in `xm:f`, the format inline in `x14:dxf`), as some exporters write every rule.

use gridcraft_model::{CfOperator, CfRule, Color, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const X14: &str = r#"<extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:conditionalFormattings>
<x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="cellIs" priority="2" operator="lessThan" id="{00000000-0000-0000-0000-000000000001}"><xm:f>4</xm:f><x14:dxf><font><color rgb="FF9C0006"/></font><fill><patternFill patternType="solid"><fgColor rgb="FFFFC7CE"/><bgColor rgb="FFFFC7CE"/></patternFill></fill></x14:dxf></x14:cfRule><xm:sqref>B2:B5</xm:sqref></x14:conditionalFormatting>
<x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="expression" priority="1" id="{00000000-0000-0000-0000-000000000002}"><xm:f>$C2="*"</xm:f><x14:dxf><fill><patternFill patternType="solid"><bgColor rgb="FFC6EFCE"/></patternFill></fill></x14:dxf></x14:cfRule><xm:sqref>A2:A5 D2:D5</xm:sqref></x14:conditionalFormatting>
<x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="cellIs" priority="3" operator="equal" id="{00000000-0000-0000-0000-000000000003}"><xm:f>0</xm:f></x14:cfRule></x14:conditionalFormatting>
</x14:conditionalFormattings></ext></extLst>"#;

fn book() -> Workbook {
    let body = format!(r#"<sheetData><row r="2"><c r="B2"><v>3</v></c></row></sheetData>{X14}"#);
    read_xlsx(&minimal(&body, &[], "", "")).unwrap().0
}

fn check(wb: &Workbook) {
    let cfs = &wb.sheet(0).unwrap().cond_formats;
    // The rule without a range is dropped.
    assert_eq!(cfs.len(), 2, "{cfs:?}");
    let less = cfs
        .iter()
        .find_map(|c| match &c.rule {
            CfRule::CellIs { op, a, b, style } => Some((c, *op, a.clone(), b.clone(), style.clone())),
            _ => None,
        })
        .expect("cellIs rule");
    let (cf, op, a, b, style) = less;
    assert_eq!(cf.ranges, vec![gridcraft_core::RangeRef::parse("B2:B5").unwrap()]);
    assert_eq!(cf.priority, 2);
    assert_eq!((op, a.as_str(), b), (CfOperator::Less, "4", None));
    assert_eq!(style.font.color, Color::Rgb(0x9C0006));
    assert_eq!(style.fill.fg, Color::Rgb(0xFFC7CE));
    let (cf, formula, style) = cfs
        .iter()
        .find_map(|c| match &c.rule {
            CfRule::Expression { formula, style } => Some((c, formula.clone(), style.clone())),
            _ => None,
        })
        .expect("expression rule");
    assert_eq!(cf.ranges.len(), 2);
    assert_eq!(formula, "$C2=\"*\"");
    assert_eq!(style.fill.fg, Color::Rgb(0xC6EFCE));
}

#[test]
fn x14_only_rules_are_read() {
    check(&book());
}

#[test]
fn x14_only_rules_survive_saving() {
    let (back, _) = read_xlsx(&write_xlsx(&book()).unwrap()).unwrap();
    check(&back);
}

//! Axis and data labels follow the display region the interface sets, like the cells do.
use gridcraft_chart::format_number;
use gridcraft_core::Region;
use gridcraft_numfmt::set_display_region;

#[test]
fn labels_follow_the_display_region() {
    assert_eq!(format_number(1234.5, "#,##0.0", None), "1,234.5");
    assert_eq!(format_number(2.5, "General", Some(0.5)), "2.5");
    set_display_region(Region::EsEs);
    assert_eq!(format_number(1234.5, "#,##0.0", None), "1.234,5");
    assert_eq!(format_number(700.0, "#,##0.00 \"€\"", None), "700,00 €");
    assert_eq!(format_number(2.5, "General", Some(0.5)), "2,5");
    assert_eq!(format_number(46326.0, "dd/mm/yyyy", None), "31/10/2026");
    set_display_region(Region::UkUa);
    assert_eq!(format_number(42000.0, "#,##0 \"₴\"", None), "42\u{a0}000 ₴");
    set_display_region(Region::EnUs);
    assert_eq!(format_number(1234.5, "#,##0.0", None), "1,234.5");
}

//! Hand-written packages exercising specific reader paths.

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::{AnchorMode, CalcMode, CfRule, Color, DrawingRef, HAlign, PatternType, StyleId, Underline, Visibility};

use super::minimal;
use crate::read_xlsx;

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

#[test]
fn shared_formulas_and_value_types() {
    let body = r#"<sheetData>
        <row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2+$A$1</f><v>3</v></c></row>
        <row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>5</v></c></row>
        <row r="3"><c r="A3" t="inlineStr"><is><r><t>rich </t></r><r><rPr><b/></rPr><t>text</t></r><rPh><t>ignored</t></rPh></is></c><c r="B3"><f t="shared" si="0"/></c></row>
        <row r="4"><c r="A4" t="b"><v>1</v></c><c r="B4" t="e"><v>#DIV/0!</v></c><c r="C4" t="str"><f>_xlfn.CONCAT("a","b")</f><v>ab</v></c><c r="D4" t="d"><v>2024-01-01T12:00:00</v></c></row>
        <row r="5"><c r="A5"><f t="array" ref="A5:B6">A1:B2*2</f><v>2</v></c><c r="B5"><v>6</v></c><c r="C5" cm="1"><f t="array" ref="C5:C7">_xlfn.SEQUENCE(3)</f><v>1</v></c></row>
        <row r="6"><c r="A6"><v>4</v></c><c r="C6"><v>2</v></c></row>
        <row><c><v>9</v></c><c><v>10</v></c></row>
    </sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.name, "Data");
    assert_eq!(s.cell(at("B2")).unwrap().formula.as_ref().unwrap().text, "A2*2+$A$1");
    assert_eq!(s.cell(at("B3")).unwrap().formula.as_ref().unwrap().text, "A3*2+$A$1");
    assert_eq!(s.value(at("B2")), Value::Number(5.0));
    assert_eq!(s.value(at("A3")), Value::text("rich text"));
    assert_eq!(s.value(at("A4")), Value::Bool(true));
    assert_eq!(s.value(at("B4")), Value::Error(CellError::Div0));
    assert_eq!(s.cell(at("C4")).unwrap().formula.as_ref().unwrap().text, "CONCAT(\"a\",\"b\")");
    assert_eq!(s.value(at("C4")), Value::text("ab"));
    assert_eq!(s.value(at("D4")), Value::Number(45292.5));
    let a5 = s.cell(at("A5")).unwrap().formula.clone().unwrap();
    assert_eq!(a5.array, Some(RangeRef::parse("A5:B6").unwrap()));
    assert!(s.cell(at("B5")).is_none() && s.cell(at("A6")).is_none(), "a legacy array's cached values are dropped too");
    let c5 = s.cell(at("C5")).unwrap().formula.clone().unwrap();
    assert_eq!(c5.array, None, "dynamic arrays are not legacy CSE arrays");
    assert_eq!(c5.text, "SEQUENCE(3)");
    assert!(s.cell(at("C6")).is_none(), "spilled cached values are dropped so the formula can spill");
    // Rows/cells without r continue from the previous position.
    assert_eq!(s.value(at("A7")), Value::Number(9.0));
    assert_eq!(s.value(at("B7")), Value::Number(10.0));
    assert!(rep.warnings.iter().any(|w| w.contains("styles")));
}

#[test]
fn workbook_settings_names_and_states() {
    let wb_extra = r#"<workbookPr date1904="1"/><workbookProtection workbookPassword="83AF" lockStructure="1"/><bookViews><workbookView activeTab="0"/></bookViews>"#;
    // Defined names must come after <sheets>; build the workbook by hand.
    let wbx = format!(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{wb_extra}<sheets><sheet name="A" sheetId="1" r:id="rId1"/><sheet name="B" sheetId="2" state="veryHidden" r:id="rId2"/><sheet name="Chart" sheetId="3" r:id="rId3"/><sheet name="C" sheetId="4" state="hidden" r:id="rId4"/></sheets>
        <definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">A!$A$1:$C$10</definedName><definedName name="_xlnm.Print_Titles" localSheetId="0">A!$A:$B,A!$1:$2</definedName><definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">A!$A$1:$B$2</definedName><definedName name="Rate" comment="tax">0.07</definedName><definedName name="Local" localSheetId="3">C!$A$1</definedName><definedName name="Fut">_xlfn.XLOOKUP(1,A!A:A,A!B:B)</definedName></definedNames><calcPr calcMode="manual" iterate="1" iterateCount="50" iterateDelta="0.01"/></workbook>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="/xl/worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet" Target="chartsheets/sheet1.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/></Relationships>"#;
    let ws = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
    let bytes = super::make_zip(&[
        ("[Content_Types].xml", super::CT.as_bytes()),
        ("_rels/.rels", super::ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wbx.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", ws.as_bytes()),
        ("xl/worksheets/sheet2.xml", ws.as_bytes()),
        ("xl/worksheets/sheet3.xml", ws.as_bytes()),
    ]);
    let (wb, rep) = read_xlsx(&bytes).unwrap();
    assert_eq!(wb.sheets.len(), 3);
    assert_eq!(wb.date_system, DateSystem::D1904);
    assert!(wb.protected_structure);
    // The legacy structure password ("password" hashes to 83AF) is kept and really verifies.
    let pw = wb.structure_password.as_ref().unwrap();
    assert!(pw.verify("password").unwrap() && !pw.verify("Password").unwrap());
    assert_eq!(wb.calc.mode, CalcMode::Manual);
    assert!(wb.calc.iterative);
    assert_eq!(wb.calc.max_iterations, 50);
    assert_eq!(wb.sheet(1).unwrap().visibility, Visibility::VeryHidden);
    assert_eq!(wb.sheet(2).unwrap().visibility, Visibility::Hidden);
    let p = &wb.sheet(0).unwrap().print;
    assert_eq!(p.print_area, RangeRef::parse("A1:C10"));
    assert_eq!(p.title_rows, Some((0, 1)));
    assert_eq!(p.title_cols, Some((0, 1)));
    assert_eq!(wb.names.len(), 3);
    let local = wb.names.iter().find(|n| n.name == "Local").unwrap();
    assert_eq!(local.scope, Some(2), "localSheetId maps past the skipped chart sheet");
    assert_eq!(wb.names.iter().find(|n| n.name == "Rate").unwrap().comment, "tax");
    assert_eq!(wb.names.iter().find(|n| n.name == "Fut").unwrap().formula, "XLOOKUP(1,A!A:A,A!B:B)");
    assert!(rep.warnings.iter().any(|w| w.contains("chartsheet")));
}

#[test]
fn styles_theme_and_sizes() {
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <numFmts count="1"><numFmt numFmtId="164" formatCode="0.000&quot;kg&quot;"/></numFmts>
      <fonts count="3"><font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
        <font><b/><i/><u val="double"/><strike/><vertAlign val="superscript"/><sz val="14"/><color rgb="FFFF0000"/><name val="Arial"/></font>
        <font><sz val="11"/><color theme="4" tint="-0.249977111117893"/><name val="Calibri"/></font></fonts>
      <fills count="5"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
        <fill><patternFill patternType="solid"><fgColor indexed="10"/><bgColor indexed="64"/></patternFill></fill>
        <fill><gradientFill degree="90"><stop position="0"><color rgb="FF00FF00"/></stop><stop position="1"><color rgb="FF0000FF"/></stop></gradientFill></fill>
        <fill><patternFill patternType="darkGrid"><fgColor theme="5"/><bgColor rgb="FF123456"/></patternFill></fill></fills>
      <borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border>
        <border diagonalUp="1"><left style="thin"><color auto="1"/></left><right style="mediumDashDot"/><top style="double"><color rgb="FF0000FF"/></top><bottom style="thick"/><diagonal style="hair"/></border></borders>
      <cellXfs count="5"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
        <xf numFmtId="164" fontId="1" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="centerContinuous" vertical="top" wrapText="1" indent="2" textRotation="135"/><protection locked="0" hidden="1"/></xf>
        <xf numFmtId="14" fontId="2" fillId="3" borderId="0"/>
        <xf numFmtId="10" fontId="0" fillId="4" borderId="0"/>
        <xf numFmtId="30" fontId="0" fillId="0" borderId="0"/></cellXfs>
      <dxfs count="1"><dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs>
    </styleSheet>"#;
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office"><a:themeElements><a:clrScheme name="x">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1>
      <a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>
      <a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
      </a:clrScheme><a:fontScheme name="f"><a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#;
    let body = r#"<sheetPr><tabColor rgb="FF00B050"/><pageSetUpPr fitToPage="1"/></sheetPr><dimension ref="A1:XFD1048576"/>
      <sheetViews><sheetView tabSelected="1" showGridLines="0" showRowColHeaders="0" zoomScale="150" rightToLeft="1" workbookViewId="0"><pane xSplit="1" ySplit="2" topLeftCell="B3" activePane="bottomRight" state="frozen"/><selection pane="bottomRight" activeCell="C5" sqref="C5"/></sheetView></sheetViews>
      <sheetFormatPr defaultRowHeight="15"/>
      <cols><col min="1" max="2" width="9.140625" customWidth="1"/><col min="3" max="3" width="20.7109375" customWidth="1" hidden="1" outlineLevel="1"/><col min="4" max="4" width="9.140625" style="1"/></cols>
      <sheetData><row r="1" ht="30" customHeight="1"><c r="A1" s="1"><v>1</v></c><c r="B1" s="2"><v>45000</v></c><c r="C1" s="3"><v>0.5</v></c><c r="D1" s="4"><v>1</v></c></row>
      <row r="2" hidden="1" outlineLevel="2" s="1" customFormat="1"/></sheetData>
      <conditionalFormatting sqref="A1:A10"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>5</formula></cfRule></conditionalFormatting>
      <pageSetup paperSize="9" orientation="landscape" fitToWidth="1" fitToHeight="0"/>"#;
    let bytes = minimal(body, &[("xl/styles.xml", styles), ("xl/theme/theme1.xml", theme)], rels, "");
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    let st1 = wb.styles.get(s.cell(at("A1")).unwrap().style);
    assert_eq!(st1.num_fmt.as_str(), "0.000\"kg\"");
    assert!(st1.font.bold && st1.font.italic && st1.font.strike);
    assert_eq!(st1.font.underline, Underline::Double);
    assert_eq!(st1.font.name, "Arial");
    assert_eq!(st1.font.size, 14.0);
    assert_eq!(st1.font.color, Color::Rgb(0xFF0000));
    assert_eq!(st1.fill.pattern, PatternType::Solid);
    assert_eq!(st1.fill.fg, Color::Rgb(0xFF0000), "indexed 10 is red");
    assert_eq!(st1.align.h, HAlign::CenterAcross);
    assert!(st1.align.wrap);
    assert_eq!(st1.align.indent, 2);
    assert_eq!(st1.align.rotation, -45);
    assert!(!st1.protection.locked && st1.protection.hidden);
    assert!(!st1.borders.diag_up.is_none() && st1.borders.diag_down.is_none());
    assert_eq!(st1.borders.top.color, Color::Rgb(0x0000FF));
    let st2 = wb.styles.get(s.cell(at("B1")).unwrap().style);
    assert_eq!(st2.num_fmt.as_str(), "m/d/yyyy");
    assert_eq!(st2.font.color, Color::Theme(4, -250));
    assert_eq!(st2.fill.fg, Color::Rgb(0x00FF00), "gradient approximated by first stop");
    let st3 = wb.styles.get(s.cell(at("C1")).unwrap().style);
    assert_eq!(st3.num_fmt.as_str(), "0.00%");
    assert_eq!(st3.fill.pattern, PatternType::DarkGrid);
    assert_eq!(st3.fill.fg, Color::Theme(5, 0));
    assert_eq!(wb.styles.get(s.cell(at("D1")).unwrap().style).num_fmt.as_str(), "General");
    // Theme: file order dk1, lt1… maps to lt1, dk1… in the model.
    assert_eq!(wb.theme.colors[0], 0xFFFFFF);
    assert_eq!(wb.theme.colors[1], 0x000000);
    assert_eq!(wb.theme.colors[2], 0xE7E6E6);
    assert_eq!(wb.theme.colors[3], 0x44546A);
    assert_eq!(wb.theme.colors[4], 0x4472C4);
    assert_eq!(wb.theme.major_font, "Calibri Light");
    // Sheet settings.
    assert_eq!(s.tab_color, Some(Color::Rgb(0x00B050)));
    assert_eq!(s.freeze, Some((2, 1)));
    assert!(!s.show_gridlines && !s.show_headings && s.right_to_left);
    assert_eq!(s.zoom, 150);
    assert_eq!(s.view_active, at("C5"));
    assert_eq!(s.col_width(0), 64.0);
    assert!(s.cols.get(&2).unwrap().hidden);
    assert_eq!(s.cols.get(&2).unwrap().size, Some(145.0));
    assert_eq!(s.cols.get(&2).unwrap().outline, 1);
    assert_ne!(s.cols.get(&3).unwrap().style, None);
    assert_eq!(s.rows.get(&0).unwrap().size, Some(40.0));
    let r2 = s.rows.get(&1).unwrap();
    assert!(r2.hidden);
    assert_eq!(r2.outline, 2);
    assert!(r2.style.is_some_and(|x| x != StyleId::DEFAULT));
    // Dxf with bgColor-only fill.
    match &s.cond_formats[0].rule {
        CfRule::CellIs { style, a, .. } => {
            assert_eq!(a, "5");
            assert!(style.font.bold);
            assert_eq!(style.fill.pattern, PatternType::Solid);
            assert_eq!(style.fill.fg, Color::Rgb(0xFFC7CE));
        }
        r => panic!("{r:?}"),
    }
    assert_eq!(s.print.paper, "A4");
    assert_eq!(s.print.fit_to, Some((1, 0)));
}

#[test]
fn strict_namespace_and_shared_strings() {
    let ct = super::CT;
    let root = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let wbx = r#"<x:workbook xmlns:x="http://purl.oclc.org/ooxml/spreadsheetml/main" xmlns:r="http://purl.oclc.org/ooxml/officeDocument/relationships"><x:sheets><x:sheet name="S" sheetId="1" r:id="rId1"/></x:sheets></x:workbook>"#;
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#;
    let sst = r#"<x:sst xmlns:x="http://purl.oclc.org/ooxml/spreadsheetml/main"><x:si><x:t xml:space="preserve"> lead &amp; trail </x:t></x:si><x:si><x:r><x:t>a</x:t></x:r><x:r><x:t>b_x000D_c</x:t></x:r></x:si></x:sst>"#;
    let ws = r#"<x:worksheet xmlns:x="http://purl.oclc.org/ooxml/spreadsheetml/main"><x:sheetData><x:row r="1"><x:c r="A1" t="s"><x:v>0</x:v></x:c><x:c r="B1" t="s"><x:v>1</x:v></x:c><x:c r="C1" t="s"><x:v>99</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
    let bytes = super::make_zip(&[
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root.as_bytes()),
        ("xl/workbook.xml", wbx.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/sharedStrings.xml", sst.as_bytes()),
        ("xl/worksheets/sheet1.xml", ws.as_bytes()),
    ]);
    let (wb, rep) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text(" lead & trail "));
    assert_eq!(s.value(at("B1")), Value::text("ab\rc"));
    assert_eq!(s.value(at("C1")), Value::Empty);
    assert!(rep.warnings.iter().any(|w| w.contains("shared strings")));
}

#[test]
fn hyperlinks_comments_tables_validation_filters() {
    let body = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>H</t></is></c></row></sheetData>
      <autoFilter ref="E1:F9"><filterColumn colId="0"><filters blank="1"><filter val="x"/><filter val="y"/></filters></filterColumn><filterColumn colId="1"><customFilters and="1"><customFilter operator="greaterThan" val="5"/><customFilter operator="lessThan" val="9"/></customFilters></filterColumn></autoFilter>
      <dataValidations count="2"><dataValidation type="list" allowBlank="1" showInputMessage="1" showErrorMessage="1" sqref="B1:B5 D1"><formula1>"a,b,c"</formula1></dataValidation>
        <dataValidation type="whole" operator="between" errorStyle="warning" showDropDown="1" sqref="C1" prompt="p" promptTitle="pt" error="e" errorTitle="et"><formula1>1</formula1><formula2>10</formula2></dataValidation></dataValidations>
      <hyperlinks><hyperlink ref="A1" r:id="rId1" tooltip="tip"/><hyperlink ref="A2" location="Data!B2"/></hyperlinks>
      <printOptions gridLines="1" horizontalCentered="1"/><pageMargins left="0.5" right="0.5" top="1" bottom="1" header="0.4" footer="0.4"/>
      <headerFooter><oddHeader>&amp;CTitle</oddHeader><oddFooter>Page &amp;P</oddFooter></headerFooter>
      <rowBreaks count="1"><brk id="10" max="16383" man="1"/></rowBreaks>
      <sheetProtection sheet="1" formatCells="0" sort="0" password="CBEB"/>
      <tableParts count="1"><tablePart r:id="rId2"/></tableParts>
      <extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"><x14:dataValidations xmlns:x14="x" xmlns:xm="m" count="1"><x14:dataValidation type="list"><x14:formula1><xm:f>Other!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>G1:G4</xm:sqref></x14:dataValidation></x14:dataValidations></ext>
        <ext uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"><x14:sparklineGroups xmlns:x14="x" xmlns:xm="m"><x14:sparklineGroup type="column" markers="1"><x14:colorSeries rgb="FF376092"/><x14:sparklines><x14:sparkline><xm:f>Data!A1:E1</xm:f><xm:sqref>F1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst>"#;
    let sheet_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a?b=1&amp;c=2" TargetMode="External"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/></Relationships>"#;
    let table = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="7" name="T1" displayName="Sales" ref="A10:C14" totalsRowCount="1"><autoFilter ref="A10:C13"/><tableColumns count="3"><tableColumn id="1" name="Region" totalsRowLabel="Total"/><tableColumn id="2" name="Amount" totalsRowFunction="sum"/><tableColumn id="3" name="Tax" totalsRowFunction="custom"><calculatedColumnFormula>Sales[[#This Row],[Amount]]*0.1</calculatedColumnFormula><totalsRowFormula>SUBTOTAL(109,[Tax])</totalsRowFormula></tableColumn></tableColumns><tableStyleInfo name="TableStyleMedium9" showFirstColumn="0" showLastColumn="1" showRowStripes="1" showColumnStripes="0"/></table>"#;
    let comments = r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Ann</author></authors><commentList><comment ref="B2" authorId="0"><text><r><rPr><b/></rPr><t>Ann:</t></r><r><t xml:space="preserve">
Check this</t></r></text></comment></commentList></comments>"#;
    let bytes = minimal(
        body,
        &[("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels), ("xl/tables/table1.xml", table), ("xl/comments1.xml", comments)],
        "",
        "",
    );
    let (wb, rep) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    let h = s.hyperlinks.get(&at("A1")).unwrap();
    assert_eq!(h.target, "https://example.com/a?b=1&c=2");
    assert_eq!(h.tooltip.as_deref(), Some("tip"));
    assert_eq!(s.hyperlinks.get(&at("A2")).unwrap().target, "Data!B2");
    let c = s.comments.get(&at("B2")).unwrap();
    assert_eq!(c.author, "Ann");
    assert_eq!(c.text, "Check this");
    let t = &s.tables[0];
    assert_eq!(t.name, "Sales");
    assert!(t.totals_row && t.header_row && t.filter_button && t.last_col && t.banded_rows);
    assert_eq!(t.columns[0].totals_label.as_deref(), Some("Total"));
    assert_eq!(t.columns[1].totals, gridcraft_model::TotalsFn::Sum);
    assert_eq!(t.columns[2].totals_label.as_deref(), Some("SUBTOTAL(109,[Tax])"));
    assert!(t.columns[2].formula.is_some());
    assert_eq!(t.style, "TableStyleMedium9");
    assert_eq!(s.validations.len(), 3);
    assert_eq!(s.validations[0].f1, "\"a,b,c\"");
    assert_eq!(s.validations[0].ranges.len(), 2);
    let v2 = &s.validations[1];
    assert!(!v2.in_cell_dropdown);
    assert_eq!(v2.f2.as_deref(), Some("10"));
    assert_eq!(v2.error_style, gridcraft_model::ErrorStyle::Warning);
    assert_eq!(s.validations[2].f1, "=Other!$A$1:$A$3");
    let af = s.autofilter.as_ref().unwrap();
    assert_eq!(af.criteria.len(), 2);
    assert!(s.print.gridlines && s.print.center_h);
    assert_eq!(s.print.margins, [0.5, 0.5, 1.0, 1.0, 0.4, 0.4]);
    assert_eq!(s.print.header, "&CTitle");
    assert_eq!(s.print.row_breaks, vec![10]);
    let p = s.protection.as_ref().unwrap();
    assert!(p.format_cells && p.sort && !p.insert_rows && p.select_locked);
    // The legacy sheet password is imported (CBEB is the hash of "test") and verifies.
    let pw = p.password.as_ref().unwrap();
    assert!(pw.verify("test").unwrap() && !pw.verify("Test").unwrap());
    assert!(!rep.warnings.iter().any(|w| w.contains("password")), "{:?}", rep.warnings);
    assert_eq!(s.sparklines.len(), 1);
    assert_eq!(s.sparklines[0].kind, gridcraft_model::SparklineKind::Column);
}

/// The sheet's hyperlinks as written: cell, relationship target (external only), location.
fn written_links(bytes: &[u8]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut pkg = crate::package::Package::open(bytes).unwrap();
    let rels = pkg.rels("xl/worksheets/sheet1.xml").unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    let Some(links) = sheet.child("hyperlinks") else { return vec![] };
    links
        .kids("hyperlink")
        .map(|h| {
            let rel = h.attr("id").map(|id| rels.iter().find(|r| r.id == id).unwrap());
            let target = rel.map(|r| {
                assert!(r.external && r.kind == "hyperlink", "{r:?}");
                r.target.clone()
            });
            (h.attr("ref").unwrap().to_string(), target, h.attr("location").map(str::to_string))
        })
        .collect()
}

/// A link to another file is a relationship (`TargetMode="External"`) in the file, with the place in
/// it as `location`. Saved as a bare `location`, Excel takes it for a place in this workbook and the
/// link no longer opens the file (#800). C16 is Apache POI's `WithMoreVariousData.xlsx`.
#[test]
fn hyperlink_to_another_file_stays_external() {
    let body = r#"<sheetData/><hyperlinks><hyperlink ref="C16" r:id="rId2"/><hyperlink ref="C17" r:id="rId3" location="Sheet1!A2"/>
      <hyperlink ref="C18" r:id="rId4"/><hyperlink ref="C19" location="Data!B2"/><hyperlink ref="C20" r:id="rId5"/></hyperlinks>"#;
    let rel = |id: &str, target: &str| {
        format!(
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{target}" TargetMode="External"/>"#
        )
    };
    let sheet_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}{}{}{}</Relationships>"#,
        rel("rId2", "WithVariousData.xlsx"),
        rel("rId3", "Source.xlsx"),
        rel("rId4", r"..\Reports\Q1.xlsx"),
        rel("rId5", "https://example.com/a.xlsx"),
    );
    let bytes = minimal(body, &[("xl/worksheets/_rels/sheet1.xml.rels", &sheet_rels)], "", "");
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    assert!(s.hyperlinks[&at("C16")].external && !s.hyperlinks[&at("C19")].external);

    let some = |s: &str| Some(s.to_string());
    let links = written_links(&crate::write_xlsx(&wb).unwrap());
    assert_eq!(
        links,
        vec![
            ("C16".into(), some("WithVariousData.xlsx"), None),
            ("C17".into(), some("Source.xlsx"), some("Sheet1!A2")),
            ("C18".into(), some(r"..\Reports\Q1.xlsx"), None),
            ("C19".into(), None, some("Data!B2")),
            ("C20".into(), some("https://example.com/a.xlsx"), None),
        ]
    );
}

#[test]
fn drawing_with_picture_and_chart() {
    let png: &[u8] = b"\x89PNG\r\n\x1a\nfakeimagedata";
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>95250</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>7</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="P" descr="logo"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr/></xdr:pic><xdr:clientData/></xdr:twoCellAnchor>
      <xdr:oneCellAnchor><xdr:from><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="4572000" cy="2743200"/>
        <xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="3" name="C"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId2"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:oneCellAnchor>
      <xdr:twoCellAnchor editAs="absolute"><xdr:from><xdr:col>7</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>8</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:sp><xdr:nvSpPr><xdr:cNvPr id="4" name="S"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom></xdr:spPr></xdr:sp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea>
      <c:barChart><c:barDir val="bar"/><c:grouping val="stacked"/><c:ser><c:idx val="0"/><c:tx><c:strRef><c:f>Data!$B$1</c:f></c:strRef></c:tx><c:spPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></c:spPr><c:cat><c:strRef><c:f>Data!$A$2:$A$5</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser><c:dLbls><c:showVal val="1"/></c:dLbls><c:axId val="1"/><c:axId val="2"/></c:barChart>
      <c:lineChart><c:grouping val="standard"/><c:ser><c:idx val="1"/><c:tx><c:v>Lit</c:v></c:tx><c:val><c:numRef><c:f>Data!$C$2:$C$5</c:f></c:numRef></c:val></c:ser><c:axId val="3"/><c:axId val="4"/></c:lineChart>
      <c:catAx><c:axId val="1"/><c:title><c:tx><c:rich><a:p><a:r><a:t>Region</a:t></a:r></a:p></c:rich></c:tx></c:title></c:catAx><c:valAx><c:axId val="2"/><c:majorGridlines/></c:valAx></c:plotArea><c:legend><c:legendPos val="t"/></c:legend></c:chart></c:chartSpace>"#;
    let drels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#;
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let body = r#"<sheetData/><drawing r:id="rId1"/>"#;
    let mut bytes = minimal(
        body,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", srels),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ("xl/charts/chart1.xml", chart),
        ],
        "",
        "",
    );
    // Add the binary image by rebuilding with it.
    {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut parts: Vec<(String, Vec<u8>)> = vec![];
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let mut d = vec![];
            std::io::Read::read_to_end(&mut f, &mut d).unwrap();
            parts.push((f.name().to_string(), d));
        }
        parts.push(("xl/media/image1.png".into(), png.to_vec()));
        let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        bytes = super::make_zip(&refs);
    }
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    let img = &s.images[0];
    assert_eq!(img.data, png);
    assert_eq!(img.mime, "image/png");
    assert_eq!(img.alt, "logo");
    assert_eq!(img.anchor.cell, at("B3"));
    assert_eq!(img.anchor.dx, 10.0);
    assert_eq!(img.anchor.width, 118.0);
    assert_eq!(img.anchor.height, 100.0);
    // A plain twoCellAnchor is "Move and size with cells".
    assert_eq!(img.anchor.mode, gridcraft_model::AnchorMode::MoveAndSize);
    let ch = &s.charts[0];
    assert_eq!(ch.kind, gridcraft_model::ChartKind::Combo);
    assert_eq!(ch.title.as_deref(), Some("Sales"));
    assert_eq!(ch.anchor.width, 480.0);
    // A oneCellAnchor is "Move but don't size with cells".
    assert_eq!(ch.anchor.mode, gridcraft_model::AnchorMode::MoveOnly);
    assert_eq!(ch.series.len(), 2);
    assert_eq!(ch.series[0].name.as_deref(), Some("Data!$B$1"));
    assert_eq!(ch.series[0].categories.as_deref(), Some("Data!$A$2:$A$5"));
    assert_eq!(ch.series[0].color, Some(Color::Rgb(0xFF0000)));
    assert_eq!(ch.series[1].name.as_deref(), Some("Lit"));
    assert!(ch.series[1].secondary);
    assert_eq!(ch.series[1].kind, Some(gridcraft_model::ChartKind::LineMarkers));
    assert!(ch.data_labels && ch.gridlines);
    assert_eq!(ch.legend, gridcraft_model::LegendPos::Top);
    assert_eq!(ch.x_title.as_deref(), Some("Region"));
    // A twoCellAnchor with editAs="absolute" is "Don't move or size with cells".
    let sp = &s.shapes[0];
    assert_eq!(sp.kind, gridcraft_model::ShapeKind::Ellipse);
    assert_eq!(sp.anchor.mode, gridcraft_model::AnchorMode::Absolute);
}

#[test]
fn grouped_objects_import_as_groups_through_nested_group_transforms() {
    // Columns are 64 px and rows 20 px. The anchor spans B2:F12 (x 64, y 20, 256 × 200 px). The
    // outer group's child box (offset 100,50 px, extent 512 × 400 px) fills it at half scale; the
    // nested group sits at child 356,250 (256 × 200) → sheet 192,120 (128 × 100) and maps its own
    // child box (0,0, 1280 × 1000 px) at a further tenth. The SmartArt frame at the back of the
    // outer group isn't supported and is dropped.
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>11</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="2" name="Outer"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
          <xdr:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2438400" cy="1905000"/><a:chOff x="952500" y="476250"/><a:chExt cx="4876800" cy="3810000"/></a:xfrm></xdr:grpSpPr>
          <xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="7" name="D"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="952500" y="476250"/><a:ext cx="952500" cy="952500"/></xdr:xfrm>
            <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" r:dm="rId9"/></a:graphicData></a:graphic></xdr:graphicFrame>
          <xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="P" descr="banner"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill>
            <xdr:spPr><a:xfrm><a:off x="1333500" y="952500"/><a:ext cx="1219200" cy="762000"/></a:xfrm></xdr:spPr></xdr:pic>
          <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="4" name="Inner"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
            <xdr:grpSpPr><a:xfrm><a:off x="3390900" y="2381250"/><a:ext cx="2438400" cy="1905000"/><a:chOff x="0" y="0"/><a:chExt cx="12192000" cy="9525000"/></a:xfrm></xdr:grpSpPr>
            <xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="5" name="C"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="3048000" y="2381250"/><a:ext cx="6096000" cy="4762500"/></xdr:xfrm>
              <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId2"/></a:graphicData></a:graphic></xdr:graphicFrame>
            <xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="6" name="T"/><xdr:cNvSpPr txBox="1"/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="6096000" cy="2381250"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr>
              <xdr:txBody><a:bodyPr/><a:p><a:r><a:t>Label</a:t></a:r></a:p></xdr:txBody></xdr:sp>
          </xdr:grpSp>
        </xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:val><c:numRef><c:f>Data!$A$1:$A$3</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let drels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#;
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", srels),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ("xl/charts/chart1.xml", chart),
            ("xl/media/image1.png", "fake image data"),
        ],
        "",
        "",
    );
    let (wb, rep) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    // Only the dropped SmartArt frame is reported.
    let drawing_warnings: Vec<&String> = rep.warnings.iter().filter(|w| w.contains("skipped") || w.contains("group")).collect();
    assert_eq!(drawing_warnings, vec!["an embedded object (SmartArt, OLE…) was skipped"]);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert_eq!(s.images.len(), 1);
    let img = &s.images[0].anchor;
    assert_eq!(s.images[0].alt, "banner");
    assert_eq!(img.cell, at("B3"));
    assert!(near(img.dx, 20.0) && near(img.dy, 5.0) && near(img.width, 64.0) && near(img.height, 40.0), "{img:?}");
    assert_eq!(img.mode, AnchorMode::MoveOnly);
    assert_eq!(s.charts.len(), 1);
    let ch = &s.charts[0].anchor;
    assert_eq!(ch.cell, at("D8"));
    assert!(near(ch.dx, 32.0) && near(ch.dy, 5.0) && near(ch.width, 64.0) && near(ch.height, 50.0), "{ch:?}");
    assert_eq!(ch.mode, AnchorMode::MoveOnly);
    assert_eq!(s.shapes.len(), 1);
    let sp = &s.shapes[0].anchor;
    assert_eq!((sp.cell, s.shapes[0].text.as_str()), (at("D7"), "Label"));
    assert!(near(sp.dx, 0.0) && near(sp.dy, 0.0) && near(sp.width, 64.0) && near(sp.height, 25.0), "{sp:?}");

    // The grouping is kept: the outer group holds the picture and the nested group (back to
    // front), which holds the chart and the text box, each with its own box and child space.
    let structure = |s: &gridcraft_model::Sheet| {
        let outer = s.groups.iter().find(|g| g.name == "Outer").unwrap().clone();
        let inner = s.groups.iter().find(|g| g.name == "Inner").unwrap().clone();
        assert_eq!(s.groups.len(), 2);
        assert_eq!(outer.members, vec![DrawingRef::Image(s.images[0].id), DrawingRef::Group(inner.id)]);
        assert_eq!(inner.members, vec![DrawingRef::Chart(s.charts[0].id), DrawingRef::Shape(s.shapes[0].id)]);
        assert_eq!((outer.anchor.mode, inner.anchor.mode), (AnchorMode::MoveOnly, AnchorMode::MoveOnly));
        assert_eq!(s.outer_group(DrawingRef::Shape(s.shapes[0].id)), Some(outer.id));
        (outer, inner)
    };
    let (outer, inner) = structure(s);
    assert_eq!(outer.anchor.cell, at("B2"));
    assert!(near(outer.anchor.width, 256.0) && near(outer.anchor.height, 200.0), "{:?}", outer.anchor);
    assert_eq!((outer.child_offset, outer.child_extent), ((952500, 476250), (4876800, 3810000)));
    assert_eq!(inner.anchor.cell, at("D7"));
    assert!(near(inner.anchor.width, 128.0) && near(inner.anchor.height, 100.0), "{:?}", inner.anchor);
    assert_eq!((inner.child_offset, inner.child_extent), ((0, 0), (12192000, 9525000)));

    // Saved: one anchor holding the outer grpSp, its members in order and in the file's child
    // coordinate spaces (the dropped SmartArt frame shrinks the outer group to its members' box).
    let saved = crate::write_xlsx(&wb).unwrap();
    let mut pkg = crate::package::Package::open(&saved).unwrap();
    let x = pkg.read_xml("xl/drawings/drawing1.xml").unwrap().unwrap();
    assert_eq!(x.children.len(), 1);
    let anchor = &x.children[0];
    assert_eq!(anchor.attr("editAs"), Some("oneCell"));
    let g = anchor.child("grpSp").unwrap();
    assert_eq!(g.path(&["nvGrpSpPr", "cNvPr"]).and_then(|c| c.attr("name")), Some("Outer"));
    let kinds = |g: &crate::xml::El| g.children.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(kinds(g), vec!["nvGrpSpPr", "grpSpPr", "pic", "grpSp"]);
    let pair = |e: Option<&crate::xml::El>, a: &str, b: &str| e.map(|e| (e.attr_i64(a).unwrap(), e.attr_i64(b).unwrap())).unwrap();
    let close = |(a, b): (i64, i64), (c, d): (i64, i64)| (a - c).abs() <= 2 && (b - d).abs() <= 2;
    let pic_xf = g.path(&["pic", "spPr", "xfrm"]);
    assert!(close(pair(pic_xf.and_then(|x| x.child("off")), "x", "y"), (1333500, 952500)));
    assert!(close(pair(pic_xf.and_then(|x| x.child("ext")), "cx", "cy"), (1219200, 762000)));
    let ig = g.child("grpSp").unwrap();
    assert_eq!(kinds(ig), vec!["nvGrpSpPr", "grpSpPr", "graphicFrame", "sp"]);
    let ch_xf = ig.path(&["graphicFrame", "xfrm"]);
    assert!(close(pair(ch_xf.and_then(|x| x.child("off")), "x", "y"), (3048000, 2381250)));
    assert!(close(pair(ch_xf.and_then(|x| x.child("ext")), "cx", "cy"), (6096000, 4762500)));
    let sp_xf = ig.path(&["sp", "spPr", "xfrm"]);
    assert!(close(pair(sp_xf.and_then(|x| x.child("off")), "x", "y"), (0, 0)));

    // Reopened: the same groups and members in the same places.
    let (back, rep) = read_xlsx(&saved).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let b = back.sheet(0).unwrap();
    let (bo, _) = structure(b);
    let same = |x: &gridcraft_model::Anchor, y: &gridcraft_model::Anchor| {
        x.cell == y.cell && near(x.dx, y.dx) && near(x.dy, y.dy) && near(x.width, y.width) && near(x.height, y.height) && x.mode == y.mode
    };
    assert!(same(&b.images[0].anchor, img), "{:?}", b.images[0].anchor);
    assert!(same(&b.charts[0].anchor, ch), "{:?}", b.charts[0].anchor);
    assert!(same(&b.shapes[0].anchor, sp), "{:?}", b.shapes[0].anchor);
    // The outer group now spans its members (B3+20,5 → 204 × 150 px) at the file's half scale.
    assert_eq!(bo.anchor.cell, at("B3"));
    assert!(near(bo.anchor.width, 204.0) && near(bo.anchor.height, 150.0), "{:?}", bo.anchor);
    assert!(close(bo.child_offset, (1333500, 952500)) && close(bo.child_extent, (3886200, 2857500)), "{:?}", (bo.child_offset, bo.child_extent));
}

/// A package whose one sheet has the drawing part `drawing`.
fn with_drawing(drawing: &str) -> Vec<u8> {
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    minimal(r#"<sheetData/><drawing r:id="rId1"/>"#, &[("xl/worksheets/_rels/sheet1.xml.rels", srels), ("xl/drawings/drawing1.xml", drawing)], "", "")
}

/// The box of the shape with text `text`.
fn shape_box(s: &gridcraft_model::Sheet, text: &str) -> (f64, f64, f64, f64) {
    let id = s.shapes.iter().find(|sh| sh.text == text).unwrap().id;
    let r = s.drawing_rect(DrawingRef::Shape(id)).unwrap();
    (r.x, r.y, r.w, r.h)
}

fn near_box(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01 && (a.2 - b.2).abs() < 0.01 && (a.3 - b.3).abs() < 0.01
}

#[test]
fn a_group_of_horizontal_lines_maps_each_axis_on_its_own() {
    // The anchor at B2 is 200 px wide and 0 high. The group's child box is 3810000 EMUs wide
    // (200 px: 1 px per 19050 EMUs) and 0 high, and starts at chOff, not at the group's off.
    let line = |name: &str, x: i64| {
        format!(
            r#"<xdr:cxnSp macro=""><xdr:nvCxnSpPr><xdr:cNvPr id="3" name="L"/><xdr:cNvCxnSpPr/></xdr:nvCxnSpPr><xdr:spPr><a:xfrm><a:off x="{x}" y="476250"/><a:ext cx="1905000" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>{name}</a:t></a:r></a:p></xdr:txBody></xdr:cxnSp>"#
        )
    };
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>76200</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="2" name="Lines"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
          <xdr:grpSpPr><a:xfrm><a:off x="609600" y="190500"/><a:ext cx="1905000" cy="0"/><a:chOff x="952500" y="476250"/><a:chExt cx="3810000" cy="0"/></a:xfrm></xdr:grpSpPr>
          {}{}
        </xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#,
        line("first", 952500),
        line("second", 2857500)
    );
    let (wb, _) = read_xlsx(&with_drawing(&drawing)).unwrap();
    let s = wb.sheet(0).unwrap();
    assert!(near_box(shape_box(s, "first"), (64.0, 20.0, 100.0, 0.0)), "{:?}", shape_box(s, "first"));
    assert!(near_box(shape_box(s, "second"), (164.0, 20.0, 100.0, 0.0)), "{:?}", shape_box(s, "second"));

    let (back, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
    let b = back.sheet(0).unwrap();
    assert_eq!(b.groups.len(), 1);
    assert!(near_box(shape_box(b, "first"), shape_box(s, "first")), "{:?}", shape_box(b, "first"));
    assert!(near_box(shape_box(b, "second"), shape_box(s, "second")), "{:?}", shape_box(b, "second"));
}

#[test]
fn a_flipped_group_mirrors_its_members_and_keeps_its_transform() {
    // The anchor spans B2:F12 (x 64, y 20, 256 × 200 px) and the group maps its child box 1:1.
    // It is flipped horizontally (and rotated a quarter turn, which isn't laid out): its members
    // are mirrored about the group's box. The nested group isn't flipped itself, but everything
    // in it is mirrored with the outer group.
    let rect = |name: &str, x: i64, y: i64, cx: i64, cy: i64| {
        format!(
            r#"<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="3" name="R"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>{name}</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#
        )
    };
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>11</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="2" name="Flipped"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
          <xdr:grpSpPr><a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/><a:ext cx="2438400" cy="1905000"/><a:chOff x="0" y="0"/><a:chExt cx="2438400" cy="1905000"/></a:xfrm></xdr:grpSpPr>
          {}{}
          <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="4" name="Inner"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
            <xdr:grpSpPr><a:xfrm><a:off x="952500" y="1428750"/><a:ext cx="952500" cy="476250"/><a:chOff x="0" y="0"/><a:chExt cx="952500" cy="476250"/></a:xfrm></xdr:grpSpPr>
            {}
          </xdr:grpSp>
        </xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#,
        rect("A", 0, 0, 476250, 381000),
        rect("B", 1866900, 952500, 571500, 190500),
        rect("C", 0, 0, 285750, 476250)
    );
    let (wb, rep) = read_xlsx(&with_drawing(&drawing)).unwrap();
    let s = wb.sheet(0).unwrap();
    // A at the left end and B at the right end swap ends; C keeps its place in the mirrored
    // nested group (x 120..220), mirrored too.
    assert!(near_box(shape_box(s, "A"), (270.0, 20.0, 50.0, 40.0)), "{:?}", shape_box(s, "A"));
    assert!(near_box(shape_box(s, "B"), (64.0, 120.0, 60.0, 20.0)), "{:?}", shape_box(s, "B"));
    assert!(near_box(shape_box(s, "C"), (190.0, 170.0, 30.0, 50.0)), "{:?}", shape_box(s, "C"));
    // The rotation and the unmirrored content are reported, not silently dropped.
    assert_eq!(rep.warnings.iter().filter(|w| w.contains("group")).count(), 2, "{:?}", rep.warnings);
    let outer = s.groups.iter().find(|g| g.name == "Flipped").unwrap();
    assert_eq!((outer.flip, outer.rotation), ((true, false), 5400000));

    // Saved with its transform, and with the members in the file's child coordinates.
    let saved = crate::write_xlsx(&wb).unwrap();
    let mut pkg = crate::package::Package::open(&saved).unwrap();
    let x = pkg.read_xml("xl/drawings/drawing1.xml").unwrap().unwrap();
    let g = x.children[0].child("grpSp").unwrap();
    let xf = g.path(&["grpSpPr", "xfrm"]).unwrap();
    assert_eq!((xf.attr("rot"), xf.flag("flipH", false), xf.flag("flipV", false)), (Some("5400000"), true, false));
    let off = |name: &str| {
        let sp = g.kids("sp").find(|sp| sp.path(&["txBody", "p", "r", "t"]).is_some_and(|t| t.text == name)).unwrap();
        let o = sp.path(&["spPr", "xfrm", "off"]).unwrap();
        (o.attr_i64("x").unwrap(), o.attr_i64("y").unwrap())
    };
    let close = |(a, b): (i64, i64), (c, d): (i64, i64)| (a - c).abs() <= 2 && (b - d).abs() <= 2;
    assert!(close(off("A"), (0, 0)), "{:?}", off("A"));
    assert!(close(off("B"), (1866900, 952500)), "{:?}", off("B"));

    // Reopened: the same places and the same transform.
    let (back, _) = read_xlsx(&saved).unwrap();
    let b = back.sheet(0).unwrap();
    for name in ["A", "B", "C"] {
        assert!(near_box(shape_box(b, name), shape_box(s, name)), "{name}: {:?}", shape_box(b, name));
    }
    let bo = b.groups.iter().find(|g| g.name == "Flipped").unwrap();
    assert_eq!((bo.flip, bo.rotation), ((true, false), 5400000));
}

/// The `a:xfrm` point or size `name` (`off`/`chOff` with `x`/`y`, `ext`/`chExt` with `cx`/`cy`).
fn xfrm_at(xf: &crate::xml::El, name: &str) -> (i64, i64) {
    let (a, b) = if name.ends_with("xt") { ("cx", "cy") } else { ("x", "y") };
    let e = xf.child(name).unwrap();
    (e.attr_i64(a).unwrap(), e.attr_i64(b).unwrap())
}

fn close_emu((a, b): (i64, i64), (c, d): (i64, i64)) -> bool {
    (a - c).abs() <= 2 && (b - d).abs() <= 2
}

/// The `xdr:sp` with text `text` in `g`.
fn sp_named<'a>(g: &'a crate::xml::El, text: &str) -> &'a crate::xml::El {
    g.kids("sp").find(|sp| sp.path(&["txBody", "p", "r", "t"]).is_some_and(|t| t.text == text)).unwrap()
}

#[test]
fn a_rotated_group_keeps_its_box_when_a_member_is_dropped() {
    // A group turned a quarter over B2:F12 (x 64, y 20, 256 × 200 px), its child box mapped 1:1,
    // holds a SmartArt frame in its left half (dropped) and rectangle A in its right half. It
    // turns about the centre of its own box, so saving it with the box around A alone would
    // move A in Excel: the group's box and child box are kept.
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>11</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="2" name="Turned"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
          <xdr:grpSpPr><a:xfrm rot="5400000"><a:off x="609600" y="190500"/><a:ext cx="2438400" cy="1905000"/><a:chOff x="0" y="0"/><a:chExt cx="2438400" cy="1905000"/></a:xfrm></xdr:grpSpPr>
          <xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="3" name="D"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="1219200" cy="1905000"/></xdr:xfrm>
            <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" r:dm="rId9"/></a:graphicData></a:graphic></xdr:graphicFrame>
          <xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="4" name="R"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="1219200" y="0"/><a:ext cx="1219200" cy="952500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>A</a:t></a:r></a:p></xdr:txBody></xdr:sp>
        </xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let (mut wb, _) = read_xlsx(&with_drawing(drawing)).unwrap();
    let saved_group = |wb: &gridcraft_model::Workbook| {
        let saved = crate::write_xlsx(wb).unwrap();
        let mut pkg = crate::package::Package::open(&saved).unwrap();
        let x = pkg.read_xml("xl/drawings/drawing1.xml").unwrap().unwrap();
        let anchor = x.children[0].clone();
        let g = anchor.child("grpSp").unwrap().clone();
        let xf = g.path(&["grpSpPr", "xfrm"]).unwrap().clone();
        assert_eq!(xf.attr("rot"), Some("5400000"));
        let a = xfrm_at(sp_named(&g, "A").path(&["spPr", "xfrm"]).unwrap(), "off");
        let from = anchor.child("from").unwrap().clone();
        let num = |n: &str| from.child(n).unwrap().text.parse::<i64>().unwrap();
        (
            (num("col"), num("colOff"), num("row"), num("rowOff")),
            xfrm_at(&xf, "off"),
            xfrm_at(&xf, "ext"),
            xfrm_at(&xf, "chOff"),
            xfrm_at(&xf, "chExt"),
            a,
        )
    };
    let (from, off, ext, ch_off, ch_ext, a) = saved_group(&wb);
    assert_eq!(from, (1, 0, 1, 0));
    assert!(close_emu(off, (609600, 190500)) && close_emu(ext, (2438400, 1905000)), "{off:?} {ext:?}");
    assert!(close_emu(ch_off, (0, 0)) && close_emu(ch_ext, (2438400, 1905000)), "{ch_off:?} {ch_ext:?}");
    assert!(close_emu(a, (1219200, 0)), "{a:?}");

    // Moved a column to the right, the group's box moves with it; its child box stays.
    let s = wb.sheet_mut(0).unwrap();
    let gid = s.groups[0].id;
    let r = s.drawing_rect(DrawingRef::Group(gid)).unwrap();
    s.transform_group(gid, r, gridcraft_model::ObjectRect { x: r.x + 64.0, ..r });
    let (from, off, ext, ch_off, ch_ext, a) = saved_group(&wb);
    assert_eq!(from, (2, 0, 1, 0));
    assert!(close_emu(off, (1219200, 190500)) && close_emu(ext, (2438400, 1905000)), "{off:?} {ext:?}");
    assert!(close_emu(ch_off, (0, 0)) && close_emu(ch_ext, (2438400, 1905000)), "{ch_off:?} {ch_ext:?}");
    assert!(close_emu(a, (1219200, 0)), "{a:?}");
}

#[test]
fn a_member_turned_a_quarter_is_placed_at_its_turned_box_and_keeps_its_rotation() {
    // The anchor starts 16 px into column A (152400 EMUs) at row 2 and is 256 × 200 px; the
    // group maps its child box 1:1. Text box T is turned 270°: its own box starts left of the
    // group (and of column A), but Excel sizes the group by its turned box, which is inside.
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>152400</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>152400</xdr:colOff><xdr:row>11</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="2" name="Labelled"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr>
          <xdr:grpSpPr><a:xfrm><a:off x="152400" y="190500"/><a:ext cx="2438400" cy="1905000"/><a:chOff x="152400" y="190500"/><a:chExt cx="2438400" cy="1905000"/></a:xfrm></xdr:grpSpPr>
          <xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="3" name="R"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="152400" y="190500"/><a:ext cx="2438400" cy="1905000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>R</a:t></a:r></a:p></xdr:txBody></xdr:sp>
          <xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="4" name="T"/><xdr:cNvSpPr txBox="1"/></xdr:nvSpPr><xdr:spPr><a:xfrm rot="16200000"><a:off x="-304800" y="762000"/><a:ext cx="1381125" cy="257175"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>T</a:t></a:r></a:p></xdr:txBody></xdr:sp>
        </xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let (wb, rep) = read_xlsx(&with_drawing(drawing)).unwrap();
    let s = wb.sheet(0).unwrap();
    // T's turned box: child x -304800 + (1381125 - 257175) / 2 = 257175 (27 px), 27 × 145 px.
    assert!(near_box(shape_box(s, "T"), (27.0, 21.0, 27.0, 145.0)), "{:?}", shape_box(s, "T"));
    assert_eq!(
        rep.warnings.iter().filter(|w| w.contains("rotated")).collect::<Vec<_>>(),
        vec!["a rotated picture or shape in a group is drawn without its rotation"]
    );
    let t = s.shapes.iter().find(|sh| sh.text == "T").unwrap().id;
    assert_eq!(s.groups[0].member_rotations, vec![(DrawingRef::Shape(t), 16200000)]);

    // Saved: the group keeps its anchor, box and child box, and T its own box and rotation.
    let saved = crate::write_xlsx(&wb).unwrap();
    let mut pkg = crate::package::Package::open(&saved).unwrap();
    let x = pkg.read_xml("xl/drawings/drawing1.xml").unwrap().unwrap();
    let from = x.children[0].child("from").unwrap();
    let num = |n: &str| from.child(n).unwrap().text.parse::<i64>().unwrap();
    assert_eq!((num("col"), num("colOff"), num("row"), num("rowOff")), (0, 152400, 1, 0));
    let g = x.children[0].child("grpSp").unwrap();
    let xf = g.path(&["grpSpPr", "xfrm"]).unwrap();
    assert!(close_emu(xfrm_at(xf, "off"), (152400, 190500)) && close_emu(xfrm_at(xf, "ext"), (2438400, 1905000)), "{:?}", xfrm_at(xf, "off"));
    assert!(close_emu(xfrm_at(xf, "chOff"), (152400, 190500)) && close_emu(xfrm_at(xf, "chExt"), (2438400, 1905000)), "{:?}", xfrm_at(xf, "chOff"));
    let txf = sp_named(g, "T").path(&["spPr", "xfrm"]).unwrap();
    assert_eq!(txf.attr("rot"), Some("16200000"));
    assert!(close_emu(xfrm_at(txf, "off"), (-304800, 762000)) && close_emu(xfrm_at(txf, "ext"), (1381125, 257175)), "{:?}", xfrm_at(txf, "off"));

    // Reopened: T in the same place, with its rotation.
    let (back, _) = read_xlsx(&saved).unwrap();
    let b = back.sheet(0).unwrap();
    assert!(near_box(shape_box(b, "T"), shape_box(s, "T")), "{:?}", shape_box(b, "T"));
    let bt = b.shapes.iter().find(|sh| sh.text == "T").unwrap().id;
    assert_eq!(b.groups[0].member_rotations, vec![(DrawingRef::Shape(bt), 16200000)]);
}

#[test]
fn drawing_with_chartex_in_alternate_content() {
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor><xdr:from><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>10</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>15</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:cx1="http://schemas.microsoft.com/office/drawing/2015/9/8/chartex" Requires="cx1">
          <xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chart xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame>
        </mc:Choice><mc:Fallback><xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:sp></mc:Fallback></mc:AlternateContent>
        <xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
    let chartex = r#"<cx:chartSpace xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex">
      <cx:chartData><cx:data id="0"><cx:strDim type="cat"><cx:f>_xlchart.v1.0</cx:f></cx:strDim><cx:numDim type="val"><cx:f>_xlchart.v1.2</cx:f></cx:numDim></cx:data></cx:chartData>
      <cx:chart><cx:title pos="t" align="ctr" overlay="0"><cx:tx><cx:txData><cx:v>Bridge</cx:v></cx:txData></cx:tx></cx:title>
        <cx:plotArea><cx:plotAreaRegion><cx:series layoutId="waterfall" uniqueId="{1}"><cx:tx><cx:txData><cx:f>_xlchart.v1.1</cx:f><cx:v>Amount</cx:v></cx:txData></cx:tx><cx:dataLabels pos="outEnd"><cx:visibility seriesName="0" categoryName="0" value="1"/></cx:dataLabels><cx:dataId val="0"/><cx:layoutPr><cx:subtotals><cx:idx val="3"/></cx:subtotals></cx:layoutPr></cx:series></cx:plotAreaRegion>
        <cx:axis id="0"><cx:catScaling gapWidth="0.5"/><cx:tickLabels/></cx:axis><cx:axis id="1"><cx:valScaling/><cx:majorGridlines/><cx:tickLabels/></cx:axis></cx:plotArea>
        <cx:legend pos="t" align="ctr" overlay="0"/></cx:chart></cx:chartSpace>"#;
    let drels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="../charts/chartEx1.xml"/></Relationships>"#;
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = super::minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", srels),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ("xl/charts/chartEx1.xml", chartex),
        ],
        "",
        // Chartex data goes through hidden names.
        r#"<definedNames><definedName name="_xlchart.v1.0" hidden="1">Data!$A$2:$A$5</definedName><definedName name="_xlchart.v1.1" hidden="1">Data!$B$1</definedName><definedName name="_xlchart.v1.2" hidden="1">Data!$B$2:$B$5</definedName></definedNames>"#,
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.charts.len(), 1);
    assert!(s.shapes.is_empty(), "the fallback isn't read as well");
    assert!(wb.names.is_empty(), "the chart's names aren't workbook names");
    let ch = &s.charts[0];
    assert_eq!(ch.kind, gridcraft_model::ChartKind::Waterfall);
    assert_eq!(ch.title.as_deref(), Some("Bridge"));
    assert_eq!(ch.anchor.cell, at("E2"));
    assert_eq!(ch.series.len(), 1);
    assert_eq!(ch.series[0].name.as_deref(), Some("Data!$B$1"));
    assert_eq!(ch.series[0].categories.as_deref(), Some("Data!$A$2:$A$5"));
    assert_eq!(ch.series[0].values, "Data!$B$2:$B$5");
    assert!(ch.data_labels && ch.gridlines);
    assert_eq!(ch.legend, gridcraft_model::LegendPos::Top);
}

#[test]
fn chartex_parts_as_excel_expects_them() {
    use gridcraft_model::{Chart, ChartKind, LegendPos, Series};
    let mut wb = gridcraft_model::Workbook::new();
    let mut sheet = gridcraft_model::Sheet::new("Data");
    sheet.charts.push(Chart {
        id: 1,
        kind: ChartKind::Waterfall,
        anchor: Default::default(),
        title: None,
        series: vec![Series {
            name: Some("Data!$B$1".into()),
            categories: Some("Data!$A$2:$A$5".into()),
            values: "Data!$B$2:$B$5".into(),
            ..Default::default()
        }],
        legend: LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    });
    wb.sheets = vec![std::sync::Arc::new(sheet)];
    let bytes = crate::write_xlsx(&wb).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    let mut part = |n: &str| {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(n).unwrap_or_else(|_| panic!("{n}")), &mut s).unwrap();
        s
    };
    // Data through hidden `_xlchart` names, with style and colour parts.
    let cx = part("xl/charts/chartEx1.xml");
    assert!(cx.contains("<cx:f>_xlchart.v1.2</cx:f>") && !cx.contains("Data!"), "{cx}");
    // Fills are spelled out, not left to the style part.
    assert!(cx.contains(r#"<cx:spPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></cx:spPr>"#), "{cx}");
    assert!(cx.contains(r#"</cx:chart><cx:spPr><a:solidFill><a:schemeClr val="bg1"/>"#), "{cx}");
    let book = part("xl/workbook.xml");
    assert!(book.contains(r#"<definedName name="_xlchart.v1.1" hidden="1">Data!$B$2:$B$5</definedName>"#), "{book}");
    let rels = part("xl/charts/_rels/chartEx1.xml.rels");
    assert!(rels.contains("relationships/chartStyle") && rels.contains("relationships/chartColorStyle"));
    assert!(part("xl/charts/style1.xml").contains("<cs:chartStyle"));
    assert!(part("xl/charts/colors1.xml").contains("<cs:colorStyle"));
    let types = part("[Content_Types].xml");
    assert!(types.contains("chartstyle+xml") && types.contains("chartcolorstyle+xml") && types.contains("chartex+xml"));
    // Reading resolves the names and doesn't keep them.
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert!(back.names.is_empty());
    let s = &back.sheet(0).unwrap().charts[0].series[0];
    assert_eq!((s.name.as_deref(), s.categories.as_deref(), s.values.as_str()), (Some("Data!$B$1"), Some("Data!$A$2:$A$5"), "Data!$B$2:$B$5"));
}

/// #377: a WordArt-style text box keeps its run size, bold, typeface, text fill and outline, its
/// centring (horizontal and vertical) and its "no outline" shape line, through a round trip.
#[test]
fn text_box_formatting_is_read_and_written() {
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:twoCellAnchor>
      <xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>7</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>8</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
      <xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="2" name="TextBox 1"/><xdr:cNvSpPr txBox="1"/></xdr:nvSpPr>
      <xdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:ln w="9525" cmpd="sng"><a:noFill/></a:ln></xdr:spPr>
      <xdr:txBody><a:bodyPr vertOverflow="clip" horzOverflow="clip" wrap="square" rtlCol="0" anchor="ctr"/><a:lstStyle/>
      <a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="5400" b="1"><a:ln w="19050"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:ln><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:latin typeface="Arial Black"/></a:rPr><a:t>DRAFT</a:t></a:r></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", srels), ("xl/drawings/drawing1.xml", drawing)],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let sp = &wb.sheet(0).unwrap().shapes[0];
    assert_eq!(sp.kind, gridcraft_model::ShapeKind::TextBox);
    assert_eq!(sp.text, "DRAFT");
    let f = &sp.format;
    assert!(f.no_line, "a:ln/a:noFill: no border");
    assert_eq!(f.font_size, Some(54.0));
    assert!(f.bold && !f.italic);
    assert_eq!(f.font.as_deref(), Some("Arial Black"));
    assert_eq!(f.text_color, Some(Color::Theme(0, 0)));
    assert_eq!(f.text_outline, Some((Color::Theme(1, 0), 1.5)));
    assert_eq!(f.align, Some(gridcraft_model::ShapeTextAlign::Center));
    assert_eq!(f.anchor, Some(gridcraft_model::ShapeTextAnchor::Middle));

    let (back, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
    let g = &back.sheet(0).unwrap().shapes[0].format;
    let rgb = |c: Option<Color>| c.and_then(|c| c.resolve(&wb.theme));
    assert_eq!((g.no_line, g.font_size, g.bold, g.font.as_deref()), (true, Some(54.0), true, Some("Arial Black")));
    assert_eq!((g.align, g.anchor), (f.align, f.anchor));
    assert_eq!(rgb(g.text_color), rgb(f.text_color));
    assert_eq!(rgb(g.text_outline.map(|o| o.0)), rgb(f.text_outline.map(|o| o.0)));
    assert_eq!(g.text_outline.map(|o| o.1), Some(1.5));
}

/// `_xludf.` marks a user-defined call even when a built-in has the same name; dropping it turns
/// the call into the built-in, and `_xludf.EOMONTH()` into a call Excel repairs away (#726).
#[test]
fn user_defined_function_prefix_round_trips() {
    let cases =
        [("A1", "_xludf.EOMONTH()"), ("A2", "_xludf.CONCAT(\"a\",\"b\")"), ("A3", "_xludf.SUM(1,2)"), ("A4", "MYUDF(1)"), ("A5", "_xludf.MYUDF(1)")];
    let rows: String = cases
        .iter()
        .enumerate()
        .map(|(i, (a, f))| format!(r#"<row r="{}"><c r="{a}"><f>{}</f></c></row>"#, i + 1, f.replace('"', "&quot;")))
        .collect();
    let (wb, _) = read_xlsx(&minimal(&format!("<sheetData>{rows}</sheetData>"), &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    for (a, f) in cases {
        assert_eq!(s.cell(at(a)).unwrap().formula.as_ref().unwrap().text, f, "read {a}");
    }
    let bytes = crate::write_xlsx(&wb).unwrap();
    let mut pkg = crate::package::Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    for (a, f) in cases {
        let cell = sheet.child("sheetData").unwrap().kids("row").flat_map(|r| r.kids("c")).find(|c| c.attr("r") == Some(a)).unwrap();
        assert_eq!(cell.child("f").unwrap().text, f, "written {a}");
    }
}

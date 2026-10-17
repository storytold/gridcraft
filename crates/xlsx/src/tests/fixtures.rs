//! Hand-written packages exercising specific reader paths.

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::{CalcMode, CfRule, Color, HAlign, PatternType, StyleId, Underline, Visibility};

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

/// Column widths count digits of the Normal font, as in Excel: a Calibri 12 workbook measures in
/// 8 px digits, so 9.71 characters are 78 px (68 with Calibri 11's 7 px), and a sheet without a
/// default width gets 8 digits plus padding, 72 px. Widths read back unchanged after a save.
#[test]
fn column_widths_follow_the_normal_font() {
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <fonts count="1"><font><sz val="12"/><name val="Calibri"/></font></fonts>
      <fills count="1"><fill><patternFill patternType="none"/></fill></fills>
      <borders count="1"><border/></borders>
      <cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellXfs>
    </styleSheet>"#;
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let body = r#"<cols><col min="10" max="10" width="9.7109375" customWidth="1"/></cols><sheetData/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[("xl/styles.xml", styles)], rels, "")).unwrap();
    assert_eq!(wb.digit_width(), 8.0);
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.col_width(9), 78.0);
    assert_eq!(s.default_col_width, 72.0);
    let (back, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
    let b = back.sheet(0).unwrap();
    assert_eq!((b.col_width(9), b.default_col_width), (78.0, 72.0));

    // The same file with Calibri 11 keeps the usual 7 px digits.
    let styles11 = styles.replace(r#"<sz val="12"/>"#, r#"<sz val="11"/>"#);
    let (wb, _) = read_xlsx(&minimal(body, &[("xl/styles.xml", &styles11)], rels, "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!((s.col_width(9), s.default_col_width), (68.0, 64.0));
}

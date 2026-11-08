//! Excel's ribbon and menu tree (feature names only), used as the parity metric: a catalog
//! entry counts as done when an implemented command with its id is registered
//! ([`implemented`]). `cargo xtask parity` writes `docs/parity-checklist.md` from this list.
//!
//! The schema is `(id, tab, group, label)`; availability is not a catalog field but the command's
//! [`crate::Status`], which has two values a catalog row can have a command for:
//!
//! - [`crate::Status::Implemented`]: the command runs. [`implemented`] counts the row as done.
//! - [`crate::Status::NotImplemented`]: the id is published ahead of its feature and answers
//!   `NOT_IMPLEMENTED` (see [`RESERVED`]). [`implemented`] counts the row as missing, as if it had
//!   no command at all, so registering a placeholder never inflates parity.
//!
//! A row with no registered command is simply missing (most of the checklist).

/// (id, tab, group, label)
pub const CATALOG: &[(&str, &str, &str, &str)] = &[
    // File
    ("file.new", "File", "File", "New"),
    ("file.open", "File", "File", "Open"),
    ("file.save", "File", "File", "Save"),
    ("file.saveAs", "File", "File", "Save As"),
    ("file.close", "File", "File", "Close"),
    ("file.print", "File", "File", "Print"),
    ("file.exportPdf", "File", "Export", "Export to PDF"),
    ("file.exportCsv", "File", "Export", "CSV"),
    ("file.exportHtml", "File", "Export", "Web Page"),
    ("file.properties", "File", "Info", "Properties"),
    ("file.options", "File", "Options", "Options"),
    // Home
    ("edit.paste", "Home", "Clipboard", "Paste"),
    ("edit.pasteSpecial", "Home", "Clipboard", "Paste Special"),
    ("edit.cut", "Home", "Clipboard", "Cut"),
    ("edit.copy", "Home", "Clipboard", "Copy"),
    ("edit.formatPainter", "Home", "Clipboard", "Format Painter"),
    ("home.fontName", "Home", "Font", "Font"),
    ("home.fontSize", "Home", "Font", "Font Size"),
    ("home.increaseFontSize", "Home", "Font", "Increase Font Size"),
    ("home.decreaseFontSize", "Home", "Font", "Decrease Font Size"),
    ("home.bold", "Home", "Font", "Bold"),
    ("home.italic", "Home", "Font", "Italic"),
    ("home.underline", "Home", "Font", "Underline"),
    ("home.doubleUnderline", "Home", "Font", "Double Underline"),
    ("home.strikethrough", "Home", "Font", "Strikethrough"),
    ("home.superscript", "Home", "Font", "Superscript"),
    ("home.subscript", "Home", "Font", "Subscript"),
    ("home.borders", "Home", "Font", "Borders"),
    ("home.drawBorder", "Home", "Font", "Draw Border"),
    ("home.fillColor", "Home", "Font", "Fill Color"),
    ("home.fontColor", "Home", "Font", "Font Color"),
    ("home.alignTop", "Home", "Alignment", "Top Align"),
    ("home.alignMiddle", "Home", "Alignment", "Middle Align"),
    ("home.alignBottom", "Home", "Alignment", "Bottom Align"),
    ("home.orientation", "Home", "Alignment", "Orientation"),
    ("home.alignLeft", "Home", "Alignment", "Align Left"),
    ("home.alignCenter", "Home", "Alignment", "Center"),
    ("home.alignRight", "Home", "Alignment", "Align Right"),
    ("home.decreaseIndent", "Home", "Alignment", "Decrease Indent"),
    ("home.increaseIndent", "Home", "Alignment", "Increase Indent"),
    ("home.wrapText", "Home", "Alignment", "Wrap Text"),
    ("home.mergeCenter", "Home", "Alignment", "Merge & Center"),
    ("home.mergeAcross", "Home", "Alignment", "Merge Across"),
    ("home.mergeCells", "Home", "Alignment", "Merge Cells"),
    ("home.unmergeCells", "Home", "Alignment", "Unmerge Cells"),
    ("home.numberFormat", "Home", "Number", "Number Format"),
    ("home.accounting", "Home", "Number", "Accounting Number Format"),
    ("home.percent", "Home", "Number", "Percent Style"),
    ("home.comma", "Home", "Number", "Comma Style"),
    ("home.increaseDecimal", "Home", "Number", "Increase Decimal"),
    ("home.decreaseDecimal", "Home", "Number", "Decrease Decimal"),
    ("home.conditionalFormat", "Home", "Styles", "Conditional Formatting"),
    ("home.clearRules", "Home", "Styles", "Clear Rules"),
    ("home.manageRules", "Home", "Styles", "Manage Rules"),
    ("home.formatAsTable", "Home", "Styles", "Format as Table"),
    ("home.cellStyle", "Home", "Styles", "Cell Styles"),
    ("home.newCellStyle", "Home", "Styles", "New Cell Style"),
    ("home.insertCells", "Home", "Cells", "Insert Cells"),
    ("home.insertRows", "Home", "Cells", "Insert Sheet Rows"),
    ("home.insertColumns", "Home", "Cells", "Insert Sheet Columns"),
    ("home.insertSheet", "Home", "Cells", "Insert Sheet"),
    ("home.deleteCells", "Home", "Cells", "Delete Cells"),
    ("home.deleteRows", "Home", "Cells", "Delete Sheet Rows"),
    ("home.deleteColumns", "Home", "Cells", "Delete Sheet Columns"),
    ("home.deleteSheet", "Home", "Cells", "Delete Sheet"),
    ("home.rowHeight", "Home", "Cells", "Row Height"),
    ("home.autofitRowHeight", "Home", "Cells", "AutoFit Row Height"),
    ("home.columnWidth", "Home", "Cells", "Column Width"),
    ("home.autofitColumnWidth", "Home", "Cells", "AutoFit Column Width"),
    ("home.defaultWidth", "Home", "Cells", "Default Width"),
    ("home.hideRows", "Home", "Cells", "Hide Rows"),
    ("home.hideColumns", "Home", "Cells", "Hide Columns"),
    ("home.unhideRows", "Home", "Cells", "Unhide Rows"),
    ("home.unhideColumns", "Home", "Cells", "Unhide Columns"),
    ("sheet.hide", "Home", "Cells", "Hide Sheet"),
    ("sheet.unhide", "Home", "Cells", "Unhide Sheet"),
    ("sheet.rename", "Home", "Cells", "Rename Sheet"),
    ("sheet.move", "Home", "Cells", "Move or Copy Sheet"),
    ("sheet.tabColor", "Home", "Cells", "Tab Color"),
    ("review.protectSheet", "Home", "Cells", "Protect Sheet"),
    ("home.lockCell", "Home", "Cells", "Lock Cell"),
    ("home.formatCells", "Home", "Cells", "Format Cells"),
    ("formulas.autoSum", "Home", "Editing", "AutoSum"),
    ("edit.fillDown", "Home", "Editing", "Fill Down"),
    ("edit.fillRight", "Home", "Editing", "Fill Right"),
    ("edit.fillUp", "Home", "Editing", "Fill Up"),
    ("edit.fillLeft", "Home", "Editing", "Fill Left"),
    ("edit.fillSeries", "Home", "Editing", "Series"),
    ("edit.flashFill", "Home", "Editing", "Flash Fill"),
    ("edit.clearAll", "Home", "Editing", "Clear All"),
    ("edit.clearFormats", "Home", "Editing", "Clear Formats"),
    ("edit.clearContents", "Home", "Editing", "Clear Contents"),
    ("edit.clearComments", "Home", "Editing", "Clear Comments"),
    ("edit.clearHyperlinks", "Home", "Editing", "Clear Hyperlinks"),
    ("data.sortAscending", "Home", "Editing", "Sort A to Z"),
    ("data.sortDescending", "Home", "Editing", "Sort Z to A"),
    ("data.sort", "Home", "Editing", "Custom Sort"),
    ("data.filter", "Home", "Editing", "Filter"),
    ("data.clearFilter", "Home", "Editing", "Clear Filter"),
    ("data.reapply", "Home", "Editing", "Reapply"),
    ("edit.find", "Home", "Editing", "Find"),
    ("edit.replace", "Home", "Editing", "Replace"),
    ("edit.goTo", "Home", "Editing", "Go To"),
    ("edit.goToSpecial", "Home", "Editing", "Go To Special"),
    ("home.addIns", "Home", "Add-ins", "Add-ins"),
    // Insert
    ("insert.pivotTable", "Insert", "Tables", "PivotTable"),
    ("insert.recommendedPivotTables", "Insert", "Tables", "Recommended PivotTables"),
    ("insert.table", "Insert", "Tables", "Table"),
    ("insert.picture", "Insert", "Illustrations", "Pictures"),
    ("insert.shape", "Insert", "Illustrations", "Shapes"),
    ("insert.icons", "Insert", "Illustrations", "Icons"),
    ("insert.smartArt", "Insert", "Illustrations", "SmartArt"),
    ("insert.screenshot", "Insert", "Illustrations", "Screenshot"),
    ("insert.recommendedCharts", "Insert", "Charts", "Recommended Charts"),
    ("insert.chart", "Insert", "Charts", "Charts"),
    ("insert.pivotChart", "Insert", "Charts", "PivotChart"),
    ("insert.map", "Insert", "Charts", "Maps"),
    ("insert.sparkline", "Insert", "Sparklines", "Sparklines"),
    ("insert.slicer", "Insert", "Filters", "Slicer"),
    ("insert.timeline", "Insert", "Filters", "Timeline"),
    ("insert.link", "Insert", "Links", "Link"),
    ("review.newComment", "Insert", "Comments", "Comment"),
    ("insert.textBox", "Insert", "Text", "Text Box"),
    ("pageLayout.headerFooter", "Insert", "Text", "Header & Footer"),
    ("insert.wordArt", "Insert", "Text", "WordArt"),
    ("insert.signatureLine", "Insert", "Text", "Signature Line"),
    ("insert.object", "Insert", "Text", "Object"),
    ("insert.equation", "Insert", "Symbols", "Equation"),
    ("insert.symbol", "Insert", "Symbols", "Symbol"),
    ("insert.checkbox", "Insert", "Cell Controls", "Checkbox"),
    // Draw
    ("draw.pen", "Draw", "Tools", "Pen"),
    ("draw.eraser", "Draw", "Tools", "Eraser"),
    ("draw.lasso", "Draw", "Tools", "Lasso Select"),
    ("draw.inkToShape", "Draw", "Convert", "Ink to Shape"),
    // Page Layout
    ("pageLayout.theme", "Page Layout", "Themes", "Themes"),
    ("pageLayout.themeColors", "Page Layout", "Themes", "Colors"),
    ("pageLayout.themeFonts", "Page Layout", "Themes", "Fonts"),
    ("pageLayout.themeEffects", "Page Layout", "Themes", "Effects"),
    ("pageLayout.margins", "Page Layout", "Page Setup", "Margins"),
    ("pageLayout.orientation", "Page Layout", "Page Setup", "Orientation"),
    ("pageLayout.size", "Page Layout", "Page Setup", "Size"),
    ("pageLayout.printArea", "Page Layout", "Page Setup", "Print Area"),
    ("pageLayout.breaks", "Page Layout", "Page Setup", "Breaks"),
    ("pageLayout.background", "Page Layout", "Page Setup", "Background"),
    ("pageLayout.printTitles", "Page Layout", "Page Setup", "Print Titles"),
    ("pageLayout.pageSetup", "Page Layout", "Page Setup", "Page Setup"),
    ("pageLayout.scaleToFit", "Page Layout", "Scale to Fit", "Scale to Fit"),
    ("view.gridlines", "Page Layout", "Sheet Options", "View Gridlines"),
    ("pageLayout.printGridlines", "Page Layout", "Sheet Options", "Print Gridlines"),
    ("view.headings", "Page Layout", "Sheet Options", "View Headings"),
    ("pageLayout.printHeadings", "Page Layout", "Sheet Options", "Print Headings"),
    ("arrange.bringForward", "Page Layout", "Arrange", "Bring Forward"),
    ("arrange.sendBackward", "Page Layout", "Arrange", "Send Backward"),
    ("arrange.selectionPane", "Page Layout", "Arrange", "Selection Pane"),
    ("arrange.align", "Page Layout", "Arrange", "Align"),
    ("arrange.group", "Page Layout", "Arrange", "Group"),
    ("arrange.rotate", "Page Layout", "Arrange", "Rotate"),
    // Formulas
    ("formulas.insertFunction", "Formulas", "Function Library", "Insert Function"),
    ("formulas.recentlyUsed", "Formulas", "Function Library", "Recently Used"),
    ("formulas.financial", "Formulas", "Function Library", "Financial"),
    ("formulas.logical", "Formulas", "Function Library", "Logical"),
    ("formulas.text", "Formulas", "Function Library", "Text"),
    ("formulas.dateTime", "Formulas", "Function Library", "Date & Time"),
    ("formulas.lookupReference", "Formulas", "Function Library", "Lookup & Reference"),
    ("formulas.mathTrig", "Formulas", "Function Library", "Math & Trig"),
    ("formulas.moreFunctions", "Formulas", "Function Library", "More Functions"),
    ("formulas.nameManager", "Formulas", "Defined Names", "Name Manager"),
    ("formulas.defineName", "Formulas", "Defined Names", "Define Name"),
    ("formulas.useInFormula", "Formulas", "Defined Names", "Use in Formula"),
    ("formulas.createFromSelection", "Formulas", "Defined Names", "Create from Selection"),
    ("formulas.tracePrecedents", "Formulas", "Formula Auditing", "Trace Precedents"),
    ("formulas.traceDependents", "Formulas", "Formula Auditing", "Trace Dependents"),
    ("formulas.removeArrows", "Formulas", "Formula Auditing", "Remove Arrows"),
    ("formulas.showFormulas", "Formulas", "Formula Auditing", "Show Formulas"),
    ("formulas.errorChecking", "Formulas", "Formula Auditing", "Error Checking"),
    ("formulas.evaluateFormula", "Formulas", "Formula Auditing", "Evaluate Formula"),
    ("formulas.watchWindow", "Formulas", "Formula Auditing", "Watch Window"),
    ("formulas.calculationOptions", "Formulas", "Calculation", "Calculation Options"),
    ("formulas.calculateNow", "Formulas", "Calculation", "Calculate Now"),
    ("formulas.calculateSheet", "Formulas", "Calculation", "Calculate Sheet"),
    // Data
    ("data.getData", "Data", "Get & Transform Data", "Get Data"),
    ("data.fromTextCsv", "Data", "Get & Transform Data", "From Text/CSV"),
    ("data.fromWeb", "Data", "Get & Transform Data", "From Web"),
    ("data.fromTableRange", "Data", "Get & Transform Data", "From Table/Range"),
    ("data.refreshAll", "Data", "Queries & Connections", "Refresh All"),
    ("data.queriesConnections", "Data", "Queries & Connections", "Queries & Connections"),
    ("data.stocks", "Data", "Data Types", "Stocks"),
    ("data.geography", "Data", "Data Types", "Geography"),
    ("data.sortAscending", "Data", "Sort & Filter", "Sort A to Z"),
    ("data.sortDescending", "Data", "Sort & Filter", "Sort Z to A"),
    ("data.sort", "Data", "Sort & Filter", "Sort"),
    ("data.filter", "Data", "Sort & Filter", "Filter"),
    ("data.clearFilter", "Data", "Sort & Filter", "Clear"),
    ("data.reapply", "Data", "Sort & Filter", "Reapply"),
    ("data.advancedFilter", "Data", "Sort & Filter", "Advanced"),
    ("data.textToColumns", "Data", "Data Tools", "Text to Columns"),
    ("data.flashFill", "Data", "Data Tools", "Flash Fill"),
    ("data.removeDuplicates", "Data", "Data Tools", "Remove Duplicates"),
    ("data.validation", "Data", "Data Tools", "Data Validation"),
    ("data.circleInvalid", "Data", "Data Tools", "Circle Invalid Data"),
    ("data.consolidate", "Data", "Data Tools", "Consolidate"),
    ("data.relationships", "Data", "Data Tools", "Relationships"),
    ("data.scenarioManager", "Data", "Forecast", "Scenario Manager"),
    ("data.goalSeek", "Data", "Forecast", "Goal Seek"),
    ("data.dataTable", "Data", "Forecast", "Data Table"),
    ("data.forecastSheet", "Data", "Forecast", "Forecast Sheet"),
    ("data.group", "Data", "Outline", "Group"),
    ("data.ungroup", "Data", "Outline", "Ungroup"),
    ("data.subtotal", "Data", "Outline", "Subtotal"),
    ("data.showDetail", "Data", "Outline", "Show Detail"),
    ("data.hideDetail", "Data", "Outline", "Hide Detail"),
    ("data.analyzeData", "Data", "Analysis", "Analyze Data"),
    ("data.solver", "Data", "Analysis", "Solver"),
    ("data.analysisToolPak", "Data", "Analysis", "Data Analysis"),
    // Review
    ("review.spelling", "Review", "Proofing", "Spelling"),
    ("review.thesaurus", "Review", "Proofing", "Thesaurus"),
    ("review.workbookStatistics", "Review", "Proofing", "Workbook Statistics"),
    ("review.checkAccessibility", "Review", "Accessibility", "Check Accessibility"),
    ("review.smartLookup", "Review", "Insights", "Smart Lookup"),
    ("review.translate", "Review", "Language", "Translate"),
    ("review.newComment", "Review", "Comments", "New Comment"),
    ("review.deleteComment", "Review", "Comments", "Delete Comment"),
    ("review.previousComment", "Review", "Comments", "Previous Comment"),
    ("review.nextComment", "Review", "Comments", "Next Comment"),
    ("review.showComments", "Review", "Comments", "Show Comments"),
    ("review.newNote", "Review", "Notes", "New Note"),
    ("review.showNote", "Review", "Notes", "Show/Hide Note"),
    ("review.showAllNotes", "Review", "Notes", "Show All Notes"),
    ("review.convertToComments", "Review", "Notes", "Convert to Comments"),
    ("review.protectSheet", "Review", "Protect", "Protect Sheet"),
    ("review.unprotectSheet", "Review", "Protect", "Unprotect Sheet"),
    ("review.protectWorkbook", "Review", "Protect", "Protect Workbook"),
    ("review.allowEditRanges", "Review", "Protect", "Allow Edit Ranges"),
    ("review.showChanges", "Review", "Changes", "Show Changes"),
    // View
    ("view.normal", "View", "Workbook Views", "Normal"),
    ("view.pageBreakPreview", "View", "Workbook Views", "Page Break Preview"),
    ("view.pageLayout", "View", "Workbook Views", "Page Layout"),
    ("view.customViews", "View", "Workbook Views", "Custom Views"),
    ("view.ruler", "View", "Show", "Ruler"),
    ("view.formulaBar", "View", "Show", "Formula Bar"),
    ("view.gridlines", "View", "Show", "Gridlines"),
    ("view.headings", "View", "Show", "Headings"),
    ("view.zoom", "View", "Zoom", "Zoom"),
    ("view.zoom100", "View", "Zoom", "100%"),
    ("view.zoomToSelection", "View", "Zoom", "Zoom to Selection"),
    ("view.newWindow", "View", "Window", "New Window"),
    ("view.arrangeAll", "View", "Window", "Arrange All"),
    ("view.freezePanes", "View", "Window", "Freeze Panes"),
    ("view.freezeTopRow", "View", "Window", "Freeze Top Row"),
    ("view.freezeFirstColumn", "View", "Window", "Freeze First Column"),
    ("view.unfreezePanes", "View", "Window", "Unfreeze Panes"),
    ("view.split", "View", "Window", "Split"),
    ("view.hideWindow", "View", "Window", "Hide"),
    ("view.unhideWindow", "View", "Window", "Unhide"),
    ("view.switchWindows", "View", "Window", "Switch Windows"),
    ("view.macros", "View", "Macros", "Macros"),
    ("macro.record", "View", "Macros", "Record Macro"),
    // Automate
    ("automate.newScript", "Automate", "Scripting Tools", "New Script"),
    ("automate.allScripts", "Automate", "Scripting Tools", "All Scripts"),
    ("automate.recordActions", "Automate", "Scripting Tools", "Record Actions"),
    ("automate.trustScripts", "Automate", "Scripting Tools", "Trust Scripts"),
    // Developer (Excel's Developer tab). One id convention per control: a Developer entry that
    // already has a command elsewhere keeps that id (`DEVELOPER_SHARED`, and Macros is
    // `view.macros` under View), and only an entry with no implementation yet reserves a
    // `developer.*` id (`RESERVED`). No `developer.*` id duplicates an existing command.
    ("insert.button", "Developer", "Controls", "Button (Form Control)"),
    ("insert.spinButton", "Developer", "Controls", "Spin Button (Form Control)"),
    ("insert.slider", "Developer", "Controls", "Scroll Bar (Form Control)"),
    ("control.format", "Developer", "Controls", "Properties"),
    ("developer.visualBasic", "Developer", "Code", "Visual Basic"),
    ("developer.recordMacro", "Developer", "Code", "Record Macro"),
    ("developer.useRelativeReferences", "Developer", "Code", "Use Relative References"),
    ("developer.macroSecurity", "Developer", "Code", "Macro Security"),
    ("developer.addIns", "Developer", "Add-ins", "Add-ins"),
    ("developer.excelAddIns", "Developer", "Add-ins", "Excel Add-ins"),
    ("developer.comAddIns", "Developer", "Add-ins", "COM Add-ins"),
    ("developer.insert", "Developer", "Controls", "Insert"),
    ("developer.designMode", "Developer", "Controls", "Design Mode"),
    ("developer.viewCode", "Developer", "Controls", "View Code"),
    ("developer.runDialog", "Developer", "Controls", "Run Dialog"),
    ("developer.xmlSource", "Developer", "XML", "Source"),
    ("developer.xmlMapProperties", "Developer", "XML", "Map Properties"),
    ("developer.expansionPacks", "Developer", "XML", "Expansion Packs"),
    ("developer.refreshData", "Developer", "XML", "Refresh Data"),
    ("developer.importXml", "Developer", "XML", "Import"),
    ("developer.exportXml", "Developer", "XML", "Export"),
    // Table Design
    ("table.rename", "Table Design", "Properties", "Table Name"),
    ("table.resize", "Table Design", "Properties", "Resize Table"),
    ("table.summarizePivot", "Table Design", "Tools", "Summarize with PivotTable"),
    ("table.removeDuplicates", "Table Design", "Tools", "Remove Duplicates"),
    ("table.convertToRange", "Table Design", "Tools", "Convert to Range"),
    ("table.insertSlicer", "Table Design", "Tools", "Insert Slicer"),
    ("table.headerRow", "Table Design", "Table Style Options", "Header Row"),
    ("table.totalRow", "Table Design", "Table Style Options", "Total Row"),
    ("table.bandedRows", "Table Design", "Table Style Options", "Banded Rows"),
    ("table.firstColumn", "Table Design", "Table Style Options", "First Column"),
    ("table.lastColumn", "Table Design", "Table Style Options", "Last Column"),
    ("table.bandedColumns", "Table Design", "Table Style Options", "Banded Columns"),
    ("table.style", "Table Design", "Table Styles", "Table Styles"),
    // Chart Design / Format
    ("chart.addElement", "Chart Design", "Chart Layouts", "Add Chart Element"),
    ("chart.quickLayout", "Chart Design", "Chart Layouts", "Quick Layout"),
    ("chart.changeColors", "Chart Design", "Chart Styles", "Change Colors"),
    ("chart.set", "Chart Design", "Chart Styles", "Chart Styles"),
    ("chart.switchRowColumn", "Chart Design", "Data", "Switch Row/Column"),
    ("chart.selectData", "Chart Design", "Data", "Select Data"),
    ("chart.changeType", "Chart Design", "Type", "Change Chart Type"),
    ("chart.move", "Chart Design", "Location", "Move Chart"),
    ("chart.formatSelection", "Format", "Current Selection", "Format Selection"),
    ("chart.shapeFill", "Format", "Shape Styles", "Shape Fill"),
    ("chart.shapeOutline", "Format", "Shape Styles", "Shape Outline"),
    // PivotTable Analyze / Design
    ("pivot.fieldList", "PivotTable Analyze", "Show", "Field List"),
    ("pivot.refresh", "PivotTable Analyze", "Data", "Refresh"),
    ("pivot.changeSource", "PivotTable Analyze", "Data", "Change Data Source"),
    ("pivot.calculatedField", "PivotTable Analyze", "Calculations", "Fields, Items & Sets"),
    ("pivot.grandTotals", "PivotTable Design", "Layout", "Grand Totals"),
    ("pivot.subtotals", "PivotTable Design", "Layout", "Subtotals"),
    ("pivot.reportLayout", "PivotTable Design", "Layout", "Report Layout"),
    ("pivot.style", "PivotTable Design", "PivotTable Styles", "PivotTable Styles"),
    ("pivot.addField", "PivotTable Analyze", "Fields", "Add Field"),
    ("pivot.removeField", "PivotTable Analyze", "Fields", "Remove Field"),
    ("pivot.moveField", "PivotTable Analyze", "Fields", "Move Field"),
    ("pivot.valueSettings", "PivotTable Analyze", "Active Field", "Value Field Settings"),
    ("pivot.filter", "PivotTable Analyze", "Filter", "Filter"),
    ("pivot.sort", "PivotTable Analyze", "Sort", "Sort"),
    ("pivot.group", "PivotTable Analyze", "Group", "Group Field"),
    ("pivot.collapse", "PivotTable Analyze", "Active Field", "Collapse Field"),
    ("pivot.delete", "PivotTable Analyze", "Actions", "Clear All"),
];

/// Developer-tab ids published before their feature lands (#2960). They are registered as
/// [`crate::Status::NotImplemented`] commands (crates/engine/src/cmd/developer.rs), so a caller
/// gets `NOT_IMPLEMENTED` instead of "unknown command", and they count as missing in [`parity`].
pub const RESERVED: &[&str] = &[
    "developer.visualBasic",
    "developer.recordMacro",
    "developer.useRelativeReferences",
    "developer.macroSecurity",
    "developer.addIns",
    "developer.excelAddIns",
    "developer.comAddIns",
    "developer.insert",
    "developer.designMode",
    "developer.viewCode",
    "developer.runDialog",
    "developer.xmlSource",
    "developer.xmlMapProperties",
    "developer.expansionPacks",
    "developer.refreshData",
    "developer.importXml",
    "developer.exportXml",
];

/// Developer-tab rows whose command already has a home elsewhere in the ribbon, and where from.
/// They keep that id on this tab too: one convention per control, so `developer.insert` would not
/// be a second name for the Insert menu, and the pane's Properties button stays `control.format`.
pub const DEVELOPER_SHARED: &[(&str, &str)] = &[
    ("view.macros", "View › Macros (the same Macros dialog, also Alt+F8)"),
    ("insert.button", "Insert › Controls, a form button"),
    ("insert.spinButton", "Insert › Controls, a form spin button"),
    ("insert.slider", "Insert › Controls, a form scroll bar"),
    ("control.format", "Format Control › Properties, on the selected form control"),
];

/// A catalog row's label, by id (the first row with it).
pub fn label(id: &str) -> Option<&'static str> {
    CATALOG.iter().find(|e| e.0 == id).map(|e| e.3)
}

/// Whether the catalog id has a working command behind it. A reserved placeholder
/// ([`RESERVED`], `NOT_IMPLEMENTED`) does not count.
pub fn implemented(id: &str) -> bool {
    crate::find_command(id).is_some_and(|c| c.status == crate::Status::Implemented)
}

/// Distinct catalog ids.
pub fn ids() -> Vec<&'static str> {
    let mut v: Vec<&str> = CATALOG.iter().map(|e| e.0).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// (implemented, total) over distinct ids.
pub fn parity() -> (usize, usize) {
    let ids = ids();
    let done = ids.iter().filter(|id| implemented(id)).count();
    (done, ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding from the #3563 review: the Developer tab uses one id convention per control. An
    /// entry that already has a command keeps that id (`DEVELOPER_SHARED`); every other entry
    /// reserves a `developer.*` id in `RESERVED`; no `developer.*` id duplicates a live command.
    #[test]
    fn developer_rows_use_one_namespace_convention() {
        let rows: Vec<&str> = CATALOG.iter().filter(|e| e.1 == "Developer").map(|e| e.0).collect();
        for id in &rows {
            if id.starts_with("developer.") {
                assert!(RESERVED.contains(id), "{id} is a Developer row but not in RESERVED");
                let spec = crate::find_command(id).unwrap_or_else(|| panic!("{id} is not registered"));
                assert_eq!(spec.status, crate::Status::NotImplemented, "{id}");
                assert!(spec.menu.is_empty(), "{id} must not appear in a menu");
            } else {
                assert!(DEVELOPER_SHARED.iter().any(|(shared, _)| shared == id), "{id} is a Developer row but not in DEVELOPER_SHARED");
                assert!(implemented(id), "{id} is shared, so it must run");
            }
        }
        // The reverse: a reserved or shared id that names a Developer row is drawn on the tab.
        for id in RESERVED {
            assert!(rows.contains(id), "{id} is reserved but has no Developer row");
        }
        for (id, _) in DEVELOPER_SHARED {
            if CATALOG.iter().any(|e| e.0 == *id && e.1 == "Developer") {
                assert!(rows.contains(id), "{id} is shared with the Developer tab but missing from it");
            }
        }
        // Macros is the tab's one live control and keeps its View id, so its catalog row is
        // under View rather than Developer.
        assert!(DEVELOPER_SHARED.iter().any(|(id, _)| *id == "view.macros"));
        assert!(implemented("view.macros"));
        // A reserved id answers its own error code, not "unknown command".
        let mut s = crate::Session::new();
        s.new_workbook();
        for id in RESERVED {
            let e = s.execute(id, serde_json::json!({})).unwrap_err();
            assert!(matches!(e, crate::EngineError::NotImplemented(_)), "{id}: {e}");
            assert!(e.to_string().starts_with("NOT_IMPLEMENTED: "), "{id}: {e}");
            let info = s.commands().into_iter().find(|c| c.id == *id).expect("listed");
            assert!(!info.enabled && info.status == crate::Status::NotImplemented, "{id}");
        }
    }

    /// A reserved id is published but not implemented: it must not move the parity metric.
    #[test]
    fn reserved_ids_count_as_missing() {
        for id in RESERVED {
            assert!(CATALOG.iter().any(|e| e.0 == *id), "{id} has no catalog row");
            assert!(!implemented(id), "{id} is reserved and must not count as implemented");
            assert!(ids().contains(id), "{id} is not a distinct catalog id");
        }
    }
}

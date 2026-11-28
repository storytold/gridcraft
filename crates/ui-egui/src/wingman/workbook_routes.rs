//! Descriptive routes, never execution permissions. Each direct adapter still owns its schema
//! and guards. Equivalents describe a workbook result, not OS clipboard or pointer automation.
pub fn route<'a>(id: &str, adapter: Option<&'a str>) -> (&'static str, Option<&'a str>) {
    if let Some(tool) = adapter {
        return ("direct", Some(tool));
    }
    match id {
        "home.fontName"
        | "home.fontSize"
        | "home.bold"
        | "home.italic"
        | "home.underline"
        | "home.doubleUnderline"
        | "home.strikethrough"
        | "home.superscript"
        | "home.subscript"
        | "home.alignTop"
        | "home.alignMiddle"
        | "home.alignBottom"
        | "home.alignLeft"
        | "home.alignCenter"
        | "home.alignRight"
        | "home.wrapText"
        | "home.orientation" => ("equivalent", Some("format_range")),
        "home.accounting" | "home.percent" | "home.comma" => ("equivalent", Some("number_format")),
        "home.mergeCenter" => ("equivalent", Some("apply_edits")),
        "home.mergeAcross" => ("equivalent", Some("apply_edits")),
        "home.increaseIndent" | "home.decreaseIndent" | "home.increaseFontSize" | "home.decreaseFontSize" | "home.justify" => {
            ("equivalent", Some("format_range"))
        }
        "home.increaseDecimal" | "home.decreaseDecimal" => ("equivalent", Some("number_format")),
        "edit.copy" | "edit.cut" => ("equivalent", Some("transfer_range")),
        "edit.fillUp" | "edit.fillLeft" | "edit.fillSeries" | "dialog:series" => ("equivalent", Some("drag_fill")),
        "formulas.autoSum" => ("equivalent", Some("set_cell")),
        "data.textToColumns" | "dialog:textToColumns" => ("equivalent", Some("reshape_delimited")),
        "dialog:dataValidation" => ("equivalent", Some("set_validation")),
        "home.manageRules" | "dialog:manageRules" => ("inspect", Some("inspect_rules")),
        "dialog:newFormattingRule" => ("equivalent", Some("conditional_format")),
        "dialog:renameSheet" => ("equivalent", Some("rename_sheet")),
        "home.formatAsTable" => ("equivalent", Some("apply_edits")),
        "edit.goTo" | "dialog:goTo" => ("equivalent", Some("select_range")),
        "view.freezeTopRow" | "view.freezeFirstColumn" => ("equivalent", Some("freeze_panes")),
        "pageLayout.orientation"
        | "pageLayout.size"
        | "pageLayout.printArea"
        | "pageLayout.printGridlines"
        | "pageLayout.printHeadings"
        | "pageLayout.scaleToFit" => ("equivalent", Some("set_page_layout")),
        "pivot.subtotals" | "pivot.reportLayout" => ("equivalent", Some("set_pivot_layout")),
        "review.showComments" | "review.showAllNotes" => ("inspect", Some("list_objects")),
        "review.showNote" => ("inspect", Some("inspect_cell")),
        "table.resize" | "dialog:tableResize" => ("inspect", Some("inspect_table")),
        "table.summarizePivot" => ("equivalent", Some("create_pivot")),
        "view.macros" => ("inspect", Some("list_scripts")),
        "draw.pen" => ("equivalent", Some("add_ink_stroke")),
        "draw.lasso" => ("inspect", Some("inspect_shapes")),
        "chart.changeType" | "dialog:chartTitle" | "chartTitle" => ("equivalent", Some("update_chart")),
        "chart.selectData" | "dialog:selectData" | "chart.addElement" => ("inspect", Some("inspect_chart")),
        "chart.formatSelection" => ("inspect", Some("inspect_chart")),
        "commandSearch" | "dialog:commandSearch" => ("inspect", Some("find_commands")),
        "data.sortAscending" | "data.sortDescending" => ("equivalent", Some("sort_rows")),
        "insert.recommendedCharts" | "dialog:recommendedCharts" => ("inspect", Some("recommend_charts")),
        "insert.textBox" => ("equivalent", Some("create_shape")),
        "edit.find" | "home.find" | "dialog:find" => ("inspect", Some("grep")),
        "formulas.tracePrecedents" | "formulas.traceDependents" | "formulas.evaluateFormula" | "dialog:evaluateFormula" => {
            ("inspect", Some("inspect_formula"))
        }
        "formulas.errorChecking" => ("inspect", Some("formula_diagnostics")),
        "formulas.insertFunction" | "dialog:insertFunction" => ("inspect", Some("find_functions")),
        "formulas.financial"
        | "formulas.logical"
        | "formulas.dateTime"
        | "formulas.lookupReference"
        | "formulas.mathTrig"
        | "formulas.moreFunctions"
        | "formulas.text" => ("inspect", Some("find_functions")),
        "dialog:defineName" => ("equivalent", Some("define_range")),
        "formulas.nameManager" | "dialog:nameManager" => ("inspect", Some("list_objects")),
        "automate.allScripts" | "dialog:macros" => ("inspect", Some("list_scripts")),
        "dialog:formatCells" | "ui:formatCells" => ("equivalent", Some("format_range")),
        "ui:columnWidth" | "dialog:columnWidth" => ("equivalent", Some("size_columns")),
        "ui:rowHeight" | "dialog:rowHeight" => ("equivalent", Some("size_rows")),
        "dialog:newComment" => ("equivalent", Some("set_comment")),
        "dialog:sort" => ("equivalent", Some("sort_rows")),
        _ => ("guide", None),
    }
}

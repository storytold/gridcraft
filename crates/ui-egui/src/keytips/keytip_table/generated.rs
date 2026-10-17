//! GENERATED from keytips.json by `cargo xtask keytips`. Edit the JSON, not this file.

use super::{Action, Entry};

/// All known keytip sequences. Intermediate levels are implicit: the matcher waits
/// whenever a pressed letter starts a deeper sequence, so only terminal entries are
/// listed here. See keytip_table.rs for the helpers that read this table.
pub const SEQUENCES: &[(&str, Entry)] = &[
    ("H", Entry::Tab("Home")),
    ("N", Entry::Tab("Insert")),
    ("JI", Entry::Tab("Draw")), // Two-letter unit. Deeper Draw letters differ between Excel and GridCraft; left blank for future contributors.
    ("P", Entry::Tab("Page Layout")),
    ("M", Entry::Tab("Formulas")),
    ("A", Entry::Tab("Data")),
    ("R", Entry::Tab("Review")),
    ("W", Entry::Tab("View")),
    ("E", Entry::Tab("Home")), // Legacy Edit-menu path (Excel 2003); leads to the same Home menus.
    ("O", Entry::Tab("Home")), // Legacy Format-menu path (Excel 2003); leads to the same Home menus.
    ("HAR", Entry::Finish(Action::Command("home.alignRight"))),
    ("HAT", Entry::Finish(Action::Command("home.alignTop"))),    // Excel: Excel: Top Align
    ("HAM", Entry::Finish(Action::Command("home.alignMiddle"))), // Excel: Excel: Middle Align
    ("HAB", Entry::Finish(Action::Command("home.alignBottom"))), // Excel: Excel: Bottom Align
    ("HAL", Entry::Finish(Action::Command("home.alignLeft"))),
    ("HAC", Entry::Finish(Action::Command("home.alignCenter"))),
    ("HVS", Entry::Finish(Action::Dialog("pasteSpecial"))),
    ("ES", Entry::Finish(Action::Dialog("pasteSpecial"))), // Excel: Classic Paste Special dialog (legacy Edit route); A/F/T/V are dialog mnemonics
    ("HOI", Entry::Finish(Action::Command("home.autofitColumnWidth"))),
    ("OCA", Entry::Finish(Action::Command("home.autofitColumnWidth"))), // Legacy Format menu.
    ("WVG", Entry::Finish(Action::Command("view.gridlines"))),
    ("H0", Entry::Finish(Action::Command("home.increaseDecimal"))), // Excel: Increase Decimals; containment(Increase Decimal)
    ("H1", Entry::Finish(Action::Command("home.bold"))),            // Excel: Bold
    ("H2", Entry::Finish(Action::Command("home.italic"))),          // Excel: Italic
    ("H3", Entry::Finish(Action::Command("home.underline"))),       // Excel: Excel: Underline; fires immediately (not a menu)
    ("H5", Entry::Finish(Action::Command("home.decreaseIndent"))),  // Excel: Decrease Indent
    ("H6", Entry::Finish(Action::Command("home.increaseIndent"))),  // Excel: Increase Indent
    ("H9", Entry::Finish(Action::Command("home.decreaseDecimal"))), // Excel: Decrease Decimals; containment(Decrease Decimal)
    ("HAN", Entry::Finish(Action::Command("home.accounting"))), // Excel: Accounting Format; fires immediately in Excel; deeper aliases dropped as unreachable
    ("HAS", Entry::Finish(Action::Command("home.addIns"))),     // Excel: Add-ins; containment(Add-ins)
    ("HB", Entry::Continue),                                    // Excel: Borders Menu; group level:
    ("HBA", Entry::Finish(Action::Command("home.borders"))),    // Excel: All Borders; containment(Borders)
    ("HBB", Entry::Finish(Action::Command("home.borders"))),    // Excel: Bottom Double Border; border-side TBD
    ("HBC", Entry::Finish(Action::Command("home.borders"))),    // Excel: Top and Thick Bottom Border; border-side TBD
    ("HBD", Entry::Finish(Action::Command("home.borders"))),    // Excel: Top and Bottom Border; border-side TBD
    ("HBE", Entry::Finish(Action::Command("home.borders"))),    // Excel: Erase Border; border-side TBD
    ("HBG", Entry::Finish(Action::Command("home.drawBorder"))), // Excel: Draw Border Grid; containment(Draw Border)
    ("HBH", Entry::Finish(Action::Command("home.borders"))),    // Excel: Thick Bottom Border; border-side TBD
    ("HBI", Entry::Finish(Action::Command("home.borders"))),    // Excel: Line Color; Line Color (submenu TBD)
    ("HBL", Entry::Finish(Action::Command("home.borders"))),    // Excel: Left Border; border-side TBD
    ("HBM", Entry::Finish(Action::Command("home.borders"))),    // Excel: More Borders; containment(Borders)
    ("HBN", Entry::Finish(Action::Command("home.borders"))),    // Excel: No Border; border-side TBD
    ("HBO", Entry::Finish(Action::Command("home.borders"))),    // Excel: Bottom Border; border-side TBD
    ("HBP", Entry::Finish(Action::Command("home.borders"))),    // Excel: Top Border; border-side TBD
    ("HBR", Entry::Finish(Action::Command("home.borders"))),    // Excel: Right Border; border-side TBD
    ("HBS", Entry::Finish(Action::Command("home.borders"))),    // Excel: Outside Borders; containment(Borders)
    ("HBT", Entry::Finish(Action::Command("home.borders"))),    // Excel: Thick Outside Borders; border-side TBD
    ("HBU", Entry::Finish(Action::Command("home.borders"))),    // Excel: Top and Double Bottom Border; border-side TBD
    ("HBW", Entry::Finish(Action::Command("home.drawBorder"))), // Excel: Draw Border
    ("HBY", Entry::Finish(Action::Command("home.borders"))),    // Excel: Line Style; Line Style (submenu TBD)
    ("HC", Entry::Finish(Action::Command("edit.copy"))),        // Excel: Copy; fires immediately in Excel; deeper aliases dropped as unreachable
    ("HDC", Entry::Finish(Action::Command("home.deleteColumns"))), // Excel: Delete Columns; Delete Columns
    ("HDD", Entry::Finish(Action::Dialog("deleteCells"))),      // Excel: Delete Cells
    ("HDR", Entry::Finish(Action::Command("home.deleteRows"))), // Excel: Delete Rows; Delete Rows
    ("HDS", Entry::Finish(Action::Command("home.deleteSheet"))), // Excel: Delete Sheet
    ("HEA", Entry::Finish(Action::Command("edit.clearAll"))),   // Excel: Clear All
    ("HEC", Entry::Finish(Action::Command("edit.clearContents"))), // Excel: Clear Contents
    ("HEF", Entry::Finish(Action::Command("edit.clearFormats"))), // Excel: Clear Formats
    ("HEL", Entry::Finish(Action::Command("edit.clearHyperlinks"))), // Excel: Clear Hyperlinks
    ("HEM", Entry::Finish(Action::Command("edit.clearComments"))), // Excel: Clear Comments and Notes; containment(Clear Comments and Notes)
    ("HFC", Entry::Finish(Action::Control("home.fontColor"))),  // Excel: Font Color; opens the font-color palette
    ("HFDC", Entry::Finish(Action::Command("home.conditionalFormat"))), // Excel: Find Conditional Formatting; containment(Conditional Formatting)
    ("HFDF", Entry::Finish(Action::Dialog("find"))),            // Excel: Find
    ("HFDG", Entry::Finish(Action::Dialog("goTo"))),            // Excel: Go To
    ("HFDM", Entry::Finish(Action::Command("edit.find"))),      // Excel: Find Notes; containment(Find)
    ("HFDO", Entry::Finish(Action::Pending("home.selectObjects"))), // pending; Excel: Select Objects; Select Objects; reserved until the command lands
    ("HFDP", Entry::Finish(Action::Command("arrange.selectionPane"))), // Excel: Selection Pane
    ("HFDR", Entry::Finish(Action::Dialog("find"))),                // Excel: Replace; opens Find and Replace
    ("HFDS", Entry::Finish(Action::Dialog("goToSpecial"))),         // Excel: Go To Special
    ("HFF", Entry::Finish(Action::Control("home.fontName"))),       // Excel: Font Selection; opens the font dropdown
    ("HFG", Entry::Finish(Action::Command("home.increaseFontSize"))), // Excel: Increase Font Size
    ("HFI", Entry::Continue),                                       // Excel: Fill; group level: containment(Fill Up)
    ("HFIA", Entry::Finish(Action::Pending("home.fillAcrossWorksheets"))), // pending; Excel: Fill Across Worksheets; Fill Across Worksheets; reserved until the command lands
    ("HFID", Entry::Finish(Action::Command("edit.fillDown"))),             // Excel: Fill Down
    ("HFIF", Entry::Finish(Action::Command("edit.flashFill"))),            // Excel: Flash Fill
    ("HFIJ", Entry::Finish(Action::Command("home.justify"))),              // Excel: Fill Justify; containment(Justify)
    ("HFIL", Entry::Finish(Action::Command("edit.fillLeft"))),             // Excel: Fill Left
    ("HFIR", Entry::Finish(Action::Command("edit.fillRight"))),            // Excel: Fill Right
    ("HFIS", Entry::Finish(Action::Dialog("series"))),                     // Excel: Fill Series; containment(Series)
    ("HFIU", Entry::Finish(Action::Command("edit.fillUp"))),               // Excel: Fill Up
    ("HFK", Entry::Finish(Action::Command("home.decreaseFontSize"))),      // Excel: Decrease Font Size
    ("HFM", Entry::Finish(Action::Dialog("formatCells"))),                 // Excel: More Number Formats; More Number Formats opens Format Cells
    ("HFP", Entry::Finish(Action::Command("edit.formatPainter"))),         // Excel: Format Painter
    ("HFQ", Entry::Finish(Action::Control("home.orientation"))),           // Excel: Orientation Menu; group level: containment(Orientation)
    ("HFS", Entry::Finish(Action::Control("home.fontSize"))),              // Excel: Font Size; opens the font-size dropdown
    ("HH", Entry::Finish(Action::Control("home.fillColor"))),              // Excel: Fill Color; opens the fill-color palette
    ("HIC", Entry::Finish(Action::Command("home.insertColumns"))),         // Excel: Insert Columns; Insert Columns
    ("HIE", Entry::Finish(Action::Dialog("insertCells"))),                 // Excel: Insert Copied Cells; Insert Cells (copied cells TBD)
    ("HII", Entry::Finish(Action::Dialog("insertCells"))),                 // Excel: Insert Cells
    ("HIR", Entry::Finish(Action::Command("home.insertRows"))),            // Excel: Insert Rows; Insert Rows
    ("HIS", Entry::Finish(Action::Command("home.insertSheet"))),           // Excel: Insert Sheet
    ("HJ", Entry::Continue),                                               // Excel: Cell Styles; group level:
    ("HJN", Entry::Finish(Action::Command("home.newCellStyle"))),          // Excel: New Cell Style
    ("HK", Entry::Finish(Action::Command("home.comma"))),                  // Excel: Comma Style
    ("HL", Entry::Continue),  // Excel: Conditional Formatting; group level: containment(Conditional Formatting)
    ("HLC", Entry::Continue), // Excel: Clear Rules Menu; group level: containment(Clear Rules)
    ("HLCE", Entry::Finish(Action::Command("home.clearRules"))), // Excel: Clear Rules From Sheet; containment(Clear Rules)
    ("HLCS", Entry::Finish(Action::Command("home.clearRules"))), // Excel: Clear Rules From Cells; containment(Clear Rules)
    ("HLCT", Entry::Finish(Action::Command("home.clearRules"))), // Excel: Clear Rules From Table; containment(Clear Rules)
    ("HLD", Entry::Finish(Action::Command("home.conditionalFormat"))), // Excel: Data Bars; Data Bars (submenu TBD)
    ("HLH", Entry::Finish(Action::Command("home.conditionalFormat"))), // Excel: Highlight Cells; Highlight Cells (submenu TBD)
    ("HLN", Entry::Finish(Action::Command("home.conditionalFormat"))), // Excel: New Rule; New Rule (submenu TBD)
    ("HLR", Entry::Finish(Action::Command("home.manageRules"))), // Excel: Manage Rules; containment(Manage Rules)
    ("HLT", Entry::Finish(Action::Command("home.conditionalFormat"))), // Excel: Top/Bottom Rules; Top/Bottom (submenu TBD)
    ("HM", Entry::Continue),  // Excel: Merge & Center Menu; group level: containment(Merge & Center)
    ("HMA", Entry::Finish(Action::Command("home.mergeAcross"))), // Excel: Merge Across
    ("HMC", Entry::Finish(Action::Command("home.mergeCenter"))), // Excel: Merge & Center
    ("HMM", Entry::Finish(Action::Command("home.mergeCells"))), // Excel: Merge Cells
    ("HMU", Entry::Finish(Action::Command("home.unmergeCells"))), // Excel: Unmerge Cells
    ("HN", Entry::Finish(Action::Control("home.numberFormat"))), // Excel: Number Format; opens the number-format dropdown
    ("HOA", Entry::Finish(Action::Command("home.autofitRowHeight"))), // Excel: AutoFit Row Height
    ("HOD", Entry::Finish(Action::Dialog("defaultWidth"))), // Excel: Default Column Width; containment(Column Width)
    ("HOE", Entry::Finish(Action::Command("home.formatCells"))), // Excel: Format Cells
    ("HOH", Entry::Finish(Action::Dialog("rowHeight"))), // Excel: Row Height
    ("HOL", Entry::Finish(Action::Command("home.lockCell"))), // Excel: Lock Cell; containment(Lock Cell)
    ("HOM", Entry::Finish(Action::Dialog("moveSheet"))), // Excel: Move or Copy Sheet
    ("HOP", Entry::Finish(Action::Dialog("protectSheet"))), // Excel: Protect Sheet
    ("HOR", Entry::Finish(Action::Dialog("renameSheet"))), // Excel: Rename Sheet
    ("HOT", Entry::Finish(Action::Command("sheet.tabColor"))), // Excel: Tab Color
    ("HOUC", Entry::Finish(Action::Command("home.hideColumns"))), // Excel: Hide Columns
    ("HOUH", Entry::Finish(Action::Command("sheet.unhide"))), // Excel: Unhide Sheet
    ("HOUL", Entry::Finish(Action::Command("home.unhideColumns"))), // Excel: Unhide Columns
    ("HOUO", Entry::Finish(Action::Command("home.unhideRows"))), // Excel: Unhide Rows
    ("HOUR", Entry::Finish(Action::Command("home.hideRows"))), // Excel: Hide Rows
    ("HOUS", Entry::Finish(Action::Command("sheet.hide"))), // Excel: Hide Sheet
    ("HOW", Entry::Finish(Action::Dialog("columnWidth"))), // Excel: Column Width
    ("HP", Entry::Finish(Action::Command("home.percent"))), // Excel: Excel: Percentage Style
    ("HSC", Entry::Finish(Action::Command("data.clearFilter"))), // Excel: Clear Filter
    ("HSF", Entry::Finish(Action::Command("data.filter"))), // Excel: Filter
    ("HSO", Entry::Finish(Action::Command("data.sortDescending"))), // Excel: Sort Z to A
    ("HSS", Entry::Finish(Action::Command("data.sortAscending"))), // Excel: Sort A to Z
    ("HSU", Entry::Finish(Action::Command("data.sort"))), // Excel: Custom Sort
    ("HT", Entry::Continue),  // Excel: Format as Table; group level:
    ("HTN", Entry::Finish(Action::Command("table.style"))), // Excel: New Table Style
    ("HTP", Entry::Finish(Action::Command("table.style"))), // Excel: New Pivot Table Style
    ("HU", Entry::Continue),  // Excel: AutoSum; group level:
    ("HUA", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Auto Average; AutoSum: Average (variant TBD)
    ("HUC", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Auto Count Numbers; AutoSum: Count (variant TBD)
    ("HUI", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Auto Min; AutoSum: Min (variant TBD)
    ("HUM", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Auto Max; AutoSum: Max (variant TBD)
    ("HV", Entry::Continue),  // Excel: Paste; group level:
    ("HVP", Entry::Finish(Action::Command("edit.paste"))), // Excel: Paste
    ("HW", Entry::Continue),  // Excel: Wrap Text Menu; group level: containment(Wrap Text)
    ("HWW", Entry::Finish(Action::Command("home.wrapText"))), // Excel: Wrap Text
    ("HX", Entry::Finish(Action::Command("edit.cut"))), // Excel: Cut
    ("NC1", Entry::Finish(Action::Control("insert.chart.column"))), // Excel: Column Chart; Column chart (kind TBD)
    ("NC2", Entry::Finish(Action::Command("review.newComment"))), // Excel: New Comment
    ("NCM", Entry::Finish(Action::Dialog("comment"))), // Excel: Excel: New Comment (modern)
    ("NCB", Entry::Finish(Action::Command("insert.checkbox"))), // Excel: Checkbox
    ("ND", Entry::Finish(Action::Control("insert.chart.scatter"))), // Excel: Scatter Chart; Scatter (kind TBD)
    ("NE", Entry::Finish(Action::Pending("insert.equation"))), // pending; Excel: Equations; Equations; reserved until the command lands
    ("NF", Entry::Continue),  // Excel: Online Pictures; group level: containment(Picture)
    ("NFP", Entry::Continue), // Excel: From Picture Menu; group level: containment(Picture)
    ("NFPF", Entry::Finish(Action::Command("insert.picture"))), // Excel: Picture From File; containment(Picture)
    ("NH1", Entry::Finish(Action::Dialog("headerFooter"))), // Excel: Header & Footer
    ("NHF", Entry::Finish(Action::Command("pageLayout.headerFooter"))), // Excel: Header & Footer
    ("NHI", Entry::Finish(Action::Control("insert.chart.hierarchy"))), // Excel: Hierarchy Chart; Hierarchy (kind TBD)
    ("NI1", Entry::Finish(Action::Command("insert.chart"))), // Excel: Waterfall Chart; Waterfall (kind TBD)
    ("NI2", Entry::Finish(Action::Dialog("insertLink"))), // Excel: Link
    ("NJ", Entry::Finish(Action::Pending("insert.object"))), // pending; Excel: Object; insert.object missing; reserved until the command lands
    ("NM1T", Entry::Finish(Action::Command("insert.picture"))), // Excel: SmartArt Picture; containment(Picture)
    ("NM2", Entry::Finish(Action::Command("insert.chart"))), // Excel: Map Chart; Map (kind TBD)
    ("NN1", Entry::Finish(Action::Control("insert.chart.line"))), // Excel: Line Chart; Line chart (kind TBD)
    ("NNS", Entry::Finish(Action::Command("insert.icons"))), // Excel: Icons
    ("NP", Entry::Continue),  // Excel: Picture; group level:
    ("NPB", Entry::Finish(Action::Command("insert.picture"))), // Excel: Picture from File; containment(Picture)
    ("NPF", Entry::Finish(Action::Command("insert.picture"))), // Excel: Place in Cell; Picture in cell (source TBD)
    ("NPI", Entry::Finish(Action::Command("insert.picture"))), // Excel: Place in Cell Menu; Picture in cell (source TBD)
    ("NPO", Entry::Finish(Action::Command("insert.picture"))), // Excel: Place over Cells Menu; Picture over cells (source TBD)
    ("NQ", Entry::Finish(Action::Control("insert.chart.pie"))), // Excel: Pie Chart; Pie (kind TBD)
    ("NR", Entry::Finish(Action::Command("insert.recommendedCharts"))), // Excel: Recommended Charts
    ("NSA", Entry::Finish(Action::Control("insert.chart.statistic"))), // Excel: Statistical Chart; Statistical chart (kind TBD)
    ("NSC", Entry::Finish(Action::Pending("insert.screenshot"))), // pending; Excel: Screenshot Menu; Screenshot; reserved until the command lands
    ("NSD", Entry::Finish(Action::Command("insert.chart"))), // Excel: Combo Chart; Combo (kind TBD)
    ("NSF", Entry::Finish(Action::Command("insert.slicer"))), // Excel: Slicer; insert.slicer missing (gap #8); reserved until the command lands
    ("NSH", Entry::Finish(Action::Command("insert.shape"))), // Excel: Shapes
    ("NSP", Entry::Finish(Action::Command("insert.recommendedPivotTables"))), // Excel: Recommended PivotTables
    ("NST", Entry::Finish(Action::Pending("insert.timeline"))), // pending; Excel: Timeline; insert.timeline missing (gap #8); reserved until the command lands
    ("NSZ", Entry::Finish(Action::Command("insert.pivotChart"))), // Excel: PivotChart; PivotChart; reserved until the command lands
    ("NT", Entry::Finish(Action::Command("insert.table"))),     // Excel: Table
    ("NU", Entry::Finish(Action::Command("insert.symbol"))),    // Excel: Symbols; containment(Symbol)
    ("NZS", Entry::Finish(Action::Command("insert.symbol"))),   // Excel: Excel: Symbol (modern)
    ("NV", Entry::Finish(Action::Command("insert.pivotTable"))), // Excel: PivotTable
    ("NW", Entry::Finish(Action::Pending("insert.wordArt"))),   // pending; Excel: WordArt; WordArt; reserved until the command lands
    ("NX", Entry::Finish(Action::Command("insert.textBox"))),   // Excel: Text Box
    ("NZK", Entry::Continue),                                   // Excel: Sparklines Menu; group level:
    ("NZKSL", Entry::Finish(Action::Dialog("sparkline"))),      // Excel: Line Sparkline
    ("NZKSO", Entry::Finish(Action::Dialog("sparkline"))),      // Excel: Column Sparkline
    ("NZKSW", Entry::Finish(Action::Dialog("sparkline"))),      // Excel: Win/Loss Sparkline
    ("NZTX", Entry::Finish(Action::Command("insert.textBox"))), // Excel: Text Box
    ("NZT", Entry::Finish(Action::Command("insert.textBox"))),  // Excel: Excel: Text Box (modern)
    ("PAE", Entry::Finish(Action::Command("arrange.sendBackward"))), // Excel: Send Backward
    ("PAF", Entry::Finish(Action::Command("arrange.bringForward"))), // Excel: Bring Forward
    ("PB", Entry::Continue),                                    // Excel: Breaks; group level:
    ("PBI", Entry::Finish(Action::Command("pageLayout.breaks"))), // Excel: Insert Page Break; Insert Page Break
    ("PBR", Entry::Finish(Action::Command("pageLayout.breaks"))), // Excel: Remove Page Break; Remove Page Break
    ("PGB", Entry::Finish(Action::Command("pageLayout.background"))), // Excel: Background; Excel badge GB (Background in the Page Setup group)
    ("PI", Entry::Finish(Action::Command("pageLayout.printTitles"))), // Excel: Print Titles
    ("PM", Entry::Continue),                                    // Excel: Margins; group level:
    ("PMA", Entry::Finish(Action::Command("pageLayout.margins"))), // Excel: Custom Margins; containment(Margins)
    ("PO", Entry::Finish(Action::Command("pageLayout.orientation"))), // Excel: Orientation
    ("PPG", Entry::Finish(Action::Command("pageLayout.printGridlines"))), // Excel: Print Gridlines
    ("PPH", Entry::Finish(Action::Command("pageLayout.printHeadings"))), // Excel: Print Headings
    ("PR", Entry::Continue),                                    // Excel: Print Area; group level:
    ("PRS", Entry::Finish(Action::Command("pageLayout.printArea"))), // Excel: Set Print Area
    ("PSP", Entry::Finish(Action::Dialog("pageSetup"))),        // Excel: Page Setup
    ("PSZ", Entry::Finish(Action::Command("pageLayout.size"))), // Excel: Size
    ("PTC", Entry::Finish(Action::Command("pageLayout.themeColors"))), // Excel: Theme Colors; containment(Colors)
    ("PTE", Entry::Finish(Action::Command("pageLayout.themeEffects"))), // Excel: Theme Effects; containment(Effects)
    ("PTF", Entry::Finish(Action::Command("pageLayout.themeFonts"))), // Excel: Theme Fonts; containment(Fonts)
    ("PTH", Entry::Continue),                                   // Excel: Themes; group level:
    ("PTHB", Entry::Finish(Action::Command("pageLayout.theme"))), // Excel: Browse Themes; containment(Themes)
    ("PVG", Entry::Finish(Action::Command("view.gridlines"))),  // Excel: View Gridlines
    ("PVH", Entry::Finish(Action::Command("view.headings"))),   // Excel: View Headings
    ("MA", Entry::Continue),                                    // Excel: Remove Arrows Menu; group level: containment(Remove Arrows)
    ("MAA", Entry::Finish(Action::Command("formulas.removeArrows"))), // Excel: Remove All Arrows; landed in the catalog; live since T7 probe
    ("MAP", Entry::Finish(Action::Pending("formulas.removePrecedentArrows"))), // pending; Excel: Remove Precedent Arrows; Remove Precedent Arrows; reserved until the command lands
    ("MB", Entry::Finish(Action::Command("formulas.calculateNow"))),           // Excel: Calculate Now
    ("MC", Entry::Finish(Action::Command("formulas.createFromSelection"))),    // Excel: Create From Selection
    ("MD", Entry::Finish(Action::Command("formulas.traceDependents"))),        // Excel: Trace Dependents
    ("ME", Entry::Finish(Action::Control("formulas.dateTime"))),               // Excel: Date & Time Menu; group level: containment(Date & Time)
    ("MF", Entry::Finish(Action::Dialog("insertFunction"))),                   // Excel: Insert Function
    ("MG", Entry::Finish(Action::Control("formulas.mathTrig"))),               // Excel: Math & Trig Menu; group level: containment(Math & Trig)
    ("MH", Entry::Finish(Action::Command("formulas.showFormulas"))),           // Excel: Show Formulas
    ("MI", Entry::Continue),                                                   // Excel: Financial Menu; group level: containment(Financial)
    ("MI1", Entry::Finish(Action::Control("formulas.financial"))),             // Excel: Excel: Financial; first item of the dropdown
    ("MJ", Entry::Finish(Action::Command("formulas.calculateSheet"))),         // Excel: Calculate Sheet
    ("MK", Entry::Continue),                                                   // Excel: Error Checking Menu; group level: containment(Error Checking)
    ("MKK", Entry::Finish(Action::Dialog("errorChecking"))),                   // Excel: Error Checking
    ("ML", Entry::Finish(Action::Control("formulas.logical"))),                // Excel: Logical Menu; group level: containment(Logical)
    ("MM", Entry::Finish(Action::Dialog("defineName"))),                       // Excel: Define Name
    ("MN", Entry::Finish(Action::Dialog("nameManager"))),                      // Excel: Name Manager
    ("MO", Entry::Finish(Action::Control("formulas.lookup"))), // Excel: Lookup & Reference Menu; group level: containment(Lookup & Reference)
    ("MP", Entry::Finish(Action::Command("formulas.tracePrecedents"))), // Excel: Trace Precedents
    ("MQ", Entry::Finish(Action::Control("formulas.moreFunctions"))), // Excel: More Functions Menu; group level: containment(More Functions)
    ("MS", Entry::Continue),                                   // Excel: Use in Formula Menu; group level: containment(Use in Formula)
    ("MSP", Entry::Finish(Action::Pending("formulas.pasteName"))), // pending; Excel: Paste Names; reserved until the command lands
    ("MT", Entry::Finish(Action::Control("formulas.text"))),   // Excel: Text Menu; group level: containment(Text)
    ("MU", Entry::Continue),                                   // Excel: AutoSum Menu; group level:
    ("MUA", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Average; AutoSum: Average (variant TBD)
    ("MUC", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Count Numbers; AutoSum: Count (variant TBD)
    ("MUF", Entry::Finish(Action::Command("formulas.moreFunctions"))), // Excel: More Functions (AutoSum); containment(More Functions)
    ("MUI", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Min; AutoSum: Min (variant TBD)
    ("MUM", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Max; AutoSum: Max (variant TBD)
    ("MUS", Entry::Finish(Action::Command("formulas.autoSum"))), // Excel: Sum
    ("MV", Entry::Finish(Action::Dialog("evaluateFormula"))),  // Excel: Evaluate Formula
    ("MW", Entry::Finish(Action::Command("formulas.watchWindow"))), // Excel: Watch Window
    ("MX", Entry::Continue),                                   // Excel: Calculation Options Menu; group level: containment(Calculation Options)
    ("MXA", Entry::Finish(Action::Command("formulas.calculationOptions"))), // Excel: Automatic Calculation; Calculation options: Automatic
    ("MXM", Entry::Finish(Action::Command("formulas.calculationOptions"))), // Excel: Manual Calculation; Calculation options: Manual
    ("MXP", Entry::Finish(Action::Command("formulas.calculationOptions"))), // Excel: Partial Calculation; Calculation options: Partial
    ("AB", Entry::Finish(Action::Dialog("subtotal"))),         // Excel: Subtotal
    ("AC", Entry::Finish(Action::Command("data.clearFilter"))), // Excel: Clear Filter
    ("AD", Entry::Finish(Action::Command("data.consolidate"))), // Excel: Consolidate
    ("AE", Entry::Finish(Action::Dialog("textToColumns"))),    // Excel: Text to Columns
    ("AF1", Entry::Continue), // Excel: Data from Picture Menu; group level: feature gap; reserved until the command lands
    ("AF1C", Entry::Finish(Action::Command("insert.picture"))), // Excel: Picture from Clipboard (source TBD)
    ("AF1P", Entry::Finish(Action::Command("insert.picture"))), // Excel: Picture from File (source TBD)
    ("AFA", Entry::Finish(Action::Pending("data.fromAccess"))), // pending; Excel: Get External Data: From Access; gap #12 Get & Transform; reserved until the command lands
    ("AFC", Entry::Finish(Action::Command("data.forecastSheet"))), // Excel: Forecast Sheet; IN FLIGHT: vangel76 PR #325; reserved until the command lands
    ("AFE", Entry::Finish(Action::Pending("data.analyzeData"))),   // pending; Excel: Analyze Data; AI feature GAP; reserved until the command lands
    ("AFF", Entry::Finish(Action::Command("edit.flashFill"))),     // Excel: Flash Fill
    ("AFO", Entry::Finish(Action::Pending("data.fromOtherSources"))), // pending; Excel: Get External Data: From Other Sources; gap #12; reserved until the command lands
    ("AFT", Entry::Finish(Action::Pending("data.fromText"))), // pending; Excel: Get External Data: From Text; gap #12; reserved until the command lands
    ("AFW", Entry::Finish(Action::Pending("data.fromWeb"))),  // pending; Excel: Get External Data: From Web; reserved until the command lands
    ("AG", Entry::Continue),                                  // Excel: Group Menu; group level: containment(Group)
    ("AGA", Entry::Finish(Action::Pending("data.autoOutline"))), // pending; Excel: Auto Outline; Auto Outline; reserved until the command lands
    ("AGG", Entry::Finish(Action::Command("data.group"))),    // Excel: Group
    ("AH", Entry::Finish(Action::Command("data.hideDetail"))), // Excel: Hide Detail
    ("AJ", Entry::Finish(Action::Command("data.showDetail"))), // Excel: Show Detail
    ("AK", Entry::Finish(Action::Command("data.queriesConnections"))), // Excel: Edit Links; Edit Links (Queries & Connections);
    ("AM", Entry::Finish(Action::Dialog("removeDuplicates"))), // Excel: Remove Duplicates
    ("AO", Entry::Finish(Action::Command("data.queriesConnections"))), // Excel: Queries & Connections
    ("AP1", Entry::Finish(Action::Command("file.properties"))), // Excel: Properties
    ("APN", Entry::Continue),                                 // Excel: Get Data (Power Query) Menu; group level: Power Query variants = GAP
    ("APND", Entry::Finish(Action::Pending("data.fromDatabase"))), // pending; Excel: From Database (Microsoft Query); gap #12; reserved until the command lands
    ("APNE", Entry::Finish(Action::Pending("data.powerQueryEditor"))), // pending; Excel: Launch Power Query Editor; gap #12; reserved until the command lands
    ("APNG", Entry::Finish(Action::Command("data.getData"))),          // Excel: Get Data (Power Query); containment(Get Data)
    ("APNH", Entry::Finish(Action::Pending("data.fromHtml"))),         // pending; Excel: From HTML; gap #12; reserved until the command lands
    ("APNL", Entry::Finish(Action::Pending("data.fromSqlServer"))), // pending; Excel: From SQL Server ODBC; gap #12; reserved until the command lands
    ("APNS", Entry::Finish(Action::Pending("data.dataSourceSettings"))), // pending; Excel: Data Source Settings; gap #12; reserved until the command lands
    ("APNT", Entry::Finish(Action::Pending("data.fromTextLegacy"))), // pending; Excel: From Text (Legacy); gap #12; reserved until the command lands
    ("APR", Entry::Finish(Action::Pending("data.recentSources"))),   // pending; Excel: Recent Sources; gap #12; reserved until the command lands
    ("APS", Entry::Finish(Action::Command("data.queriesConnections"))), // Excel: Show Queries
    ("APT", Entry::Finish(Action::Pending("data.fromTable"))),       // pending; Excel: From Table (query); gap #12; reserved until the command lands
    ("AQ", Entry::Finish(Action::Command("data.advancedFilter"))),   // Excel: Advanced Filter
    ("AR", Entry::Continue),                                         // group: group: Queries & Connections
    ("ARA", Entry::Finish(Action::Command("data.refreshAll"))),      // Excel: Refresh All
    ("ARC", Entry::Finish(Action::Pending("data.cancelRefresh"))),   // pending; Excel: Cancel Refresh; gap #12; reserved until the command lands
    ("ARO", Entry::Finish(Action::Pending("data.connectionProperties"))), // pending; Excel: Connection Properties; gap #12; reserved until the command lands
    ("ARR", Entry::Finish(Action::Command("pivot.refresh"))),             // Excel: Refresh
    ("ARS", Entry::Finish(Action::Pending("data.refreshStatus"))),        // pending; Excel: Refresh Status; gap #12; reserved until the command lands
    ("ASA", Entry::Finish(Action::Command("data.sortAscending"))),        // Excel: Sort A to Z
    ("ASD", Entry::Finish(Action::Command("data.sortDescending"))),       // Excel: Sort Z to A
    ("ASS", Entry::Finish(Action::Dialog("sort"))),                       // Excel: Sort
    ("AT", Entry::Finish(Action::Command("data.filter"))),                // Excel: Filter
    ("AU", Entry::Continue),                                              // Excel: Ungroup Menu; group level: containment(Ungroup)
    ("AUC", Entry::Finish(Action::Pending("data.clearOutline"))), // pending; Excel: Clear Outline; feature gap; reserved until the command lands
    ("AUU", Entry::Finish(Action::Command("data.ungroup"))),      // Excel: Ungroup
    ("AV", Entry::Continue),                                      // Excel: Data Validation Menu; group level: containment(Data Validation)
    ("AVI", Entry::Finish(Action::Command("data.circleInvalid"))), // Excel: Circle Invalid Data
    ("AVR", Entry::Finish(Action::Pending("data.clearValidationCircles"))), // pending; Excel: Clear Validation Circles; reserved until the command lands
    ("AVV", Entry::Finish(Action::Command("data.validation"))),             // Excel: Data Validation
    ("AW", Entry::Continue),                                                // group: group: What-If (goalSeek/scenarioManager/dataTable)
    ("AWG", Entry::Finish(Action::Dialog("goalSeek"))),                     // Excel: Goal Seek
    ("AWS", Entry::Finish(Action::Dialog("scenarioManager"))),              // Excel: Scenario Manager
    ("AWT", Entry::Finish(Action::Command("data.dataTable"))),              // Excel: Data Table
    ("AX", Entry::Finish(Action::Pending("data.existingConnections"))), // pending; Excel: Existing Connections; gap #12; reserved until the command lands
    ("AY", Entry::Finish(Action::Command("data.reapply"))),             // Excel: Reapply Filter; containment(Reapply)
    ("RA1", Entry::Finish(Action::Dialog("accessibility"))),            // Excel: Check Accessibility
    ("RA2", Entry::Finish(Action::Command("review.showComments"))), // Excel: Show All Comments; Show All Comments; reserved until the command lands
    ("RC", Entry::Finish(Action::Dialog("comment"))),               // Excel: New Comment
    ("RD", Entry::Finish(Action::Command("review.deleteComment"))), // Excel: Delete Comment
    ("RG", Entry::Finish(Action::Pending("review.showChanges"))),   // pending; Excel: Track Changes; reserved until the command lands
    ("RL", Entry::Finish(Action::Pending("review.translate"))),     // pending; Excel: Translate; feature gap; reserved until the command lands
    ("RN", Entry::Finish(Action::Command("review.nextComment"))),   // Excel: Next Comment
    ("RO", Entry::Finish(Action::Command("review.protectWorkbook"))), // Excel: Protect and Share Workbook
    ("RPS", Entry::Finish(Action::Dialog("protectSheet"))),         // Excel: Protect Sheet
    ("RPW", Entry::Finish(Action::Command("review.protectWorkbook"))), // Excel: Protect Workbook
    ("RRS", Entry::Finish(Action::Pending("review.smartLookup"))),  // pending; Excel: Smart Lookup; feature gap; reserved until the command lands
    ("RS", Entry::Finish(Action::Dialog("spelling"))),              // Excel: Spelling
    ("RTN", Entry::Finish(Action::Command("review.newNote"))),      // Excel: New Note
    ("RV", Entry::Finish(Action::Command("review.previousComment"))), // Excel: Previous Comment
    ("WA", Entry::Finish(Action::Command("view.arrangeAll"))),      // Excel: Arrange All
    ("WC", Entry::Finish(Action::Command("view.customViews"))),     // Excel: Custom Views
    ("WF", Entry::Continue),                                        // Excel: Freeze Panes Menu; group level:
    ("WFC", Entry::Finish(Action::Command("view.freezeFirstColumn"))), // Excel: Freeze First Column
    ("WFF", Entry::Finish(Action::Command("view.freezePanes"))),    // Excel: Freeze Panes
    ("WFR", Entry::Finish(Action::Command("view.freezeTopRow"))),   // Excel: Freeze Top Row
    ("WG", Entry::Finish(Action::Command("view.zoomToSelection"))), // Excel: Zoom To Selection
    ("WH", Entry::Finish(Action::Command("view.hideWindow"))),      // Excel: Hide Window
    ("WI", Entry::Finish(Action::Command("view.pageBreakPreview"))), // Excel: Page Break Preview
    ("WJ", Entry::Finish(Action::Control("view.zoom"))),            // Excel: Zoom To 100%; runs the 100% zoom handler (percent param)
    ("WL", Entry::Finish(Action::Command("view.normal"))),          // Excel: Normal View; containment(Normal)
    ("WM", Entry::Continue),                                        // Excel: Macros Menu; group level: containment(Macros)
    ("WMR", Entry::Finish(Action::Command("view.macros"))),         // Excel: Record Macro; Record Macro (macros menu)
    ("WMV", Entry::Finish(Action::Command("view.macros"))),         // Excel: View Macros; containment(Macros)
    ("WN", Entry::Finish(Action::Command("view.newWindow"))),       // Excel: New Window
    ("WP", Entry::Finish(Action::Command("view.pageLayout"))),      // Excel: Page Layout View
    ("WQ", Entry::Finish(Action::Dialog("zoom"))),                  // Excel: Zoom; opens the Zoom dialog
    ("WR", Entry::Finish(Action::Command("view.ruler"))),           // Excel: Ruler
    ("WS", Entry::Finish(Action::Command("view.split"))),           // Excel: Split
    ("WT", Entry::Finish(Action::Pending("view.resetWindowPosition"))), // pending; Excel: Reset Window Position; reserved until the command lands
    ("WU", Entry::Finish(Action::Command("view.unhideWindow"))),    // Excel: Unhide Window
    ("WVC", Entry::Finish(Action::Pending("view.focusCell"))),      // pending; Excel: Focus Cell Menu; Focus Cell; reserved until the command lands
    ("WVF", Entry::Finish(Action::Control("view.formulaBar"))),     // Excel: Toggle Formula Bar; toggles the formula bar
    ("WVH", Entry::Finish(Action::Command("view.headings"))),       // Excel: Toggle Headings; containment(Headings)
    ("WVN", Entry::Finish(Action::Pending("view.navigation"))),     // pending; Excel: Navigation; Navigation pane; reserved until the command lands
    ("WVR", Entry::Finish(Action::Command("view.ruler"))),          // Excel: Ruler
    ("WW", Entry::Finish(Action::Command("view.switchWindows"))),   // Excel: Switch Windows
    ("JAM", Entry::Finish(Action::Command("chart.formatSelection"))), // Excel: Chart format: Format Selection pane; containment(Format Selection)
    ("JCA", Entry::Finish(Action::Command("chart.addElement"))),    // Excel: Chart: Add Chart Element; containment(Add Chart Element)
    ("JCC", Entry::Finish(Action::Command("chart.changeType"))),    // Excel: Chart: Change Chart Type; containment(Change Chart Type)
    ("JTS", Entry::Finish(Action::Command("table.style"))),         // Excel: Table: table style
    ("JTV", Entry::Finish(Action::Command("table.summarizePivot"))), // Excel: Table: Summarize with PivotTable; containment(Summarize with PivotTable)
    ("FA", Entry::Finish(Action::Command("file.saveAs"))), // Excel: Save / Save As page; fires immediately in Excel; deeper aliases dropped as unreachable
    ("FC", Entry::Finish(Action::Command("file.close"))),  // Excel: Close workbook
    ("FEA", Entry::Finish(Action::Command("file.exportPdf"))), // Excel: Export: create PDF/XPS; Export: create PDF/XPS
    ("FEC", Entry::Finish(Action::Command("file.saveAs"))), // Excel: Export: change file type; Export: change file type
    ("FI", Entry::Finish(Action::Command("file.properties"))), // Excel: Info page; fires immediately in Excel; deeper aliases dropped as unreachable
    ("FN", Entry::Finish(Action::Command("file.new"))),    // Excel: New page; New page
    ("FO", Entry::Finish(Action::Command("file.open"))),   // Excel: Open page; fires immediately in Excel; deeper aliases dropped as unreachable
    ("FP", Entry::Finish(Action::Command("file.print"))),  // Excel: Print page; containment(Print)
    ("Q", Entry::Finish(Action::Pending("ui.search"))),    // pending; Excel: Search / Tell Me; Alt+Q search box does not exist yet (issue #49)
];

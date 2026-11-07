//! German translations for the interface strings in i18n.rs. Labels follow the wording German
//! spreadsheet users know; anything absent falls back to English.

/// Returns the German translation for `text`, or `None` so callers fall back to English.
pub(super) fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Paste" => "Einfügen",
        "Paste Values" => "Werte einfügen",
        "Paste Formulas" => "Formeln einfügen",
        "Paste Formatting" => "Formatierung einfügen",
        "Transpose" => "Transponieren",
        "Paste Link" => "Verknüpfung einfügen",
        "Paste Special…" => "Inhalte einfügen…",
        "Format Painter (double-click to keep it on)" => "Format übertragen (Doppelklick zum Beibehalten)",
        "Bold" => "Fett",
        "Italic" => "Kursiv",
        "Double Underline" => "Doppelt unterstrichen",
        "Strikethrough" => "Durchgestrichen",
        "Underline" => "Unterstrichen",
        "Increase Font Size" => "Schriftgrad vergrößern",
        "Decrease Font Size" => "Schriftgrad verkleinern",
        "Wrap Text" => "Zeilenumbruch",
        "Merge & Center" => "Verbinden und zentrieren",
        "Merge Cells" => "Zellen verbinden",
        "Merge Across" => "Zeilenweise verbinden",
        "Unmerge Cells" => "Zellverbindung aufheben",
        "Fill Color" => "Füllfarbe",
        "Font Color" => "Schriftfarbe",
        "Borders" => "Rahmen",
        "Top\nAlign" => "Oben\nausrichten",
        "Middle\nAlign" => "Vertikal\nzentrieren",
        "Bottom\nAlign" => "Unten\nausrichten",
        "Left" => "Links",
        "Right" => "Rechts",
        "Center" => "Zentriert",
        "Horizontal" => "Horizontal",
        "Vertical Text" => "Vertikaler Text",
        "Decrease Indent" => "Einzug verkleinern",
        "Increase Indent" => "Einzug vergrößern",
        "Orientation" => "Ausrichtung",
        "Angle Counterclockwise" => "Gegen den Uhrzeigersinn neigen",
        "Angle Clockwise" => "Im Uhrzeigersinn neigen",
        "Rotate Text Up" => "Text nach oben drehen",
        "Rotate Text Down" => "Text nach unten drehen",
        "Accounting Number Format" => "Buchhaltungszahlenformat",
        "Percent Style" => "Prozentformat",
        "Comma Style" => "Kommaformat",
        "Increase Decimal" => "Dezimalstelle hinzufügen",
        "Decrease Decimal" => "Dezimalstelle entfernen",
        "More Accounting Formats…" => "Weitere Buchhaltungsformate…",
        "Conditional\nFormatting" => "Bedingte\nFormatierung",
        "Conditional Formatting" => "Bedingte Formatierung",
        "Format\nas Table" => "Als Tabelle\nformatieren",
        "Cell\nStyles" => "Zell-\nformate",
        "Format Cells…" => "Zellen formatieren…",
        "Format Cell Alignment…" => "Zellausrichtung…",
        "Format\nPane" => "Format-\nbereich",
        "Format" => "Format",
        "Insert" => "Einfügen",
        "Header Menu|Insert" => "Zellen einfügen",
        "Insert Cells…" => "Zellen einfügen…",
        "Delete" => "Löschen",
        "Select Data…" => "Daten auswählen…",
        "Format Chart Area…" => "Diagrammbereich formatieren…",
        "Number Stored as Text" => "Als Text gespeicherte Zahl",
        "Convert to Number" => "In eine Zahl umwandeln",
        "Delete Cells…" => "Zellen löschen…",
        "Insert Sheet Rows" => "Blattzeilen einfügen",
        "Insert Sheet Columns" => "Blattspalten einfügen",
        "Delete Sheet Rows" => "Blattzeilen löschen",
        "Delete Sheet Columns" => "Blattspalten löschen",
        "Row Height…" => "Zeilenhöhe…",
        "AutoFit Row Height" => "Zeilenhöhe automatisch anpassen",
        "Column Width…" => "Spaltenbreite…",
        "AutoFit Column Width" => "Spaltenbreite automatisch anpassen",
        "Default Width…" => "Standardbreite…",
        "Hide Rows" => "Zeilen ausblenden",
        "Unhide Rows" => "Zeilen einblenden",
        "Hide Columns" => "Spalten ausblenden",
        "Unhide Columns" => "Spalten einblenden",
        "Lock Cell" => "Zelle sperren",
        "Clear" => "Löschen",
        "Clear All" => "Alles löschen",
        "Clear Formats" => "Formate löschen",
        "Clear Contents" => "Inhalte löschen",
        "Clear Comments and Notes" => "Kommentare und Notizen löschen",
        "Clear Hyperlinks" => "Hyperlinks löschen",
        "AutoSum" => "AutoSumme",
        "Flash\nFill" => "Blitz-\nvorschau",
        "Flash Fill" => "Blitzvorschau",
        "Fill" => "Ausfüllen",
        "Clear Print Area" => "Druckbereich aufheben",
        "Find &\nSelect" => "Suchen und\nAuswählen",
        "Find…" => "Suchen…",
        "Replace…" => "Ersetzen…",
        "Go To…" => "Gehe zu…",
        "Go To Special…" => "Inhalte auswählen…",
        "PivotTable" => "PivotTable",
        "Recommended\nCharts" => "Empfohlene\nDiagramme",
        "Table" => "Tabelle",
        "Pictures" => "Bilder",
        "Shapes" => "Formen",
        "Icons" => "Symbole",
        "Text\nBox" => "Text-\nfeld",
        "Comment" => "Kommentar",
        "New\nComment" => "Neuer\nKommentar",
        "New Note" => "Neue Notiz",
        "Show/Hide Note" => "Notiz anzeigen/ausblenden",
        "Header &\nFooter" => "Kopf- und\nFußzeile",
        "Text to\nColumns" => "Text in\nSpalten",
        "Link" => "Link",
        "Symbol" => "Symbol",
        "Equation" => "Formel",
        "Insert New Equation..." => "Neue Formel einfügen",
        "Sparklines" => "Sparklines",
        "Header & Footer" => "Kopf- und Fußzeile",
        "Draw with ink" => "Mit Freihand zeichnen",
        "Eraser" => "Radierer",
        "Ink to\nShape" => "Freihand in\nForm umwandeln",
        "Select\nObjects" => "Objekte\nauswählen",
        "Themes" => "Designs",
        "Margins" => "Seitenränder",
        "Left Margin" => "Linker Seitenrand",
        "Right Margin" => "Rechter Seitenrand",
        "Top Margin" => "Oberer Seitenrand",
        "Bottom Margin" => "Unterer Seitenrand",
        "Header Margin" => "Kopfzeilenabstand",
        "Footer Margin" => "Fußzeilenabstand",
        "Size" => "Größe",
        "Print\nArea" => "Druck-\nbereich",
        "Set Print Area" => "Druckbereich festlegen",
        "Breaks" => "Umbrüche",
        "Insert Page Break" => "Seitenumbruch einfügen",
        "Remove Page Break" => "Seitenumbruch entfernen",
        "Reset All Page Breaks" => "Alle Seitenumbrüche zurücksetzen",
        "Print\nTitles" => "Druck-\ntitel",
        "Narrow" => "Schmal",
        "Wide" => "Breit",
        "Normal" => "Normal",
        "Portrait" => "Hochformat",
        "Landscape" => "Querformat",
        "Custom Margins…" => "Benutzerdefinierte Seitenränder…",
        "Page Setup" => "Seite einrichten",
        "Left margin (in):" => "Linker Seitenrand (Zoll):",
        "Right margin (in):" => "Rechter Seitenrand (Zoll):",
        "Top margin (in):" => "Oberer Seitenrand (Zoll):",
        "Bottom margin (in):" => "Unterer Seitenrand (Zoll):",
        "Header (in):" => "Kopfzeile (Zoll):",
        "Footer (in):" => "Fußzeile (Zoll):",
        "Page order" => "Seitenreihenfolge",
        "Down, then over" => "Seiten nach unten, dann nach rechts",
        "Over, then down" => "Seiten nach rechts, dann nach unten",
        "Insert\nFunction" => "Funktion\neinfügen",
        "Name\nManager" => "Namens-\nmanager",
        "Define Name" => "Namen definieren",
        "Use in Formula" => "In Formel verwenden",
        "Create from Selection" => "Aus Auswahl erstellen",
        "Trace Precedents" => "Spur zum Vorgänger",
        "Trace Dependents" => "Spur zum Nachfolger",
        "Remove Arrows" => "Pfeile entfernen",
        "Show Formulas" => "Formeln anzeigen",
        "Error Checking" => "Fehlerüberprüfung",
        "Trace Error" => "Spur zum Fehler",
        "Circular References" => "Zirkelbezüge",
        "Python Mode" => "Python-Modus",
        "Python Object" => "Python-Objekt",
        "Excel Value" => "Excel-Wert",
        "Copy as CSV" => "Als CSV kopieren",
        "Copy as Markdown" => "Als Markdown kopieren",
        "Save as .csv…" => "Als .csv speichern…",
        "Spill Here" => "Hier überlaufen lassen",
        "Spill One Line per Row" => "Eine Textzeile pro Zeile überlaufen lassen",
        "The table isn't kept in the file. Run the cell to see it." => {
            "Die Tabelle wird nicht in der Datei gespeichert. Führen Sie die Zelle aus, um sie zu sehen."
        }
        "This table is empty." => "Diese Tabelle ist leer.",
        "This table has no rows." => "Diese Tabelle hat keine Zeilen.",
        "Copied only what is shown: the rest of the table isn't in the copy." => {
            "Nur das Angezeigte wurde kopiert: Der Rest der Tabelle ist nicht in der Kopie."
        }
        "Saved only what is shown: the rest of the table isn't in the file." => {
            "Nur das Angezeigte wurde gespeichert: Der Rest der Tabelle ist nicht in der Datei."
        }
        "The table is saved." => "Die Tabelle wurde gespeichert.",
        "Only the first {rows} of {total} rows are shown." => "Nur die ersten {rows} von {total} Zeilen werden angezeigt.",
        "Only the first {cols} of {total} columns are shown." => "Nur die ersten {cols} von {total} Spalten werden angezeigt.",
        "Some long text is cut." => "Langer Text ist teilweise abgeschnitten.",
        "Copying and saving take only what is shown. Spill Here gives the whole table." => {
            "Kopieren und Speichern erfassen nur das Angezeigte. „Hier überlaufen lassen“ gibt die ganze Tabelle aus."
        }
        "Save Python Table as CSV" => "Python-Tabelle als CSV speichern",
        "Python Table" => "Python-Tabelle",
        "Convert to Normal Cell" => "In normale Zelle konvertieren",
        "Show Card" => "Karte anzeigen",
        "Insert\nPython" => "Python\neinfügen",
        "Insert Python (Ctrl+Alt+Shift+P)" => "Python einfügen (Ctrl+Alt+Shift+P)",
        "Python Editor" => "Python-Editor",
        "Edit in Python Editor" => "Im Python-Editor bearbeiten",
        "Unsaved changes" => "Nicht gespeicherte Änderungen",
        "The code for {cell} has unsaved changes." => "Der Code für {cell} hat nicht gespeicherte Änderungen.",
        "The code for {cell} in {workbook} has unsaved changes." => "Der Code für {cell} in {workbook} hat nicht gespeicherte Änderungen.",
        "The code can't be longer: a Python cell holds {max} characters, counted as a PY formula." => {
            "Der Code kann nicht länger werden: Eine Python-Zelle fasst {max} Zeichen, als PY-Formel gezählt."
        }
        "{cell} changed while it was open in the Python editor. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} hat sich geändert, während die Zelle im Python-Editor geöffnet war. Kopieren Sie den Code, oder schließen Sie den Editor und öffnen Sie ihn erneut für die Zelle."
        }
        "{cell} changed while it was open in the Python editor, so the code was not applied. Copy the code, or close the editor and open it again on the cell." => {
            "{cell} hat sich geändert, während die Zelle im Python-Editor geöffnet war, daher wurde der Code nicht übernommen. Kopieren Sie den Code, oder schließen Sie den Editor und öffnen Sie ihn erneut für die Zelle."
        }
        "{apply} applies the code. Esc closes the editor." => "{apply} übernimmt den Code. Esc schließt den Editor.",
        "The Python editor's unsaved code was copied to the clipboard." => {
            "Der nicht gespeicherte Code des Python-Editors wurde in die Zwischenablage kopiert."
        }
        "Apply" => "Übernehmen",
        "Discard" => "Verwerfen",
        "Cancel" => "Abbrechen",
        "Evaluate Formula" => "Formel auswerten",
        "Watch Window" => "Überwachungsfenster",
        "Calculation\nOptions" => "Berechnungs-\noptionen",
        "Calculate Now" => "Jetzt berechnen",
        "Calculate Sheet" => "Blatt berechnen",
        "Automatic" => "Automatisch",
        "Automatic Except for Data Tables" => "Automatisch, außer für Datentabellen",
        "Manual" => "Manuell",
        "Sort &\nFilter" => "Sortieren und\nFiltern",
        "Sort" => "Sortieren",
        "Sort A to Z" => "Von A bis Z sortieren",
        "Sort Z to A" => "Von Z bis A sortieren",
        "Custom Sort…" => "Benutzerdefiniertes Sortieren…",
        "Filter" => "Filtern",
        "Reapply" => "Erneut anwenden",
        "Get Data\n(Text/CSV)" => "Daten abrufen\n(Text/CSV)",
        "Remove\nDuplicates" => "Duplikate\nentfernen",
        "Data\nValidation" => "Daten-\nüberprüfung",
        "Data Validation" => "Datenüberprüfung",
        "Data Validation…" => "Datenüberprüfung…",
        "Circle Invalid Data" => "Ungültige Daten einkreisen",
        "Clear Validation" => "Datenüberprüfung löschen",
        "Group" => "Gruppieren",
        "Ungroup" => "Gruppierung aufheben",
        "Show Detail" => "Details anzeigen",
        "Hide Detail" => "Details ausblenden",
        "Subtotal" => "Teilergebnis",
        "What-If\nAnalysis" => "Was-wäre-wenn-\nAnalyse",
        "Data Table…" => "Datentabelle…",
        "Scenario Manager…" => "Szenario-Manager…",
        "Goal Seek…" => "Zielwertsuche…",
        "Spelling" => "Rechtschreibung",
        "Check\nAccessibility" => "Barrierefreiheit\nprüfen",
        "Threaded Comments" => "Threadkommentare",
        "Show\nComments" => "Kommentare\nanzeigen",
        "Protect\nWorkbook" => "Arbeitsmappe\nschützen",
        "Unprotect\nSheet" => "Blattschutz\naufheben",
        "Protect Sheet…" => "Blatt schützen…",
        "Unprotect Sheet…" => "Blattschutz aufheben…",
        "Comments" => "Kommentare",
        "Notes" => "Notizen",
        "Page Break\nPreview" => "Seitenumbruch-\nvorschau",
        "Page\nLayout" => "Seiten-\nlayout",
        "Formula Bar" => "Formelleiste",
        "Name Box" => "Namensfeld",
        "Cell Editor" => "Zelleneditor",
        "Font Size" => "Schriftgröße",
        "Gridlines" => "Gitternetzlinien",
        "Headings" => "Überschriften",
        "Zoom" => "Zoom",
        "Magnification" => "Vergrößerung",
        "Fit selection" => "An Markierung anpassen",
        "Forecast\nSheet" => "Prognose-\nblatt",
        "Add footer" => "Fußzeile hinzufügen",
        "Add header" => "Kopfzeile hinzufügen",
        "Click to add data" => "Klicken, um Daten hinzuzufügen",
        "Current Date" => "Aktuelles Datum",
        "Current\nDate" => "Aktuelles\nDatum",
        "Current\nTime" => "Aktuelle\nUhrzeit",
        "Current Time" => "Aktuelle Uhrzeit",
        "File Name" => "Dateiname",
        "File\nName" => "Datei-\nname",
        "File\nPath" => "Datei-\npfad",
        "File Path" => "Dateipfad",
        "Footer" => "Fußzeile",
        "Different First Page" => "Erste Seite anders",
        "Different Odd & Even Pages" => "Gerade und ungerade Seiten anders",
        "Scale with Document" => "Mit Dokument skalieren",
        "Align with Page Margins" => "An Seitenrändern ausrichten",
        "Go to Footer" => "Zur Fußzeile wechseln",
        "Go to Header" => "Zur Kopfzeile wechseln",
        "Go to\nFooter" => "Zur\nFußzeile",
        "Go to\nHeader" => "Zur\nKopfzeile",
        "Header" => "Kopfzeile",
        "(none)" => "(keine)",
        "Number\nof Pages" => "Anzahl\nder Seiten",
        "Number of Pages" => "Anzahl der Seiten",
        "Page\nNumber" => "Seiten-\nzahl",
        "Page Number" => "Seitenzahl",
        "Ruler" => "Lineal",
        "Sheet Name" => "Blattname",
        "Sheet\nName" => "Blatt-\nname",
        "Animations" => "Animationen",
        "Glide the selection outline to the cell you move to instead of jumping there" => {
            "Den Auswahlrahmen zur angesteuerten Zelle gleiten lassen, statt zu springen"
        }
        "Insert Copied Cells" => "Kopierte Zellen einfügen",
        "Insert Copied Cells…" => "Kopierte Zellen einfügen…",
        "Insert Cut Cells" => "Ausgeschnittene Zellen einfügen",
        "Insert Paste" => "Einfügen und verschieben",
        "Previous" => "Zurück",
        "Next" => "Weiter",
        "Previous Comment" => "Vorheriger Kommentar",
        "Next Comment" => "Nächster Kommentar",
        "Show All Notes" => "Alle Notizen anzeigen",
        "Convert to Comments" => "In Kommentare konvertieren",
        "Checkbox" => "Kontrollkästchen",
        "Insert checkboxes in the selected cells" => "Kontrollkästchen in die ausgewählten Zellen einfügen",
        "Button" => "Schaltfläche",
        "Slider" => "Schieberegler",
        "Spin Button" => "Drehfeld",
        "Draw a button that runs a macro or a command" => "Eine Schaltfläche zeichnen, die ein Makro oder einen Befehl ausführt",
        "Draw a slider that sets a whole number in a linked cell" => {
            "Einen Schieberegler zeichnen, der eine ganze Zahl in einer verknüpften Zelle festlegt"
        }
        "Draw a spin button that steps a whole number in a linked cell" => {
            "Ein Drehfeld zeichnen, das eine ganze Zahl in einer verknüpften Zelle schrittweise ändert"
        }
        "Edit Text" => "Text bearbeiten",
        "Assign Action…" => "Aktion zuweisen…",
        "Format Control…" => "Steuerelement formatieren…",
        "Sheet\nRight-to-Left" => "Blatt von rechts\nnach links",
        "Sheet Right-to-Left" => "Blatt von rechts nach links",
        "Data\nAnalysis" => "Daten-\nanalyse",
        "Data Analysis" => "Datenanalyse",
        "Solver" => "Solver",
        "Allow Edit\nRanges" => "Bearbeiten von\nBereichen zulassen",
        "Allow Edit Ranges" => "Bearbeiten von Bereichen zulassen",
        "Drag to freeze columns" => "Ziehen, um Spalten zu fixieren",
        "Drag to freeze rows" => "Ziehen, um Zeilen zu fixieren",
        "Select\nData" => "Daten\nauswählen",
        "File" => "Datei",
        "Back" => "Zurück",
        "New" => "Neu",
        "Open" => "Öffnen",
        "Info" => "Informationen",
        "Save" => "Speichern",
        "Save As" => "Speichern unter",
        "Export" => "Exportieren",
        "Close" => "Schließen",
        "Options" => "Optionen",
        "Account" => "Konto",
        "Product Information" => "Produktinformationen",
        "About GridCraft" => "Info zu GridCraft",
        "Version" => "Version",
        "Platform" => "Plattform",
        "Licence" => "Lizenz",
        "Version, contributors and the AI models that helped" => "Version, Mitwirkende und die KI-Modelle, die geholfen haben",
        "Blank workbook" => "Leere Arbeitsmappe",
        "Recent" => "Zuletzt verwendet",
        "No recent workbooks yet." => "Noch keine zuletzt verwendeten Arbeitsmappen.",
        "More workbooks" => "Weitere Arbeitsmappen",
        "Modified" => "Geändert",
        "Not available" => "Nicht verfügbar",
        "Just now" => "Gerade eben",
        "Yesterday" => "Gestern",
        "{n} min ago" => "vor {n} Min.",
        "{n} h ago" => "vor {n} Std.",
        "{n} days ago" => "vor {n} Tagen",
        "{n} weeks ago" => "vor {n} Wochen",
        "{n} months ago" => "vor {n} Monaten",
        "{n} years ago" => "vor {n} Jahren",
        "Browse…" => "Durchsuchen…",
        "Not saved yet" => "Noch nicht gespeichert",
        "Location" => "Speicherort",
        "Sheets" => "Blätter",
        "Protect Workbook" => "Arbeitsmappe schützen",
        "Sheet structure is locked: sheets can't be added, moved, renamed or deleted." => {
            "Die Blattstruktur ist gesperrt: Blätter können nicht hinzugefügt, verschoben, umbenannt oder gelöscht werden."
        }
        "Lock the sheet structure so sheets can't be added, moved, renamed or deleted." => {
            "Sperrt die Blattstruktur, sodass Blätter nicht hinzugefügt, verschoben, umbenannt oder gelöscht werden können."
        }
        "Properties" => "Eigenschaften",
        "Title" => "Titel",
        "Subject" => "Thema",
        "Author" => "Autor",
        "Company" => "Firma",
        "Keywords" => "Stichwörter",
        "Description" => "Beschreibung",
        "No workbook is open." => "Es ist keine Arbeitsmappe geöffnet.",
        "Save as type" => "Dateityp",
        "Excel Workbook (*.xlsx)" => "Excel-Arbeitsmappe (*.xlsx)",
        "CSV UTF-8 (Comma delimited) (*.csv)" => "CSV UTF-8 (durch Trennzeichen getrennt) (*.csv)",
        "Web Page (*.html)" => "Webseite (*.html)",
        "PDF (*.pdf)" => "PDF-Datei (*.pdf)",
        "Download" => "Herunterladen",
        "A CSV file, web page or PDF is a copy: the workbook keeps its own file." => {
            "Eine CSV-Datei, Webseite oder PDF ist eine Kopie: Die Arbeitsmappe behält ihre eigene Datei."
        }
        "Save the workbook under another name or in another folder." => {
            "Speichert die Arbeitsmappe unter einem anderen Namen oder in einem anderen Ordner."
        }
        "Print Active Sheets" => "Aktive Blätter drucken",
        "Print Entire Workbook" => "Gesamte Arbeitsmappe drucken",
        "Fit Sheet on One Page" => "Blatt auf einer Seite darstellen",
        "Page Setup…" => "Seite einrichten…",
        "Page {page} of {pages}" => "Seite {page} von {pages}",
        "Nothing to print." => "Nichts zu drucken.",
        "Previous page" => "Vorherige Seite",
        "Next page" => "Nächste Seite",
        "Create PDF" => "PDF erstellen",
        "Save the sheet as a PDF document that keeps its page layout." => "Speichert das Blatt als PDF-Dokument mit unverändertem Seitenlayout.",
        "Change File Type" => "Dateityp ändern",
        "CSV (Comma delimited)" => "CSV (Trennzeichen-getrennt)",
        "Save the active sheet as comma-separated text." => "Speichert das aktive Blatt als durch Kommas getrennten Text.",
        "Web Page (.html)" => "Webseite (.html)",
        "Save the workbook as a web page." => "Speichert die Arbeitsmappe als Webseite.",
        "Options|General" => "Allgemein",
        "Options|Save" => "Speichern",
        "Show formula bar" => "Bearbeitungsleiste anzeigen",
        "Highlight the active cell's row" => "Zeile der aktiven Zelle hervorheben",
        "Display options for this workbook" => "Optionen für die Anzeige dieser Arbeitsmappe",
        "Show horizontal scroll bar" => "Horizontale Bildlaufleiste anzeigen",
        "Show vertical scroll bar" => "Vertikale Bildlaufleiste anzeigen",
        "Show sheet tabs" => "Blattregisterkarten anzeigen",
        "Edit Hyperlink…" => "Link bearbeiten…",
        "Edit Hyperlink" => "Link bearbeiten",
        "Open Hyperlink" => "Link öffnen",
        "Remove Hyperlink" => "Link entfernen",
        "Resize formula bar" => "Größe der Bearbeitungsleiste ändern",
        "Enable AutoComplete for cell values" => "AutoVervollständigen für Zellwerte aktivieren",
        "Save .xlsx workbooks automatically after every change" => ".xlsx-Arbeitsmappen nach jeder Änderung automatisch speichern",
        "Workbook calculation" => "Arbeitsmappenberechnung",
        "Enable iterative calculation" => "Iterative Berechnung aktivieren",
        "Maximum Iterations:" => "Maximale Iterationszahl:",
        "Maximum Change:" => "Maximale Änderung:",
        "Maximum Iterations must be a whole number from 1 to 32767." => "Die maximale Iterationszahl muss eine ganze Zahl von 1 bis 32767 sein.",
        "Maximum Change must be a number of 0 or more." => "Die maximale Änderung muss eine Zahl größer oder gleich 0 sein.",
        "Calculation settings are saved with the workbook." => "Berechnungseinstellungen werden mit der Arbeitsmappe gespeichert.",
        "Interface size" => "Größe der Benutzeroberfläche",
        "Auto" => "Automatisch",
        "100%" => "100%",
        "Zoom to\nSelection" => "Zoom auf die\nAuswahl",
        "Freeze\nPanes" => "Fenster\nfixieren",
        "Freeze Panes" => "Fenster fixieren",
        "Freeze Top Row" => "Oberste Zeile fixieren",
        "Freeze First Column" => "Erste Spalte fixieren",
        "Unfreeze Panes" => "Fixierung aufheben",
        "Dark Mode" => "Dunkler Modus",
        "Interface language" => "Anzeigesprache",
        "Command\nPalette" => "Befehls-\npalette",
        "Agent\nControl" => "Agenten-\nsteuerung",
        "Action\nJournal" => "Aktions-\nprotokoll",
        "About\nGridCraft" => "Über\nGridCraft",
        "Share" => "Teilen",
        "Add Chart\nElement" => "Diagrammelement\nhinzufügen",
        "Change\nChart Type" => "Diagrammtyp\nändern",
        "Switch\nRow/Column" => "Zeile/Spalte\nwechseln",
        "Data Labels: None" => "Datenbeschriftungen: Keine",
        "Data Labels: Show" => "Datenbeschriftungen: Anzeigen",
        "Legend: None" => "Legende: Keine",
        "Legend: Bottom" => "Legende: Unten",
        "Legend: Top" => "Legende: Oben",
        "Legend: Right" => "Legende: Rechts",
        "First slice angle" => "Winkel des ersten Segments",
        "Doughnut hole size" => "Größe des Ringlochs",
        "Gridlines: None" => "Gitternetzlinien: Keine",
        "Gridlines: Show" => "Gitternetzlinien: Anzeigen",
        "Chart\nTitle" => "Diagramm-\ntitel",
        "Delete\nChart" => "Diagramm\nlöschen",
        "Convert\nto Range" => "In Bereich\nkonvertieren",
        "Table\nStyles" => "Tabellen-\nformate",
        "Top 10 Items" => "Obere 10 Elemente",
        "Top 10%" => "Obere 10 %",
        "Bottom 10 Items" => "Untere 10 Elemente",
        "Bottom 10%" => "Untere 10 %",
        "Above Average" => "Überdurchschnittlich",
        "Below Average" => "Unterdurchschnittlich",
        "Clear Rules from Entire Sheet" => "Regeln im gesamten Blatt löschen",
        "Clear Rules from Selected Cells" => "Regeln in den ausgewählten Zellen löschen",
        "Constants" => "Konstanten",
        "Data Bars" => "Datenbalken",
        "Color Scales" => "Farbskalen",
        "Icon Sets" => "Symbolsätze",
        "Home" => "Start",
        "Draw" => "Zeichnen",
        "Page Layout" => "Seitenlayout",
        "Formulas" => "Formeln",
        "Data" => "Daten",
        "Review" => "Überprüfen",
        "View" => "Ansicht",
        "Automate" => "Automatisieren",
        "Table Design" => "Tabellenentwurf",
        "Chart Design" => "Diagrammentwurf",
        "Ready" => "Bereit",
        "Enter" => "Eingeben",
        "Edit" => "Bearbeiten",
        "Point" => "Zeigen",
        "Calculate" => "Berechnen",
        "Select destination and press Enter or choose Paste" => "Ziel auswählen und die Eingabetaste drücken oder „Einfügen“ wählen",
        "General" => "Standard",
        "Number" => "Zahl",
        "Currency" => "Währung",
        "Accounting" => "Buchhaltung",
        "Date" => "Datum",
        "Time" => "Uhrzeit",
        "Percentage" => "Prozentsatz",
        "Fraction" => "Bruch",
        "Scientific" => "Wissenschaftlich",
        "Text" => "Text",
        "Special" => "Sonderformat",
        "Custom" => "Benutzerdefiniert",
        "Short Date" => "Kurzes Datum",
        "Long Date" => "Langes Datum",
        "More Number Formats…" => "Weitere Zahlenformate…",
        "Financial" => "Finanzmathematik",
        "Recently Used" => "Zuletzt verwendet",
        "Logical" => "Logisch",
        "Date & Time" => "Datum & Uhrzeit",
        "Lookup & Reference" => "Nachschlagen & Verweisen",
        "Math & Trig" => "Mathematik & Trigonometrie",
        "More Functions" => "Weitere Funktionen",
        "Page Layout|Orientation" => "Seitenausrichtung",
        "Display theme" => "Anzeigedesign",
        "System" => "System",
        "Light" => "Hell",
        "Dark" => "Dunkel",
        "Sheet Options|View" => "Anzeige",
        "Print" => "Drucken",
        "Print What" => "Was drucken",
        // grid.rs cell and row/column header context menus.
        "Cut" => "Ausschneiden",
        "Copy" => "Kopieren",
        "Insert…" => "Zellen einfügen…",
        "Delete…" => "Löschen…",
        "Filter by Selected Cell's Value" => "Nach Wert der gewählten Zelle filtern",
        "New Comment" => "Neuer Kommentar",
        "Pick From Drop-down List…" => "Aus Dropdownliste auswählen…",
        "Define Name…" => "Namen definieren…",
        "Link…" => "Link…",
        "Hide" => "Ausblenden",
        "Unhide" => "Einblenden",
        "Insert Sheet" => "Blatt einfügen",
        "Rename" => "Umbenennen",
        "Move or Copy…" => "Verschieben oder kopieren…",
        "Duplicate" => "Duplizieren",
        "Tab Color" => "Registerfarbe",
        "No Color" => "Keine Farbe",
        "Unhide…" => "Einblenden…",
        "Select All Sheets" => "Alle Blätter auswählen",
        "AutoSave" => "AutoSpeichern",
        "AutoSave: save after every change (workbooks saved to a file)" => {
            "AutoSpeichern: nach jeder Änderung speichern (Arbeitsmappen, die in einer Datei gespeichert sind)"
        }
        "Previous sheet" => "Vorheriges Blatt",
        "Next sheet" => "Nächstes Blatt",
        "All sheets" => "Alle Blätter",
        "New sheet (⇧F11)" => "Neues Blatt (⇧F11)",
        "Text Box" => "Textfeld",
        "Macros" => "Makros",
        "Record Macro" => "Makro aufzeichnen",
        "Stop Recording" => "Aufzeichnung beenden",
        "Use Relative References" => "Relative Verweise verwenden",
        "Run\nMacro" => "Makro ausführen",
        "Macros…" => "Makros…",
        "Recording" => "Aufzeichnung",
        "Stop" => "Beenden",
        "New Macro" => "Neues Makro",
        "Show" => "Anzeigen",
        "Collapse dialog" => "Dialogfeld reduzieren",
        "Expand dialog" => "Dialogfeld erweitern",
        "Python|Off" => "Aus",
        "Python|Not Set Up" => "Nicht eingerichtet",
        "Python|Ready" => "Bereit",
        "Python|Running" => "Läuft",
        "Python|Blocked" => "Blockiert",
        "Enable Python in cells" => "Python in Zellen aktivieren",
        "Waiting for Python cells to finish before saving..." => "Vor dem Speichern wird auf den Abschluss der Python-Zellen gewartet...",
        "Save Now" => "Jetzt speichern",
        "The workbook was saved, but these Python cells weren't finished, so the file has their last results, if they had any: {cells}" => {
            "Die Arbeitsmappe wurde gespeichert, aber diese Python-Zellen wurden nicht fertig, daher enthält die Datei ihre letzten Ergebnisse, falls vorhanden: {cells}"
        }
        "{cells} and {count} more" => "{cells} und {count} weitere",
        "{version}, too old: Python cells need 3.11 or newer" => "{version}, zu alt: Python-Zellen benötigen 3.11 oder neuer",
        "Looking for Python…" => "Python wird gesucht…",
        "Copied to the clipboard." => "In die Zwischenablage kopiert.",
        "This workbook has Python code. Enable it to run its cells; until then they keep the values saved in the file." => {
            "Diese Arbeitsmappe enthält Python-Code. Aktivieren Sie ihn, um die Zellen auszuführen; bis dahin behalten sie die in der Datei gespeicherten Werte."
        }
        "Enable" => "Aktivieren",
        "Python: running {cell}, {seconds} s" => "Python: {cell} wird ausgeführt, {seconds} s",
        "Python: {state}" => "Python: {state}",
        "Open the Python environment" => "Python-Umgebung öffnen",
        "Nothing on this system: Python doesn't run here" => "Auf diesem System nichts vorhanden: Python läuft hier nicht",
        "GridCraft starts Python with an empty environment" => "GridCraft startet Python mit einer leeren Umgebung",
        "macOS sandbox profile" => "macOS-Sandbox-Profil",
        "Linux Landlock" => "Linux Landlock",
        "Linux seccomp filter" => "Linux-seccomp-Filter",
        "Open Python Settings" => "Python-Einstellungen öffnen",
        "Choose Interpreter" => "Interpreter wählen",
        "Copy Install Command" => "Installationsbefehl kopieren",
        "Run Again" => "Erneut ausführen",
        "Reset" => "Zurücksetzen",
        "Python Environment" => "Python-Umgebung",
        "Python Packages" => "Python-Pakete",
        "Python Problems" => "Python-Probleme",
        "Python Run Log" => "Python-Ausführungsprotokoll",
        "What Python Can Do" => "Was Python kann",
        "GridCraft is checking Python." => "GridCraft prüft Python.",
        "Check Again" => "Erneut prüfen",
        "Interpreter" => "Interpreter",
        "Chosen by you:" => "Von Ihnen gewählt:",
        "Automatic found:" => "Automatisch gefunden:",
        "What runs:" => "Was ausgeführt wird:",
        "None found yet." => "Noch nichts gefunden.",
        "Sandbox" => "Sandbox",
        "Package folders" => "Paketordner",
        "What Python can read" => "Was Python lesen kann",
        "uv manages Python versions and packages. To get it, run:" => "uv verwaltet Python-Versionen und -Pakete. Um es zu erhalten, führen Sie aus:",
        "Reading files outside Python: blocked" => "Dateien außerhalb von Python lesen: blockiert",
        "Writing files: blocked" => "Dateien schreiben: blockiert",
        "Using the network: blocked" => "Das Netzwerk verwenden: blockiert",
        "Starting other programs: blocked" => "Andere Programme starten: blockiert",
        "Each time Python starts, GridCraft tries each of these first, and doesn't run Python unless every one is refused." => {
            "Bei jedem Start von Python versucht GridCraft zuerst jede dieser Aktionen und führt Python nur aus, wenn alle abgelehnt werden."
        }
        "Install a package with uv. GridCraft only copies the command; you run it in a terminal, then choose Check Again." => {
            "Installieren Sie ein Paket mit uv. GridCraft kopiert nur den Befehl; Sie führen ihn in einem Terminal aus und wählen dann Erneut prüfen."
        }
        "Package name" => "Paketname",
        "Other package managers" => "Andere Paketverwaltungen",
        "For people who manage Python another way: choose any interpreter by path in File › Options › Python. With pip:" => {
            "Für alle, die Python anders verwalten: Wählen Sie unter Datei › Optionen › Python einen beliebigen Interpreter über seinen Pfad. Mit pip:"
        }
        "Packages: {count}" => "Pakete: {count}",
        "No problems." => "Keine Probleme.",
        "This workbook" => "Diese Arbeitsmappe",
        "No runs yet." => "Noch keine Ausführungen.",
        "Go To Cell" => "Zu Zelle gehen",
        "This run printed nothing." => "Diese Ausführung hat nichts ausgegeben.",
        "Python cells can" => "Python-Zellen können",
        "Read cells through xl()" => "Zellen über xl() lesen",
        "GridCraft sends only the cells that xl() names" => "GridCraft sendet nur die Zellen, die xl() nennt",
        "Import installed packages" => "Installierte Pakete importieren",
        "The sandbox lets Python read its own installation" => "Die Sandbox erlaubt Python, die eigene Installation zu lesen",
        "Compute" => "Rechnen",
        "Within the time and memory limits in File › Options › Python" => {
            "Innerhalb der Zeit- und Speicherlimits unter Datei › Optionen › Python"
        }
        "Printed output is kept in the Run Log" => "Ausgegebener Text wird im Ausführungsprotokoll gespeichert",
        "Use threads" => "Threads verwenden",
        "Threads stay inside the one Python process" => "Threads bleiben im einen Python-Prozess",
        "Python cells cannot" => "Python-Zellen können nicht",
        "Read your files" => "Ihre Dateien lesen",
        "Write files" => "Dateien schreiben",
        "Use the network" => "Das Netzwerk verwenden",
        "Start programs" => "Programme starten",
        "See environment variables" => "Umgebungsvariablen sehen",
        "Open windows" => "Fenster öffnen",
        "Draw plots. The data goes to a sheet and a chart is built from it." => {
            "Diagramme zeichnen. Die Daten gehen in ein Blatt, und daraus wird ein Diagramm erstellt."
        }
        "Run this workbook's Python cells" => "Python-Zellen dieser Arbeitsmappe ausführen",
        "Off by default. Only you can turn this on, here: no file, macro or agent can. A workbook opened from a file also needs its own Enable." => {
            "Standardmäßig aus. Nur Sie können dies hier einschalten: keine Datei, kein Makro und kein Agent kann es. Eine aus einer Datei geöffnete Arbeitsmappe benötigt außerdem ihr eigenes Aktivieren."
        }
        "Limits" => "Limits",
        "While Python is off, GridCraft starts nothing, so it can't yet say which version it would use." => {
            "Solange Python aus ist, startet GridCraft nichts und kann daher noch nicht sagen, welche Version verwendet würde."
        }
        "Automatic would try first:" => "Automatisch würde zuerst versuchen:",
        "Set up Python with uv" => "Python mit uv einrichten",
        "Run these in a terminal, in order. GridCraft looks in this environment first, so there is nothing more to choose. It only copies the commands; it never runs them." => {
            "Führen Sie diese Befehle der Reihe nach in einem Terminal aus. GridCraft sucht zuerst in dieser Umgebung, sodass nichts weiter zu wählen ist. Die Befehle werden nur kopiert, nie ausgeführt."
        }
        "Any Python 3.11 or newer can be chosen by path with Browse…. To add packages to it with pip:" => {
            "Jedes Python ab 3.11 kann mit Durchsuchen… über seinen Pfad gewählt werden. So fügen Sie ihm Pakete mit pip hinzu:"
        }
        "Time limit for one cell" => "Zeitlimit für eine Zelle",
        " seconds" => " Sekunden",
        "Memory limit" => "Speicherlimit",
        " MB" => " MB",
        "At a limit Python is restarted and its variables are cleared; the next run starts fresh." => {
            "Bei Erreichen eines Limits wird Python neu gestartet und seine Variablen werden gelöscht; die nächste Ausführung beginnt neu."
        }
        "On macOS the memory limit is a check twice a second, not a hard cap." => {
            "Unter macOS ist das Speicherlimit eine Prüfung zweimal pro Sekunde, keine feste Obergrenze."
        }
        "Editor" => "Editor",
        "Initialization" => "Initialisierung",
        "The code that runs before this workbook's Python cells" => "Der Code, der vor den Python-Zellen dieser Arbeitsmappe ausgeführt wird",
        "Open Initialization" => "Initialisierung öffnen",
        "The initialization code has unsaved changes." => "Der Initialisierungscode enthält nicht gespeicherte Änderungen.",
        "The initialization code of {workbook} has unsaved changes." => {
            "Der Initialisierungscode von {workbook} enthält nicht gespeicherte Änderungen."
        }
        "The code can't be longer: initialization code holds {max} characters." => {
            "Der Code kann nicht länger werden: Initialisierungscode fasst {max} Zeichen."
        }
        "The initialization code changed while it was open in the editor. Copy your code, or close the editor and open it again." => {
            "Der Initialisierungscode wurde geändert, während er im Editor geöffnet war. Kopieren Sie Ihren Code, oder schließen Sie den Editor und öffnen Sie ihn erneut."
        }
        "The initialization code changed while it was open in the editor, so your code was not applied. Copy it, or close the editor and open it again." => {
            "Der Initialisierungscode wurde geändert, während er im Editor geöffnet war, daher wurde Ihr Code nicht übernommen. Kopieren Sie ihn, oder schließen Sie den Editor und öffnen Sie ihn erneut."
        }
        "The initialization code uses a package this Python doesn't have. GridCraft skipped the lines that need it and ran the rest." => {
            "Der Initialisierungscode verwendet ein Paket, das dieses Python nicht hat. GridCraft hat die Zeilen übersprungen, die es benötigen, und den Rest ausgeführt."
        }
        "Select a Python cell first." => "Wählen Sie zuerst eine Python-Zelle.",
        "Output As" => "Ausgabe als",
        "Run\nCell" => "Zelle\nausführen",
        "Run the selected Python cell" => "Die ausgewählte Python-Zelle ausführen",
        "Run\nAll" => "Alle\nausführen",
        "Run every Python cell in the workbook, in order" => "Alle Python-Zellen der Arbeitsmappe der Reihe nach ausführen",
        "Nothing is running." => "Es läuft nichts.",
        "Stop the running cell" => "Die laufende Zelle beenden",
        "Clear Python's variables and run every cell again" => "Variablen von Python löschen und alle Zellen erneut ausführen",
        "Python Calculation" => "Python-Berechnung",
        "When this workbook's Python cells run" => "Wann die Python-Zellen dieser Arbeitsmappe ausgeführt werden",
        "A Python cell is running." => "Eine Python-Zelle läuft.",
        "Python is ready." => "Python ist bereit.",
        "Which Python runs cells" => "Welches Python Zellen ausführt",
        "Packages" => "Pakete",
        "What this Python has installed" => "Was dieses Python installiert hat",
        "Check Sandbox" => "Sandbox prüfen",
        "Check Python and the sandbox again, and show the report" => "Python und die Sandbox erneut prüfen und den Bericht anzeigen",
        "What Python\nCan Do" => "Was Python\nkann",
        "What Python cells can and can't do here" => "Was Python-Zellen hier können und nicht können",
        "This Workbook: Enabled" => "Diese Arbeitsmappe: aktiviert",
        "This Workbook: Disabled" => "Diese Arbeitsmappe: deaktiviert",
        "Only a person can change this." => "Das kann nur eine Person ändern.",
        "Run Log" => "Ausführungsprotokoll",
        "Each run's cell, time and printed output" => "Zelle, Zeit und Ausgabe jeder Ausführung",
        "Clear the Run Log" => "Das Ausführungsprotokoll löschen",
        "Problems" => "Probleme",
        "Every Python cell with something to say" => "Jede Python-Zelle mit einer Meldung",
        "Diagnostics" => "Diagnose",
        "uv environment" => "uv-Umgebung",
        "uv Python" => "uv-Python",
        "virtual environment" => "virtuelle Umgebung",
        "conda environment" => "conda-Umgebung",
        "system" => "System",
        "Python cells need a sandbox, and GridCraft has none for this system." => {
            "Python-Zellen benötigen eine Sandbox, und GridCraft hat für dieses System keine."
        }
        "The sandbox didn't hold when GridCraft checked it, so Python won't run." => {
            "Die Sandbox hat bei der Prüfung durch GridCraft nicht standgehalten, daher wird Python nicht ausgeführt."
        }
        "Python is off. Turn it on in Settings to run Python cells." => {
            "Python ist aus. Schalten Sie es in den Einstellungen ein, um Python-Zellen auszuführen."
        }
        "GridCraft can't find Python 3.11 or newer. Set one up with uv, or choose one." => {
            "GridCraft findet kein Python 3.11 oder neuer. Richten Sie eines mit uv ein, oder wählen Sie eines."
        }
        "This Python is too old: Python cells need 3.11 or newer." => "Dieses Python ist zu alt: Python-Zellen benötigen 3.11 oder neuer.",
        "This is Apple's placeholder for Python, which GridCraft doesn't use. Set up Python with uv, or choose another." => {
            "Dies ist der Platzhalter von Apple für Python, den GridCraft nicht verwendet. Richten Sie Python mit uv ein, oder wählen Sie ein anderes."
        }
        "GridCraft couldn't work out what this Python needs to run." => "GridCraft konnte nicht ermitteln, was dieses Python zum Ausführen benötigt.",
        "Python didn't start." => "Python wurde nicht gestartet.",
        "Python isn't on for this workbook, so its cells keep the values saved in the file." => {
            "Python ist für diese Arbeitsmappe nicht aktiviert, daher behalten ihre Zellen die in der Datei gespeicherten Werte."
        }
        "This session doesn't run Python: cells keep the values saved in the file." => {
            "Diese Sitzung führt kein Python aus: Zellen behalten die in der Datei gespeicherten Werte."
        }
        "This workbook imports packages this Python doesn't have." => "Diese Arbeitsmappe importiert Pakete, die dieses Python nicht hat.",
        "pandas isn't installed in this Python, so ranges come back as lists of rows." => {
            "pandas ist in diesem Python nicht installiert, daher werden Bereiche als Listen von Zeilen zurückgegeben."
        }
        "The initialization code failed, so Python cells can't run." => {
            "Der Initialisierungscode ist fehlgeschlagen, daher können Python-Zellen nicht ausgeführt werden."
        }
        "This code isn't valid Python." => "Dieser Code ist kein gültiges Python.",
        "The code raised an error." => "Der Code hat einen Fehler ausgelöst.",
        "The code uses a name that isn't defined." => "Der Code verwendet einen nicht definierten Namen.",
        "This name is defined in a later cell. Python cells run in row order, sheet by sheet." => {
            "Dieser Name ist in einer späteren Zelle definiert. Python-Zellen werden in Zeilenreihenfolge ausgeführt, Blatt für Blatt."
        }
        "This Python doesn't have that package." => "Dieses Python hat dieses Paket nicht.",
        "This package needs a library outside what the sandbox lets Python read." => {
            "Dieses Paket benötigt eine Bibliothek außerhalb dessen, was die Sandbox Python lesen lässt."
        }
        "Python cells can't read or write files on your computer. Put the data on a sheet and read it with xl()." => {
            "Python-Zellen können keine Dateien auf Ihrem Computer lesen oder schreiben. Legen Sie die Daten in ein Blatt und lesen Sie sie mit xl()."
        }
        "Python cells can't use the network. Put the data on a sheet and read it with xl()." => {
            "Python-Zellen können das Netzwerk nicht verwenden. Legen Sie die Daten in ein Blatt und lesen Sie sie mit xl()."
        }
        "Python cells can't start other programs or processes." => "Python-Zellen können keine anderen Programme oder Prozesse starten.",
        "This picture is too large to draw. Make figsize or dpi smaller." => {
            "Dieses Bild ist zu groß zum Zeichnen. Verkleinern Sie figsize oder dpi."
        }
        "The picture appears in the cell by itself. Python cells can't write files, so this line would fail." => {
            "Das Bild erscheint von selbst in der Zelle. Python-Zellen können keine Dateien schreiben, daher würde diese Zeile fehlschlagen."
        }
        "This package draws in a web browser, which Python cells can't open. Use matplotlib or seaborn." => {
            "Dieses Paket zeichnet in einem Webbrowser, den Python-Zellen nicht öffnen können. Verwenden Sie matplotlib oder seaborn."
        }
        "Python Output" => "Python-Ausgabe",
        "Display Plot over Cells" => "Diagramm über Zellen anzeigen",
        "Create Reference" => "Bezug erstellen",
        "Copy Image" => "Bild kopieren",
        "Save Image…" => "Bild speichern…",
        "The image is saved." => "Das Bild wurde gespeichert.",
        "The image is on the clipboard." => "Das Bild befindet sich in der Zwischenablage.",
        "Image" => "Bild",
        "size" => "Größe",
        "Resize" => "Größe ändern",
        "This picture shows the last result that cell gave." => "Dieses Bild zeigt das letzte Ergebnis dieser Zelle.",
        "Select a Python cell that gave a picture first." => "Wählen Sie zuerst eine Python-Zelle aus, die ein Bild geliefert hat.",
        "xl() needs its reference written as text in the code, such as xl(\"A1:B5\")." => {
            "xl() benötigt seinen Bezug als Text im Code, etwa xl(\"A1:B5\")."
        }
        "This range is too large to send to Python." => "Dieser Bereich ist zu groß, um an Python gesendet zu werden.",
        "xl() names something that isn't a cell, range, table or defined name here." => {
            "xl() nennt etwas, das hier keine Zelle, kein Bereich, keine Tabelle und kein definierter Name ist."
        }
        "The code ran past the time limit. Python was restarted, so its variables were cleared." => {
            "Der Code hat das Zeitlimit überschritten. Python wurde neu gestartet, daher wurden seine Variablen gelöscht."
        }
        "The code went over the memory limit. Python was restarted, so its variables were cleared." => {
            "Der Code hat das Speicherlimit überschritten. Python wurde neu gestartet, daher wurden seine Variablen gelöscht."
        }
        "The result is too large to bring back to the sheet." => "Das Ergebnis ist zu groß, um es zurück ins Blatt zu holen.",
        "The printed output was cut off at its limit." => "Die ausgegebene Ausgabe wurde am Limit abgeschnitten.",
        "Python stopped unexpectedly and was restarted, so its variables were cleared." => {
            "Python wurde unerwartet beendet und neu gestartet, daher wurden seine Variablen gelöscht."
        }
        "The result is empty." => "Das Ergebnis ist leer.",
        "The result needs cells that aren't empty." => "Das Ergebnis benötigt Zellen, die nicht leer sind.",
        "The result isn't a number a cell can hold." => "Das Ergebnis ist keine Zahl, die eine Zelle aufnehmen kann.",
        "This date or length of time is outside what a cell can hold." => {
            "Dieses Datum oder diese Zeitspanne liegt außerhalb dessen, was eine Zelle aufnehmen kann."
        }
        "This cell is running." => "Diese Zelle wird ausgeführt.",
        "What this cell reads has changed since it last ran." => "Was diese Zelle liest, hat sich seit der letzten Ausführung geändert.",
        "This cell was stopped before it finished." => "Diese Zelle wurde vor dem Abschluss beendet.",
        "xl() needs its reference written as text, such as xl(\"A1:B5\"). A computed reference can't be read." => {
            "xl() benötigt seinen Bezug als Text, etwa xl(\"A1:B5\"). Ein berechneter Bezug kann nicht gelesen werden."
        }
        "This module needs the network or other programs, which Python cells can't use." => {
            "Dieses Modul benötigt das Netzwerk oder andere Programme, die Python-Zellen nicht verwenden können."
        }
        "This Python doesn't have this package." => "Dieses Python hat dieses Paket nicht.",
        "Python cells can't open windows or ask for input." => "Python-Zellen können keine Fenster öffnen oder Eingaben anfordern.",
        "Python cells don't see your environment variables." => "Python-Zellen sehen Ihre Umgebungsvariablen nicht.",
        "Open Settings" => "Einstellungen öffnen",
        "Show Printed Output" => "Gedruckte Ausgabe anzeigen",
        "Hide Printed Output" => "Gedruckte Ausgabe ausblenden",
        "Line {line}" => "Zeile {line}",
        "Line {line}: {message}" => "Zeile {line}: {message}",
        "Python {version}, ready" => "Python {version}, bereit",
        "Python {version}, running" => "Python {version}, wird ausgeführt",
        _ => return None,
    })
}

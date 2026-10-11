# Engine error messages. Key: error-<code>; the codes mirror gridcraft_engine::EngineError.
# Original wording for GridCraft (English reference).

error-unknown-command = Unknown command “{ $command }”.
error-command-disabled = “{ $command }” is not available right now: { $reason }
error-bad-params = Invalid parameters for “{ $command }”: { $message }
error-no-document = No workbook is open.
error-internal = Internal error in “{ $command }” (the workbook was kept as it was): { $message }
error-other = { $message }
error-invalid-formula = There is a problem with this formula: { $message }
error-invalid-locale = The language or regional settings are not valid: { $message }
error-protected-sheet = This change is not allowed: the cell or chart is on a protected sheet.
error-encrypted-workbook = “{ $name }” is password-protected. Remove the password in Excel and try again; GridCraft cannot open encrypted workbooks yet.
error-legacy-workbook = “{ $name }” is an Excel 97–2003 (.xls) workbook or another legacy binary file. Save it as .xlsx in Excel and try again.
error-import-failed = Cannot import “{ $name }”: { $message }
error-import-only-format = { $format } supports data import only. Save as .xlsx or another supported export format.
error-text-box-too-long = Text boxes can hold at most { $limit } characters.
error-chart-range-too-large = The source range is too large for a chart.

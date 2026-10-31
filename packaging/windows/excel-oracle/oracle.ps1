<#
Excel function oracle: takes the function calls a pull request asserts in its tests, evaluates each in real Excel
(COM) and writes a comparison of what the PR expects against what Excel returns, plus the answers in a form the
corpus repo can hold for `gridcraft-cli check`.

  gh pr diff 2463 --repo storytold/gridcraft > pr.diff
  oracle.ps1 -Diff pr.diff [-Out C:\oracle-out] [-TimeoutSec 60]
  oracle.ps1 -CaseFile cases.tsv [-Out ...]        # cases written by hand or kept from an earlier run

Cases come from the lines a diff adds. Recognised assertions (crates/functions test helpers):
  close(ev("F", vec![n(1.0), t("a"), b(true), e(CellError::Num), empty(), col(&[1.0, 2.0])]), 3.5)
  close_tol(..., 3.5, 1e-6)   is_text(ev(..), "x")   is_err(ev(..), CellError::Num)   assert_eq!(ev(..), nv(1.0))
Arguments `d(y, m, d)` become DATE(y,m,d); col(&[..]) / row(&[..]) become array constants. A call with other
argument shapes (references, nested values) is listed as SKIP with the reason; a bare `ev(..)` is evaluated with no
expectation. Calls that were only context lines in the diff, and tests that build a workbook (calc/tests.rs), are
not read: give those as -CaseFile.

Writes into -Out:
  cases.tsv     id, Excel formula, expected (kind:value), tab separated with \\ \t \r \n escapes - input for a rerun,
                and the key to oracle.xlsx
  results.tsv   id, formula, expected, excel, verdict (MATCH / DIFF / NOEXPECT / SKIP)
  results.md    the same as a table for a PR comment
  oracle.xlsx   saved by Excel: sheet Oracle holds one formula per row (A id, B formula with Excel's cached value,
                C expected); array results spill on sheet Arrays. A workbook with formula cells and cached
                values is what `gridcraft-cli check` takes as oracle. Never commit it to this repository; it
                belongs in the corpus repo.

Excel is a temporary workbook in an instance this script started, DisplayAlerts off, macros disabled; it is closed
without saving over anything but oracle.xlsx and quit. A hang past -TimeoutSec kills only that instance (found by
its window's process id). Verdicts: MATCH, DIFF, NOEXPECT (no assertion around the call), SKIP (an argument or
expectation not read, or a function that is never evaluated; the reason is in the Excel column) and FAIL (Excel timed
out or failed). Worksheet functions that reach outside the workbook (WEBSERVICE, CALL, RTD, IMAGE...) are never
evaluated: a diff or cases file is untrusted input. Exit code 1 when any case is DIFF or the run fails.
#>
param(
    [string]$Diff,
    [string]$CaseFile,
    [string]$Out = (Join-Path ([IO.Path]::GetTempPath()) 'gridcraft-excel-oracle'),
    [int]$TimeoutSec = 60
)
$ErrorActionPreference = 'Stop'
if (-not $Diff -and -not $CaseFile) { throw 'give -Diff <pr.diff> or -CaseFile <cases.tsv>' }
$inv = [Globalization.CultureInfo]::InvariantCulture
$errText = @{ Null = '#NULL!'; Div0 = '#DIV/0!'; Value = '#VALUE!'; Ref = '#REF!'; Name = '#NAME?'; Num = '#NUM!'; NA = '#N/A'; Spill = '#SPILL!'; Calc = '#CALC!' }

# ---------------------------------------------------------------- Rust source scanning

# Index of the bracket that closes the one at $open; strings are skipped. -1 when unbalanced.
function Find-Close([string]$s, [int]$open) {
    $depth = 0
    for ($i = $open; $i -lt $s.Length; $i++) {
        $c = $s[$i]
        if ($c -eq '"') {
            $i++
            while ($i -lt $s.Length -and $s[$i] -ne '"') { if ($s[$i] -eq '\') { $i++ }; $i++ }
        } elseif ($c -in '(', '[', '{') { $depth++ }
        elseif ($c -in ')', ']', '}') { $depth--; if ($depth -eq 0) { return $i } }
    }
    -1
}

# Splits on commas outside brackets and strings; an empty piece is kept when it is not the trailing one. Callers wrap the result in @().
function Split-Top([string]$s) {
    $parts = @(); $depth = 0; $from = 0
    for ($i = 0; $i -lt $s.Length; $i++) {
        $c = $s[$i]
        if ($c -eq '"') {
            $i++
            while ($i -lt $s.Length -and $s[$i] -ne '"') { if ($s[$i] -eq '\') { $i++ }; $i++ }
        } elseif ($c -in '(', '[', '{') { $depth++ }
        elseif ($c -in ')', ']', '}') { $depth-- }
        elseif ($c -eq ',' -and $depth -eq 0) { $parts += $s.Substring($from, $i - $from).Trim(); $from = $i + 1 }
    }
    $tail = $s.Substring($from).Trim()
    if ($tail -ne '') { $parts += $tail }
    $parts
}

function ConvertFrom-RustString([string]$lit) {
    if ($lit -notmatch '^"(.*)"$' -and $lit -notmatch '^r#*"(.*)"#*$') { return $null }
    $body = $Matches[1]
    if ($lit.StartsWith('r')) { return $body }
    [regex]::Replace($body, '\\(u\{([0-9a-fA-F]+)\}|.)', {
        param($m)
        if ($m.Groups[2].Success) { return [string][char]::ConvertFromUtf32([Convert]::ToInt32($m.Groups[2].Value, 16)) }
        switch ($m.Groups[1].Value) { 'n' { "`n" } 't' { "`t" } 'r' { "`r" } default { $m.Groups[1].Value } }
    })
}

# Worksheet functions that reach outside the workbook (network, code, files); a PR diff or cases file is untrusted
# input and DisplayAlerts / AutomationSecurity do not stop worksheet functions, so these are never evaluated.
$denied = '\b(WEBSERVICE|FILTERXML|CALL|REGISTER(\.ID)?|RTD|SQL\.REQUEST|HYPERLINK|EXEC|IMAGE|STOCKHISTORY|CUBE\w+)\s*\('
function Get-DeniedReason([string]$formula) {
    if ($formula -match $denied) { return "function not evaluated (external I/O): $($Matches[1])" }
    # A reference into another workbook or a path would read a file; text and error literals may hold '!' or '[' without naming one, so they go first.
    $outside = [regex]::Replace($formula, '"(?:[^"]|"")*"', '""')
    $outside = [regex]::Replace($outside, (($errText.Values | ForEach-Object { [regex]::Escape($_) }) -join '|'), '')
    if ($outside -match "[\[\]!\\']") { return "reference outside the workbook not evaluated: $formula" }
    $null
}

# A tab or newline inside a case would break the TSV, so fields are escaped (\\ \t \r \n) and decoded when a cases file is read.
function ConvertTo-Cell([string]$s) { ($s -replace '\\', '\\' -replace "`t", '\t' -replace "`r", '\r' -replace "`n", '\n') }
function ConvertFrom-Cell([string]$s) {
    [regex]::Replace($s, '\\(.)', { switch ($args[0].Groups[1].Value) { 't' { "`t" } 'r' { "`r" } 'n' { "`n" } default { $args[0].Groups[1].Value } } })
}

function ConvertTo-ExcelText([string]$s) { '"' + $s.Replace('"', '""') + '"' }

# Rust numeric expression -> Excel expression; $null when it holds anything but arithmetic.
function ConvertTo-ExcelNumber([string]$e) {
    $e = ($e -replace '(\d)_?f64', '$1' -replace 'f64::', '').Trim()
    if ($e -match '^(-?)\s*d\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\)$') {
        return "$($Matches[1])DATE($($Matches[2]),$($Matches[3]),$($Matches[4]))"
    }
    if ($e -match '^[\d.eE+\-*/() ]+$') { return $e }
    $null
}

function ConvertTo-ExcelError([string]$e) {
    if ($e -match '^(?:Value::Error\(\s*)?(?:CellError::)(\w+)\)?$' -and $errText.ContainsKey($Matches[1])) { return $errText[$Matches[1]] }
    $null
}

# One argument of ev(..) -> Excel text, or $null with the reason in $script:why.
function ConvertTo-ExcelArg([string]$a) {
    if ($a -match '^n\((.*)\)$') { $x = ConvertTo-ExcelNumber $Matches[1]; if ($null -ne $x) { return $x } }
    elseif ($a -match '^t\((.*)\)$') { $x = ConvertFrom-RustString $Matches[1]; if ($null -ne $x) { return ConvertTo-ExcelText $x } }
    elseif ($a -match '^b\((true|false)\)$') { return $Matches[1].ToUpper() }
    elseif ($a -match '^e\((.*)\)$') { $x = ConvertTo-ExcelError $Matches[1]; if ($null -ne $x) { return $x } }
    elseif ($a -eq 'empty()') { return '' }
    elseif ($a -match '^(?:rf\(|av\()?(col|row)\(&\[(.*?)\]\)\)?$') {
        $nums = @(Split-Top $Matches[2] | ForEach-Object { ConvertTo-ExcelNumber $_ })
        if ($nums -notcontains $null -and $nums.Count) { return '{' + ($nums -join $(if ($Matches[1] -eq 'col') { ';' } else { ',' })) + '}' }
    }
    $script:why = "argument not supported: $a"
    $null
}

# What the assertion around ev(..) expects: @{ Kind = num|text|err|bool|none|skip; Value; Tol }.
function Get-Expected([string]$wrapper, [string[]]$rest) {
    $none = @{ Kind = 'none'; Value = ''; Tol = 1e-9 }
    if (-not $wrapper -or $rest.Count -eq 0) { return $none }
    $x = $rest[0]
    switch -Regex ($wrapper) {
        '^close(_tol)?$' {
            $num = ConvertTo-ExcelNumber $x
            if ($null -eq $num) { return @{ Kind = 'skip'; Value = "expected not arithmetic: $x"; Tol = 0 } }
            $tol = 1e-9
            if ($wrapper -eq 'close_tol' -and $rest.Count -gt 1) {
                $t = ConvertTo-ExcelNumber $rest[1]
                $parsed = 0.0
                if ($null -eq $t -or -not [double]::TryParse($t, [Globalization.NumberStyles]::Float, $inv, [ref]$parsed)) {
                    return @{ Kind = 'skip'; Value = "tolerance not a literal: $($rest[1])"; Tol = 0 }
                }
                $tol = $parsed
            }
            return @{ Kind = 'num'; Value = $num; Tol = $tol }
        }
        '^is_text$' { $s = ConvertFrom-RustString $x; if ($null -ne $s) { return @{ Kind = 'text'; Value = $s; Tol = 0 } } }
        '^is_err$' { $s = ConvertTo-ExcelError $x; if ($null -ne $s) { return @{ Kind = 'err'; Value = $s; Tol = 0 } } }
        '^assert_eq!$' {
            if ($x -match '^(?:nv|Value::Number)\((.*)\)$') { $num = ConvertTo-ExcelNumber $Matches[1]; if ($null -ne $num) { return @{ Kind = 'num'; Value = $num; Tol = 1e-9 } } }
            elseif ($x -match '^(?:tv|Value::from|Value::Text)\((.*)\)$') { $s = ConvertFrom-RustString $Matches[1]; if ($null -ne $s) { return @{ Kind = 'text'; Value = $s; Tol = 0 } } }
            elseif ($x -match '^Value::Bool\((true|false)\)$') { return @{ Kind = 'bool'; Value = $Matches[1].ToUpper(); Tol = 0 } }
            elseif ($x -match '^Value::Error\(') { $s = ConvertTo-ExcelError $x; if ($null -ne $s) { return @{ Kind = 'err'; Value = $s; Tol = 0 } } }
        }
    }
    @{ Kind = 'skip'; Value = "expectation not recognised: $wrapper($x)"; Tol = 0 }
}

# Every ev("F", vec![..]) call in the added lines of a unified diff.
function Read-Diff([string]$path) {
    $cases = @(); $text = New-Object Text.StringBuilder; $file = ''
    $flush = {
        param($file, $src)
        if ($file -notmatch '^crates/functions/') { return } # only the function tests are documented as read
        $tests = [regex]::Matches($src, '\bfn\s+(\w+)\s*\(')
        foreach ($m in [regex]::Matches($src, '\bev\(\s*"([A-Za-z_][\w.]*)"\s*,\s*vec!\s*\[')) {
            $script:why = $null
            $name = $m.Groups[1].Value
            $argsOpen = $m.Index + $m.Length - 1
            $argsClose = Find-Close $src $argsOpen
            $callClose = Find-Close $src ($src.IndexOf('(', $m.Index))
            if ($argsClose -lt 0 -or $callClose -lt 0) { continue }
            $test = ($tests | Where-Object { $_.Index -lt $m.Index } | Select-Object -Last 1)
            $id = "$([IO.Path]::GetFileNameWithoutExtension($file))::" + $(if ($test) { $test.Groups[1].Value } else { '?' })
            $script:seen[$id] = 1 + [int]$script:seen[$id]
            $id += "#$($script:seen[$id])"
            $pre = $src.Substring([math]::Max(0, $m.Index - 40), [math]::Min(40, $m.Index))
            $wrapper = if ($pre -match '(close_tol|close|is_text|is_err|assert_eq!)\(\s*$') { $Matches[1] } else { $null }
            $rest = @()
            if ($wrapper -and $src.Substring($callClose + 1) -match '^\s*,') {
                # The wrapper's remaining arguments run to its closing parenthesis.
                $tail = $src.Substring($callClose + 1)
                $end = Find-Close ('(' + $tail) 0
                if ($end -gt 0) { $rest = @(Split-Top $tail.Substring(1, $end - 2)) }
            }
            $converted = @(Split-Top $src.Substring($argsOpen + 1, $argsClose - $argsOpen - 1) | ForEach-Object { ConvertTo-ExcelArg $_ })
            $exp = Get-Expected $wrapper $rest
            $formula = $null
            if (-not $script:why -and $exp.Kind -ne 'skip') { $formula = "=$($name.ToUpper())($($converted -join ','))" }
            if ($formula -and ($denial = Get-DeniedReason $formula)) { $formula = $null; $script:why = $denial }
            $skip = if ($script:why) { $script:why } elseif ($exp.Kind -eq 'skip') { $exp.Value } else { $null }
            $script:found += , [pscustomobject]@{ Id = $id; Formula = $formula; Kind = $exp.Kind; Expected = $exp.Value; Tol = $exp.Tol; Skip = $skip; Raw = $src.Substring($m.Index, $callClose - $m.Index + 1) }
        }
    }
    $script:found = @(); $script:seen = @{}
    foreach ($line in [IO.File]::ReadLines($path)) {
        if ($line -match '^diff --git a/\S+ b/(\S+)') {
            if ($text.Length) { & $flush $file $text.ToString(); [void]$text.Clear() }
            $file = $Matches[1]
        } elseif ($line.StartsWith('+') -and -not $line.StartsWith('+++')) { [void]$text.AppendLine($line.Substring(1)) }
        elseif ($line.StartsWith(' ') -and $text.Length) { [void]$text.AppendLine($line.Substring(1)) } # context keeps a test's lines together
    }
    if ($text.Length) { & $flush $file $text.ToString() }
    # Context lines were only there to join statements; a call counts only when it carries an added line.
    $added = [Collections.Generic.HashSet[string]]::new()
    foreach ($line in [IO.File]::ReadLines($path)) { if ($line.StartsWith('+') -and -not $line.StartsWith('+++')) { [void]$added.Add($line.Substring(1).Trim()) } }
    $script:found | Where-Object { $call = ($_.Raw -split "`r?`n" | Select-Object -First 1).Trim(); $added | Where-Object { $_.Contains($call) } | Select-Object -First 1 }
}

# ---------------------------------------------------------------- cases

New-Item -ItemType Directory -Path $Out -Force | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).ProviderPath
if ($Diff) { $cases = @(Read-Diff (Resolve-Path -LiteralPath $Diff).ProviderPath) }
else {
    $cases = @(Get-Content -Encoding UTF8 -LiteralPath $CaseFile | Where-Object { $_ -and -not $_.StartsWith('#') } | ForEach-Object {
        $f = $_ -split "`t", 3
        $formula = ConvertFrom-Cell $f[1]
        $kind, $val = 'none', ''
        if ($f.Count -gt 2 -and (ConvertFrom-Cell $f[2]) -match '(?s)^(num|text|err|bool):(.*)$') { $kind, $val = $Matches[1], $Matches[2] }
        # A numeric expectation is evaluated by Excel, so it must be plain arithmetic, like the ones read from a diff.
        $denial = Get-DeniedReason $formula
        # A line without a formula is skipped with a reason, or the rows of Excel's answers would pair with the wrong cases.
        if (-not $denial -and -not "$formula".Trim()) { $denial = 'no formula' }
        # A mistyped expectation (nmu:2) must not pass for a call that has none.
        $given = if ($f.Count -gt 2) { ConvertFrom-Cell $f[2] } else { '' }
        if (-not $denial -and $given.Trim() -and $kind -eq 'none') { $denial = "expectation not recognised: $given" }
        if (-not $denial -and $kind -eq 'num') {
            $norm = ConvertTo-ExcelNumber $val
            if ($null -eq $norm) { $denial = "expected not arithmetic: $val" } else { $val = $norm }
        }
        [pscustomobject]@{ Id = $f[0]; Formula = $(if ($denial) { $null } else { $formula }); Kind = $kind; Expected = $val; Tol = 1e-9; Skip = $denial; Raw = '' }
    })
}
if (-not $cases.Count) { throw 'no cases found' }
$todo = @($cases | Where-Object { $_.Formula })

# ---------------------------------------------------------------- Excel

# Child PowerShell so a hang can be killed after $TimeoutSec without taking this script with it.
$probe = {
    param($todoFile, $resultFile, $xlsx)
    $x = $null
    try {
        Add-Type -Namespace Win -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);'
        $x = New-Object -ComObject Excel.Application
        $xlPid = [uint32]0
        [void][Win.U]::GetWindowThreadProcessId([IntPtr]$x.Hwnd, [ref]$xlPid)
        "PID`t$xlPid"
        $x.Visible = $false; $x.DisplayAlerts = $false; $x.AutomationSecurity = 3
        $cases = Get-Content -Raw -LiteralPath $todoFile | ConvertFrom-Json
        $wb = $x.Workbooks.Add()
        $ws = $wb.Worksheets.Item(1); $ws.Name = 'Oracle'
        $arrays = $wb.Worksheets.Add([Type]::Missing, $ws); $arrays.Name = 'Arrays'
        $ws.Columns.Item(2).ColumnWidth = 40; $ws.Columns.Item(4).ColumnWidth = 30
        $out = @(); $k = 0; $i = 0
        foreach ($c in $cases) {
            $i++
            $ws.Cells.Item($i, 1).NumberFormat = '@' # an id that starts with = must stay text
            $ws.Cells.Item($i, 1).Value2 = $c.Id
            $ws.Cells.Item($i, 3).NumberFormat = '@'
            $ws.Cells.Item($i, 3).Value2 = "$($c.Kind):$($c.Expected)"
            $cell = $ws.Cells.Item($i, 2)
            $cell.NumberFormat = 'General'
            $rejected = $null
            # Excel refuses some formulas outright (wrong argument count): that is its answer for the case, not a failed run.
            try { $cell.Formula2 = $c.Formula } catch { try { $cell.Formula = $c.Formula } catch { $rejected = $_.Exception.Message -replace '\s+', ' ' } }
            if ($rejected) {
                $out += [pscustomobject]@{ Id = $c.Id; Type = 'rejected'; Excel = "formula rejected: $rejected"; Expected = $null }
                continue
            }
            $x.Calculate()
            # A multi-cell result is moved before the next row is written, or that row would block its spill.
            $multi = $false
            try { $multi = $cell.HasSpill -and $cell.SpillingToRange.Cells.Count -gt 1 } catch {}
            if ($cell.Text -eq '#SPILL!' -or $multi) {
                $cell.Clear()
                $k++
                $cell = $arrays.Cells.Item(2, 1 + 40 * ($k - 1))
                $arrays.Cells.Item(1, 1 + 40 * ($k - 1)).NumberFormat = '@'
                $arrays.Cells.Item(1, 1 + 40 * ($k - 1)).Value2 = $c.Id
                try { $cell.Formula2 = $c.Formula } catch { $cell.Formula = $c.Formula }
                $x.Calculate()
                $ws.Cells.Item($i, 2).Value2 = "see Arrays!R2C$(1 + 40 * ($k - 1))"
            }
            $cell.Columns.Item(1).ColumnWidth = 24
            $shown = ''; $type = 'num'
            $spill = $null
            try { if ($cell.HasSpill) { $spill = $cell.SpillingToRange } } catch {}
            if ($spill) {
                $v = $spill.Value2; $rows = $spill.Rows.Count; $cols = $spill.Columns.Count
                $parts = for ($r = 1; $r -le [math]::Min($rows, 20); $r++) {
                    (1..[math]::Min($cols, 20) | ForEach-Object { $e = $spill.Cells.Item($r, $_); if ($e.Value2 -is [int]) { $e.Text } else { [string]$e.Value2 } }) -join ','
                }
                $shown = '{' + ($parts -join ';') + '}'; $type = 'array'
            } else {
                $v = $cell.Value2
                if ($v -is [int] -and $cell.Text -match '^#') { $shown = $cell.Text; $type = 'err' }
                elseif ($v -is [bool]) { $shown = "$v".ToUpper(); $type = 'bool' }
                elseif ($v -is [string]) { $shown = $v; $type = 'text' }
                elseif ($null -eq $v) { $shown = ''; $type = 'text' }
                else { $shown = ([double]$v).ToString('R', [Globalization.CultureInfo]::InvariantCulture); $type = 'num' }
            }
            $exp = $null
            if ($c.Kind -eq 'num') { try { $exp = [double]$x.Evaluate($c.Expected) } catch {} }
            $out += [pscustomobject]@{ Id = $c.Id; Type = $type; Excel = $shown; Expected = $exp }
        }
        $ws.Activate()
        $wb.SaveAs($xlsx, 51) # xlOpenXMLWorkbook
        $wb.Close($false)
        ConvertTo-Json -InputObject @($out) -Depth 4 | Set-Content -LiteralPath $resultFile -Encoding UTF8
        'DONE'
    } catch {
        "FAIL`t" + ($_.Exception.Message -replace '\s+', ' ')
    } finally {
        if ($x) { try { $x.Quit() } catch {} }
    }
}

$xlsx = Join-Path $Out 'oracle.xlsx'
Remove-Item -LiteralPath $xlsx -Force -ErrorAction SilentlyContinue # never leave an earlier run's workbook beside the new cases
$state = 'OK'
if ($todo.Count) {
    $todoFile = Join-Path $Out 'todo.json'; $resultFile = Join-Path $Out 'excel.json'
    Remove-Item -LiteralPath $resultFile -Force -ErrorAction SilentlyContinue
    ConvertTo-Json -InputObject @($todo | Select-Object Id, Formula, Kind, Expected) | Set-Content -LiteralPath $todoFile -Encoding UTF8
    # Excel cannot save to a network path (a WSL share), so it saves locally and the file is copied.
    $local = Join-Path ([IO.Path]::GetTempPath()) ('gridcraft-oracle-' + [guid]::NewGuid().ToString('N') + '.xlsx')
    $job = Start-Job -ScriptBlock $probe -ArgumentList $todoFile, $resultFile, $local
    $done = Wait-Job $job -Timeout $TimeoutSec
    $lines = @(Receive-Job $job -Keep -ErrorAction SilentlyContinue)
    $xlPid = ($lines | Where-Object { $_ -like "PID`t*" } | Select-Object -First 1) -replace '^PID\t', ''
    # Excel goes first: a job blocked in a COM call may not return from Stop-Job.
    if (-not $done -and $xlPid) { Stop-Process -Id ([int]$xlPid) -Force -ErrorAction SilentlyContinue }
    if (-not $done) { Stop-Job $job }
    Remove-Job $job -Force
    if ($xlPid) { Wait-Process -Id ([int]$xlPid) -Timeout 15 -ErrorAction SilentlyContinue; Stop-Process -Id ([int]$xlPid) -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $todoFile -Force
    # Only a finished run leaves a workbook: a killed SaveAs can leave a partial file.
    if ($done -and ($lines | Select-Object -Last 1) -eq 'DONE' -and (Test-Path -LiteralPath $local)) { Move-Item -LiteralPath $local -Destination $xlsx -Force }
    else { Remove-Item -LiteralPath $local -Force -ErrorAction SilentlyContinue }
    if (-not $done) {
        $state = "TIMEOUT: no answer in $TimeoutSec s"
        # Excel was still starting, so its process id was never reported and nothing is stopped: it may be this run's or someone else's.
        if (-not $xlPid) { $state += '; Excel never reported its process id, so none was stopped (check for a stray EXCEL.EXE started with /automation)' }
    }
    elseif (($lines | Select-Object -Last 1) -ne 'DONE') { $state = "FAIL: $($lines | Select-Object -Last 1)" }
}

# ---------------------------------------------------------------- comparison

# Results come back in case order; pair by position so two cases in one test (same id) stay apart.
# Windows PowerShell 5.1 hands a JSON array back as one object, so it is enumerated explicitly.
$excelRows = @(if ($state -eq 'OK' -and $todo.Count) { $parsed = ConvertFrom-Json (Get-Content -Raw -LiteralPath (Join-Path $Out 'excel.json')); foreach ($r in $parsed) { $r } })
$rows = @(); $j = 0
foreach ($c in $cases) {
    $excel = ''; $verdict = 'SKIP'; $exp = ConvertTo-Cell "$($c.Kind):$($c.Expected)"
    if ($c.Kind -eq 'none') { $exp = '-' }
    if ($c.Skip) { $excel = $c.Skip }
    elseif ($state -ne 'OK') { $excel = $state; $verdict = 'FAIL' }
    else {
        $r = $excelRows[$j++]
        $excel = ConvertTo-Cell $r.Excel
        $verdict = switch ($c.Kind) {
            'none' { 'NOEXPECT' }
            'num' {
                if ($r.Type -ne 'num' -or $null -eq $r.Expected) { 'DIFF' }
                else { $d = [math]::Abs([double]$r.Excel - [double]$r.Expected); if ($d -le $c.Tol * [math]::Max(1.0, [math]::Abs([double]$r.Expected))) { 'MATCH' } else { 'DIFF' } }
            }
            'bool' { if ($r.Type -eq 'bool' -and $r.Excel -eq $c.Expected) { 'MATCH' } else { 'DIFF' } }
            'err' { if ($r.Type -eq 'err' -and $r.Excel -eq $c.Expected) { 'MATCH' } else { 'DIFF' } }
            default { if ($r.Type -eq 'text' -and $r.Excel -ceq $c.Expected) { 'MATCH' } else { 'DIFF' } }
        }
        if ($c.Kind -eq 'num' -and $null -ne $r.Expected) { $exp = 'num:' + ([double]$r.Expected).ToString('R', $inv) }
    }
    $rows += [pscustomobject]@{ Id = $c.Id; Formula = ConvertTo-Cell $c.Formula; Expected = $exp; Excel = $excel; Verdict = $verdict }
}

$tsv = @($rows | ForEach-Object { "$($_.Id)`t$($_.Formula)`t$($_.Expected)`t$($_.Excel)`t$($_.Verdict)" })
$utf8 = [Text.UTF8Encoding]::new($false) # no BOM: the files are meant for other tools
[IO.File]::WriteAllLines((Join-Path $Out 'results.tsv'), [string[]](@("id`tformula`texpected`texcel`tverdict") + $tsv), $utf8)
# cases.tsv is a rerun input; evaluated numbers replace expressions so it can run without the diff.
[IO.File]::WriteAllLines((Join-Path $Out 'cases.tsv'), [string[]]@($rows | Where-Object { $_.Formula } | ForEach-Object { "$($_.Id)`t$($_.Formula)`t$(if ($_.Expected -eq '-') { '' } else { $_.Expected })" }), $utf8)
$md = @('| Case | Formula | PR expects | Excel | |', '|---|---|---|---|---|')
$md += $rows | ForEach-Object { $cells = @($_.Id, $_.Formula, $_.Expected, $_.Excel) | ForEach-Object { '`' + ($_ -replace '`', "'" -replace '\|', '\|') + '`' }; "| $($cells -join ' | ') | $($_.Verdict) |" }
[IO.File]::WriteAllLines((Join-Path $Out 'results.md'), [string[]]$md, $utf8)

$count = @{}; $rows | Group-Object Verdict | ForEach-Object { $count[$_.Name] = $_.Count }
$rows | Format-Table -AutoSize -Wrap | Out-String -Width 220 | Write-Output
"{0} cases: {1}" -f $rows.Count, (($count.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Value) $($_.Name)" }) -join ', ')
if ($state -ne 'OK') { $state }
if ($state -ne 'OK' -or $count['DIFF']) { exit 1 }

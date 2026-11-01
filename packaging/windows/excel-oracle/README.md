# Excel function oracle

Runs the function calls a pull request asserts through real Excel (COM) and writes what the PR expects next to what
Excel returns. Windows with Excel only (Windows PowerShell 5.1 is enough). See `oracle.ps1` for the recognised
assertions and the options.

```powershell
gh pr diff 2463 --repo storytold/gridcraft > pr.diff
.\oracle.ps1 -Diff pr.diff -Out C:\oracle-out
.\oracle.ps1 -CaseFile C:\oracle-out\cases.tsv -Out C:\oracle-out2   # rerun the same cases
```

From WSL: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <path>\oracle.ps1 ...` (Excel cannot save to a
`\\wsl.localhost` path, so the script saves locally and copies).

Output in `-Out`:

- `results.md` / `results.tsv`: per case `MATCH`, `DIFF`, `NOEXPECT` (no assertion around the call), `SKIP` (an
  argument or expectation the script does not read, or a function that reaches outside the workbook such as
  `WEBSERVICE`, which is never evaluated; the reason is in the Excel column) or `FAIL` (Excel timed out or failed).
  A formula Excel refuses (wrong argument count) is Excel's answer for that case and shows as `formula rejected`.
  Exit code 1 on any `DIFF` or a failed Excel run.
- `oracle.xlsx`: saved by Excel, one formula per row on sheet `Oracle` (id, formula with Excel's cached value,
  expected); array results spill on sheet `Arrays`. This is the shape `gridcraft-cli check` takes as oracle (formula
  cells with the producer's cached values), so it is what the corpus repo holds. It is Excel output: never commit it
  here.
- `cases.tsv`: id, formula, expected; keep it with the workbook to regenerate it.

`ev("F", vec![..])` calls in `crates/functions` tests are read from the lines the diff adds (other files are ignored). Tests that build a
workbook (`crates/calc/src/tests.rs`) are not parsed; write their formulas into a cases file.

Excel runs in a child process with a timeout (60 s by default), as a new instance with alerts off and macros disabled.
On a hang only that instance is stopped. If startup times out before Excel reports its process id, nothing is stopped
(the result says so): look for a stray `EXCEL.EXE` started with `/automation`.
Formulas that call a function reaching outside the workbook, or that name another workbook or a path, are skipped.

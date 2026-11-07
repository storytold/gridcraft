# Public workbook corpus

The manifest pins 24 Apache POI spreadsheet test workbooks to commit `2c00b85` and records each file's SHA-256. Apache POI is licensed under Apache-2.0. The workbooks are downloaded into a temporary directory for each run and are never stored in this repository.

`run.sh` checks every download against its manifest hash, then runs `gridcraft-cli check --json` over the set. The report records cached-value comparison and XLSX round-trip results; mismatches are measurements to compare between runs, not a zero-failure acceptance rule. The check command's exit code 1 is expected for mismatches. Download, hash, usage, or malformed-report errors still fail the script.

GitHub Actions runs this job weekly and on demand. It is not a required check. These files are not an Excel-computed oracle and do not verify that Excel opens GridCraft's saved workbooks without repair; that needs separate Office verification.

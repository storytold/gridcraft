param(
    [string]$Executable = "target/release/examples/large_workbooks.exe",
    [string]$Label = "current",
    [string[]]$Datasets = @("A", "B", "C", "F", "G", "H", "D", "E"),
    [int]$Repeats = 5,
    [string]$InputWorkbook = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$exe = (Resolve-Path (Join-Path $root $Executable)).Path
$outputDir = Join-Path $root "target/perf/$Label"
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
git -C $root rev-parse HEAD | Set-Content (Join-Path $outputDir "commit.txt")
git -C $root status --short | Set-Content (Join-Path $outputDir "working-tree.txt")
rustc -Vv | Set-Content (Join-Path $outputDir "rustc.txt")
Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors | Format-List | Out-File (Join-Path $outputDir "hardware.txt")
Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory | Format-List | Out-File (Join-Path $outputDir "hardware.txt") -Append
Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version | Format-List | Out-File (Join-Path $outputDir "hardware.txt") -Append
if ($InputWorkbook) {
    $InputWorkbook = (Resolve-Path -LiteralPath $InputWorkbook).Path
    Get-FileHash -LiteralPath $InputWorkbook -Algorithm SHA256 | Format-List | Out-File (Join-Path $outputDir "input-hash.txt")
    $Datasets = @("USER")
}
Get-FileHash -LiteralPath $exe -Algorithm SHA256 | Format-List | Out-File (Join-Path $outputDir "executable-hash.txt")
foreach ($dataset in $Datasets) {
    $rows = switch ($dataset) { "D" {500000} "E" {1000000} default {100000} }
    $stdout = Join-Path $outputDir "$dataset.txt"
    $stderr = Join-Path $outputDir "$dataset.stderr.txt"
    $benchmarkArgs = if ($InputWorkbook) { @('"' + $InputWorkbook + '"', $Repeats) } else { @($dataset, $rows, $Repeats) }
    $process = Start-Process -FilePath $exe -ArgumentList $benchmarkArgs -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    # Keep a handle open: Windows PowerShell otherwise loses ExitCode after Refresh/exit.
    $null = $process.Handle
    $peak = 0L
    $sampled = 0L
    while (-not $process.HasExited) {
        $process.Refresh()
        $peak = [Math]::Max($peak, $process.PeakWorkingSet64)
        $sampled = [Math]::Max($sampled, $process.WorkingSet64)
        Start-Sleep -Milliseconds 100
    }
    $process.WaitForExit()
    "exit_code=$($process.ExitCode) peak_working_set_bytes=$peak sampled_peak_bytes=$sampled sampling_ms=100" | Set-Content (Join-Path $outputDir "$dataset.memory.txt")
    Get-Content $stdout
    Get-Content (Join-Path $outputDir "$dataset.memory.txt")
    if ($process.ExitCode -ne 0) { throw "Benchmark $dataset failed; see $stderr" }
}

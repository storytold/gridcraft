<#
Excel open check: saves workbooks with GridCraft, opens each result in real Excel (COM) and records per
file whether Excel loads it as it is or has to repair it (docs/file-format-parity.md, "Excel open check").

  check.ps1 -Cli C:\gridcraft\gridcraft-cli.exe [-Inputs C:\workbooks] [-Out C:\out] [-Csv result.csv]
            [-TimeoutSec 120]

Files saved:
  - every .xlsx / .xlsm in -Inputs, opened and saved with `gridcraft-cli run --in --out` (the original
    is opened in Excel too, so a file Excel itself dislikes or loads slowly is told apart from a
    GridCraft regression);
  - every *.jsonl scenario next to this script (one feature each), run on the Sales sample.

Each file is opened read-only with CorruptLoad = xlNormalLoad (0), so a part Excel would repair is a
failure instead of a silent fix, with DisplayAlerts off and VBA macros disabled so nothing waits on a prompt.
AutomationSecurity does not cover Excel 4.0 (XLM) macros, which would still prompt, so a workbook that
holds XLM macro sheets is never opened: it is reported as SKIP-XLM and the run does not pass.
Excel is a fresh instance per file; a file that hangs or kills Excel is recorded as TIMEOUT or CRASH and
that instance (found by its window's process id) is stopped. Nothing is written next to the inputs; any other
Excel, such as one the person starts meanwhile, is never touched. Exit code 1 when any saved file is not PASS.
#>
param(
    [Parameter(Mandatory = $true)][string]$Cli,
    [string]$Inputs,
    [string]$Out = (Join-Path ([IO.Path]::GetTempPath()) 'gridcraft-excel-open-check'),
    [string]$Csv,
    [int]$TimeoutSec = 120
)
$ErrorActionPreference = 'Stop'

# Runs in a child PowerShell so a hang can be killed after $TimeoutSec without taking this script with it.
$probe = {
    param($path)
    $x = $null
    try {
        # Compiled before Excel starts, so nothing between the start and the PID line can fail or stall on it.
        Add-Type -Namespace Win -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);'
        $x = New-Object -ComObject Excel.Application
        # Report this instance's process id so the caller can stop exactly it and nothing else.
        $xlPid = [uint32]0
        [void][Win.U]::GetWindowThreadProcessId([IntPtr]$x.Hwnd, [ref]$xlPid)
        "PID`t$xlPid"
        $x.Visible = $false
        $x.DisplayAlerts = $false
        $x.AutomationSecurity = 3 # msoAutomationSecurityForceDisable: no VBA macros (XLM is screened by Test-HasXlm)
        # Named arguments: PowerShell can't skip the optional ones in between positionally.
        $wb = [System.__ComObject].InvokeMember('Open', [Reflection.BindingFlags]::InvokeMethod, $null, $x.Workbooks,
            @($path, 0, $true, 0), $null, $null, @('Filename', 'UpdateLinks', 'ReadOnly', 'CorruptLoad'))
        $sheets = $wb.Worksheets.Count + $wb.Chartsheets.Count
        $wb.Close($false)
        "PASS`t$sheets sheets"
    } catch {
        "FAIL`t" + ($_.Exception.Message -replace '\s+', ' ')
    } finally {
        # Also covers a failure before the PID line: this process's own instance is quit, no other is touched.
        if ($x) { try { $x.Quit() } catch {} }
    }
}

# True when the package holds Excel 4.0 macro sheets, which ForceDisable does not stop and Excel would prompt for.
function Test-HasXlm([string]$path) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    # Not a zip at all: nothing to screen, Excel gets to report it.
    try { $zip = [IO.Compression.ZipFile]::OpenRead($path) } catch { return $false }
    try { return [bool]($zip.Entries | Where-Object { $_.FullName -like 'xl/macrosheets/*' }) } finally { $zip.Dispose() }
}

function Test-InExcel([string]$path) {
    if (Test-HasXlm $path) { return [pscustomobject]@{ State = 'SKIP-XLM'; Seconds = 0; Detail = 'Excel 4.0 macro sheets; not opened' } }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $job = Start-Job -ScriptBlock $probe -ArgumentList $path
    $done = Wait-Job $job -Timeout $TimeoutSec
    $lines = @(Receive-Job $job -Keep -ErrorAction SilentlyContinue)
    $failed = $job.State -eq 'Failed'
    if (-not $done) { Stop-Job $job }
    Remove-Job $job -Force
    $xlPid = ($lines | Where-Object { $_ -like "PID`t*" } | Select-Object -First 1) -replace '^PID\t', ''
    $out = $lines | Where-Object { $_ -notlike "PID`t*" } | Select-Object -Last 1
    if (-not $done) { $out = "TIMEOUT`tno answer in $TimeoutSec s" } elseif ($failed -or -not $out) { $out = "CRASH`tno result (Excel died)" }
    if ($xlPid) {
        # A clean Quit lets Excel exit by itself; give it a moment, then stop only this instance.
        Wait-Process -Id ([int]$xlPid) -Timeout 15 -ErrorAction SilentlyContinue
        Stop-Process -Id ([int]$xlPid) -Force -ErrorAction SilentlyContinue
    }
    $state, $detail = $out -split "`t", 2
    [pscustomobject]@{ State = $state; Seconds = [math]::Round($clock.Elapsed.TotalSeconds, 1); Detail = $detail }
}

$saved = Join-Path $Out 'saved'
Remove-Item -LiteralPath $saved -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $saved -Force | Out-Null

# (name, original or $null, saved) per file
$work = @()
if ($Inputs) {
    foreach ($f in Get-ChildItem -LiteralPath $Inputs -File | Where-Object { $_.Extension -in '.xlsx', '.xlsm' } | Sort-Object Name) {
        $work += , @($f.Name, $f.FullName, (Join-Path $saved $f.Name), @('run', '--in', $f.FullName, '--quiet'))
    }
}
foreach ($s in Get-ChildItem -Path $PSScriptRoot -Filter *.jsonl | Sort-Object Name) {
    $work += , @("scenario:$($s.BaseName)", $null, (Join-Path $saved "$($s.BaseName).xlsx"), @('run', '--sample', 'sales', '--script', $s.FullName, '--quiet'))
}

$rows = @()
foreach ($w in $work) {
    $name, $orig, $dest, $cliArgs = $w
    # The CLI reports on stderr, which Stop would turn into an exception.
    Remove-Item -LiteralPath $dest -Force -ErrorAction SilentlyContinue
    $ErrorActionPreference = 'Continue'
    & $Cli @cliArgs --out $dest *> $null
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    $base = if ($orig) { (Test-InExcel $orig) } else { $null }
    $r = if ($code -eq 0 -and (Test-Path -LiteralPath $dest)) { Test-InExcel $dest } else { [pscustomobject]@{ State = 'SAVE-FAIL'; Seconds = 0; Detail = "gridcraft-cli exit code $code" } }
    $rows += [pscustomobject]@{
        File = $name; Original = if ($base) { $base.State } else { '-' }; Saved = $r.State
        Seconds = $r.Seconds; Detail = $r.Detail
    }
}
$rows | Format-Table -AutoSize -Wrap | Out-String -Width 220 | Write-Output
if ($Csv) { $rows | Export-Csv -NoTypeInformation -Path $Csv }
$bad = @($rows | Where-Object { $_.Saved -ne 'PASS' -or $_.Original -eq 'SKIP-XLM' }).Count
$savedBad = @($rows | Where-Object Saved -ne 'PASS').Count
"{0} of {1} saved files pass" -f ($rows.Count - $savedBad), $rows.Count
if ($bad) { exit 1 }

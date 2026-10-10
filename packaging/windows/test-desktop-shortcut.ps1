<#
.SYNOPSIS
  Exercise the actual MSI's desktop-shortcut choices on a disposable GitHub Windows runner.
.DESCRIPTION
  Compiles original no-op GUI/CLI fixtures instead of Rust, uses GridCraft's existing icon,
  and builds adjacent MSI versions. Installs the real package (including its registrations),
  checks default/explicit choices, repair and major upgrades, then uninstalls in finally.
  Run through the Packaging lint workflow, never on a developer's Windows installation.
#>
param([string] $WorkDir)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'This install test is restricted to disposable GitHub-hosted Windows runners.'
}
if (-not $WorkDir) { $WorkDir = Join-Path $env:RUNNER_TEMP 'gridcraft-shortcut-smoke' }
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$InstallDir = Join-Path $env:ProgramFiles 'GridCraft'
$DesktopLink = Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'GridCraft.lnk'
$StartLink = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'GridCraft.lnk'
$ChoiceKey = 'HKLM:\Software\GridCraft\Installer'
$ComponentKey = 'HKLM:\Software\GridCraft\Installer'
$UninstallRoot = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$Icon = Join-Path $Root 'assets\app-icon\gridcraft.ico'
if ((Test-Path $InstallDir) -or (Test-Path $DesktopLink) -or (Test-Path $StartLink) -or
    (Test-Path 'HKLM:\Software\GridCraft') -or (Test-Path 'HKCU:\Software\GridCraft')) {
  throw 'Refusing to run against an existing GridCraft installation or its settings.'
}
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$Bin = Join-Path $WorkDir 'bin'
New-Item -ItemType Directory -Force -Path $Bin | Out-Null

function Assert-That([bool] $Condition, [string] $Message) {
  if (-not $Condition) { throw $Message }
}

function Get-ProductCode([string] $Msi) {
  $installer = New-Object -ComObject WindowsInstaller.Installer
  $database = $null
  $view = $null
  $record = $null
  try {
    $database = $installer.OpenDatabase($Msi, 0)
    $view = $database.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductCode''')
    $view.Execute()
    $record = $view.Fetch()
    if (-not $record) { throw "No ProductCode in $Msi" }
    return $record.StringData(1)
  } finally {
    if ($view) { $view.Close() }
    foreach ($item in $record, $view, $database, $installer) {
      if ($item) { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($item) | Out-Null }
    }
  }
}

function Invoke-Msi([string] $Name, [string[]] $Arguments) {
  $log = Join-Path $WorkDir "$Name.log"
  # /norestart is unconditional. Each operation and the workflow have time limits.
  $process = Start-Process msiexec.exe -ArgumentList ($Arguments + @('/qn', '/norestart', '/l*v', "`"$log`"")) -PassThru
  try {
    if (-not $process.WaitForExit(120000)) {
      $process.Kill()
      throw "$Name exceeded two minutes; see $log"
    }
    if ($process.ExitCode -ne 0) {
      if (Test-Path $log) { Get-Content $log -Tail 60 | Write-Host }
      throw "$Name exited $($process.ExitCode); see $log"
    }
    Write-Host "ok $Name"
  } finally {
    $process.Dispose()
  }
}

function Assert-Package($Package) {
  $key = Join-Path $UninstallRoot $Package.Code
  $registered = Get-ItemProperty -Path $key -ErrorAction Stop
  Assert-That ($registered.DisplayVersion -eq $Package.Version) "Wrong installed product version: $($Package.Version)"
  foreach ($exe in 'gridcraft.exe', 'gridcraft-cli.exe') {
    Assert-That (Test-Path (Join-Path $InstallDir $exe)) "$exe missing after installation"
  }
  Assert-That (Test-Path $StartLink) 'The existing Start Menu shortcut is missing'
}

function Assert-Desktop([bool] $Enabled) {
  Assert-That ((Test-Path $DesktopLink) -eq $Enabled) "Desktop shortcut presence should be $Enabled"
  $marker = Get-ItemPropertyValue $ComponentKey -Name DesktopShortcutComponent -ErrorAction SilentlyContinue
  Assert-That (($null -ne $marker) -eq $Enabled) 'Shortcut component marker must match the machine-wide shortcut'
  Assert-That ($null -eq (Get-ItemPropertyValue 'HKCU:\Software\GridCraft\Installer' -Name DesktopShortcutComponent -ErrorAction SilentlyContinue)) 'Shortcut component must not leave a per-user marker'
  $choice = Get-ItemPropertyValue -Path $ChoiceKey -Name DesktopShortcut
  Assert-That ($choice -eq $(if ($Enabled) { '1' } else { '0' })) 'Installer did not persist the selected desktop choice'
  if (-not $Enabled) { return }
  $shell = New-Object -ComObject WScript.Shell
  $shortcut = $null
  try {
    $shortcut = $shell.CreateShortcut($DesktopLink)
    Assert-That ($shortcut.TargetPath -ieq (Join-Path $InstallDir 'gridcraft.exe')) 'Desktop shortcut targets the wrong executable'
    Assert-That ($shortcut.WorkingDirectory.TrimEnd('\') -ieq $InstallDir.TrimEnd('\')) 'Desktop shortcut has the wrong working directory'
    Assert-That ($shortcut.IconLocation -match '^(.*),\s*0$') 'Desktop shortcut must use icon index zero'
    $iconPath = [Environment]::ExpandEnvironmentVariables($Matches[1].Trim('"'))
    Assert-That (Test-Path $iconPath) 'Desktop shortcut icon is missing'
    Assert-That ((Get-FileHash $iconPath).Hash -eq (Get-FileHash $Icon).Hash) 'Desktop shortcut does not use the packaged GridCraft icon'
  } finally {
    foreach ($item in $shortcut, $shell) {
      if ($item) { [Runtime.InteropServices.Marshal]::FinalReleaseComObject($item) | Out-Null }
    }
  }
}

function Assert-Removed($Package) {
  Assert-That (-not (Test-Path (Join-Path $UninstallRoot $Package.Code))) 'MSI still registered after removal'
  Assert-That (-not (Test-Path (Join-Path $InstallDir 'gridcraft.exe'))) 'App executable remains after uninstall'
  Assert-That (-not (Test-Path (Join-Path $InstallDir 'gridcraft-cli.exe'))) 'CLI executable remains after uninstall'
  Assert-That (-not (Test-Path $DesktopLink)) 'Desktop shortcut remains after uninstall'
  Assert-That (-not (Test-Path $StartLink)) 'Start Menu shortcut remains after uninstall'
  Assert-That ($null -eq (Get-ItemPropertyValue $ChoiceKey -Name DesktopShortcut -ErrorAction SilentlyContinue)) 'Saved choice remains after uninstall'
  Assert-That ($null -eq (Get-ItemPropertyValue $ComponentKey -Name DesktopShortcutComponent -ErrorAction SilentlyContinue)) 'Shortcut component registration remains after uninstall'
}

function Uninstall-AsSystem($Package) {
  # Exercise removal under a different account without creating a user or credentials.
  # This function is covered by the disposable hosted-runner guard at the top of the script.
  $taskName = 'GridCraft-shortcut-' + [Guid]::NewGuid().ToString('N')
  $log = Join-Path $WorkDir 'uninstall-as-system.log'
  $action = New-ScheduledTaskAction -Execute (Join-Path $env:WINDIR 'System32\msiexec.exe') `
    -Argument "/x $($Package.Code) /qn /norestart /l*v `"$log`""
  $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
  $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 2)
  try {
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings | Out-Null
    $started = Get-Date
    Start-ScheduledTask -TaskName $taskName
    $deadline = $started.AddMinutes(2)
    do {
      Start-Sleep -Milliseconds 250
      $task = Get-ScheduledTask -TaskName $taskName
      $info = Get-ScheduledTaskInfo -TaskName $taskName
      if ($info.LastRunTime -ge $started.AddSeconds(-1) -and $task.State -ne 'Running' -and $info.LastTaskResult -ne 267009) {
        Assert-That ($info.LastTaskResult -eq 0) "Cross-account uninstall failed with $($info.LastTaskResult); see $log"
        Write-Host 'ok uninstall-as-system'
        return
      }
    } while ((Get-Date) -lt $deadline)
    throw "Cross-account uninstall exceeded two minutes; see $log"
  } finally {
    if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
      Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
      Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    }
  }
}

# Original fixture source, compiled by the .NET Framework compiler already on the runner.
# https://learn.microsoft.com/dotnet/csharp/language-reference/compiler-options/
$source = Join-Path $WorkDir 'Stub.cs'
'public static class Program { public static void Main() { } }' | Set-Content -Path $source -Encoding utf8
$csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
foreach ($fixture in @(@('gridcraft.exe', 'winexe'), @('gridcraft-cli.exe', 'exe'))) {
  & $csc /nologo /platform:x64 "/target:$($fixture[1])" "/out:$(Join-Path $Bin $fixture[0])" $source
  if ($LASTEXITCODE -ne 0) { throw 'Could not compile installer fixture' }
}
$packages = @()
foreach ($version in '90.0.0', '90.0.1', '90.0.2') {
  $msi = Join-Path $WorkDir "gridcraft-$version.msi"
  & wix build (Join-Path $PSScriptRoot 'gridcraft.wxs') -arch x64 `
    -d "Version=$version" -d "BinDir=$Bin" -d "IconPath=$Icon" -o $msi | Write-Host
  if ($LASTEXITCODE -ne 0) { throw "WiX failed to build $version" }
  $packages += [pscustomobject]@{ Path = $msi; Code = (Get-ProductCode $msi); Version = $version }
}
$first, $second, $third = $packages
$failure = $null
try {
  Invoke-Msi 'default-off' @('/i', "`"$($first.Path)`"")
  Assert-Package $first
  Assert-Desktop $false
  # Repair uses existing choices; opt-in/out is only a fresh-install or major-upgrade option.
  # https://learn.microsoft.com/windows-server/administration/windows-commands/msiexec
  Invoke-Msi 'repair-off' @('/famus', $first.Code)
  Assert-Desktop $false
  Invoke-Msi 'upgrade-off' @('/i', "`"$($second.Path)`"")
  Assert-Package $second
  Assert-That (-not (Test-Path (Join-Path $UninstallRoot $first.Code))) 'Upgrade left the previous MSI installed'
  Assert-Desktop $false
  Invoke-Msi 'uninstall-off' @('/x', $second.Code)
  Assert-Removed $second

  Invoke-Msi 'explicit-on' @('/i', "`"$($first.Path)`"", 'DESKTOPSHORTCUT=1')
  Assert-Package $first
  Assert-Desktop $true
  Remove-Item -LiteralPath $DesktopLink
  Invoke-Msi 'repair-on' @('/famus', $first.Code)
  Assert-Desktop $true
  Invoke-Msi 'upgrade-on' @('/i', "`"$($second.Path)`"")
  Assert-Package $second
  Assert-That (-not (Test-Path (Join-Path $UninstallRoot $first.Code))) 'Upgrade left the previous MSI installed'
  Assert-Desktop $true
  Invoke-Msi 'upgrade-explicit-off' @('/i', "`"$($third.Path)`"", 'DESKTOPSHORTCUT=0')
  Assert-Package $third
  Assert-That (-not (Test-Path (Join-Path $UninstallRoot $second.Code))) 'Opt-out upgrade left the previous MSI installed'
  Assert-Desktop $false
  Invoke-Msi 'repair-after-opt-out' @('/famus', $third.Code)
  Assert-Desktop $false
  Invoke-Msi 'uninstall-after-opt-out' @('/x', $third.Code)
  Assert-Removed $third

  Invoke-Msi 'cross-account-install' @('/i', "`"$($first.Path)`"", 'DESKTOPSHORTCUT=1')
  Assert-Desktop $true
  Uninstall-AsSystem $first
  Assert-Removed $first
} catch {
  $failure = $_
} finally {
  # Only uninstall the exact fixture products built above, including after a failed assertion.
  foreach ($package in $packages) {
    if (Test-Path (Join-Path $UninstallRoot $package.Code)) {
      try { Invoke-Msi "cleanup-$($package.Version)" @('/x', $package.Code) }
      catch {
        Write-Warning $_
        if (-not $failure) { $failure = $_ }
      }
    }
  }
}
if ($failure) { throw $failure }
Write-Host 'Desktop shortcut smoke passed: default off, explicit on/off, repair, upgrade persistence and cross-account uninstall.'

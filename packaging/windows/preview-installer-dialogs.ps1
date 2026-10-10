<#
.SYNOPSIS
  Capture native MSI dialog previews without installing the package.
.DESCRIPTION
  Opens the MSI read-only and uses Windows Installer's inactive UI preview API.
  These show authored dialog layout, not an end-to-end installation or runtime messages.
  https://learn.microsoft.com/windows/win32/msi/previewing-the-user-interface
#>
param(
  [Parameter(Mandatory = $true)] [string] $Msi,
  [Parameter(Mandatory = $true)] [string] $OutputDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
  throw 'Native MSI dialog previews require Windows.'
}
$Msi = (Resolve-Path -LiteralPath $Msi).Path
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class GridcraftMsiPreview {
    [DllImport("msi.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    public static extern uint MsiOpenDatabaseW(string path, IntPtr persist, out uint database);
    [DllImport("msi.dll")]
    public static extern uint MsiEnableUIPreview(uint database, out uint preview);
    [DllImport("msi.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    public static extern uint MsiPreviewDialogW(uint preview, string dialog);
    [DllImport("msi.dll")]
    public static extern uint MsiCloseHandle(uint handle);
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    private delegate bool WindowCallback(IntPtr window, IntPtr state);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(WindowCallback callback, IntPtr state);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    public static IntPtr FindDialog(uint process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr state) {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != process) return true;
            var title = new StringBuilder(128);
            GetWindowText(window, title, title.Capacity);
            if (title.ToString() != "GridCraft Setup") return true;
            found = window;
            return false;
        }, IntPtr.Zero);
        return found;
    }
}
'@
function Assert-MsiResult([uint32] $Result, [string] $Operation) {
  if ($Result -ne 0) { throw "$Operation failed with MSI error $Result" }
}
[uint32] $database = 0
[uint32] $preview = 0
try {
  Assert-MsiResult ([GridcraftMsiPreview]::MsiOpenDatabaseW($Msi, [IntPtr]::Zero, [ref] $database)) 'Open read-only MSI'
  $dialogs = 'DesktopShortcutDlg', 'SetupCompleteDlg', 'SetupCanceledDlg', 'SetupFailedDlg', 'GridcraftErrorDlg', 'FilesInUse'
  foreach ($dialog in $dialogs) {
    # Give each dialog its own preview session; closing its handle releases the UI.
    Assert-MsiResult ([GridcraftMsiPreview]::MsiEnableUIPreview($database, [ref] $preview)) "Enable preview for $dialog"
    Assert-MsiResult ([GridcraftMsiPreview]::MsiPreviewDialogW($preview, $dialog)) "Preview $dialog"
    $window = [IntPtr]::Zero
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($window -eq [IntPtr]::Zero -and $wait.Elapsed.TotalSeconds -lt 5) {
      [Windows.Forms.Application]::DoEvents()
      $window = [GridcraftMsiPreview]::FindDialog([uint32] $PID)
      Start-Sleep -Milliseconds 50
    }
    if ($window -eq [IntPtr]::Zero) { throw "No native window found for $dialog" }
    [Windows.Forms.Application]::DoEvents()
    $rect = New-Object GridcraftMsiPreview+Rect
    if (-not [GridcraftMsiPreview]::GetWindowRect($window, [ref] $rect)) { throw "Cannot measure $dialog" }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 1 -or $height -lt 1 -or $width -gt 4096 -or $height -gt 4096) { throw "Invalid preview dimensions for $dialog" }
    $bitmap = New-Object Drawing.Bitmap($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
      $dc = $graphics.GetHdc()
      try {
        if (-not [GridcraftMsiPreview]::PrintWindow($window, $dc, 0)) { throw "Capture failed for $dialog" }
      } finally { $graphics.ReleaseHdc($dc) }
      $colors = New-Object 'Collections.Generic.HashSet[int]'
      for ($y = 0; $y -lt $height; $y += [Math]::Max(1, [int] ($height / 32))) {
        for ($x = 0; $x -lt $width; $x += [Math]::Max(1, [int] ($width / 32))) {
          $null = $colors.Add($bitmap.GetPixel($x, $y).ToArgb())
        }
      }
      if ($colors.Count -lt 2) { throw "Empty capture for $dialog" }
      $bitmap.Save((Join-Path $OutputDir "$dialog.png"), [Drawing.Imaging.ImageFormat]::Png)
    } finally {
      $graphics.Dispose()
      $bitmap.Dispose()
    }
    Assert-MsiResult ([GridcraftMsiPreview]::MsiCloseHandle($preview)) "Close preview for $dialog"
    $preview = 0
    Write-Host "Saved native MSI dialog preview: $dialog"
  }
  'Native MSI dialog previews only. No installation was run. Support dialogs have no live error or process data.' |
    Set-Content -LiteralPath (Join-Path $OutputDir 'README.txt') -Encoding utf8
} finally {
  if ($preview -ne 0) {
    $null = [GridcraftMsiPreview]::MsiCloseHandle($preview)
  }
  if ($database -ne 0) { $null = [GridcraftMsiPreview]::MsiCloseHandle($database) }
}

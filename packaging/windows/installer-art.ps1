<#
.SYNOPSIS
  Draw original, build-only artwork for WiX's folder browser and invalid-folder message.
.DESCRIPTION
  No image files are copied from another product. Outputs stay in the packaging work directory.
#>
param([Parameter(Mandatory = $true)] [string] $OutputDir)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$green = [Drawing.Color]::FromArgb(27, 112, 74)
$banner = New-Object Drawing.Bitmap(493, 58)
$graphics = [Drawing.Graphics]::FromImage($banner)
try {
  $graphics.Clear([Drawing.Color]::FromArgb(238, 247, 241))
  $banner.Save((Join-Path $OutputDir 'banner.bmp'), [Drawing.Imaging.ImageFormat]::Bmp)
} finally { $graphics.Dispose(); $banner.Dispose() }

foreach ($name in 'up', 'new', 'warning') {
  $bitmap = New-Object Drawing.Bitmap(32, 32)
  $graphics = [Drawing.Graphics]::FromImage($bitmap)
  $pen = New-Object Drawing.Pen($green, 3)
  $brush = New-Object Drawing.SolidBrush($green)
  $png = New-Object IO.MemoryStream
  try {
    $graphics.Clear([Drawing.Color]::Transparent)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    switch ($name) {
      'up' {
        $graphics.DrawLine($pen, 16, 27, 16, 6)
        $graphics.DrawLine($pen, 16, 6, 6, 16)
        $graphics.DrawLine($pen, 16, 6, 26, 16)
      }
      'new' {
        $graphics.DrawRectangle($pen, 4, 10, 24, 17)
        $graphics.DrawLines($pen, [Drawing.Point[]] @(
          [Drawing.Point]::new(4, 10), [Drawing.Point]::new(4, 5),
          [Drawing.Point]::new(13, 5), [Drawing.Point]::new(17, 10)))
        $graphics.DrawLine($pen, 16, 14, 16, 23)
        $graphics.DrawLine($pen, 11, 19, 21, 19)
      }
      'warning' {
        $graphics.DrawEllipse($pen, 3, 3, 26, 26)
        $graphics.DrawLine($pen, 16, 9, 16, 18)
        $graphics.FillEllipse($brush, 14, 22, 4, 4)
      }
    }
    $bitmap.Save($png, [Drawing.Imaging.ImageFormat]::Png)
    # ICO header and one PNG-backed 32px image; no native icon handles to release.
    $stream = [IO.File]::Create((Join-Path $OutputDir "$name.ico"))
    $writer = New-Object IO.BinaryWriter($stream)
    try {
      $writer.Write([uint16] 0); $writer.Write([uint16] 1); $writer.Write([uint16] 1)
      $writer.Write([byte] 32); $writer.Write([byte] 32)
      $writer.Write([byte] 0); $writer.Write([byte] 0)
      $writer.Write([uint16] 1); $writer.Write([uint16] 32)
      $writer.Write([uint32] $png.Length); $writer.Write([uint32] 22)
      $writer.Write($png.ToArray())
    } finally { $writer.Dispose(); $stream.Dispose() }
  } finally {
    $png.Dispose(); $brush.Dispose(); $pen.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
  }
}

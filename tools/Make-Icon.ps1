# Renders the app icon (blue disc, white play triangle, spectrum ticks) to a multi-size .ico.
param([string]$Out = (Join-Path $PSScriptRoot "..\src\WebSkinPlayer\app.ico"))
Add-Type -AssemblyName PresentationCore, PresentationFramework, WindowsBase
$sizes = 16, 24, 32, 48, 64, 256
$pngs = @()
foreach ($s in $sizes) {
    $dv = New-Object System.Windows.Media.DrawingVisual
    $dc = $dv.RenderOpen()
    $r = $s / 2.0
    $center = New-Object System.Windows.Point $r, $r
    $grad = New-Object System.Windows.Media.RadialGradientBrush
    $grad.GradientOrigin = New-Object System.Windows.Point 0.35, 0.28
    $grad.GradientStops.Add((New-Object System.Windows.Media.GradientStop ([System.Windows.Media.Color]::FromRgb(0x8F, 0xDB, 0xFF)), 0))
    $grad.GradientStops.Add((New-Object System.Windows.Media.GradientStop ([System.Windows.Media.Color]::FromRgb(0x1E, 0x78, 0xC8)), 0.55))
    $grad.GradientStops.Add((New-Object System.Windows.Media.GradientStop ([System.Windows.Media.Color]::FromRgb(0x0A, 0x2A, 0x5C)), 1))
    $ring = New-Object System.Windows.Media.Pen ((New-Object System.Windows.Media.SolidColorBrush ([System.Windows.Media.Color]::FromArgb(200, 0x06, 0x18, 0x38)))), ([Math]::Max(1, $s / 32))
    $dc.DrawEllipse($grad, $ring, $center, $r - $s / 32, $r - $s / 32)
    # play triangle
    $tri = New-Object System.Windows.Media.StreamGeometry
    $ctx = $tri.Open()
    $ctx.BeginFigure((New-Object System.Windows.Point ($s * 0.40), ($s * 0.28)), $true, $true)
    $ctx.LineTo((New-Object System.Windows.Point ($s * 0.40), ($s * 0.72)), $true, $false)
    $ctx.LineTo((New-Object System.Windows.Point ($s * 0.76), ($s * 0.50)), $true, $false)
    $ctx.Close()
    $dc.DrawGeometry([System.Windows.Media.Brushes]::White, $null, $tri)
    # little spectrum ticks on the left (only where there is room)
    if ($s -ge 32) {
        $bar = New-Object System.Windows.Media.SolidColorBrush ([System.Windows.Media.Color]::FromArgb(230, 0xB8, 0xF0, 0xFF))
        $hs = 0.10, 0.18, 0.13
        for ($i = 0; $i -lt 3; $i++) {
            $bh = $s * $hs[$i]; $x = $s * (0.20 + $i * 0.055)
            $dc.DrawRectangle($bar, $null, (New-Object System.Windows.Rect $x, ($s * 0.62 - $bh), ($s * 0.035), $bh))
        }
    }
    $dc.Close()
    $rtb = New-Object System.Windows.Media.Imaging.RenderTargetBitmap $s, $s, 96, 96, ([System.Windows.Media.PixelFormats]::Pbgra32)
    $rtb.Render($dv)
    $enc = New-Object System.Windows.Media.Imaging.PngBitmapEncoder
    $enc.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($rtb))
    $ms = New-Object System.IO.MemoryStream
    $enc.Save($ms)
    $pngs += , $ms.ToArray()
}
# ICO container with PNG-compressed entries
$fs = [System.IO.File]::Create($Out)
$w = New-Object System.IO.BinaryWriter $fs
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $b = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$b); $w.Write([byte]$b); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write($p) }
$w.Close()
"wrote $Out"

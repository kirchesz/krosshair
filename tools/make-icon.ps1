# Draws the app icon, assets\icon\krosshair.ico: the About screen's logo, a rounded #393939
# square with the #f4f4f4 crosshair glyph (ring r=7 and four ticks in a 24-unit box, round caps).
# Big sizes keep the About proportions (glyph 40 of 72); small ones get a bigger, bolder glyph so
# it still reads in the taskbar and title bars. Every size is drawn 4x larger and scaled down.
#   .\tools\make-icon.ps1        (then cargo build picks it up)
param([string]$Out = (Join-Path $PSScriptRoot '..\assets\icon\krosshair.ico'), [string]$Previews)
Add-Type -AssemblyName System.Drawing

# side in px -> glyph share of the side, stroke in glyph units
function Get-Style([int]$Side) {
    if ($Side -le 24) { return 0.80, 2.4 }
    if ($Side -le 48) { return 0.68, 2.1 }
    return (40 / 72), 1.75
}

function New-Icon([int]$Side) {
    $glyphShare, $stroke = Get-Style $Side
    $ss = 4
    $full = $Side * $ss
    $big = New-Object System.Drawing.Bitmap $full, $full
    $g = [System.Drawing.Graphics]::FromImage($big)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)

    $d = [single]($full * 32 / 72)   # corner diameter: radius 16 of 72, as in About
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($full - $d, 0, $d, $d, 270, 90)
    $path.AddArc($full - $d, $full - $d, $d, $d, 0, 90)
    $path.AddArc(0, $full - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#393939'))), $path)

    $k = $full * $glyphShare / 24
    $off = ($full - 24 * $k) / 2
    $pen = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#f4f4f4')), ([single]($stroke * $k))
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $g.DrawEllipse($pen, [single]($off + 5 * $k), [single]($off + 5 * $k), [single](14 * $k), [single](14 * $k))
    foreach ($l in @(@(12, 2, 12, 7), @(12, 17, 12, 22), @(2, 12, 7, 12), @(17, 12, 22, 12))) {
        $g.DrawLine($pen, [single]($off + $l[0] * $k), [single]($off + $l[1] * $k), [single]($off + $l[2] * $k), [single]($off + $l[3] * $k))
    }
    $g.Dispose()

    $small = New-Object System.Drawing.Bitmap $Side, $Side
    $g = [System.Drawing.Graphics]::FromImage($small)
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.PixelOffsetMode = 'HighQuality'
    $g.CompositingQuality = 'HighQuality'
    $g.DrawImage($big, 0, 0, $Side, $Side)
    $g.Dispose(); $big.Dispose()
    return $small
}

# An .ico whose entries are PNGs (fine for every size since Windows Vista).
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = foreach ($side in $sizes) {
    $bmp = New-Icon $side
    if ($Previews) {
        New-Item -ItemType Directory -Force $Previews | Out-Null
        $bmp.Save((Join-Path $Previews "icon-$side.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    }
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$fs = [System.IO.File]::Create($Out)
$w = New-Object System.IO.BinaryWriter $fs
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $side = $sizes[$i]
    $dim = if ($side -ge 256) { 0 } else { $side }   # 0 means 256
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $w.Write($png) }
$w.Dispose()
Get-Item $Out | Select-Object FullName, Length

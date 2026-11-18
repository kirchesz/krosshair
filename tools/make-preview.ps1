# Draws the README preview, docs\images\preview.png: a screenshot of the window with rounded
# corners and a soft shadow, on a generated wallpaper (blurred blobs of Carbon blue, purple and
# teal on near-black), so it looks like the app on a desktop. The wallpaper is drawn here rather
# than taken from somewhere, so the picture has no third-party art in it.
#   .\tools\make-preview.ps1                       (from docs\images\gallery.png)
#   .\tools\make-preview.ps1 -Shot C:\Temp\shot.png  (a new screenshot, client area of the window)
param(
    [string]$Shot = (Join-Path $PSScriptRoot '..\docs\images\gallery.png'),
    [string]$Out = (Join-Path $PSScriptRoot '..\docs\images\preview.png'),
    [int]$Margin = 200,
    [int]$Radius = 10
)
Add-Type -AssemblyName System.Drawing

# The pixel work (wallpaper and blurred shadow) is too slow in PowerShell, so it is done here on
# plain BGRA byte arrays.
Add-Type -TypeDefinition @'
using System;
public static class Preview {
    // Near-black base with big gaussian blobs of color, plus a little noise so the gradients
    // don't band once the PNG is shown in 8 bits.
    public static byte[] Wallpaper(int w, int h) {
        double[][] blobs = {
            // x, y (share of the size), radius (share of the width), r, g, b, strength
            new double[] { 0.10, 0.12, 0.45, 0x45, 0x89, 0xff, 0.70 },
            new double[] { 0.95, 0.05, 0.42, 0x8a, 0x3f, 0xfc, 0.70 },
            new double[] { 0.80, 1.05, 0.45, 0x08, 0xbd, 0xba, 0.45 },
            new double[] { 0.02, 1.00, 0.32, 0xee, 0x53, 0x96, 0.35 },
            new double[] { 0.50, 0.50, 0.30, 0x0f, 0x62, 0xfe, 0.25 },
        };
        var px = new byte[w * h * 4];
        var rng = new Random(7);
        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                double r = 10, g = 11, b = 18;
                foreach (var o in blobs) {
                    double dx = x - o[0] * w, dy = y - o[1] * h, rad = o[2] * w;
                    double k = o[6] * Math.Exp(-(dx * dx + dy * dy) / (rad * rad));
                    r += o[3] * k; g += o[4] * k; b += o[5] * k;
                }
                double n = rng.NextDouble() - rng.NextDouble();
                int i = (y * w + x) * 4;
                px[i] = Clamp(b + n); px[i + 1] = Clamp(g + n); px[i + 2] = Clamp(r + n); px[i + 3] = 255;
            }
        }
        return px;
    }

    // Darkens the wallpaper under a blurred rounded rectangle: the window's shadow.
    public static void Shadow(byte[] px, int w, int h, int x0, int y0, int rw, int rh, int radius, int blur, double opacity) {
        var a = new double[w * h];
        for (int y = y0; y < y0 + rh; y++) {
            for (int x = x0; x < x0 + rw; x++) {
                double cx = Math.Max(Math.Max(x0 + radius - x - 0.5, x + 0.5 - (x0 + rw - radius)), 0);
                double cy = Math.Max(Math.Max(y0 + radius - y - 0.5, y + 0.5 - (y0 + rh - radius)), 0);
                if (cx * cx + cy * cy <= radius * radius && x >= 0 && y >= 0 && x < w && y < h) a[y * w + x] = 1;
            }
        }
        // Three box blurs come close to a gaussian.
        for (int pass = 0; pass < 3; pass++) { a = Box(a, w, h, blur, true); a = Box(a, w, h, blur, false); }
        for (int i = 0; i < w * h; i++) {
            double k = 1 - a[i] * opacity;
            for (int c = 0; c < 3; c++) px[i * 4 + c] = Clamp(px[i * 4 + c] * k);
        }
    }

    static double[] Box(double[] src, int w, int h, int r, bool across) {
        var dst = new double[w * h];
        int lines = across ? h : w, len = across ? w : h;
        for (int l = 0; l < lines; l++) {
            double sum = 0;
            for (int j = -r; j <= r; j++) sum += At(src, w, across, l, Math.Min(Math.Max(j, 0), len - 1));
            for (int j = 0; j < len; j++) {
                Set(dst, w, across, l, j, sum / (2 * r + 1));
                sum += At(src, w, across, l, Math.Min(j + r + 1, len - 1)) - At(src, w, across, l, Math.Max(j - r, 0));
            }
        }
        return dst;
    }

    static double At(double[] a, int w, bool across, int l, int j) { return across ? a[l * w + j] : a[j * w + l]; }
    static void Set(double[] a, int w, bool across, int l, int j, double v) { if (across) a[l * w + j] = v; else a[j * w + l] = v; }
    static byte Clamp(double v) { return (byte)Math.Max(0, Math.Min(255, Math.Round(v))); }
}
'@

function New-RoundedRect([single]$X, [single]$Y, [single]$W, [single]$H, [single]$R) {
    $d = 2 * $R
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $p.AddArc($X, $Y, $d, $d, 180, 90)
    $p.AddArc($X + $W - $d, $Y, $d, $d, 270, 90)
    $p.AddArc($X + $W - $d, $Y + $H - $d, $d, $d, 0, 90)
    $p.AddArc($X, $Y + $H - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

$win = [System.Drawing.Image]::FromFile((Resolve-Path $Shot))
$w = $win.Width + 2 * $Margin
$h = [int][Math]::Round($w * 10 / 16)             # a 16:10 screen
if ($h -lt $win.Height + 120) { $h = $win.Height + 120 }
$x0 = $Margin
$y0 = [int](($h - $win.Height) / 2)

$px = [Preview]::Wallpaper($w, $h)
[Preview]::Shadow($px, $w, $h, $x0, $y0 + 16, $win.Width, $win.Height, $Radius, 24, 0.65)

$bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$lock = $bmp.LockBits((New-Object System.Drawing.Rectangle 0, 0, $w, $h), 'WriteOnly', $bmp.PixelFormat)
[System.Runtime.InteropServices.Marshal]::Copy($px, 0, $lock.Scan0, $px.Length)
$bmp.UnlockBits($lock)

$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.PixelOffsetMode = 'Half'                        # pixel edges on whole coordinates, so the shot stays 1:1
$path = New-RoundedRect $x0 $y0 $win.Width $win.Height $Radius
$brush = New-Object System.Drawing.TextureBrush $win
$brush.TranslateTransform($x0, $y0)
$g.FillPath($brush, $path)
# A faint light edge, like the border Windows 11 draws around dark windows.
$edge = New-RoundedRect ($x0 + 0.5) ($y0 + 0.5) ($win.Width - 1) ($win.Height - 1) ($Radius - 0.5)
$g.DrawPath((New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(40, 255, 255, 255)), 1), $edge)
$g.Dispose()

$win.Dispose()
$bmp.Save([System.IO.Path]::GetFullPath($Out), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"wrote $Out ($w x $h)"

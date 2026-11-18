param([string]$App, [string]$Bench, [string]$Dir)

# End-to-end check: run krosshair with a test configuration and let the bench window (named like
# a game RTSS hooks) measure where the crosshair lands.
#   -App    path to krosshair.exe
#   -Bench  path to the built osdtest.cpp, named after a game with an RTSS profile
#   -Dir    work folder: needs gallery\ with the preset PNGs and an empty shots\ for the snapshots
$cfgPath = "$Dir\e2e-config.json"
$env:KROSSHAIR_CONFIG = $cfgPath
function Layer([string]$file, [int]$anchor, [int]$ox, [int]$oy, [bool]$on = $true) { @{ file = $file; anchor = $anchor; ox = $ox; oy = $oy; on = $on } }
$cases = @(
    @{ name = 'cross-gap centered';        expect = '0,0';        mask = 0xffffff; power = $true;  layers = @((Layer 'cross-gap.png' 4 0 0)) },
    @{ name = 'cross-gap offset +5,-3';    expect = '+5,-3';      mask = 0xffffff; power = $true;  layers = @((Layer 'cross-gap.png' 4 5 -3)) },
    @{ name = 'dot-small (4x4) centered';  expect = '0,0';        mask = 0xffffff; power = $true;  layers = @((Layer 'dot-small.png' 4 0 0)) },
    @{ name = 'ring + dot, corners, red';  expect = '0,0 + snap'; mask = 0xff4040; power = $true;  layers = @((Layer 'ring.png' 4 0 0), (Layer 'dot.png' 0 10 10), (Layer 'cross.png' 8 -10 -10), (Layer 'cross-bold.png' 4 0 0 $false)) },
    @{ name = 'atlas file deleted';        expect = '0,0 (heals)'; mask = 0xffffff; power = $true;  layers = @((Layer 'cross-gap.png' 4 0 0)); sabotage = $true },
    @{ name = 'overlay off';               expect = 'nothing';    mask = 0xffffff; power = $false; layers = @((Layer 'cross-gap.png' 4 0 0)) }
)
$n = 0
foreach ($c in $cases) {
    @{ gallery_dir = "$Dir\gallery"; mask = $c.mask; power = $c.power; current = 0; profiles = @(@{ name = 'E2E'; layers = $c.layers }) } |
        ConvertTo-Json -Depth 6 | Set-Content $cfgPath -Encoding utf8
    $p = Start-Process $App -PassThru
    Start-Sleep -Milliseconds 2000
    if ($c.sabotage) { Remove-Item "$env:ProgramData\krosshair\osd_*.png" -Force; Start-Sleep -Milliseconds 2500 }
    $out = "$Dir\e2e_out.txt"
    $b = Start-Process $Bench -ArgumentList '--watch', "$Dir\shots\e2e$n.bmp" -NoNewWindow -PassThru -RedirectStandardOutput $out
    if (-not $b.WaitForExit(20000)) { Stop-Process -Id $b.Id -Force; $res = 'TIMEOUT' } else { $res = (Get-Content $out) -replace '^watch\s+', '' }
    "{0,-28} expect {1,-12} {2}" -f $c.name, $c.expect, $res
    [void]$p.CloseMainWindow()
    if (-not $p.WaitForExit(4000)) { Stop-Process -Id $p.Id -Force; "  app killed" }
    $n++
}
# After the app is closed nothing may stay on screen.
$b = Start-Process $Bench -ArgumentList '--watch' -NoNewWindow -PassThru -RedirectStandardOutput "$Dir\e2e_out.txt"
if ($b.WaitForExit(20000)) { "{0,-28} expect {1,-12} {2}" -f 'after the app closed', 'nothing', ((Get-Content "$Dir\e2e_out.txt") -replace '^watch\s+', '') }
"leftover atlas files: " + @(Get-ChildItem "$env:ProgramData\krosshair" -Filter 'osd_*.png' -ErrorAction SilentlyContinue).Count
Add-Type -AssemblyName System.Drawing
Get-ChildItem "$Dir\shots\e2e*.bmp" | ForEach-Object { $i = [System.Drawing.Image]::FromFile($_.FullName); $i.Save(($_.FullName -replace '\.bmp$', '.png'), [System.Drawing.Imaging.ImageFormat]::Png); $i.Dispose(); Remove-Item $_.FullName }

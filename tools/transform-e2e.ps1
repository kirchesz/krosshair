param([string]$App, [string]$Bench, [string]$Gallery, [string]$Out, [string]$Only = '')

# krosshair at a game zoom of 2. The bench's RTSS profile has zoom 1, so a slot of our own holds a
# global <FNT=...,2>, which makes the whole OSD zoom 2, and KROSSHAIR_ZOOM=2 tells krosshair.
Add-Type @'
using System; using System.IO.MemoryMappedFiles; using System.Text;
public static class Slot {
    static long Find(MemoryMappedViewAccessor v, string owner, bool take) {
        uint size = v.ReadUInt32(20), off = v.ReadUInt32(24), n = v.ReadUInt32(28);
        for (int pass = 0; pass < 2; pass++)
            for (uint i = 1; i < n; i++) {
                long e = off + i * size;
                var b = new byte[256]; v.ReadArray(e + 256, b, 0, 256);
                string o = Encoding.ASCII.GetString(b); o = o.Substring(0, Math.Max(0, o.IndexOf('\0')));
                if (o == owner) return e;
                if (pass == 1 && take && o.Length == 0) { var ob = Encoding.ASCII.GetBytes(owner + "\0"); v.WriteArray(e + 256, ob, 0, ob.Length); return e; }
            }
        return -1;
    }
    public static bool Write(string owner, string text) {
        using (var m = MemoryMappedFile.OpenExisting("RTSSSharedMemoryV2")) using (var v = m.CreateViewAccessor()) {
            long e = Find(v, owner, true); if (e < 0) return false;
            var tb = Encoding.ASCII.GetBytes(text + "\0"); v.WriteArray(e + 512, tb, 0, tb.Length);
            v.Write(32, v.ReadUInt32(32) + 1); return true;
        }
    }
    public static string Read(string owner) {
        using (var m = MemoryMappedFile.OpenExisting("RTSSSharedMemoryV2")) using (var v = m.CreateViewAccessor()) {
            long e = Find(v, owner, false); if (e < 0) return "(no slot)";
            var b = new byte[4096]; v.ReadArray(e + 512, b, 0, 4096); string s = Encoding.ASCII.GetString(b); return s.Substring(0, Math.Max(0, s.IndexOf('\0'))).Replace("\b", "\\b");
        }
    }
    public static void Release(string owner) {
        using (var m = MemoryMappedFile.OpenExisting("RTSSSharedMemoryV2")) using (var v = m.CreateViewAccessor()) {
            long e = Find(v, owner, false); if (e < 0) return;
            uint size = v.ReadUInt32(20); v.WriteArray(e, new byte[size], 0, (int)size); v.Write(32, v.ReadUInt32(32) + 1);
        }
    }
}
'@
$cfg = "$Out\zoom-config.json"
function L([string]$file, [int]$anchor, [int]$ox, [int]$oy) { @{ file = $file; anchor = $anchor; ox = $ox; oy = $oy; on = $true } }
$all = @(
    @{ n = 'rotate 90, cross-gap +5,-3 (+3,+5)';  fnt = $false; z = 1; rot = 90; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'rotate 90, t-shape';                 fnt = $false; z = 1; rot = 0;  l = @((L 't-shape.png' 4 0 0)) },
    @{ n = 'rotate 90b, t-shape';                fnt = $false; z = 1; rot = 90; l = @((L 't-shape.png' 4 0 0)) },
    @{ n = 'rotate 45, cross-gap (~28x28)';     fnt = $false; z = 1; rot = 45; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 0.5, cross-gap (10x10)';       fnt = $false; z = 1; scale = 0.5; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 0.5, +6,-4 (+3,-2)';          fnt = $false; z = 1; scale = 0.5; l = @((L 'cross-gap.png' 4 6 -4)) },
    @{ n = 'scale 0.25, cross-gap (5x5)';       fnt = $false; z = 1; scale = 0.25; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 0.5 at zoom 2 (10x10)';       fnt = $true;  z = 2; scale = 0.5; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 1.05, cross-gap (21x21)';     fnt = $false; z = 1; scale = 1.05; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 2.35, +5,-3 (47x47, +12,-7)'; fnt = $false; z = 1; scale = 2.35; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'scale 0.85 at zoom 2 (17x17)';      fnt = $true;  z = 2; scale = 0.85; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 1.5, cross-gap (30x30)';      fnt = $false; z = 1; scale = 1.5; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 1.5, +6,-4 (+9,-6)';         fnt = $false; z = 1; scale = 1.5; l = @((L 'cross-gap.png' 4 6 -4)) },
    @{ n = 'scale 2, cross-gap centered (40x40)'; fnt = $false; z = 1; scale = 2; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'scale 2, cross-gap +5,-3 (+10,-6)';  fnt = $false; z = 1; scale = 2; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'scale 3, dot-small (12x12)';         fnt = $false; z = 1; scale = 3; l = @((L 'dot-small.png' 4 0 0)) },
    @{ n = 'scale 2 at zoom 2, +5,-3 (40x40)';   fnt = $true;  z = 2; scale = 2; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'zoom 1, cross-gap centered';         fnt = $false; z = 1; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'zoom 2 not compensated (old way)';   fnt = $true;  z = 1; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'zoom 2, cross-gap centered';         fnt = $true;  z = 2; l = @((L 'cross-gap.png' 4 0 0)) },
    @{ n = 'zoom 2, cross-gap +5,-3';            fnt = $true;  z = 2; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'zoom 2, dot-small centered';         fnt = $true;  z = 2; l = @((L 'dot-small.png' 4 0 0)) },
    @{ n = 'zoom 2, ring +1,+1';                 fnt = $true;  z = 2; l = @((L 'ring.png' 4 1 1)) },
    @{ n = 'zoom 2, cross-gap +4,-4';            fnt = $true;  z = 2; l = @((L 'cross-gap.png' 4 4 -4)) },
    @{ n = 'zoom 1, cross-gap +5,-3';            fnt = $false; z = 1; l = @((L 'cross-gap.png' 4 5 -3)) },
    @{ n = 'zoom 1, dot-small centered';         fnt = $false; z = 1; l = @((L 'dot-small.png' 4 0 0)) },
    @{ n = 'zoom 2, dot.png top left +5,+7';     fnt = $true;  z = 2; l = @((L 'dot.png' 0 5 7)) },
    @{ n = 'zoom 2, dot.png bottom right -5,-7'; fnt = $true;  z = 2; l = @((L 'dot.png' 8 -5 -7)) }
)
$cases = if ($Only) { $all | Where-Object { $_.n -match $Only } } else { $all }
# RTSS draws the test into every process it has hooked: stop if that is anything but the bench.
Add-Type @"
using System; using System.IO.MemoryMappedFiles; using System.Text; using System.Collections.Generic;
public static class HookedApps {
    public static string[] Paths() {
        var list = new List<string>();
        using (var mmf = MemoryMappedFile.OpenExisting("RTSSSharedMemoryV2", MemoryMappedFileRights.Read))
        using (var v = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read)) {
            uint size = v.ReadUInt32(8), off = v.ReadUInt32(12), n = v.ReadUInt32(16);
            for (uint i = 0; i < n; i++) {
                long e = off + i * size; uint pid = v.ReadUInt32(e); if (pid == 0) continue;
                try { System.Diagnostics.Process.GetProcessById((int)pid); } catch { continue; }
                var name = new byte[260]; v.ReadArray(e + 4, name, 0, 260);
                string s = Encoding.ASCII.GetString(name); list.Add(s.Substring(0, Math.Max(0, s.IndexOf('\0'))));
            }
        }
        return list.ToArray();
    }
}
"@
function Game-Hooked { [HookedApps]::Paths() | Where-Object { $_ -ne $Bench } }
$env:KROSSHAIR_CONFIG = $cfg
try {
    $n = 0
    foreach ($c in $cases) {
        $g = Game-Hooked
        if ($g) { 'RTSS has hooked a game (' + ($g -join ', ') + '): stopping.'; break }
        @{ gallery_dir = $Gallery; mask = 16777215; power = $true; scale = $(if ($c.scale) { $c.scale } else { 1 }); rotation = $(if ($c.rot) { $c.rot } else { 0 }); current = 0; profiles = @(@{ name = 'Zoom'; layers = $c.l }) } | ConvertTo-Json -Depth 6 | Set-Content $cfg -Encoding utf8
        if ($c.fnt) { [void][Slot]::Write('krosshair_zoomtest', '<FNT=Cascadia Code,-29,700,2>') } else { [Slot]::Release('krosshair_zoomtest') }
        $env:KROSSHAIR_ZOOM = "$($c.z)"
        $p = Start-Process $App -PassThru; Start-Sleep -Milliseconds 2000
        $b = Start-Process $Bench -ArgumentList '--watch', "$Out\z$n.bmp" -NoNewWindow -PassThru -RedirectStandardOutput "$Out\z_out.txt"
        if (-not $b.WaitForExit(20000)) { Stop-Process -Id $b.Id -Force; $res = 'TIMEOUT' } else { $res = (Get-Content "$Out\z_out.txt") -replace '^watch\s+', '' }
        "{0,-36} {1}" -f $c.n, $res
        "   slot: " + ([Slot]::Read("krosshair") -replace '^<LI="[^"]*">', '')
        [void]$p.CloseMainWindow(); if (-not $p.WaitForExit(4000)) { Stop-Process -Id $p.Id -Force; "  app killed" }
        $n++
    }
}
finally {
    [Slot]::Release('krosshair_zoomtest')
    Remove-Item Env:KROSSHAIR_CONFIG, Env:KROSSHAIR_ZOOM -ErrorAction SilentlyContinue
}

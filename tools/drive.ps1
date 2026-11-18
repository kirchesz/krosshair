param([string]$Exe, [string]$OutDir, [string[]]$AppArgs = @(), [string[]]$Steps = @(), [int]$Wait = 3000, [string]$Config = '')

# Starts the app and drives it by posting mouse and key messages to its window, so the real
# cursor never moves. Steps:
#   click X Y | move X Y | down X Y | up X Y | drag X1 Y1 X2 Y2 | key VK | wait MS | shot NAME | snap NAME (no settling delay) | run SCRIPTBLOCK
#   hold X Y (move with the left button down) | hit X Y (prints the WM_NCHITTEST result) | nc HT X Y [dbl] | state LABEL | restore
# Coordinates are client pixels (the screenshot minus its 8 px left and 1 px top frame).
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Drive {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
'@
[void][Drive]::SetProcessDPIAware()
if ($Config) { $env:KROSSHAIR_CONFIG = $Config }

$p = if ($AppArgs.Count) { Start-Process $Exe -ArgumentList $AppArgs -PassThru } else { Start-Process $Exe -PassThru }
try {
    Start-Sleep -Milliseconds $Wait
    $p.Refresh()
    $h = $p.MainWindowHandle
    if ($h -eq [IntPtr]::Zero) { "no window"; return }
    function LP([int]$x, [int]$y) { [IntPtr](($y -shl 16) -bor ($x -band 0xffff)) }
    function Post([uint32]$msg, [int]$w, [IntPtr]$l) { [void][Drive]::PostMessageW($h, $msg, [IntPtr]$w, $l); Start-Sleep -Milliseconds 60 }
    function Shot([string]$name) {
        $r = New-Object Drive+RECT
        [void][Drive]::GetWindowRect($h, [ref]$r)
        $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $dc = $g.GetHdc(); [void][Drive]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
        $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    }
    foreach ($s in $Steps) {
        $a = $s -split ' '
        switch ($a[0]) {
            'move'  { Post 0x200 0 (LP $a[1] $a[2]) }
            'down'  { Post 0x200 0 (LP $a[1] $a[2]); Post 0x201 1 (LP $a[1] $a[2]) }
            'up'    { Post 0x202 0 (LP $a[1] $a[2]) }
            'hold'  { Post 0x200 1 (LP $a[1] $a[2]) }
            'click' { Post 0x200 0 (LP $a[1] $a[2]); Post 0x201 1 (LP $a[1] $a[2]); Post 0x202 0 (LP $a[1] $a[2]); Start-Sleep -Milliseconds 150 }
            'drag'  {
                $x1 = [int]$a[1]; $y1 = [int]$a[2]; $x2 = [int]$a[3]; $y2 = [int]$a[4]
                Post 0x200 0 (LP $x1 $y1); Post 0x201 1 (LP $x1 $y1)
                foreach ($i in 1..8) { Post 0x200 1 (LP ($x1 + ($x2 - $x1) * $i / 8) ($y1 + ($y2 - $y1) * $i / 8)) }
                Post 0x202 0 (LP $x2 $y2); Start-Sleep -Milliseconds 150
            }
            'type'  { foreach ($ch in ($s.Substring(5)).ToCharArray()) { Post 0x102 ([int]$ch) ([IntPtr]1) } }
            'key'   { Post 0x100 ([int]$a[1]) ([IntPtr]1); Post 0x101 ([int]$a[1]) ([IntPtr]0xC0000001L) }
            'hit'   {
                Post 0x200 0 (LP $a[1] $a[2]); Start-Sleep -Milliseconds 200
                $pt = New-Object Drive+POINT; $pt.X = [int]$a[1]; $pt.Y = [int]$a[2]; [void][Drive]::ClientToScreen($h, [ref]$pt)
                "hit $($a[1]),$($a[2]) -> " + [Drive]::SendMessageW($h, 0x84, [IntPtr]::Zero, (LP $pt.X $pt.Y))
            }
            'nc'    {
                # nc HT X Y [dbl]: a real non-client click (title bar, window buttons) at client X,Y.
                $pt = New-Object Drive+POINT; $pt.X = [int]$a[2]; $pt.Y = [int]$a[3]; [void][Drive]::ClientToScreen($h, [ref]$pt)
                Post 0xA0 ([int]$a[1]) (LP $pt.X $pt.Y); Start-Sleep -Milliseconds 150
                if ($a[4] -eq 'dbl') { Post 0xA3 ([int]$a[1]) (LP $pt.X $pt.Y) } else { Post 0xA1 ([int]$a[1]) (LP $pt.X $pt.Y) }
                Post 0xA2 ([int]$a[1]) (LP $pt.X $pt.Y); Start-Sleep -Milliseconds 400
            }
            'state' { $p.Refresh(); "state $($a[1]): exited=$($p.HasExited) visible=$([Drive]::IsWindowVisible($h)) minimized=$([Drive]::IsIconic($h)) maximized=$([Drive]::IsZoomed($h))" }
            'restore' { [void][Drive]::ShowWindow($h, 9); Start-Sleep -Milliseconds 300 }
            'wait'  { Start-Sleep -Milliseconds ([int]$a[1]) }
            'shot'  { Start-Sleep -Milliseconds 250; Shot $a[1] }
            'snap'  { Shot $a[1] }
            'run'   { & ([scriptblock]::Create($s.Substring(4))) }
        }
    }
}
finally {
    [void]$p.CloseMainWindow()
    if (-not $p.WaitForExit(4000)) { Stop-Process -Id $p.Id -Force; "killed" }
}

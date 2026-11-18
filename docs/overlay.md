# How the overlay works

RTSS shows text written to its shared memory (`RTSSSharedMemoryV2`) by client programs. The text can
contain tags, and two of them make an image overlay possible: `<LI="file">` loads a PNG and
`<I=w,h,x,y,w,h>` draws a part of it.

krosshair packs the pictures of all visible layers, and every frame of the animated ones, into one
atlas PNG. Each layer becomes one sprite at its anchor, moved by its offset with a layer extent
`<E>`. Transparent margins are cropped, identical frames stored once. For a layer at the center,
moved 5 px right:

```
<LI="C:\ProgramData\krosshair\osd_1a2b3c4d_0011aabb.png"><P4><L200><E=30,20,5><S=1><I=20,20,4,4,20,20>\b<P=0,0><L254>
```

An animation plays by rewriting that text with the coordinates of the next frame when its time
comes. A separate thread does it with 1 ms timer precision while something is animated.

## On-Screen Display zoom

**The game's "On-Screen Display zoom" is ignored**: the crosshair is pixel exact at any zoom from 1
to 5. RTSS measures everything in zoomed pixels, so at zoom 2 the text above would draw a 40 x 40
crosshair. krosshair reads the zoom of the game (from the RTSS app entry of the last foreground 3D
application, checked four times a second) and draws each sprite at 1/zoom of its size, which RTSS
scales back to exactly 1:1. The same layer at zoom 2:

```
<P4><L200><E=16,10,5><S=1><I=11,10,3,4,22,20>
```

Offsets and extents can only be whole zoomed pixels then, so the part of the atlas a sprite takes is
widened with transparent pixels (here one on each side) to land on that grid.

## Scale and rotation

The crosshair scale is applied before all that, in the atlas: pictures are enlarged by repeating
pixels on whole values or resampled by area averaging otherwise (after cropping, so a big
picture doesn't grow in memory first), and offsets multiplied. The rotation is applied after the
scale, also in the atlas. Letting RTSS stretch or turn the sprite instead (`<I>` takes an angle)
would blur even whole steps and quarter turns.

## What it depends on

Things measured on RTSS 7.3.5 that this depends on (`src/overlay.rs`):

- `<P0>`..`<P8>` are "sticky" positions, a 3x3 grid that RTSS places itself. The window size and the
  OSD origin don't matter, which is why nothing is calculated from the resolution.
- All sizes and positions in tags are in zoomed pixels. `<I=w,h,x,y,sw,sh>` takes the shown size
  (`w`, `h`) and the part of the image (`sw`, `sh`) separately, so a sprite shown at 1/zoom of its
  size is exactly 1:1 and as sharp as at zoom 1. Measured at zoom 2 at the center and three edge
  anchors, with and without offsets.
- `<FNT=face,height,weight,zoom>` sets font and zoom for the whole OSD, the framerate counter and
  every other program's text included, and the hooked game keeps it even after the text that set it
  is gone. That rules it out for ignoring the zoom.
- The order has to be `<Pn><Ln>`. The other way round the layer lands in the wrong place.
- Sprites are stacked in text order: a later one is drawn on top, whatever the layer numbers.
- `<E=w,h,origin>` makes the layer `w` x `h` and puts its content in the `origin` cell (0..8, the
  3x3 grid again) of that box. It is pixel exact at every anchor. It can only enlarge.
- **RTSS keeps one image for the whole OSD.** While the OSD holds a single `<LI>` image, rewriting
  the text costs nothing: 50 rewrites a second left the bench at about 6000 fps with no frame over
  2 ms. With two different images (from two programs, or two `<LI>` files), every OSD update
  reloads them: the bench dropped to 31 fps with 42 ms frames, and even without any rewrites it got
  two frames over 10 ms a second. Worse, every `<I>` sprite is cut from whichever image was loaded
  last, so one program's crosshair is drawn with pieces of the other's picture. That is why
  everything goes into one atlas, and why an image from another program in the OSD (an
  OverlayEditor layout with pictures, another crosshair tool) breaks the crosshair and makes the
  game stutter.
- An atlas up to 2048 x 2048 is drawn; with a side of 4096 or more it is not. When the animations
  don't fit, the biggest ones are shown as still pictures and the header says so.
- Text rewrites up to 60 a second all reach the screen; at 100 a second about one in eight is
  skipped (RTSS redraws the OSD at most every 10 ms). GIF frames shorter than 20 ms are shown for
  100 ms, the way browsers treat them.
- A layer lasts until the next `<L>` tag, and RTSS appends its own text (the framerate counter)
  after the clients' text. Left in the crosshair layer it makes the layer bigger and shifts the
  sprite, so the text ends with `\b<P=0,0><L254>`, and a second slot (`<P=0,0><S=100>`) restores
  the text size.
- `<S=1>` before `<I>` shrinks the text line to almost nothing. With `<S=10>` the line was still 4
  zoomed pixels tall in a 29 px font, and sprites smaller than that sat up to 2 px too high.
- `<LI>` fails on long paths (and very long ones can crash the game), so the atlas is written to
  `%ProgramData%\krosshair`. RTSS caches images by path, so every atlas gets a new file name.
- If the atlas file is missing, RTSS draws garbage instead of the sprite. The overlay thread
  checks once a second that the file is still there and writes it again under a new name.
- Writing takes the `dwBusy` lock and always releases it. If the renderer holds the lock, the
  write is retried shortly after.

## Limits

The OSD text is shared by every hooked game, so with two games running at different zooms
the crosshair is exact only in the one that was in front last. Above zoom 5 it is no longer exact.
A crosshair with an odd thickness can't be centered on an even-sized screen; it is centered on the
pixel right and below the middle.

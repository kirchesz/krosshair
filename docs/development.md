# Local development

krosshair is written in Rust with [gpui](https://www.gpui.rs/), the UI framework of the Zed
editor.

Requirements:

- Windows
- Rust (stable)
- MSVC build tools and the Windows SDK (`rc.exe` on PATH for the app icon)
- RTSS, to see the overlay

```powershell
git clone https://github.com/kirchesz/krosshair
cd krosshair
cargo build --release
```

Common commands:

| Command | Purpose |
| --- | --- |
| `cargo build --release` | Builds `target\release\krosshair.exe` |
| `cargo test` | Checks the rasterizer, the atlas layout, the configuration and the update check |
| `cargo clippy --all-targets` | Lints |
| `.\tools\make-icon.ps1` | Draws the app icon again after the logo changes |
| `.\tools\make-preview.ps1` | Draws the README preview, `docs/images/preview.png`, from the window screenshot `docs/images/gallery.png` (rounded corners, shadow, generated wallpaper) |
| `.\target\release\krosshair.exe --screen settings` | Opens a screen right away |

Only one copy runs at a time, so close the real one before starting a test build. Testing against
RTSS, without disturbing a game, is described [below](#testing-against-rtss).

## Releases and updates

GitHub Actions builds everything (`.github/workflows`):

- `ci.yml` runs on every push and pull request: clippy with warnings as errors, then the tests.
- `release.yml` runs on a tag `vX.Y.Z`. It checks that the tag matches `version` in
  `Cargo.toml`, tests, builds, and publishes a GitHub release with `krosshair.exe` and
  `krosshair.exe.sha256`, with notes generated from the commits.

To release, set the new version in `Cargo.toml` and commit it as `chore(release): vX.Y.Z`
(commit messages follow Conventional Commits, see [AGENTS.md](../AGENTS.md#commits)), then:

```powershell
git tag v0.2.0
git push origin v0.2.0
```

The app checks for updates when *Check for updates* is on: 5 seconds after the start, then every
12 hours, and right away when the setting is switched on (`src/update.rs`, `src/ui/updates.rs`).
It asks the GitHub API for the latest release, which skips drafts and pre-releases. A release
newer than the running version shows in the header with an *Update* button and a link to its
notes. *Update* works in these steps:

1. It downloads `krosshair.exe` next to the running one and checks it against
   `krosshair.exe.sha256`.
2. It renames the running exe to `krosshair.exe.old`; Windows allows renaming a running exe.
3. It moves the new exe into the old name and starts it with `--after-update <pid>`.
4. The new copy waits for the old one to quit, then removes `krosshair.exe.old`.
5. The old copy quits the usual way, taking the crosshair down.

If any step fails, the old exe stays in place, and the header shows why with a link to download
the release by hand. Requests go through WinHTTP, so they use the system proxy and certificates;
the User-Agent is `krosshair/<version>`.

## Testing against RTSS

`tools/` has what was used to check the overlay and the interface:

- `osdtest.cpp` is a small D3D11 window that RTSS hooks like a game. With `--watch` it reports
  where the crosshair landed relative to the window center. RTSS only draws in processes its
  profiles allow, so name the binary after a game that has a profile. Build it with
  `cl /EHsc /I "<RTSS folder>\SDK\Include" osdtest.cpp`.
- `animtest.cpp` is a bench for sprite animation. It rewrites the OSD text N times a second to show
  the next frame of a sprite sheet and reads back from the screen which frame got there. With
  `--watch` it only reads back, for checking krosshair playing a GIF made like its sprite sheet.
  With `nocap`, or `--texts <file>` (cycles the lines of a file), it measures only the frame rate
  and the longest frames, because capturing the screen costs far more than RTSS does.
- `e2e.ps1` runs krosshair with test configurations and the bench, and prints the measured
  offsets.
- `drive.ps1` starts krosshair and clicks through it by posting messages to its window (the real
  cursor doesn't move), taking screenshots of the window on the way. Its `state` step prints
  whether the window has exited, is visible, minimized or maximized.

Switches for this: the `KROSSHAIR_CONFIG` environment variable names another configuration file,
`KROSSHAIR_ZOOM` replaces the zoom read from the game, `KROSSHAIR_RUN_KEY` names a registry key
under HKCU to use instead of the Run key, `KROSSHAIR_UPDATE_URL` replaces the GitHub API address
of the latest release (a local server answering like it tests the whole update from a copy of
the exe in another folder), and `--screen gallery|editor|draw|layer|settings|about` opens that
screen at start. Only one copy runs at a time, so close the real one before testing.

RTSS draws the OSD text in every hooked game at once, so make sure RTSS has hooked nothing but the
bench before testing (the app entries in its shared memory list the hooked processes): test
crosshairs show up in it, and an `<FNT>` used for testing stays in that game until it restarts
(an empty `<FNT>` tag in a slot resets it to the profile's font and zoom). The bench window uses an
RTSS profile with zoom 1. To check zoom 2, write `<FNT=<font>,<height>,
<weight>,2>` into a slot of your own (it makes the whole OSD zoom 2 for that process) and start
krosshair with `KROSSHAIR_ZOOM=2`.

## App icon

The app icon is the About screen's logo, a rounded gray square with the crosshair glyph, in
`assets/icon/krosshair.ico`. `build.rs` puts it into the exe with `rc.exe`, which has to be on
PATH; without it the exe builds with no icon and a warning. The `.ico` is drawn by
`tools/make-icon.ps1`. The big sizes keep the About proportions, and the sizes up to 48 px get a
larger, bolder glyph so it still reads in the taskbar. Run the script again after changing the
logo.

## Fonts and colors

The interface fonts, Archivo and IBM Plex Mono (Regular and Medium), are built into the program
from `assets/fonts`, so nothing has to be installed. Both are under the SIL Open Font License 1.1;
the license texts are next to the font files. If the fonts can't be loaded, Segoe UI and Consolas
are used.

The colors are those of IBM Carbon's g100 theme (`@carbon/themes` 11): neutral grays from
`background` to `text-primary`, `link-primary` blue for the accent, `interactive` blue for a switch
that is on, `support-error` red. They are in `src/ui/theme.rs` with the token each one comes from;
everything else, hairlines and grid lines included, is derived from them.

## Source layout

| File | What it holds |
|---|---|
| `src/model.rs` | Data types: editor layers, profiles, configuration, preset crosshairs |
| `src/raster.rs` | Editor layers to pixels, PNG and GIF reading, PNG writing |
| `src/overlay.rs` | Layers to atlas and OSD text, the thread that keeps RTSS up to date |
| `src/rtss.rs` | RTSS shared memory: taking, writing and freeing OSD slots |
| `src/store.rs` | Configuration file and gallery folder |
| `src/tray.rs` | Minimize or close to the notification area: icon, its menu, the first-time question |
| `src/autorun.rs` | Starting with Windows: the Run key value |
| `src/displays.rs` | The connected displays and their names |
| `src/update.rs` | Updates: the latest GitHub release, downloading, checking and swapping the exe |
| `src/ui/` | The window: `mod.rs` (state, navigation, header, profiles), one file per screen, `widgets.rs` |
| `assets/fonts/` | The interface fonts and their licenses |
| `assets/icon/` | The app icon and the resource script that puts it into the exe |

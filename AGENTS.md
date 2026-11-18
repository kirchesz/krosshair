# AGENTS.md

Notes for coding agents (and people) working on krosshair: a crosshair overlay for Windows games,
drawn through the on-screen display of RivaTuner Statistics Server (RTSS). Rust, with
[gpui](https://www.gpui.rs/) for the window. Windows only.

Read [docs/development.md](docs/development.md) for building, releases, testing and the source
layout, and [docs/overlay.md](docs/overlay.md) before touching `src/overlay.rs` or `src/rtss.rs`:
it records what was measured in RTSS and why the code works the way it does.

## Build and check

```powershell
cargo build --release
cargo clippy --release --all-targets -- -D warnings
cargo test --release
```

- The build needs the MSVC build tools and the Windows SDK. `rc.exe` must be on PATH (a
  Developer PowerShell, or the MSVC environment), or the exe builds without its icon.
- CI runs exactly the clippy and test commands above; run both before calling a change done.
- There is no `rustfmt.toml` and the code is not run through rustfmt. Match the surrounding code:
  long builder chains of gpui elements stay on one line where they fit in about 120 columns.

## Running it safely

- **Only one copy runs at a time.** A second one only brings the first one's window to the front
  and exits. The user may have their own copy running; close it before testing a build.
  Closing it with `WM_CLOSE` can be held by the tray setting or the first-time tray question, so
  check that the process really exited.
- **Never test with the user's configuration.** Point `KROSSHAIR_CONFIG` at a copy with
  `"power": false`, so nothing reaches RTSS, and with `"tray_asked": true` unless the tray question
  is what you test.
- **RTSS draws the OSD in every hooked game at once.** Make sure no game is hooked before
  anything that turns the overlay on; an `<FNT>` tag sent for testing stays in that game until it
  restarts.
- **Drive the window by posting messages**, never by moving the real cursor:
  `tools/drive.ps1` clicks, drags, types, takes screenshots and prints the window state.
  `--screen settings` (or `gallery`, `editor`, `layer`, `about`...) opens a screen at start.
- `KROSSHAIR_UPDATE_URL` points the update check at a local server instead of GitHub; test
  updates on a copy of the exe in another folder, since a successful update replaces the exe.

## Code conventions

- **Comments say why.** Every item has a doc comment in plain sentences; inline comments explain
  what is not obvious, such as RTSS behavior, Win32 quirks and measured values. Keep them true
  when the code changes.
- **One screen per file** in `src/ui/`. Shared widgets live in `widgets.rs`, state and navigation
  in `mod.rs`.
- **Colors only from `src/ui/theme.rs`** (IBM Carbon g100 tokens), through `c()`, `alpha()`,
  `mix()` and `fade()`. No new hex values in screens.
- **Hover and switch animations** go through `.fx(self, cx, ...)` and `flip`/`flip_amount`. Every
  element that uses them needs an id that is unique on screen. A knob drawn in two places takes
  its `Place` into account (see `Knob::element_id`).
- **Configuration is read from older files too.** A new field in `Config` gets
  `#[serde(default)]`, or a default function. A renamed or reshaped one is migrated in
  `store::parse_config`. Either way a test reads an old file.
- **Win32 calls** use the `windows` crate. Add the feature to `Cargo.toml` rather than a new crate
  when Windows already does the job (WinHTTP for downloads, for example).
- **Edit files one change at a time.** No regex replacements across files: one such replacement
  once corrupted two source files before there was a commit to restore them from.
- **When behavior changes, update the docs**: `docs/development.md` for the build, releases,
  testing and layout, `docs/overlay.md` for anything RTSS. Keep `README.md` short; it is the front
  page.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<scope>): <summary>

<body: what changed and why, wrapped at 72 columns>
```

- **Types:** `feat` (something new for users), `fix` (a bug), `perf`, `refactor` (no change in
  behavior), `docs`, `test`, `ci`, `build` (Cargo, `build.rs`, icon, dependencies), `chore`.
- **Scopes** (optional, one word), by area: `overlay`, `rtss`, `raster`, `store`, `tray`,
  `update`, `ui`, or a screen: `layers`, `gallery`, `editor`, `settings`, `about`.
- **Summary:** imperative, lower case, no period at the end, at most 72 characters.
  `feat(tray): ask what minimize and close do on first use`, not `Added tray question.`
- **Body:** say why, when the summary doesn't. Mention measured numbers and RTSS behavior there.
- **Breaking changes**, such as a configuration that older versions can't read: add `!` after the
  type or scope and a `BREAKING CHANGE:` line in the body.
- One logical change per commit. Formatting-only changes go in their own `refactor` or `chore`
  commit.

Releases: bump `version` in `Cargo.toml` in a commit `chore(release): vX.Y.Z`, then tag it
`vX.Y.Z` and push the tag. The release workflow builds and publishes it; see
[docs/development.md](docs/development.md#releases-and-updates).

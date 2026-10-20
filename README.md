# Mega Win Alt Tab

## Security Notice

Mega Win Alt Tab releases are currently unsigned Windows executables. This
project is open source, but Azure Artifact Signing requires a monthly paid plan,
so official releases are not code-signed at this time.

Some antivirus products may show false positives because Mega Win Alt Tab uses
automation-style Windows features: it registers a global hotkey, enumerates
visible windows and window titles, enumerates running processes to identify
background apps, captures DWM thumbnails, uses UI Automation to read browser tab
titles, launches selected apps and folders, writes user configuration files
under the user profile, can add a per-user startup registry entry, and checks
GitHub Releases for updates.

Mega Win Alt Tab is not malware and is not a virus. You are welcome to inspect
the source code and build it yourself. Do not install releases from anywhere
other than the official GitHub Releases page for this repository, or from a
build you created yourself from source.

Mega Win Alt Tab is a native Rust Windows switcher opened with a configurable
global shortcut (`Ctrl+Alt+Space` by default). It shows switchable top-level
windows, live DWM thumbnails, typed search, and Chrome tab-title results.

## Build and Run

```powershell
cargo run
```

Press `Ctrl+Alt+Space` (the default shortcut) to open the overlay, then type to
search.

When the app is running, it also adds a notification-area icon. Click the icon
to open the switcher. Right-click it to open the switcher, configure the global
shortcut, open the app log, copy the log path, toggle `Run at startup`, or exit
the application. The startup option writes a per-user Windows startup entry
that launches the app with `--startup`. Normal launches show a short splash
screen; `--startup` launches quietly to the tray.

The hamburger button in the switcher header opens the same menu as right-clicking
the notification-area icon, including configuration, log, startup, and exit
commands.

The `Configure` submenu offers several reliable shortcut presets plus
`Change Shortcut Keys...`. When a custom shortcut is active, its combination is
shown as a checked `Custom: ...` item in this menu. To change it, choose `Change
Shortcut Keys...`, hold at least one of Ctrl, Alt, or Shift, and press another
key in the capture panel. The panel shows the pending combination and remains
open until you choose `Apply`, press Enter, cancel, or close it. `Apply` and
Enter save the pending shortcut; `Cancel`, Esc, and `[x]` discard it. Presets
apply immediately. Applied shortcuts are stored in
`%LOCALAPPDATA%\MegaWinAltTab\settings.json`. If Windows reports that a shortcut
is already in use, the app displays a notification, writes the rejected
shortcut and Windows error to the log, and keeps the previous working shortcut.
Navigation and action keys used while the overlay is open are currently fixed.

If the configured shortcut is already registered by another application, Mega
Win Alt Tab keeps running and shows a notification explaining that the hotkey
is not available. The tray icon remains usable. Runtime diagnostics are written
to `%LOCALAPPDATA%\MegaWinAltTab\mega-win-alt-tab.log`; use `Open app log file`
or `Copy log file path` from the tray menu when reporting a tray, hotkey, or
startup problem. New entries use local system time in
`YYYY-MM-DD HH:MM:SS` format.

The startup entry stores the exact executable path and file name that was
running when you enabled `Run at startup`. If you download a new version with a
different `.exe` name or location, run that new executable and toggle
`Run at startup` again so Windows starts the new copy.

When enabling `Run at startup`, the app also checks for possible older startup
entries created from early numbered release executable names, such as
`mega-win-alt-tab-vX.Y.Z-windows-x64.exe`. If it finds any, it shows the entry
names and paths and asks whether to remove them. Choosing `No` leaves them alone
and still sets the current copy to run at startup.

When the overlay is shown, the app checks GitHub Releases in the background. If
a newer release exists, the overlay shows an update notice that opens the
GitHub Releases page when clicked. It does not auto-download or install updates.

## Keys

| Key | What it does |
| --- | --- |
| Configured shortcut (default `Ctrl+Alt+Space`) | Open or hide the switcher overlay. |
| Text input | Search window titles, Chrome tab titles, app names, or favorite folders in app mode. |
| `Backspace` | Delete the last character in the search text. |
| `Up` | Move selection up. |
| `Down` | Move selection down. |
| `Tab` | Move selection down. |
| `Enter` | Activate the selected window/tab, launch the selected app, or open the selected favorite folder, then close the overlay. |
| `Esc` | Close the overlay without quitting the app. |
| `Left` | Show the selected window/tab without moving keyboard focus away from the overlay; tap again to briefly highlight it. |
| `Right` | Show the selected window/tab without moving keyboard focus away from the overlay; tap again to briefly highlight it. |
| `Ctrl+F` | Maximize the selected window, or restore it if it is already maximized. |
| `Ctrl+S` | Minimize the selected window. |
| `Ctrl+W` | Close the selected window, or remove the selected favorite folder in app mode. |
| `Ctrl+Home` | Center the selected window as a square on wide or portrait screens, or fill classic 5:4 and 4:3 screens without maximizing. |
| `Ctrl+Page Up` | Size the selected window into the top or left band before the centered area. |
| `Ctrl+Page Down` | Size the selected window into the bottom or right band after the centered area. |
| `Ctrl+Left` | Move the selected window/tab parent, or matching running app window, to the previous screen in the Windows layout order. |
| `Ctrl+Right` | Move the selected window/tab parent, or matching running app window, to the next screen in the Windows layout order. |
| `Ctrl+D` | Toggle all-desktops mode for windows and tabs. |
| `Ctrl+?` / `Ctrl+/` | Toggle app launcher mode while preserving the current search text. |

## Quality Checks

Run the same checks locally that GitHub Actions runs:

```powershell
.\scripts\check.ps1
```

That script runs formatting, tests, linting, and a debug build:

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
```

CI is configured in `.github/workflows/ci.yml` and runs on `windows-latest` for
pushes to `main` and pull requests. The Windows runner matters because the app
uses Windows APIs for window enumeration, thumbnails, tray behavior, and
activation.

## Releases

Releases are created from GitHub Actions with the `Release Windows` workflow.
Run it manually, enter a version such as `0.2.0`, and it will:

- Update the version in `Cargo.toml` and `Cargo.lock`.
- Run the same checks as CI.
- Build `target\release\mega-win-alt-tab.exe`.
- Commit the version bump, tag `vX.Y.Z`, and create a GitHub release.
- Attach both a standalone Windows `.exe` and a `.zip` containing the executable,
  README, and companion Chrome extension.

## Chrome Tab Search

The app has two Chrome tab providers:

- Windows UI Automation scans visible Chrome windows without setup and tries to
  select the matching tab when you activate a tab result.
- The optional companion extension in `extension/chrome` reports tab titles and
  enables more reliable exact-tab activation.

When a Chrome window title represents the same page as a matching tab result,
the tab result is shown and the duplicate browser-window result is hidden.

## App Launcher Mode

While the overlay is open, press `Ctrl+?` (or `Ctrl+/`) to switch the current
search into app launcher mode. The app scans current-user and all-users Start
Menu application shortcuts, Windows App Paths, Windows Start app catalog
entries, running background apps that match installed app identities, and
user-added favorite folders. Help files, documents, and website shortcuts in the
Start Menu are skipped. It collapses duplicate app names and duplicate launch
targets, then launches the selected app or opens the selected folder with Enter.
Background app results are labeled `Running in background`. Multi-process apps
are treated as one application, and Mega Win Alt Tab prefers a process that owns
a restorable hidden window. Packaged apps are activated through their registered
Windows app identity; other apps try to show a hidden app window first, then fall
back to launching the app again.

To add a favorite folder, drag a folder onto the overlay. The app stores
favorites in `%LOCALAPPDATA%\MegaWinAltTab\favorite-folders.json`, switches to
app mode, and selects the dropped folder. Favorite folder results show the
folder name and full path. In app mode, select a favorite folder and press
`Ctrl+W` to remove it.

## All Desktops Mode

While the overlay is open, press `Ctrl+D` to include windows and browser tabs
from other Windows virtual desktops. Results are labeled `Current desktop`,
`Other desktop`, or `Desktop unknown`. Selecting an off-desktop window uses the
official Windows virtual desktop API to move that window onto the current
desktop and then focus it.

To install the extension, open `chrome://extensions`, enable developer mode,
choose `Load unpacked`, and select:

```text
D:\github\mega-win-alt-tab\extension\chrome
```

## Notes

- This first version does not replace the built-in Windows Alt+Tab.
- Search covers window titles, app names, and tab titles. It does not index page
  contents or URLs.
- On multi-monitor setups, each thumbnail shows the Windows display number for
  the screen containing that window.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).

# Mega Win Alt Tab

Mega Win Alt Tab is a native Rust Windows switcher opened with `Ctrl+Alt+Space`.
It shows switchable top-level windows, live DWM thumbnails, typed search, and
Chrome tab-title results.

## Build and Run

```powershell
cargo run
```

Press `Ctrl+Alt+Space` to open the overlay, then type to search.

When the app is running, it also adds a notification-area icon. Right-click that
icon to toggle `Run at startup` or choose `Exit` to quit the application. The
startup option writes a per-user Windows startup entry that launches the app
with `--startup`. Normal launches show a short splash screen; `--startup`
launches quietly to the tray. Click the icon to open the switcher.

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
| `Ctrl+Alt+Space` | Open or hide the switcher overlay. |
| Text input | Search window titles, Chrome tab titles, or app names in app mode. |
| `Backspace` | Delete the last character in the search text. |
| `Up` | Move selection up. |
| `Down` | Move selection down. |
| `Tab` | Move selection down. |
| `Enter` | Activate the selected window/tab, or launch the selected app in app mode, then close the overlay. |
| `Esc` | Close the overlay without quitting the app. |
| `Left` | Show the selected window/tab without moving keyboard focus away from the overlay; tap again to briefly highlight it. |
| `Right` | Show the selected window/tab without moving keyboard focus away from the overlay; tap again to briefly highlight it. |
| `Ctrl+F` | Maximize the selected window, or restore it if it is already maximized. |
| `Ctrl+S` | Minimize the selected window. |
| `Ctrl+W` | Close the selected window with the app's normal close behavior. |
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
Menu shortcuts, Windows App Paths, and packaged Windows apps from the Start app
catalog. It collapses duplicate app names and duplicate launch targets, then
launches the selected app with Enter.

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

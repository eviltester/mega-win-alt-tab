# Mega Win Alt Tab

Mega Win Alt Tab is a native Rust Windows switcher opened with `Ctrl+Alt+Space`.
It shows switchable top-level windows, live DWM thumbnails, typed search, and
Chrome tab-title results.

## Build and Run

```powershell
cargo run
```

Press `Ctrl+Alt+Space` to open the overlay. Type to search, use Up/Down to move,
Right Arrow to bring the selected window forward while keeping the switcher open,
Left Arrow to move the selected window to the next screen,
Ctrl+Right/Ctrl+Left to resize thumbnails, Ctrl+D to include windows and tabs
from all virtual desktops, Ctrl+? to toggle matching installed apps, Enter to
activate or launch the selected result and close the switcher, and Esc to close.

When the app is running, it also adds a notification-area icon. Right-click that
icon and choose `Exit` to quit the application. Click the icon to open the
switcher.

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
search into app launcher mode. The app scans the current-user and all-users
Start Menu shortcuts, collapses duplicate app names, and launches the selected
shortcut with Enter.

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

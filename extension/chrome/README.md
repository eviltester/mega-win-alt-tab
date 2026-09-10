# Mega Win Alt Tab Chrome Extension

This optional extension augments the Windows accessibility fallback by sending
Chrome tab titles to the native Rust app on `http://127.0.0.1:39276`.

## Install for development

1. Open `chrome://extensions`.
2. Enable `Developer mode`.
3. Choose `Load unpacked`.
4. Select this directory: `D:\temp\mega-win-alt-tab\extension\chrome`.

The desktop app still works without this extension, but tab activation is more
reliable when the extension is installed.

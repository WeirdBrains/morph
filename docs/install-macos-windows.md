# Installing Morph on macOS and Windows (unsigned builds)

Morph's macOS and Windows builds are pre-releases and are **not signed** with
an Apple Developer ID or a Windows code-signing certificate. The system warns
once before the first start. The steps below get past that warning; download
only from the [GitHub releases](https://github.com/WeirdBrains/morph/releases)
and check the file against the release's `SHA256SUMS`.

## macOS (11 Big Sur or later; Apple silicon and Intel)

The app is universal (arm64 + x86_64) and ad-hoc signed, not notarized.

1. Download `morph-<version>-universal-apple-darwin.dmg` (or the `.zip`).
2. Check it: `shasum -a 256 morph-*-universal-apple-darwin.dmg` and compare
   with `SHA256SUMS`.
3. Open the dmg and drag **Morph** to **Applications** (from the zip: double-click
   it and move `Morph.app` to Applications).
4. First start, Gatekeeper says Morph "cannot be opened because Apple cannot
   check it for malicious software" (or "is damaged"):
   - macOS 15 and later: try to open Morph once, then open **System Settings >
     Privacy & Security**, scroll to Security, click **Open Anyway** next to
     the Morph message and confirm with your password.
   - macOS 14 and earlier: Control-click (right-click) Morph in Applications,
     choose **Open**, then **Open** in the dialog.
   - Or in Terminal, remove the download quarantine:
     `xattr -dr com.apple.quarantine /Applications/Morph.app`
5. Later starts open normally.

From a terminal: `/Applications/Morph.app/Contents/MacOS/morph --version`.
Data lives in `~/.morph` (or `$MORPH_HOME`).

## Windows (10 21H2 or later, 11; x64)

Morph uses the WebView2 runtime, which ships with Windows 11 and current
Windows 10. If it is missing, install the Evergreen runtime from
<https://developer.microsoft.com/microsoft-edge/webview2/>.

Installer (`morph-<version>-x86_64-pc-windows-msvc.msi`):

1. Check it in PowerShell: `Get-FileHash morph-*-x86_64-pc-windows-msvc.msi`
   and compare with `SHA256SUMS` (lower case there).
2. Double-click the msi. SmartScreen shows "Windows protected your PC":
   click **More info**, then **Run anyway**. (If the browser blocked the
   download, choose **Keep** in its downloads list first.)
3. The installer asks for administrator rights and installs to
   `C:\Program Files\Morph`, with a **Morph** entry in the Start menu.
   Uninstall from **Settings > Apps**.

Zip (`morph-<version>-x86_64-pc-windows-msvc.zip`, no installation):

1. Right-click the zip, **Properties**, tick **Unblock**, **OK** (or
   `Unblock-File morph-*.zip` in PowerShell); this avoids the SmartScreen
   prompt for the files inside.
2. Extract it anywhere and run `morph.exe`. If SmartScreen still appears:
   **More info > Run anyway**.

From a terminal: `morph.exe --version`. Data lives in `%USERPROFILE%\.morph`
(or `%MORPH_HOME%`).

## What differs from Linux in these builds

- Pages are the system's own engine: WKWebView (Safari's) on macOS, WebView2
  (Edge's Chromium) on Windows, as child views of Morph's window.
- On macOS every Ctrl shortcut also works with Cmd (Cmd+T, Cmd+L, Cmd+W ...).
- Not there yet: making Morph the default browser, links from other apps on
  macOS (Apple Events), popups counted on the tab (they open as the engine's
  own windows), a crashed-page panel on Windows. On macOS before 14 all
  profiles share one website data store.

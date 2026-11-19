# Messages Desktop

A small, maintained Windows desktop app for [Google Messages for Web](https://messages.google.com/web).
It replaces the abandoned Electron wrapper (stuck on Electron 19, no updates since 2023).

Not affiliated with Google.

## Why it's different

- **The browser engine stays patched.** The app uses the WebView2 (Microsoft Edge) runtime built into
  Windows, which Microsoft updates on its own. It can't get stuck on an old engine.
- **The app updates itself.** New versions are published as signed GitHub Releases and installed
  automatically in the background (only while the window isn't in use).
- **Real installer.** Per-user install, Start menu entry, uninstall from Settings. No admin rights needed.
- **Small.** About 10 MB instead of ~150 MB.

## Features

- Closes to the tray so messages keep arriving; left-click the tray icon to show or hide
- Native Windows notifications; clicking one opens that conversation
- Unread indicator on the taskbar button and tray icon
- Optional start with Windows (starts hidden in the tray)
- Only one copy runs; launching again brings the window forward
- Remembers window size and position
- Links open in your default browser
- Stays paired between restarts; Ctrl +/- to zoom

Pair it the same way as the website: open Messages on your phone, choose **Device pairing**, and scan the
QR code. Google may block signing in with a Google account inside embedded browsers, so QR pairing is the
reliable route.

## Building

Requirements: Windows 10/11, [Rust](https://rustup.rs) (stable, MSVC), Node.js 20+, and the
Visual Studio C++ build tools.

```powershell
npm ci
npm run dev      # run a development build
npm run build    # build the installer into src-tauri\target\release\bundle\nsis
```

## Releasing

1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`.
2. Commit and push to `main`.
3. The **Release** workflow sees the new version, builds the installer, signs the update package and
   publishes release `v<version>` with `latest.json`. (It can also be run by hand from the Actions tab.) Installed copies update within a few hours, or right away from
   tray menu > **Check for updates**.

The workflow needs the repository secret `TAURI_SIGNING_PRIVATE_KEY` (the update signing key). The matching
public key is in `tauri.conf.json`. If the private key is lost, installed copies can't verify new updates
and must be reinstalled by hand once with a build signed by a new key.

## How it works

- `src-tauri/src/lib.rs` creates the window, tray and plugins.
- `src-tauri/src/inject.js` runs inside the Messages page and forwards notifications, the unread count
  and external links to the app. Only `messages.google.com` can call the app, and only these three
  commands (`src-tauri/capabilities/messages-web.json`).
- `src-tauri/src/notify.rs` shows the toasts; `src-tauri/src/updates.rs` handles self-updates.

# my-browser

A memory-efficient web browser for macOS, written in Rust on top of the system WebKit (`WKWebView`). Built for everyday browsing and YouTube, with a built-in ad blocker, an encrypted password manager, and first-class control by Claude (an in-browser chat panel and an MCP server for Claude Code).

> Personal project, version 0.1.0. It runs well as a daily driver on the author's Mac, but it is ad-hoc signed and not notarized, so it is not yet packaged for distribution.

## Download

Get the latest DMG from [Releases](https://github.com/deryfebriantara/My-browser/releases/latest), open it, and drag **my-browser** to **Applications**. Requires macOS 14 or later on Apple Silicon.

The app is ad-hoc signed and not notarized, so macOS blocks the first launch:
- **macOS 15:** open it once, then click **Open Anyway** in **System Settings > Privacy & Security**.
- **macOS 14:** right-click the app, choose **Open**, then **Open** again.
- Or run `xattr -dr com.apple.quarantine /Applications/my-browser.app`.

## Features

### Browsing
- Tabs with keyboard shortcuts (⌘T, ⌘W, ⇧⌘T, ⌘1–9, ⌃⇥), session restore, and back/forward.
- **Tab sleep:** background tabs release their WebKit process after 30 minutes unseen, or when more than 5 tabs are awake. When macOS reports memory pressure, all eligible background tabs sleep immediately. Tabs playing media and tabs with unsent typed text (comments, forms, messages) are kept awake. Scroll position and YouTube playback position are restored on wake.
- **History** (⌘Y) with per-day grouping, search, and Clear History. URLs are normalized: tracking parameters and fragments are stripped, and reloads, tab wakes, and session restores are not counted as new visits.
- **Address bar autocomplete:** inline completion and a native suggestion dropdown built from local history and open tabs, including "Switch to tab". Nothing you type is sent to a search engine until you press Enter.
- **Ad blocker** compiled from EasyList and EasyPrivacy into WebKit content rules, with a per-site toggle. YouTube video ads are skipped automatically.
- Downloads go to `~/Downloads`.
- HTTP/3 is disabled by default, because QUIC can collapse on networks that throttle UDP (see [`docs/networking.md`](docs/networking.md)).

### Password manager
- Encrypted vault: Argon2id (256 MiB, 3 passes) protects a random vault key, and entries are encrypted with XChaCha20-Poly1305. The vault file is created with `0600` permissions.
- Offers to save passwords after you log in. Fill with ⌘\ only when you ask; it never fills automatically. Matching is by exact origin.
- Capture and fill scripts run in an isolated WebKit content world that page scripts cannot see.
- Auto-locks after 10 minutes idle, on screen lock, and on sleep.
- Copied passwords are marked concealed and cleared from the clipboard after 30 seconds.
- CSV import (Chrome, Safari/Passwords, Firefox, Bitwarden) and CSV export, plus a password generator.

### Claude integration
- **Ask Claude panel** (⌘J): chat about the current page using your Claude Code subscription; no API key needed. The panel is read-only: it can read and screenshot pages but cannot navigate, click, type, or run scripts. Sign-in happens in a browser tab.
- **MCP server** (`my-browser --mcp`) for Claude Code, with 26 tools:
  - tabs and navigation;
  - page text, element listing, and inspection;
  - screenshots (viewport, full page, single element);
  - device and dark-mode emulation;
  - console and network logs;
  - YouTube control;
  - memory usage;
  - history search;
  - a one-call `check_page` for web development.
- **Client approval:** every program that connects is identified from the kernel (peer PID and executable path) and must be approved once: Always Allow, Allow Once, or Deny.
- Clicking, typing, and running JavaScript additionally require the **Claude > Allow Claude to Interact** menu toggle.
- **Pick Element for Claude** (⌘⇧E): point at an element on the page, then ask Claude about it.

### Memory
Measured on an Apple Silicon Mac with 16 GB RAM, one sample each (details and caveats in [`docs/testing/measurements.md`](docs/testing/measurements.md)):

| Scenario | my-browser | Safari 18.6 |
|---|---|---|
| One YouTube tab, 1080p, after 2 minutes | 1,415 MB | 1,812 MB |
| Four tabs, before and after three of them slept | 399 MB → 123 MB | – |

## Requirements

- macOS 14 or later (developed on macOS 15)
- Rust (stable, via [rustup](https://rustup.rs))
- Xcode Command Line Tools: `xcode-select --install`
- Optional: [Claude Code](https://claude.com/claude-code), for the Ask Claude panel and the MCP server

## Build and run

```sh
scripts/update-filters.sh   # download EasyList/EasyPrivacy and compile the ad-block rules (first time and occasionally)
scripts/run.sh              # build a release bundle into dist/my-browser.app and open it
```

`scripts/bundle.sh` only builds `dist/my-browser.app`, which is ad-hoc signed. `scripts/package.sh` wraps it into a DMG and a zip in `dist/`.

Releases are built by CI: bump `CFBundleShortVersionString` in `resources/Info.plist`, then push a matching tag (for example `git tag v0.2.0 && git push origin v0.2.0`). The workflow checks, builds, and publishes the DMG and zip on the Releases page.

To register the MCP server with Claude Code:

```sh
scripts/pasang-mcp.sh       # adds my-browser to Claude Code (user scope); start a new Claude Code session afterwards
```

## Development

```sh
scripts/check.sh            # rustfmt, clippy -D warnings, all tests (including a real-WebKit smoke test), cargo audit
scripts/check.sh --quick    # format + clippy only
git config core.hooksPath .githooks   # run the quick check before every commit
```

| Crate | Purpose |
|---|---|
| `crates/core` | Browser logic without macOS dependencies: tabs, tab sleep, history, suggestions, settings, control protocol |
| `crates/engine-webkit` | `Engine`/`Page` implementation on `WKWebView` |
| `crates/app` | AppKit application: window, menus, address bar, panels, dialogs, control socket |
| `crates/mcp` | MCP server that relays tools to the browser's control socket |
| `crates/vault` | Encrypted password vault |
| `crates/memtool`, `crates/blocklist-gen` | Memory measurement and ad-block rule generator |

Settings live in `~/Library/Application Support/my-browser/settings.json`. Quit the browser before editing that file. See [`docs/development.md`](docs/development.md) for testing conventions and debug-only test hooks.

### Documentation
- [`docs/tab-sleep.md`](docs/tab-sleep.md): tab sleep
- [`docs/history.md`](docs/history.md): history and address bar suggestions
- [`docs/password.md`](docs/password.md): password manager
- [`docs/ask-claude.md`](docs/ask-claude.md): Ask Claude panel
- [`docs/mcp.md`](docs/mcp.md): MCP server and client approval
- [`docs/networking.md`](docs/networking.md): why HTTP/3 is disabled
- [`docs/development.md`](docs/development.md): development, tests, and debug hooks
- [`docs/testing/manual-checklist.md`](docs/testing/manual-checklist.md): manual test checklist
- [`docs/testing/measurements.md`](docs/testing/measurements.md): memory measurements

[`docs/superpowers/`](docs/superpowers/) holds the original design spec and implementation plan (Indonesian, historical).

## Roadmap

**Next**
- [ ] Reload Ignoring Cache (⇧⌘R) and Clear Website Data
- [ ] Bookmarks
- [ ] Find in page and zoom
- [ ] Autofill and unsent-text protection inside iframes
- [ ] Private window (no history, non-persistent storage)

**Later**
- [ ] Chrome/Safari web extensions via WebKit's `WKWebExtension` API (macOS 15.4+)
- [ ] Touch ID unlock for the password vault through the Secure Enclave (requires an Apple Developer certificate)
- [ ] Developer ID signing, notarization, and auto-update for distribution
- [ ] Passkeys
- [ ] Sync across devices

**Ideas**
- [ ] Project mode for the Ask Claude panel (work with local files)
- [ ] Per-site settings (blocker, HTTP/3, permissions) in one place

## Security notes

- Page content is treated as untrusted input for Claude. The panel only gets read-only tools, and MCP clients need explicit approval.
- Interaction (click, type, JavaScript) is off at every launch.
- `run_js` is refused on pages with a filled password field, and no tool exposes the password vault.
- Debug-only test hooks are compiled out of release builds.

## License

[MIT](LICENSE) © 2026 Dery Febriantara

The bundled ad-blocking rules are generated from [EasyList and EasyPrivacy](https://easylist.to) (GPLv3 / CC BY-SA 3.0); they are downloaded at build time and are not stored in this repository.

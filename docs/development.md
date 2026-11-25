# Development

A short guide to building, testing, and checking my-browser.

## Crate layout

| Crate | Contents |
|---|---|
| `crates/core` | Browser logic without macOS: tabs, tab sleep, history, address bar suggestions, sessions, settings, control protocol. Most tests live here. |
| `crates/engine-webkit` | Implementation of the `Engine`/`Page` traits (`crates/core/src/engine.rs`) with WKWebView. |
| `crates/app` | AppKit app: windows, menus, address bar, panels, dialogs, control socket. |
| `crates/mcp` | MCP server (`my-browser --mcp`) that forwards tools to the control socket. |
| `crates/vault` | Encrypted password vault (no macOS). |
| `crates/memtool`, `crates/blocklist-gen` | Memory measurement tool and blocker list generator. |

Rule: logic that can be tested without AppKit goes in `core` as pure functions. `app` only translates AppKit events into `core` calls and carries out the results.

## Building and running

```sh
cargo build                    # debug
scripts/run.sh                 # release build, bundle, then open dist/my-browser.app
scripts/bundle.sh              # only build dist/my-browser.app (ad-hoc codesign)
```

`scripts/bundle.sh` deletes the old bundle and then copies the new one, so it is safe to run while the browser is open. Changes take effect after the browser is reopened.

## Checks

```sh
scripts/check.sh               # format, clippy with no warnings, all tests, dependency audit
scripts/check.sh --quick       # format and clippy only (about 1 second with a warm cache)
```

- Formatting follows `rustfmt.toml` (lines up to 120 characters). Format the code with `cargo fmt --all`.
- Clippy runs with `-D warnings`: a single warning fails the check.
- The dependency audit uses `cargo audit` (RustSec database). Install it once with `cargo install cargo-audit --locked`.
- **Git hook:** `.githooks/pre-commit` runs `scripts/check.sh --quick` before every commit. Enable it once per clone with `git config core.hooksPath .githooks`. Skip it once with `git commit --no-verify`.
- **CI:** `.github/workflows/ci.yml` runs `scripts/check.sh` on a GitHub macOS runner. This repo has no remote yet, so the workflow has never run.

## Tests

| Type | Command | Notes |
|---|---|---|
| Unit | `cargo test -p my-browser-core` (also `-p my-browser-vault`, `-p my-browser-mcp`, `-p my-browser`) | Fast, no windows |
| WebKit smoke | `cargo test -p engine-webkit --test smoke` | Real WKWebView on the main thread (`harness = false`), needs a logged-in macOS session |
| Suggestion benchmark | `cargo test --release -p my-browser-core suggest_10k -- --ignored --nocapture` | Limit of 5 ms per keystroke for 10,000 history entries |
| E2E | Python scripts outside the repo | See below |
| Manual | `docs/testing/manual-checklist.md` | Things that need the user's eyes and hands |

### E2E tests on a separate instance

E2E tests run a debug build with temporary folders, so the browser you use every day is not touched:

- `HOME` and `MYBROWSER_DATA_DIR` point to a temporary folder. The path must be short, for example under `$TMPDIR`, because Unix socket paths are limited to about 104 bytes.
- Tools are called through `my-browser --mcp` with JSON-RPC on stdin and stdout.
- The test instance is stopped by its pid, not by quitting the app named my-browser.

Debug-build-only hooks (absent from release builds; check with `strings dist/my-browser.app/Contents/MacOS/my-browser`):

| Hook | Purpose |
|---|---|
| `MYBROWSER_DIALOG_SELFTEST=<text>` | Answers every dialog automatically; secret fields are filled with that text |
| `MYBROWSER_DIALOG_CHOICE=<n>` | Button to choose in dialogs (0 = first button) |
| `MYBROWSER_CHAT_SELFTEST=<file>` | Sends one line of the file to the Ask Claude panel each time the panel opens |
| `debug_ui` tool | Types and presses keys in the address bar through the field editor, sends menu actions, reads window state, quits (`crates/app/src/debug_ui.rs`) |

`MYBROWSER_DEBUG=1` prints extra logs (global events and tab sleep state) to stderr, in release builds too.

## Style

- All UI text (menus, dialogs, notifications, address bar, windows, panels) and the docs are in English. Code comments, developer logs, and text read by Claude (the panel system prompt, MCP tool descriptions and messages) are in Indonesian.
- Commits use Conventional Commits prefixes (`feat`, `fix`, `refactor`, `style`, `docs`, `test`).

# Controlling my-browser from Claude Code (MCP)

Claude Code can view and manage tabs, read pages, take screenshots, and (with permission) interact with pages in my-browser.

## Installation

```sh
scripts/pasang-mcp.sh            # ~/.claude profile
scripts/pasang-mcp.sh ~/.claude-work   # another profile
```

This script registers `dist/my-browser.app/Contents/MacOS/my-browser --mcp` as a user-scope MCP server. Start a new Claude Code session so its tools appear (check with `/mcp`). If the browser is not running when a tool is called, the MCP server opens it automatically.

## How it works

```
Claude Code ──stdio JSON-RPC──▶ my-browser --mcp ──control.sock──▶ my-browser (window)
```

- `my-browser --mcp` is the same binary in windowless mode (crate `crates/mcp`).
- The browser listens on `~/Library/Application Support/my-browser/control.sock` (mode 0600, same user only). There is no network port.
- The protocol between the two: `crates/core/src/control.rs`, one JSON line per request.

## Tools

| Tool | Purpose | Needs interaction permission |
|---|---|---|
| `list_tabs`, `open_tab`, `close_tab`, `activate_tab` | Manage tabs. `list_tabs` reports each tab's sidebar section (`kind`: `favorite`, `pinned`, `today`); `close_tab` on a favorite or pinned tab unloads it instead of removing it | No |
| `navigate`, `go_back`, `go_forward`, `reload` | Navigation, waits for the page to finish loading | No |
| `sleep_tab`, `memory_usage` | Put a tab to sleep, memory per tab | No |
| `search_history` | Search the history of visited pages: URL, title, last visit time, visit count (see `docs/history.md`) | No |
| `read_page`, `list_elements`, `scroll` | Read the page | No |
| `screenshot` | The active tab's screen; `full_page` for the whole page (split every 1600 px, max 6 images); `selector` for one element (small elements are scaled up 2x) | No |
| `emulate` | Emulate a phone (390×844 + iPhone user agent), tablet, desktop, custom size, dark/light mode; `device=reset` to go back | No |
| `get_selection` | The element you pointed at through **Claude > Pick Element for Claude**: selector, style, HTML, and its image | No |
| `check_page` | One call: reload (or open a url), wait, console errors, failed requests, screenshot | No |
| `youtube` | Status, play, pause, seek, speed | No |
| `console_logs` | Console, uncaught errors (source and stack), rejected promises | No |
| `network_log` | fetch/XHR requests: status, duration, error; response bodies for local sites | No |
| `wait_for` | Wait for an element or text to appear, become visible, or disappear | No |
| `inspect_element` | Position, size, computed style, outerHTML of one element | No |
| `click`, `type`, `run_js` | Interact with the page (`run_js` awaits Promises) | **Yes** |

## Building a website with Claude

Ask Claude to build or fix a website, for example "build a coffee shop landing page, run it, and check it in the browser". The flow Claude follows:

1. Write the code and run a local server (`npm run dev`, `python3 -m http.server`, and the like).
2. `navigate` to `http://localhost:<port>`.
3. After every change: `check_page`. It does the same as `reload` followed by `console_logs` (warn and error), `network_log` (`failed_only`), and `screenshot`, but in one call. For pages that load data later, set `wait_for` to the selector to wait for.
4. Other screen sizes: `emulate` with `device=mobile` or `tablet`, and `color_scheme=dark`. When done, `emulate device=reset`. You see the change in the window: the page appears phone-width at the top center. If Claude forgets to reset it, use the **Claude > Reset Device Emulation** menu.
5. Visual details: `screenshot` with `selector`, `full_page`, or `inspect_element`.

### Pointing out an element to Claude

Instead of describing "the orange button below the heading", choose **Claude > Pick Element for Claude** (⌘⇧E). The element under the cursor is highlighted; click the element you mean (Esc to cancel). This click is not passed to the page, so buttons and links do not fire. Then just tell Claude, for example "make the element I selected bigger". Claude calls `get_selection` and receives a unique selector, the text, position, computed style, outerHTML, and a cropped image of that element.

### Screenshot notes

A regular screenshot is a capture of the active tab's screen, including video. `full_page`, `selector`, and `get_selection` images are re-rendered from the page through WebKit PDF, so off-screen parts are drawn too and they work for background tabs. Video and WebGL canvases can appear blank in this kind of image, and lazy-loaded images below the fold may not have loaded yet.

The console and network recorders are installed at the start of every page and are capped (200 console entries, 300 requests). Response bodies are stored only for local sites (`localhost`, local IPs, `.local`, `.test`, `file:`), because response bodies from real sites can contain personal data. The engine is WebKit, so the results also show how the page behaves in Safari.

## Program permissions (MCP auth)

The control socket can only be opened by the same macOS user. That means any program you run, so every program must also be allowed first.

- When a program connects for the first time, my-browser comes to the front and asks: "Allow Claude Code to control my-browser?". The dialog shows the program's location, pid, and working folder, so you can tell which Claude Code session is asking. The choices:
  - **Always Allow:** the program's location is saved in `mcp_allowed_clients` in `settings.json`.
  - **Allow Once:** applies until that connection closes.
  - **Deny:** applies until the browser is closed.
- The first request waits up to 35 seconds for your answer. If you answer after that, the answer still applies to subsequent requests.
- **Identity comes from the kernel**, not from what the program claims. The socket peer's pid points to `my-browser --mcp`, and the parent of that process (for example `claude`) is treated as the one requesting access. A foreign program that runs `my-browser --mcp` itself shows up under its own name.
- **Programs updated while still running.** Claude Code updates itself in the background. After that, the kernel can no longer return the file path for a session that is already running (`proc_pidpath` fails). As a fallback, the browser uses the path the program was launched from (`KERN_PROCARGS2`, symlinks resolved), provided the program file actually exists at that path (`crates/app/src/procinfo.rs`). If it still cannot be identified, the program receives an error message suggesting it restart its session, rather than a connection that silently drops.
- **The Ask Claude panel** is identified by process ancestry (its Claude process is a child of this browser), so it is not asked. The browser also limits it to read-only commands, as a second layer on top of the panel's tool list.
- Allowed programs can read history through `search_history`, and the dialog text mentions this. Programs allowed before this tool existed are not asked again.
- The **Claude > Revoke MCP Client Access** menu removes all permissions; every program will be asked again.
- Permissions are stored per program location. After Claude Code is updated to a different location (for example when the Node version in nvm changes), you will be asked once more.

## Interaction permission

`click`, `type`, and `run_js` only run after the **Claude > Allow Claude to Interact** menu is turned on. This permission is always off when the browser opens. While it is on, the page card has an orange outline, the sidebar shows a "Claude can interact" pill with a **Turn Off** button, the window title carries the same marker, and every Claude action appears as a short notice at the top of the page card.

The reason: your logins (Google and others) live in this browser, and the page text Claude reads can contain malicious instructions. Password input values are never sent by `list_elements`.

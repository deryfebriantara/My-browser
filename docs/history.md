# History and address bar suggestions

my-browser records the pages you open, suggests them as you type in the address bar, and provides a History window to search and delete them. All suggestions come from the history and tabs on this Mac. What you type is never sent to a search engine before you press Enter.

## How to use

- **Address bar.** Type part of an address or title. Addresses you open often are completed inline (for example, `you` becomes `youtube.com`), with the added part selected. Keep typing to overwrite it; Backspace deletes it.
  - ↑ ↓ select a suggestion, Enter opens the highlighted suggestion, and clicking works too.
  - The first Escape closes the suggestion list, the second Escape restores the tab's address.
  - Tab accepts the completion.
  - A **Switch to tab** row switches to a tab that is already open, without reloading it. If the tab you started from is empty, it is closed.
  - The last row is always **Search “…”** on the search engine.
- **History window:** **History > Show All History** (⌘Y). The list is grouped by day and can be searched. Click opens in this tab, ⌘-click or the ↗ button opens in a new tab, and the ✕ button deletes a single page.
- **Clear History:** **History > Clear History…** or the button in the History window. The options are Last hour, Today, Today and yesterday, or All history.

## What is recorded

One entry per page, holding the URL, title, visit count, the number of times the address was typed, and the first and last visit times (`crates/core/src/history.rs:25`).

- **SPA pages are recorded too.** For example, clicking a video on YouTube changes the address through `history.pushState` without loading a new page. WebKit reports such address changes while the page is not loading, and the browser tells them apart from a regular page load (`crates/engine-webkit/src/observer.rs:49`, `crates/core/src/browser.rs:486`).
- **What does not increase the count:** reloading, waking a sleeping tab, restoring the session when the browser opens, and reopening a closed tab. Each tab remembers the last page it already recorded (`crates/core/src/history.rs:93`, `crates/core/src/browser.rs:90`, `:295`).
- **URLs are cleaned before they are recorded** (`crates/core/src/history.rs:56`):
  - the fragment (`#section`) and any username or password in the URL are dropped;
  - tracking parameters are dropped: `utm_*`, `fbclid`, `gclid`, `dclid`, `msclkid`, `mc_eid`, `igshid` (`:22`);
  - on YouTube watch pages, the `t` parameter is dropped. This parameter is added when a tab is put to sleep (see `docs/tab-sleep.md`), so without the cleanup every sleep and wake would create a new entry.
- **Not recorded:** addresses other than http/https (`about:`, `data:`, `file:`), URLs longer than 4096 bytes, and redirected URLs. Only the final destination address is recorded.
- **Typed addresses are kept apart from searches and links.** An address typed in the address bar increases the typed count. Searches and links do not (`crates/core/src/browser.rs:334`).
- History can be turned off with `"history_enabled": false` in `settings.json`.

## Storage

- File: `~/Library/Application Support/my-browser/history.json`, with permissions 0600 from the moment it is created (`crates/core/src/persist.rs:26`). Its contents are not encrypted, the same as Safari and Chrome history.
- The limits are 10,000 pages and 90 days. The pages last opened longest ago are dropped first (`crates/core/src/history.rs:16`, `:18`). Titles are truncated at 300 characters.
- Saved at most once every 2 minutes while browsing (`crates/app/src/state.rs:38`), always on quit, and right after anything is deleted.
- **Clear History deletes whole entries** last opened within the selected range (`crates/core/src/history.rs:186`). Visits are not stored one by one, so a page that was also opened before that range is deleted too. This is intentional: deleting too much is safer than deleting too little.
- **What is not cleared:** open tabs, the back/forward list in each tab, `session.json`, the recently closed tabs list, cookies, logins, and cache.

## How suggestions are chosen

`crates/core/src/suggest.rs`, called every time the address bar contents change.

1. **The first row** is the default action when Enter is pressed:
   - the inline-completed page, if there is one;
   - otherwise, "Open address" for text that is an address;
   - if it is not an address, "Search “…”".
2. **"Switch to tab":** at most 3 matching tabs (`:13`).
3. **Pages from history**, ranked, without duplicates.
4. **"Search “…”"** in the last row. At most 8 rows in total (`:11`).

Matches (`crates/core/src/suggest.rs:236`), from strongest to weakest:
1. start of the host (`you` to `youtube.com`);
2. start of the address without the scheme (`github.com/serv`);
3. every word is at the start of a word in the title or URL;
4. every word appears anywhere. Words shorter than 3 letters must still be at the start of a word, so `ub` does not match `youtube`.

The weight is the visit count plus three times the typed count, then reduced by age: full weight up to 4 days, then 0.7, 0.5, and 0.3 after a month (`:263`).

**Inline completion** (`:277`) applies only to ASCII text without spaces:
- Without `/`: hosts typed at least once, or visited at least three times (`:341`).
- With `/`: only addresses that have been typed (`:300`).

A page opened only once through a link still appears as a suggestion, but is not completed automatically. Completion also does not happen after Backspace or Delete, when the cursor is not at the end of the text, or while an IME (e.g. Japanese input) is composing characters (`crates/core/src/address_edit.rs`).

**Speed:** matching runs on the main thread on every keystroke. With 10,000 entries, the average is 1.3 ms per keystroke in a release build, measured with the `suggest_10k_is_fast` test (`cargo test --release -p my-browser-core suggest_10k -- --ignored --nocapture`). The first version took 31.5 ms because hosts were searched linearly and every entry was reallocated. Hosts are now looked up through a `HashMap`, and ASCII matching does not allocate.

**Native suggestion list:** a borderless child window below the address field, without a WebView (`crates/app/src/suggest_popup.rs`). Showing it does not add a WebContent process (tested E2E). A borderless window never becomes the key window, so the cursor stays in the address bar.

## Claude and history

The MCP tool `search_history` (read-only) searches history. Its parameters are `query` (every word must appear in the title or URL), `limit` (default 20, maximum 100), and `days`. It returns the URL, title, last-opened time in ISO 8601 with a time zone, and visit count (`crates/app/src/control.rs:534`).

- This tool is also available in the Ask Claude panel, which only gets read tools (`crates/core/src/claude_chat.rs:27`).
- External MCP programs must still be allowed through the program permission dialog. The dialog text mentions "read your browsing history".
- Page titles come from sites and can contain malicious instructions, so the tool results and the tool description remind Claude not to follow them. The Ask Claude panel cannot open URLs, so history cannot be sent out from there.
- MCP programs allowed before this feature existed can read history automatically, without being asked again. Revoke access with **Claude > Revoke MCP Client Access** if you do not want that.

## Code

- `crates/core/src/history.rs`: entries, URL cleanup, recording, search, limits, file.
- `crates/core/src/suggest.rs`: suggestion ranking and inline completion.
- `crates/core/src/address_edit.rs`: address bar keyboard behavior (pure, no AppKit).
- `crates/core/src/browser.rs`: when visits are recorded, `navigate_from_address`, `open_suggestion`.
- `crates/engine-webkit/src/observer.rs`: tells pushState apart from page loads.
- `crates/app/src/address_bar.rs` and `suggest_popup.rs`: field editor and suggestion list.
- `crates/app/src/history_window.rs` and `.html`: History window and Clear History (`clear_dialog`, `:186`). Titles are only inserted as text (`history_window.html:91`).

## Testing

- **Core unit tests:** 14 in `history.rs`, 15 for recording and suggestions in `browser.rs`, 17 in `suggest.rs` (plus one benchmark), and 9 in `address_edit.rs`.
- **WebKit smoke test:** pushState and `#fragment` are reported as same-document navigations; page loads are still `UrlChanged`.
- **E2E on a test instance (24 of 24 passed):**
  - redirect, SPA, and fragment;
  - reload and sleep/wake do not increase the count;
  - typing an address, inline completion, Backspace, arrow keys, two-step Escape, and "Switch to tab";
  - suggestion list position, and no additional WebContent process;
  - 0600 permissions, and contents intact after reopening;
  - History window, deleting a single entry, and Clear History.
- **Debug-build-only test hook:** `debug_ui` on the control socket (`crates/app/src/debug_ui.rs`) types through `insertText:` and presses keys through `doCommandBySelector:` on the real field editor. This hook does not exist in release builds.

## Not yet verified

- Keystrokes in E2E are sent through the field editor (`insertText:` and `doCommandBySelector:`), not a physical keyboard. Typing directly, mouse clicks on suggestion rows, VoiceOver, and the Japanese IME have not been tried. All of them are on the manual checklist.
- Recording a video click on real YouTube has not been tried. What was tested is a local page that calls `pushState`.
- The suggestion list has only been seen in dark mode.

## Out of scope

- **Bookmarks** do not exist yet.
- **Search engine suggestions** are intentionally not built, by the user's decision: keystrokes must not be sent outside.
- **History sync** across devices and **history import** from other browsers do not exist yet.
- **Private mode** (windows without history) does not exist yet; the only option is turning history off with `history_enabled`.

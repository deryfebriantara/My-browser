# Arc-style redesign: sidebar, Spaces, auto-archive, command bar

Status: draft for approval (2026-11-26).

## Why

The current window looks plain: three stacked rows of standard controls (title bar, a 36 pt nav bar with a plain text field, and a 28 pt strip of push-button tabs), about 92 pt of chrome, with no favicons and no color. The owner wants a browser that looks good and that they enjoy using every day.

After comparing three directions side by side (compact one-row toolbar like Safari, classic tabs-above-toolbar like Chrome, sidebar like Arc and Zen), the owner chose the sidebar direction. They also chose the full Arc-style feature set, not only the look:
- a colored sidebar with vertical tabs;
- the page shown as a rounded card;
- favorites as icons;
- **Spaces**, two of them (Work and Personal), with **separate logins**;
- automatic archiving of everyday tabs;
- a command bar on ⌘T.

Success means the owner uses it as their daily browser. Work and Personal can be signed in to different accounts of the same site at the same time. The window looks like the approved mockup, and memory use does not get worse.

## Goals

1. Replace the title bar, nav bar, and tab strip with a sidebar on the left. The web page sits in a rounded card on a colored gradient background.
2. Tabs in three sections per Space: **Favorites** (icon grid), **Pinned** (never archived), and **Today** (everyday tabs).
3. **Spaces**: Work and Personal out of the box. More can be created, renamed, re-themed, and deleted. Each Space has its own color theme, tabs, favorites, history, archive, and **website data store** (cookies, logins, cache, local storage).
4. Today tabs that have not been opened for 12 hours are archived automatically. Archived tabs can be found and reopened.
5. A command bar (⌘T, ⌘L, or a click on the address) opens addresses, searches, switches tabs, finds history and archived tabs, and runs commands. It replaces the current suggestion dropdown.
6. Real favicons everywhere, with a fallback.

## Non-goals

- Arc features beyond the list above: Boosts, Easels, Notes, Little Arc, split view, Peek, sync, and a sidebar on the right.
- Multiple windows. The app stays single-window; Spaces are per window.
- A separate bookmarks UI. Favorites and Pinned cover that for now. The "Bookmarks" roadmap item is replaced by this work.
- Chrome extensions (still postponed by the owner).
- Reusing Arc's branding, icons, or assets. This is a sidebar-style browser with its own look.

## Anti-goals (failure even if it "works")

- **Memory regression.** Chrome must not add a WebContent process or a large amount of UI-process memory. Memory efficiency is the reason this browser exists.
- **Logins leaking between Spaces.** Examples: a tab opened from a Work page loading in Personal's store, favicon requests sending cookies, the panel's sign-in tab using the wrong store.
- **Losing user data during migration:** open tabs, history, logins, or saved passwords.
- **Archiving destroying work:** archiving a tab with unsent typed text or playing media.
- **Breaking Claude integration:** the MCP tools and the Ask Claude panel must keep working.

## Constraints

- macOS 14+ on Apple Silicon, Rust with objc2/AppKit and WKWebView. `WKWebsiteDataStore.dataStoreForIdentifier:` is available from macOS 14, which matches the minimum.
- Existing architecture: browser logic without macOS dependencies lives in `crates/core` and is unit-tested with `FakeEngine`; WebKit specifics live in `crates/engine-webkit`; AppKit UI lives in `crates/app`.
- Existing behavior that must keep working:
  - tab sleep (30 minutes unseen, 5 awake, memory pressure; drafts and media keep tabs awake);
  - history recording rules;
  - password capture and fill (⌘\);
  - the ad blocker;
  - HTTP/3 alt-svc clearing;
  - the debug-only test hooks (`debug_ui`), which are compiled out of release builds;
  - `scripts/check.sh` and CI.
- UI text is English. Code comments follow the existing convention (Indonesian).
- The repository is public: no personal data in fixtures, docs, or screenshots.

## Approaches considered

| Approach | Trade-off | Verdict |
|---|---|---|
| **AppKit native** (custom NSViews, CAGradientLayer, child NSWindow for the command bar) | More code to make it look polished, but no extra process, and it matches the existing UI code (`suggest_popup.rs`, `address_bar.rs`) | **Chosen** |
| Sidebar and command bar as HTML in a WKWebView (like the Ask Claude panel) | Easiest to make pretty, but needs an always-alive WebContent process (tens of MB) for the chrome. That works against the browser's main purpose | Rejected |
| SwiftUI hosted in AppKit | Nice declarative UI, but adds a second language and toolchain to the build and CI | Rejected |

The owner handed the "how" to the builder; the AppKit choice is the builder's call.

## Decisions made with the owner

- Direction: sidebar (Arc/Zen style), full feature set (sidebar and look, favorites, Spaces, auto-archive, command bar).
- Spaces: **Work** and **Personal**, with **separate logins**.
- Existing logins and tabs go to **Personal**; Work starts empty.
- Per Space: tabs, favorites, history, and address suggestions.
- Shared: **one password vault**. If a site has a work and a personal account, ⌘\ offers both. Also shared: the ad blocker, settings, and downloads.
- The approved mockup (described below):
  - favorites as an icon row;
  - Pinned and Today sections, with tabs archived after 12 hours;
  - a "Clear" button on Today;
  - the Space switcher at the bottom (⌃1 Work, ⌃2 Personal, plus a "+" button);
  - back, forward, reload, and ✦ Ask Claude in the sidebar's top row;
  - sleeping tabs dimmed with ☾;
  - the sidebar can be hidden with ⌘S;
  - the command bar is centered over the page.

## Design

### 1. Window layout

```
┌──────────────────────────────────────────────────────────────────┐
│ ● ● ●          ‹  ›  ↻   ✦ │╭────────────────────────────────╮  │
│ ╭────────────────────────╮ ││                                │  │
│ │ 🔒 example.com/article │ ││                                │  │
│ ╰────────────────────────╯ ││                                │  │
│ ╭────╮╭────╮╭────╮╭────╮   ││          web page              │  │
│ │ ▶  ││ M  ││ ●☾ ││ ✦☾ │   ││       (rounded card)           │  │
│ ╰────╯╰────╯╰────╯╰────╯   ││                                │  │
│ PINNED                     ││                                │  │
│  ▣ Mail · Inbox         ☾  ││                                │  │
│ ────────────────────────── ││                                │  │
│ TODAY               Clear  ││                                │  │
│  ＋ New Tab                ││                                │  │
│ ▌▶ Some video title…       ││                                │  │
│  ● A repository page    ☾  ││                                │  │
│                            ││                                │  │
│ ● Work ⌃1  ● Personal ⌃2 ＋ │╰────────────────────────────────╯  │
└──────────────────────────────────────────────────────────────────┘
        gradient background of the active Space behind everything
```

- **Window style:** `Titled | Closable | Miniaturizable | Resizable | FullSizeContentView`, with a transparent titlebar and hidden title. The window title is still set to the active tab's title, for the Window menu and Mission Control.
- **Background:** a gradient view fills the content view (`CAGradientLayer`, the colors of the active Space's theme).
- **Sidebar:** left side, default width 240 pt, resizable by dragging its right edge (200–360 pt), with the width persisted. It has no background of its own; the gradient shows through. Empty areas of the sidebar and the 8 pt margins drag the window.
- **Card:** holds the active tab's web view.
  - It is inset 8 pt from the top, right, and bottom, with a 10 pt continuous corner radius and a soft shadow.
  - Rounded clipping is done on a container layer. Shadow and clipping are separate layers, and the shadow uses a `shadowPath`.
  - When the current Space has no open tab, the card shows a native empty state (Space name, "Press ⌘T to open a page") instead of a blank web view.
- **Ask Claude panel:** a second card to the right of the page card, with the same radius and the existing width of 380 pt, separated by 8 pt.
- **Traffic lights:** stay at the system position. The sidebar's top row (back, forward, reload or stop, and ✦ on the right) is vertically aligned with them.
- **Hidden sidebar (⌘S):**
  - The card fills the window, with an 8 pt inset on all sides, and the traffic lights are hidden.
  - Resting the pointer on the left edge (8 pt) for about 150 ms slides the sidebar in as a floating overlay: rounded, with a shadow and the Space gradient, above the card. It hides again when the pointer leaves.
  - Whether the sidebar is visible is persisted.
- **Full screen:** the same layout. The builder may drop the outer insets.
- **Status cues:** two cues live today in places this redesign removes, so they move.
  - **"Claude can interact"** is a safety cue that must stay unmissable. Today it is appended to the window title, which is now hidden.
    - While interaction is on, the page card gets a 2 pt orange outline, visible even when the sidebar is hidden.
    - The sidebar shows a pill under the top row: "Claude can interact", with a "Turn Off" button.
    - The window title keeps its suffix, for Mission Control.
    - This placement is the builder's call.
  - **Short notices** (`NavBar::show_notice`, for example "History cleared") currently live in the nav bar, which is removed.
    - They become a toast at the top center of the page card, a subview above the web view, with the same timing as today.
    - This is the builder's call.

### 2. Sidebar contents

From top to bottom:

1. **Top row:** back, forward, reload (stop while loading), and ✦ Ask Claude.
2. **Address display:**
   - Shows a lock icon and the host plus path of the active tab, truncated at the tail.
   - Clicking it opens the command bar in current-tab mode (§8).
   - Until stage 4 it remains the existing editable address field (see Stages).
3. **Favorites:**
   - A grid of square tiles: about 44 pt, as many columns as fit (4 at the default width), wrapping onto more rows.
   - Each tile shows the favicon (20 pt). The tile of the visible tab is highlighted; a sleeping or unloaded tile shows a small ☾.
   - Hovering shows a tooltip with the title.
4. **Pinned:** a section label, then tab rows.
5. A divider.
6. **Today:** a section label with a "Clear" button on the right, a "＋ New Tab" row, then tab rows. Newest tabs are at the top, directly under "New Tab".
7. **Space switcher:**
   - Pinned to the bottom. It shows one pill per Space (theme-color dot, name, shortcut hint) and a "＋" for a new Space.
   - When the pills do not fit, the inactive Spaces collapse to dots, with the name in a tooltip.

Pinned and Today scroll together in one scroll view when they overflow. The top row, address display, favorites, and switcher stay fixed.

**Tab row:**
- 28 pt high: favicon (16 pt) and the title on one line, truncated at the tail.
- On hover: a ✕ button for Today tabs (close), or a "–" button for Pinned tabs (unload).
- The visible tab has a translucent white background and a subtle shadow.
- A sleeping or unloaded tab is shown at 50% opacity with ☾. A tab playing audio shows a speaker glyph.
- If no favicon is available, a monogram is shown (§10).

**Context menu on a tab or tile:**
- Pin Tab / Unpin Tab
- Add to Favorites / Remove from Favorites
- Move to Space ▸ (§4)
- Copy Link
- Sleep Tab
- Close Tab (on Pinned and Favorites: "Unload Tab")

**Drag and drop:**
- Reorder within a section.
- Drag between Favorites, Pinned, and Today to change a tab's section.
- Dropping onto a Space pill moves the tab to that Space. This is the builder's call, optional.

### 3. Tab sections and lifecycle

Every tab belongs to one Space and has a `kind`: `Favorite`, `Pinned`, or `Today`. Order within a Space is: favorites, pinned, today.

- **Favorite and Pinned are persistent slots.** Each remembers its *saved URL* (the URL at the moment it was pinned or favorited) and its current URL.
  - **Close (⌘W, the "–" button, or Unload Tab):** releases the web view and **resets the slot to its saved URL**. The tab stays in the sidebar. This is the builder's call, matching Arc.
  - **Automatic tab sleep:** unchanged; it keeps the current URL and scroll position as it does today.
  - The context menu adds "Set Current Page as Pinned URL". This is the builder's call.
- **Today tabs:**
  - ⌘W closes them into the existing recently-closed stack (⇧⌘T reopens).
  - The stack remembers each tab's Space, and ⇧⌘T reopens the tab in that Space, switching to it if needed. A Work page never reopens with Personal's logins.
  - This is the builder's call.
  - They are archived automatically after inactivity (§7).
- **New tabs** always go to the top of Today in the Space where they were opened. This covers ⌘T, links with `target=_blank` or `window.open` (`PageEvent::OpenInNewTab`, which today returns nil to WebKit and opens a tab by URL), and MCP `open_tab`.
  - A tab opened from a page goes into the **same Space as the page that opened it**, so that it uses the same logins.
- **After closing the visible tab:** activate the next tab below it in sidebar order, or the one above if there is none. If the Space has no other tab, show the empty state.
- **Restore after relaunch:** all tabs start asleep except the active tab of the active Space. This is how session restore already behaves for background tabs. Favorite and Pinned slots that were unloaded stay unloaded.
- **Existing tab sleep:** stays global. Tabs in inactive Spaces are "unseen", so they sleep after the idle time. `max_awake` counts awake tabs across all Spaces.

### 4. Spaces

A Space has:
- a stable id;
- a name;
- a theme (preset id);
- a **store id**: `Default`, or `Identified(UUID)`;
- its tabs, with sections and order;
- the tab that was active when the Space was last shown;
- an archive;
- a history file.

- **Initial Spaces** (when stage 2 ships):
  - **Personal** uses the default store and gets all existing tabs and history.
  - **Work** gets a fresh identified store and starts with no tabs.
  - No setup is needed.
- **Switching:**
  - Ways to switch: click a pill, ⌃1–⌃9 (the n-th Space), ⌘⌥← / ⌘⌥→ (previous and next Space), or a command bar entry.
  - Switching shows that Space's last active tab, or the empty state, and cross-fades the gradient (about 200 ms, or no animation when Reduce Motion is on).
  - VoiceOver announces the Space name.
  - If macOS Mission Control shortcuts take ⌃1–⌃9, the menu items and ⌘⌥←/→ still work. Document this in the user docs.
- **Creating a Space:**
  - "＋" or the "New Space…" command opens a sheet with a name field and a theme swatch grid.
  - Every new Space gets its own identified store; separate logins are not optional.
- **Editing:** right-clicking a pill offers Rename…, Theme ▸, and Delete Space…. These are also available as commands.
- **Deleting a Space:**
  - Confirmation text: "Delete Space “<name>”? Its tabs, history, archived tabs, and website data (logins, cookies, cache) will be deleted. Saved passwords are kept."
  - Close the Space's web views, then call `removeDataStoreForIdentifier:`.
  - If the deleted Space used the default store, clear all website data of the default store instead.
  - The last remaining Space cannot be deleted.
- **Move to Space:**
  - Reopens the tab's current URL as a new Today tab in the target Space, which means a new web view in the target store, with the target Space's logins. Then it closes the original.
  - The menu item says "Move to Space ▸ Work". The docs explain that the page reloads with that Space's logins.
  - For a Pinned or Favorite tab, the slot itself moves: it keeps its kind and saved URL in the target Space and is removed from the source Space. "Close" means unload for those tabs, so closing would leave the slot behind. This is the builder's call.

### 5. Separate logins (website data stores)

- **Store per Space:** Personal uses `WKWebsiteDataStore.defaultDataStore()`, so existing logins stay and older builds still see them. Other Spaces use `WKWebsiteDataStore.dataStoreForIdentifier(uuid)`. The UUIDs are persisted in the Space state.
- **Engine API change:**
  - `Engine::create_page(store: StoreId, on_event)`.
  - The engine caches one `Retained<WKWebsiteDataStore>` per store id.
  - New: `remove_store(uuid, done)` and `clear_store(StoreId, done)`.
  - `forget_http3_hints()` clears the alt-svc type in **every store in use**, not only the default one.
- **Web content always uses its Space's store:**
  - tab web views;
  - tabs opened from pages (§3);
  - pages the Ask Claude panel opens in a browser tab for sign-in, which open in the active Space.
  - The Ask Claude panel itself keeps its non-persistent store (unchanged).
- **The password bridge, ad-block content rules, and user scripts** are per `WKUserContentController` and do not depend on the store. They keep working unchanged.
- **Processes:** each web view already has its own WebContent process, so a second store does not add WebContent processes. Its networking-session overhead must be measured (see Verification).
- **Gate:** before building the Space UI, a real-WebKit smoke test must prove that cookies and `localStorage` set in one store are invisible in another, and that a store can be removed after its pages are closed.

### 6. What is per Space and what is shared

| Data | Scope |
|---|---|
| Tabs, favorites, pinned, archive | per Space |
| History and address/command-bar suggestions | per Space |
| Website data (cookies, logins, cache, local storage) | per Space (§5) |
| Password vault | shared (one master password) |
| Ad blocker and per-site exceptions, settings, downloads folder | shared |
| Favicon cache (§10) | shared (icons are public; fetched without cookies) |

- **History window (⌘Y):** shows the active Space's history, with the title "History — <Space>".
- **Clear History…:** applies to the active Space, and the dialog names it.
  - It also deletes archived-tab entries archived in the chosen time range, and the dialog says so.
  - This is the builder's call: the archive holds URLs and titles, just like history.
- **`history_enabled`:** stays a global setting.

### 7. Automatic archiving of Today tabs

- **What gets archived:** a Today tab is archived when it has not been the visible tab for `archive_after_hours` (setting, default **12**; 0 means never).
  - Archiving removes the tab from the sidebar, releases its web view, and adds an entry `{url, title, archived_at}` to its Space's archive.
- **Time is wall-clock time** (unix seconds), stored per tab in the persisted state as `last_active`. It keeps counting across Mac sleep and app restarts. This is unlike tab sleep, which uses `Instant`.
- **Never archived:**
  - Favorites and Pinned;
  - the visible tab;
  - tabs playing media;
  - tabs with unsent typed text, using the existing `Activity { media, draft }` check, the same one tab sleep uses.
  - Asleep tabs have no draft by construction.
  - Returning to a Space whose Today tabs were all archived shows the empty state.
- **When the check runs:** on the existing periodic tick.
- **Grace period:** the first time a build with archiving starts, every Today tab's `last_active` is set to the current time, once. Otherwise all tabs older than 12 hours would vanish at once on upgrade. This is the builder's call.
- **Archive limits:** newest first, at most 500 entries or 30 days per Space. A URL archived again replaces its older entry.
- **Reopening:** an archived tab opens as a new Today tab at the top. Scroll position is not restored.
  - **List view (required in stage 3):** the command bar only arrives in stage 4, so stage 3 needs its own way to reopen.
    - It is a "Show Archived Tabs" item in the History menu, opening an "Archived" view of the History window for the active Space, with search and reopen.
    - The exact form is the builder's call.
  - **Command bar:** from stage 4, also via its "Archived" section (§8).
- **"Clear" in the Today header:** archives every Today tab of the Space except the visible tab and protected tabs. Keeping the visible tab is the builder's call.

### 8. Command bar

- **Opening it:**
  - **New-tab mode:** ⌘T or "＋ New Tab". No tab exists until something is chosen. Esc creates nothing, so there are no blank tabs.
  - **Current-tab mode:** ⌘L or a click on the address display. The text starts as the current URL, fully selected.
- **Look:**
  - A borderless child window (the pattern of `suggest_popup.rs`), centered horizontally over the page card, about 70 pt below the card's top.
  - Width: min(560 pt, card width − 40 pt), with a 14 pt radius and a dark translucent background.
  - A translucent black scrim dims the card. No web view is involved.
- **Input:** 50 pt high, 16 pt font, with inline completion as in today's address bar.
- **Results,** grouped under small section headers, up to about 10 rows in total:
  1. **Default row:** the inline-completion target, "Open <address>", or "Search “q”" (from `suggest.rs`).
  2. **Open Tabs** (up to 3): the current Space first, then other Spaces, with the Space name as detail. Choosing one switches to that Space.
  3. **History** of this Space (up to 5).
  4. **Archived** in this Space (up to 2).
  5. **Commands** (up to 3).
  6. **Search “q”**, if it is not already the default row.
- **Keys:**
  - ↑ and ↓ move the selection. Enter opens the selected row. Tab accepts the inline completion.
  - Esc closes the bar. A click on the scrim also closes it.
  - The existing `address_edit.rs` state machine and `suggest.rs` ranking are reused and extended with sections.
- **Enter:**
  - In new-tab mode, it opens a new Today tab. "Switch to tab" rows switch instead.
  - In current-tab mode, it navigates the current tab. ⌘Enter opens a new tab instead (builder's call).
- **Empty input:** shows no rows in new-tab mode. The builder may add recents later.
- **Commands:**
  - Built by walking the main menu: every enabled item with an action becomes a command with its title and shortcut. So everything in the menus is available, and the list stays in sync automatically.
  - Dynamic commands are added: "Switch to Space <name>", "New Space…", "Rename Space…", "Delete Space…", "Clear Today Tabs", "Toggle Sidebar".
  - Matching is by word prefix on the title. Running a command sends the same action as the menu item.
- **Replaces:** the current dropdown (`suggest_popup.rs`). The address display in the sidebar becomes non-editable.

### 9. Keyboard shortcuts

| Shortcut | Action |
|---|---|
| ⌘T | Command bar, new-tab mode (stages 1–3: today's behavior) |
| ⌘L | Command bar, current-tab mode (stages 1–3: focus address) |
| ⌘W | Close a Today tab; unload a Pinned or Favorite tab |
| ⇧⌘T | Reopen closed tab (unchanged) |
| ⌘1–⌘8, ⌘9 | n-th tab in the current Space's sidebar order; ⌘9 is the last tab |
| ⌃⇥ / ⌃⇧⇥ | Next or previous tab in the current Space's sidebar order |
| ⌃1–⌃9 | Switch to Space n |
| ⌘⌥← / ⌘⌥→ | Previous or next Space |
| ⌘S | Toggle sidebar. WKWebView sends keys to the page first, so on pages that handle ⌘S themselves (editors), the page wins. This is the same as Arc and is acceptable |
| ⌘D | Pin or unpin the current tab (builder's call; no bookmarks exist to conflict with) |

### 10. Favicons

- **Candidates:**
  - After a page finishes loading, a small script in the existing isolated content world returns icon candidates: `link[rel~=icon]` with `sizes`, and `apple-touch-icon`.
  - Pick the smallest icon of at least 32 px. Fall back to `/favicon.ico` on the page's origin.
- **Fetching:**
  - An app-level fetcher using an ephemeral `NSURLSession` with **no cookie storage**, `HTTPShouldSetCookies = false`, http(s) only, a 10 s timeout, and a 512 KB limit.
  - Requests never carry a Space's cookies, so they cannot link Spaces.
- **Decoding:** with `NSImage`; formats it cannot decode are skipped. The result is scaled to 32×32 px PNG.
- **Plain-http icons:** App Transport Security only allows plain-http loads inside web content (`NSAllowsArbitraryLoadsInWebContent`). The app-level fetcher therefore cannot load `http://` icons, and those sites show the monogram. Do not widen ATS for this.
- **Cache:**
  - On disk in `favicons/`, keyed by a hash of the host. A cached icon is refreshed on a visit if it is older than 7 days.
  - Capped at about 2,000 files or 20 MB (least recently used are evicted), with a small in-memory cache for visible rows.
  - Sleeping, restored, and archived tabs use the cached icon for their host.
- **Fallback monogram:** the first letter of the host without `www.`, on a rounded tile whose color is derived from a hash of the host.
- **Privacy:** clearing a Space's history must not leave a trail of icons. After any Clear History, delete cached icons for hosts that no longer appear in any Space's tabs, history, or archive.

### 11. Persistence and migration

- **New state file:** `spaces.json`, with its own version number. It holds the Spaces in order, the active Space, and for each Space:
  - id, name, theme, store id;
  - the active tab index;
  - the archive;
  - the tabs, each with kind, saved URL, current URL, title, `scroll_y`, and `last_active`.
- **Written atomically** with 0600 permissions (`persist::write_private`).
- **First launch of the new build:**
  - If `spaces.json` is missing and `session.json` exists, its tabs are imported into Personal's Today, and its active tab stays active.
  - `session.json` is **left untouched**, so an older build still opens with its old session (downgrade-safe).
- **History:** Personal keeps using the existing `history.json` unchanged, so older builds still read it. Other Spaces use `history-<space-id>.json`. All history files are 0600.
- **New settings:** `sidebar_width`, `sidebar_visible`, `archive_after_hours` (default 12).
- **Corrupt `spaces.json`:** set aside as `.bad` (existing `read_json` behavior), then fall back to importing `session.json`.
  - Logins must not be orphaned. List the identified stores with `fetchAllDataStoreIdentifiers:`, and re-attach every store not referenced by a Space as an empty Space named "Recovered Space n".
  - This recovery is the builder's call.
- **Newer files are never overwritten:** `spaces.json` carries a format version.
  - A build that finds a newer version than it understands (for example after rolling back one stage) leaves the file untouched.
  - It runs without saving tab state and shows a notice explaining why.
  - This guard is the builder's call.
- **Store cleanup:** a store is never removed except when the user deletes its Space. There is no automatic cleanup of orphaned store identifiers; a parse failure must not delete logins.

### 12. Claude integration (MCP and the Ask Claude panel)

- **Tab ids:** stay globally unique across Spaces.
- **`list_tabs`:** returns the tabs of all Spaces, each with `space` (name) and `kind` (`favorite`, `pinned`, `today`).
- **`open_tab`:** gets an optional `space` (name). The default is the active Space, and the new tab goes to the top of Today.
- **`activate_tab`:** for a tab in another Space, switches to that Space.
- **`close_tab`:** on a Pinned or Favorite tab, unloads it (the same as ⌘W).
- **`search_history`:**
  - Gets an optional `space`. By default it searches all Spaces, and each entry has a `space` field.
  - Searching all Spaces by default is the builder's call: Spaces separate logins and suggestions, they do not hide anything from the owner's own assistant.
- **Unchanged:** the tool count (26) and the approval and interaction rules.
- **Docs:** update the INSTRUCTIONS text and `docs/mcp.md`.

### 13. Themes

- **Presets:** about 8, each with three color stops, an angle, a text color for the sidebar (light or dark), and a dot color.
  - Default Personal: purple to pink (`#2b2470 → #6a3fa0 → #c45a8c`).
  - Default Work: navy to teal (`#0f3a5f → #1f6f8b → #3aa39a`).
- **System appearance:** the gradient does not change with light or dark mode.
- **Contrast:** sidebar text and section labels must reach a contrast ratio of at least 4.5:1 against the lightest stop of every preset. Tune the presets, not the rule.
  - The mockup colors fail this for white text: the last stop of Personal (`#c45a8c`) reaches about 4.0:1 and the last stop of Work (`#3aa39a`) about 3.1:1.
  - Their light ends get darker, Work noticeably so.
- **Increase Contrast:** when the system setting is on, use a solid, darker version of the theme behind the sidebar.

### 14. Accessibility

- **VoiceOver:**
  - Each sidebar row and tile is an accessibility element (button, selected state) with a label "<title>, <host>[, asleep][, playing audio]".
  - Space pills read as "Space <name>, n of m".
- **Keyboard:** the command bar covers full keyboard use. Tab-key navigation through the sidebar when Full Keyboard Access is on is the builder's call.
- **Reduce Motion:** no slide or cross-fade animations.

## Stages (delivery order)

Each stage is usable on its own, and gets its own plan and release.

1. **Window, sidebar, and favorites**, with the data model ready for Spaces.
   - Includes: §1, §2, §3, §10, and §11, with exactly one Space (Personal); the hidden sidebar; context menus; drag and drop within the sidebar.
   - The title bar, nav bar, and tab strip are removed.
   - **Interim address bar:** the existing editable address field and suggestion dropdown, anchored to the sidebar's address field. The dropdown may be wider than the sidebar and overlap the card.
2. **Spaces:** §4, §5 (smoke-test gate first), §6, §12, and §13, including Work created automatically.
3. **Automatic archiving:** §7, including the archived-tabs list view and the grace period.
4. **Command bar:** §8, and ⌘T and ⌘L switch to the new behavior.

Update the README (features, roadmap) and the user docs in `docs/` with each stage.

## Verification

- **Core unit tests** (with `FakeEngine` and a fake clock):
  - section order; pin, unpin, and favorite transitions; the saved URL;
  - ⌘W semantics per kind; new-tab placement; the Space of tabs opened from a page;
  - the last active tab per Space; `⌘1–9` order;
  - archive rules (wall clock, protected tabs, caps, de-duplication);
  - `spaces.json` round trip;
  - import from `session.json`, with the file left byte-identical;
  - command-bar sections and ranking; menu-command matching.
- **Real-WebKit smoke tests:**
  - cookie and `localStorage` isolation between the default store and an identified store, served from a local HTTP server;
  - a tab opened from a page in an identified store uses the same store;
  - store removal after its pages close;
  - alt-svc clearing reaches all stores.
- **End-to-end tests** on a test instance with a temporary `HOME` and a short `MYBROWSER_DATA_DIR`:
  - Extend `debug_ui` with `sidebar_state`, `sidebar_click`, `sidebar_menu`, `switch_space`, `command_bar_state`, `command_bar_type`, and `command_bar_key`. Add their names to CI's release-binary hook check.
  - Scenarios per stage:
    - pin, unload, and favorite;
    - migration from a real `session.json`;
    - two Spaces signed in differently on the same local test site;
    - archive after a faked clock advance;
    - the command bar's switch-to-tab across Spaces;
    - Esc creates no tab.
- **Visual checks:**
  - screenshots in light and dark system appearance;
  - at the minimum window width;
  - with 30 tabs (scrolling);
  - with the sidebar hidden and in overlay;
  - compared with the approved mockup.
- **Memory and CPU gate** (method from `docs/testing/measurements.md`):
  - (a) UI-process memory with 2 Spaces × 10 tabs (mostly asleep) compared with the current build with 20 tabs: at most +15 MB.
  - (b) The number of WebContent processes equals the number of awake tabs; the chrome adds none.
  - (c) Record the networking process memory with two stores.
  - (d) CPU during 1080p YouTube playback with the rounded card compared with the current build: no regression beyond measurement noise. Investigate anything above +10% relative.
- **CI:** `scripts/check.sh` stays green on every commit.

## Left to the builder

- Exact sizes, spacing, SF Symbols, animation curves, and the final preset colors (within the contrast rule).
- Module layout and the split of code between `core`, `engine-webkit`, and `app`. The file layout of the state files beyond what §11 requires.
- Whether sidebar resizing and dropping onto Space pills land in their stage or slightly later.
- The empty-state design, and recents in the empty command bar.
- How favicon candidates are scored, beyond §10.
- Calls already marked "builder's call" above. The owner can still override any of them:
  - status cue placement;
  - the saved-URL reset on unload, and "Set Current Page as Pinned URL";
  - ⇧⌘T reopening in the original Space;
  - moving Pinned and Favorite slots;
  - Clear History also clearing the archive;
  - the archiving grace period;
  - keeping the visible tab on Clear;
  - recovery of orphaned stores, and the newer-file guard;
  - ⌘Enter and ⌘D;
  - the default `search_history` scope.

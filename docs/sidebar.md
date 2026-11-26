# Sidebar and page card

my-browser shows its tabs in a colored sidebar on the left, and the page in a rounded card next to it. There is no title bar or tab strip; the window buttons sit at the top of the sidebar.

From top to bottom, the sidebar holds:

1. **Top row:** back, forward, reload (stop while loading), and ✦ Ask Claude.
2. **"Claude can interact"** pill with a **Turn Off** button, only while Claude may click, type, and run JavaScript. The page card also gets an orange outline.
3. **Address field.** It works as before: type an address or a search, with inline completion and suggestions.
4. **Favorites:** a grid of site icons.
5. **Pinned** tabs.
6. **Today:** everyday tabs, newest at the top, under **＋ New Tab**.

The favorites grid stays in place; Pinned and Today scroll together.

## Sections

| Section | Shown as | Closing it (⌘W, ✕, –) |
|---|---|---|
| Favorites | Icon tiles | **Unloads** the tab and resets it to its saved address |
| Pinned | Rows | **Unloads** the tab and resets it to its saved address |
| Today | Rows | Closes the tab (⇧⌘T reopens it) |

When a tab becomes a favorite or is pinned, its address is saved. Unloading releases the page and its memory, but the tab stays in the sidebar and opens at the saved address next time. **Set Current Page as Pinned URL** (right-click, or the File menu) saves the current address instead.

Unloading is different from [tab sleep](tab-sleep.md): a sleeping tab keeps its current page and scroll position and wakes up where it was.

Tabs opened from a page (links that open a new tab) and new tabs go to the top of Today.

## Moving tabs

- **Right-click** a row or tile: Pin Tab / Unpin Tab, Add to Favorites / Remove from Favorites, Set Current Page as Pinned URL, Copy Link, Sleep Tab, Close Tab or Unload Tab.
- **⌘D** pins or unpins the current tab.
- **Drag** a row or tile to reorder it, or to move it between Favorites, Pinned, and Today. A white line shows where it will land.

## Shortcuts

| Shortcut | Action |
|---|---|
| ⌘T | New tab (the address field gets focus) |
| ⌘L | Focus the address field |
| ⌘W | Close a Today tab, or unload a pinned or favorite tab |
| ⌘D | Pin or unpin the current tab |
| ⌘S | Hide or show the sidebar |
| ⌘1–⌘8, ⌘9 | The n-th tab in sidebar order; ⌘9 is the last |
| ⌃⇥, ⌃⇧⇥ | Next or previous tab in sidebar order |

Pages that handle ⌘S themselves (editors such as code-server) get the key first, as in other browsers.

## Hidden sidebar

⌘S hides the sidebar and the window buttons; the page card fills the window. Rest the pointer on the left edge of the window for a moment and the sidebar slides in as a floating panel; move away and it hides again. With **Reduce Motion** on, it appears without sliding.

Drag the right edge of the sidebar to change its width (200–360 pt). The width and whether the sidebar is hidden are kept in `settings.json` (`sidebar_width`, `sidebar_visible`).

## Favicons

Icons are fetched once per site, without cookies, and cached as 32 px PNG files in `favicons/` inside the data folder. They are refreshed after 7 days. Sites without an icon, sites that only serve plain `http://` icons, and pages that are not web pages show a colored letter instead. Clearing history also removes the icons of sites that no longer appear in any tab or in history.

## Where tabs are saved

Tabs, their sections, and saved addresses are stored in `spaces.json` (permissions 0600) in the data folder. The format already describes Spaces; this version has one Space, **Personal**.

On the first launch after upgrading, the tabs in the old `session.json` are imported into Today. `session.json` itself is not changed, so an older build of my-browser still opens with its old tabs. If `spaces.json` was written by a newer build, this build leaves it untouched and does not save tabs (a notice says so).

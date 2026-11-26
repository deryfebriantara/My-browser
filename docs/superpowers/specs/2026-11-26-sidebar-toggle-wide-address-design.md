# Sidebar toggle button and wide address editor

Status: approved by the owner (2026-11-26). A small change on top of stage 1 of the [sidebar redesign](2026-11-26-arc-style-redesign-design.md).

## Why

The owner wants two things:
- **Auto-hide the sidebar:** it appears when the pointer reaches the left edge, and the owner can switch between hidden and shown. Auto-hide already exists in 0.2.0 (⌘S, or **View > Hide Sidebar**), but there is no visible control, so the owner did not find it.
- **See the full address while typing.** The address field lives in the sidebar, which is only about 200 pt wide, so long URLs are cut off.

## Decisions (approved)

1. **Sidebar toggle button.** An icon button (`sidebar.left`) in the sidebar's top row, left of back/forward/reload.
   - Sidebar docked: the button switches to auto-hide (the same as ⌘S), with the tooltip "Hide Sidebar (⌘S)".
   - Sidebar showing as a floating overlay: the button docks it again, with the tooltip "Keep Sidebar Open (⌘S)".
   - The choice is persisted, as today (`sidebar_visible`).
2. **Wide address editor.** Clicking the address, ⌘L, ⌘T, or an empty tab opens a wide bar over the top of the page:
   - It starts at the sidebar address's left edge and is level with it.
   - Width: up to 760 pt (min(760, window width − its x − 16)), height 34 pt, font 14 pt.
   - It has a dark background, white text, and a shadow, and shows the full URL. The suggestions appear directly below it.
   - When editing ends (Enter, Escape then click away, or switching tabs), the bar disappears.
   - When the sidebar is not being edited, it shows a one-line address display.

Rejected alternative: letting the field grow taller inside the sidebar and wrap. Long URLs would take 3–4 lines and push the tab list down.

## How (builder's calls)

- **Sidebar address:** it becomes a non-editable display, a borderless button showing the URL truncated at the tail. Clicking it sends `focusAddress:`.
- **Editing:** it moves to one editable field inside a floating box added to the window's content view above the sidebar and the card, hidden while not editing.
  - That field keeps the role of `Sidebar::address`, so the suggestion logic, keyboard handling, delegate checks, and test hooks keep working unchanged.
  - This matches the later command-bar stage (display in the sidebar, editor floating over the page).
- **Layout:** the toggle button sits leftmost in the right-aligned top-row group. Button spacing tightens from 28 pt to 26 pt, so five buttons clear the window buttons at the minimum sidebar width (200 pt).
- **Auto-hide mode:** focusing the address already brings in the overlay and keeps it while typing. The editor is positioned against the overlay's address display.

## Verification

- **E2E (debug hooks):**
  - while editing at a 1280 pt window, the editor field is at least 600 pt wide;
  - the editor is hidden after Enter;
  - the toggle button switches docked → hidden, and overlay → docked;
  - the setting persists across a relaunch.
- `scripts/check.sh` passes.
- **Manual:** click the address display with the mouse, and check the look in light and dark mode.

# Sidebar toggle button and wide address editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A visible button to switch the sidebar between docked and auto-hide. Editing the address happens in a wide bar over the page that shows the full URL.

**Architecture:** Both changes are app-only (`crates/app`).
- **Toggle:** a new button in `Sidebar` sends the existing `toggleSidebar:` action.
- **Address:** the sidebar's address field becomes a clickable display. The editable `NSTextField` (still `Sidebar::address`) moves into a floating box that `Ui` adds above everything and shows only while editing.

**Tech Stack:** Rust, `objc2` 0.6 / `objc2-app-kit` 0.3 (existing crates only).

**Spec:** `docs/superpowers/specs/2026-11-26-sidebar-toggle-wide-address-design.md`

## Global Constraints

- **No new crates; native views only.** The editor box is a `PaintView` with layer properties: no `drawRect`, no WebView.
- **UI text:** English. Code comments: Indonesian.
- **Sizes:** editor width min(760, window width − x − 16) pt, height 34 pt, font 14 pt. Top-row button spacing 26 pt.
- **Tooltips:** "Hide Sidebar (⌘S)" while docked; "Keep Sidebar Open (⌘S)" in the overlay.
- **E2E:** test instances use a temporary `HOME` and a short `MYBROWSER_DATA_DIR`. Build the debug binary first (`cargo build -p my-browser`), and stop instances by pid.
- **Every commit:** `scripts/check.sh` passes; Indonesian commit message ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Resizing the window while the editor is open:** the editor follows the display's position and the new width.
2. **Narrow window (800 pt):** the editor stays inside the window (width > 0, right edge ≤ window width − 16).
3. **Switching tabs while editing:** editing ends and the editor disappears (`end_address_editing`).
4. **Auto-hide mode:** ⌘L shows the overlay with the editor aligned to the overlay's address display. The overlay stays while typing.
5. **Escape, then clicking the page:** the editor disappears (`controlTextDidEndEditing` path), and the display shows the tab's URL.

---

### Task 1: Sidebar toggle button

**Files:**
- Modify: `crates/app/src/sidebar.rs` (top row), `crates/app/src/window.rs` (`relayout` passes the mode), `crates/app/src/debug_ui.rs`

**Interfaces:**
- Produces:

```rust
// sidebar.rs
pub fn set_mode_hint(&self, mode: SidebarMode);   // tooltip of the toggle button
#[cfg(debug_assertions)] pub fn toggle_button(&self) -> Retained<NSButton>;
// debug_ui: "sidebar_click" with text "toggle" -> performClick on the toggle button
```

- **Button:** SF Symbol `sidebar.left`, action `toggleSidebar:` (existing; it docks when not docked and hides when docked). It is the leftmost button of the right-aligned group: x = w − 8 − 24 − 4 × 26.

- [ ] **Step 1: Write the failing E2E check** (scratchpad script, debug binary):

```text
launch 2 example tabs
sidebar_click "toggle"                     -> window_layout.mode == "hidden"
sidebar_hover_edge, wait 0.5 s             -> mode == "overlay"
sidebar_click "toggle"                     -> mode == "docked"
sidebar_click "toggle"; quit; relaunch     -> mode == "hidden"
```

- [ ] **Step 2: Run it.** Expected: FAIL (`sidebar_click "toggle"` errors with "id tab tidak valid").

- [ ] **Step 3: Implement the button, `set_mode_hint` (called from `Ui::relayout`), the 26 pt spacing, and the debug hook.**

- [ ] **Step 4: Run the E2E and `scripts/check.sh`.** Expected: all checks pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src docs/superpowers
git commit -m "feat(app): tombol sembunyikan/tampilkan sidebar di baris atas"
```

---

### Task 2: Wide address editor

**Files:**
- Modify:
  - `crates/app/src/paint.rs` (`PaintView::raised`);
  - `crates/app/src/sidebar.rs` (display button, editor box);
  - `crates/app/src/window.rs` (adds the editor box to the root, `show_editor`, repositions on relayout);
  - `crates/app/src/state.rs` (`focus_address` shows the editor; `end_address_editing` and the deferred `address_editing_ended` check hide it);
  - `crates/app/src/debug_ui.rs`;
  - `docs/sidebar.md`, `docs/testing/manual-checklist.md`.

**Interfaces:**
- Consumes: `Sidebar::address` (stays the editable field, so `address_bar.rs`, `actions.rs::with_address`, and the debug hooks are unchanged).
- Produces:

```rust
// paint.rs
pub fn raised(mtm: MainThreadMarker, color: &NSColor, radius: f64) -> Retained<PaintView>;   // fill + layer shadow

// sidebar.rs
pub fn editor_box(&self) -> Retained<PaintView>;             // floating box holding `address`, hidden by default
pub fn display_frame_in_window(&self) -> NSRect;              // frame of the address display
// update_nav: sets the display title always; the editor text only when not editing

// window.rs
pub fn show_editor(&self, visible: bool);                     // positions (x, y from the display; width min(760, W - x - 16); height 34) and shows/hides
#[cfg(debug_assertions)] pub fn editor_visible(&self) -> bool;
// debug_ui: window_layout gains "editor_visible"; "sidebar_click" with text "address" -> performClick on the display
```

**Look:**
- Editor box: color sRGB (0.11, 0.11, 0.13, 0.97), radius 10, shadow as for the card. White 14 pt text, white insertion point (`style_editor`).
- Display: borderless left-aligned button, 13 pt, white 85%, single line truncated at the tail, inside the existing rounded box.

- [ ] **Step 1: Write the failing E2E checks** (1280×800 window, then 800×500):

```text
focus_address                 -> address_state.field_frame width >= 600; window_layout.editor_visible == true
type "https://example.org/" + insertNewline:   -> editor_visible == false; active tab url == "https://example.org/"
sidebar_click "address"       -> address_state.editing == true; editor_visible == true
key cancelOperation: x2, sidebar_click "<other tab id>"   -> editor_visible == false     (Review Focus 3)
window_size 800x500, focus_address   -> field right edge <= 800 - 16 and width > 0 (window coords)  (Review Focus 2)
window_size 1000x700 while editing   -> editor_visible == true, x unchanged, width == min(760, 1000 - x - 16)  (Review Focus 1)
toggleSidebar, focusAddress:  -> mode == "overlay", editor x == overlay display x  (Review Focus 4)
```

- [ ] **Step 2: Run them.** Expected: FAIL (field width about 204, `editor_visible` missing).

- [ ] **Step 3: Implement `PaintView::raised`, the display and editor box in `Sidebar`, and `Ui::show_editor` (called from `relayout` while visible), plus the state wiring and the debug fields.** The editor is shown before `makeFirstResponder`, so editing starts in a visible field.

- [ ] **Step 4: Run the E2E and `scripts/check.sh`.** Expected: all checks pass.

- [ ] **Step 5: Docs.**
  - `docs/sidebar.md`: the toggle button, and that typing opens a wide bar with the full address.
  - Manual checklist: clicking the display with the mouse; the editor look in light and dark mode.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src docs
git commit -m "feat(app): address bar lebar saat mengetik, alamat lengkap terlihat"
```

# Arc-style redesign, stage 1 (window, sidebar, favorites) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the title bar, nav bar, and tab strip with a colored sidebar (favorites, Pinned, Today) next to a rounded page card. Persist tabs in a Spaces-ready `spaces.json` with exactly one Space (Personal).

**Architecture:**
- **Core (`crates/core`, no macOS dependencies):**
  - tab sections and their rules (`tabs.rs`);
  - the sidebar model;
  - themes and contrast;
  - favicon selection and the disk cache;
  - the `spaces.json` format and import from `session.json`.
- **App (`crates/app`, AppKit):**
  - native views: a gradient background, the sidebar, row and tile views, and the card;
  - pure layout math in `layout.rs`;
  - a favicon fetcher on an ephemeral `NSURLSession`.
- **No WebView in the chrome.**

**Tech Stack:**
- Rust (toolchain from `rust-toolchain.toml`);
- `objc2` 0.6 with `objc2-foundation`, `objc2-app-kit`, and `objc2-web-kit` 0.3. `objc2-quartz-core` 0.3 is new as a direct dependency but is already in `Cargo.lock`; use it for `CAGradientLayer` and layer shadows;
- `serde`/`serde_json`, `tempfile` for tests.

**Spec:** `docs/superpowers/specs/2026-11-26-arc-style-redesign-design.md` (stage 1 = §1, §2, §3, §10, §11 with one Space, plus the hidden sidebar, status cues, and the interim address bar).

## Global Constraints

- **Platform:** macOS 14+ on Apple Silicon.
- **Dependencies:** no new crates except `objc2-quartz-core = "0.3"` (workspace dependency). `cargo build --offline` must work.
- **Chrome:** the chrome adds no WebView and no process. The sidebar, rows, card, toast, and suggestion popup are native views.
- **Text:** UI text is English. Code comments are Indonesian, following the existing files.
- **Public repo:** no personal data in fixtures, docs, or screenshots. Use `example.com`, `example.org`, and local test servers.
- **Every commit:**
  - `scripts/check.sh` passes (rustfmt, clippy `-D warnings`, all tests including `crates/engine-webkit/tests/smoke.rs`, `cargo audit`).
  - The message is Indonesian conventional style and ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. The repo-local `user.email` is already the noreply address.
- **Agent shells:** run `export PATH="$HOME/.cargo/bin:$PATH"` first, because `cargo` is not on `PATH`.
- **Test instances:**
  - Use a temporary `HOME` and a short `MYBROWSER_DATA_DIR` under `$TMPDIR`, because of the 104-byte socket path limit.
  - Launch `target/debug/my-browser <urls>`.
  - Drive it with `target/debug/my-browser --mcp` (JSON-RPC `tools/call`, tool `debug_ui` with `{action, text}`, and the normal tools).
  - Stop test instances by pid. Never quit the owner's running browser.
  - E2E driver scripts live outside the repo (session scratchpad), as in earlier work.
- **Debug-only hooks** go behind `#[cfg(debug_assertions)]`. Every new `debug_ui` action name that could leak gets added to the release-binary `strings` check in `.github/workflows/ci.yml`.
- **Values pinned by the spec:**
  - sidebar default 240 pt, range 200–360;
  - outer inset 8 pt; card radius 10 pt (continuous);
  - Ask Claude panel 380 pt wide, 8 pt from the card;
  - tab row 28 pt with a 16 pt favicon; favorite tile about 44 pt with a 20 pt favicon;
  - hidden-sidebar hover edge 8 pt with about 150 ms delay;
  - toast lasts 6 s (today's `NOTICE_SECS`);
  - interact outline 2 pt orange;
  - text contrast ≥ 4.5:1;
  - favicon cache: 32×32 px PNG, refreshed after 7 days, capped at 2,000 files / 20 MB, 512 KB download limit, 10 s timeout, https only.
- **Files:**
  - `spaces.json` is written 0600 (`persist::write_private`).
  - `session.json` is never written again (it is only read for the import).
  - `history.json` stays Personal's history, with an unchanged format.

## Review Focus

1. **Long, emoji, RTL, or empty titles in rows and tiles:**
   - Expected: one line, tail truncation, 28 pt rows, no panic on multi-byte titles; an empty title shows the host, a blank tab shows nothing.
   - Covered by Task 2 (model test) and Task 7 (E2E with a 300-character emoji title).
2. **Clicking ✕ on rows in quick succession:**
   - Removing a row view during its own action must not crash, and the final row count must be right.
   - Covered by Task 7 (E2E: 10 rapid `sidebar_click … :close` calls).
3. **Narrow window with the panel open and a wide sidebar:**
   - No negative widths. The sidebar shrinks toward 200 before the card collapses.
   - Covered by Task 7 (`layout.rs` unit test).
4. **Hostile favicon responses** (garbage bytes, a huge file, a zero-size image, an http redirect, a slow server):
   - The monogram shows, nothing hangs, and nothing is cached.
   - Covered by Task 6 (`normalize_icon` tests and fetcher rules).
5. **Odd `session.json` on first launch** (invalid URLs, `active_index` out of range, no tabs, a corrupt file):
   - Valid tabs are imported, the active tab falls back to the first, an empty result gives one blank tab, and `session.json` stays byte-identical.
   - Covered by Task 4 (unit tests and the persistence test).

---

### Task 1: Tab sections in `TabList`

**Files:**
- Modify: `crates/core/src/tabs.rs` (struct `Tab`, `TabList`, tests)
- Modify: `crates/core/src/browser.rs` (`close_tab` adapts to the new return type)
- Modify: `crates/core/src/browser/tests.rs` (only assertions that depended on "open after active")

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind { Favorite, Pinned, #[default] Today }

// New fields on Tab:
pub kind: TabKind,
pub saved_url: Option<Url>,   // Some only for Favorite/Pinned
pub saved_title: String,

impl Tab {
    pub fn is_blank(&self) -> bool;         // kind == Today && url.is_none()
    pub fn display_title(&self) -> String;  // title, else host, else "New Tab" (moved from app state.rs)
}

pub enum Closed { Removed(Tab), Unloaded }

impl TabList {
    pub fn open(&mut self, url: Option<Url>, now: Instant, activate: bool) -> TabId; // top of Today; None reuses an active blank tab
    pub fn close(&mut self, id: TabId, now: Instant) -> Option<Closed>;
    pub fn place(&mut self, id: TabId, kind: TabKind, at: usize) -> bool;            // at = index within the target section, clamped
    pub fn set_saved_to_current(&mut self, id: TabId) -> bool;
    pub fn visible(&self) -> impl Iterator<Item = &Tab>;                             // sidebar order, blank tabs excluded
    pub fn section(&self, kind: TabKind) -> impl Iterator<Item = &Tab>;              // visible tabs of one kind
}
```

- Rules this task fixes:
  - **Order:** the Vec order is the sidebar order, and sections are always contiguous: Favorite, then Pinned, then Today.
  - **Blank tabs:** switching away from an active blank tab removes it (it is not pushed to the closed stack).
  - **Closing a Favorite or Pinned tab** returns `Closed::Unloaded`, keeps the tab at its index, and resets it: `url = saved_url`, `title = saved_title`, `scroll_y = 0.0`, `restore_scroll = false`, `video_time = None`, `committed_url = None`, `can_back = can_forward = false`, `awake = false`.
  - **After closing or unloading the active tab:** activate the next visible tab below it, else the one above. If there is none, push a blank Today tab and activate it.
  - **Closed stack:** only Today tabs enter it, remembering their index within Today. `reopen_closed` inserts at `today_start + min(index, today_len)`, asleep.
  - **`place`:**
    - Today → Favorite or Pinned copies `url` and `title` into `saved_url` and `saved_title`; refused (`false`) for a blank tab.
    - Moving into Today clears both.
    - Favorite ↔ Pinned keeps them.
  - **`activate_index(n)`, `activate_next`, `activate_previous`:** work over `visible()`. Index ≥ 8 means the last visible tab. From a blank active tab, "next" goes to the first visible tab and "previous" to the last.

- [ ] **Step 1: Write the failing tests** in the `tabs.rs` test module. Replace `open_inserts_after_active_and_activates`, and convert the existing tests that open `None` tabs to use URLs:

```rust
#[test] fn open_puts_new_tab_at_top_of_today()                // [P(pinned), A] + open(B) => urls [P, B, A]
#[test] fn open_none_reuses_active_blank_tab()                // two open(None) calls return the same id; count unchanged
#[test] fn switching_away_from_blank_tab_removes_it()         // blank active, activate(A) => blank gone, reopen_closed() is None
#[test] fn place_into_pinned_saves_url_and_title()            // saved_url == url, saved_title == title, kind Pinned, index 0
#[test] fn place_back_to_today_clears_saved_and_uses_index()  // place(P, Today, 1) => Today order [A, P]; saved_url None
#[test] fn place_blank_tab_is_refused()                       // returns false, kind still Today
#[test] fn sections_stay_contiguous_after_moves()             // random sequence of place() over 6 tabs; kinds() is sorted Favorite<Pinned<Today
#[test] fn closing_pinned_tab_unloads_and_resets_to_saved_url() // Some(Closed::Unloaded); still present; url == saved; scroll_y 0; active == tab below
#[test] fn closing_only_slot_leaves_blank_today_tab_active()  // only tab is a favorite: after close, active tab is_blank()
#[test] fn closing_active_activates_below_else_above()
#[test] fn reopen_closed_goes_back_into_today_at_its_position()
#[test] fn activate_index_and_cycling_skip_blank_tab()
#[test] fn display_title_falls_back_to_host_then_new_tab()
#[test] fn set_saved_to_current_updates_slot()                // Pinned tab navigated to /b: after call saved_url ends with "/b"
```

Keep `closed_stack_keeps_at_most_20` and `deactivated_tab_gets_last_viewed_now` passing, with URL tabs.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser-core tabs::`
Expected: compile errors for `TabKind`, `place`, `Closed`, and the other new names.

- [ ] **Step 3: Implement the `Tab` fields and the `TabList` methods above in `crates/core/src/tabs.rs`**

Keep a private `fn section_bounds(&self, kind: TabKind) -> (usize, usize)` (start, end) and use it for every insertion. Do the blank-tab removal inside `activate`.

- [ ] **Step 4: Adapt `Browser::close_tab`** (`crates/core/src/browser.rs:180`) to `Option<Closed>`:
  - For `Closed::Unloaded`, set `tab.visit_key = tab.url.as_ref().and_then(visit_key)`, so waking the slot is not a new history visit.
  - Then wake the new active tab exactly as today.
  - Move `display_title` call sites in `crates/app` (`state.rs`, `control.rs`) to `Tab::display_title()` and delete the app copy.

- [ ] **Step 5: Run all core tests and the app build**

Run: `cargo test -p my-browser-core && cargo build -p my-browser`
Expected: PASS. Fix only assertions in `browser/tests.rs` that encoded "open after active". Do not change the behavior they test.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/tabs.rs crates/core/src/browser.rs crates/core/src/browser/tests.rs crates/app/src
git commit -m "feat(core): bagian tab Favorite, Pinned, Today dan tab kosong yang tidak menumpuk"
```

---

### Task 2: Browser operations for sections, the sidebar model, and the `Loaded` change

**Files:**
- Create: `crates/core/src/sidebar.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/browser.rs`, `crates/core/src/browser/tests.rs`

**Interfaces:**
- Consumes: `TabKind`, `TabList::{place, set_saved_to_current, section, visible}`, `Tab::display_title` (Task 1).
- Produces:

```rust
// browser.rs
pub enum UiChange { /* existing… */ Loaded(TabId) }     // pushed on LoadFinished when the tab URL is http or https
impl<E: Engine> Browser<E> {
    pub fn place_tab(&mut self, id: TabId, kind: TabKind, at: usize) -> bool;   // dirty + UiChange::Tabs on success
    pub fn toggle_pin(&mut self, id: TabId) -> bool;       // Today -> Pinned (end); Pinned|Favorite -> Today (0)
    pub fn toggle_favorite(&mut self, id: TabId) -> bool;  // Favorite -> Today (0); Today|Pinned -> Favorite (end)
    pub fn set_pinned_url(&mut self, id: TabId) -> bool;
}

// sidebar.rs
pub fn site_host(url: &Url) -> Option<String>;   // lowercase host without a leading "www."; None for non-http(s)
pub struct RowInfo {
    pub id: TabId, pub kind: TabKind, pub title: String, pub host: Option<String>,
    pub url: Option<String>, pub active: bool, pub asleep: bool, pub playing: bool,
}
impl RowInfo { pub fn accessibility_label(&self) -> String }   // "<title>, <host>[, asleep][, playing audio]"
pub struct SidebarModel { pub favorites: Vec<RowInfo>, pub pinned: Vec<RowInfo>, pub today: Vec<RowInfo> }
pub fn sidebar_model(tabs: &TabList) -> SidebarModel;           // blank tabs excluded
```

- `asleep` = `!awake && url.is_some()`. `playing` = `media_active`.

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/sidebar.rs`:

```rust
#[test] fn model_groups_sections_in_order_and_skips_blank()
#[test] fn model_keeps_long_emoji_and_rtl_titles_intact()   // 300 × "🦀", "مرحبا بالعالم": title unchanged, no panic
#[test] fn empty_title_shows_host()                         // title "" + https://www.example.com/x => "example.com"
#[test] fn accessibility_label_mentions_asleep_and_audio()  // "Docs, example.com, asleep, playing audio"
#[test] fn site_host_strips_www_and_rejects_non_http()      // "https://WWW.Example.com/" => "example.com"; "about:blank" => None
```

In `crates/core/src/browser/tests.rs`, using `FakeEngine`:

```rust
#[test] fn toggle_pin_moves_between_today_and_pinned()
#[test] fn toggle_favorite_moves_to_end_of_favorites()
#[test] fn closing_pinned_tab_drops_page_and_wake_is_not_a_visit()  // pages dropped; activate again => page created loading saved URL; visit_count unchanged
#[test] fn tab_opened_from_page_goes_to_top_of_today()              // PageEvent::OpenInNewTab from tab A => new tab is first Today tab
#[test] fn load_finished_emits_loaded_only_for_http_pages()         // https => Loaded(id) in take_changes(); data: URL => none
#[test] fn place_tab_marks_session_dirty()                          // session_if_dirty().is_some() after place_tab
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser-core -- sidebar:: browser::`
Expected: FAIL (`sidebar` module and the new methods missing).

- [ ] **Step 3: Implement `sidebar.rs` and the `Browser` methods with the signatures above.**
  - `OpenInNewTab` already goes through `open_tab`, so top-of-Today placement comes from Task 1; the test pins it.
  - Push `UiChange::Loaded(id)` in the `LoadFinished` arm.
  - `UiChange` stays `PartialEq + Eq`; `change()` deduplicates as before.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p my-browser-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): operasi pin/favorit, model sidebar, dan perubahan Loaded"
```

---

### Task 3: Themes and contrast

**Files:**
- Create: `crates/core/src/theme.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces:

```rust
pub type Rgb = [u8; 3];
pub struct Theme { pub id: &'static str, pub stops: [Rgb; 3], pub angle_deg: f64, pub dot: Rgb }
pub const PERSONAL: Theme;   // id "personal", purple -> pink; mockup start #2b2470, #6a3fa0, light end tuned down from #c45a8c
pub const WORK: Theme;       // id "work", navy -> teal; mockup start #0f3a5f, #1f6f8b, light end tuned down from #3aa39a
pub const PRESETS: &[Theme];             // [PERSONAL, WORK] in this stage
pub fn theme(id: &str) -> &'static Theme; // unknown id => &PERSONAL
pub const SECONDARY_ALPHA: f64;          // section labels and asleep rows (white at this alpha)
pub fn contrast_ratio(fg: Rgb, bg: Rgb) -> f64;  // WCAG 2.x relative luminance
pub fn blend(fg: Rgb, bg: Rgb, alpha: f64) -> Rgb;
```

- Sidebar text is white. Keep each preset's hue and darken only the stops that fail.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn contrast_ratio_matches_known_values() {
    assert!((contrast_ratio([255,255,255], [0,0,0]) - 21.0).abs() < 0.01);
    assert!((contrast_ratio([255,255,255], [0xc4,0x5a,0x8c]) - 4.05).abs() < 0.05); // why the mockup pink is tuned
}
#[test] fn every_preset_stop_passes_for_text_and_secondary_text() {
    for t in PRESETS { for s in t.stops {
        assert!(contrast_ratio([255,255,255], s) >= 4.5, "{} {:?}", t.id, s);
        assert!(contrast_ratio(blend([255,255,255], s, SECONDARY_ALPHA), s) >= 4.5, "{} {:?}", t.id, s);
    }}
}
#[test] fn unknown_theme_falls_back_to_personal()
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser-core theme::`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement `theme.rs`**

Pick the darkened end stops and the largest `SECONDARY_ALPHA` < 1.0 for which the second test passes, and note the chosen values in a comment.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p my-browser-core theme::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/theme.rs crates/core/src/lib.rs
git commit -m "feat(core): tema gradasi Personal dan Work dengan uji kontras 4.5:1"
```

---

### Task 4: `spaces.json`, import from `session.json`, and sidebar settings

**Files:**
- Create: `crates/core/src/spaces_file.rs`
- Modify:
  - `crates/core/src/lib.rs`, `crates/core/src/session.rs` (keep `SessionFile::load` for the import; delete `to_session`, `from_session`, and `save` once unused);
  - `crates/core/src/browser.rs` (constructor, `session_if_dirty`, `space()`);
  - `crates/core/src/settings.rs`;
  - `crates/app/src/persistence.rs`, `crates/app/src/main.rs`, `crates/app/src/state.rs`.

**Interfaces:**
- Consumes: `TabKind`, `Tab::{saved_url, saved_title}` (Task 1).
- Produces:

```rust
// spaces_file.rs
pub const SPACES_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpacesFile { pub version: u32, pub active_space: usize, pub spaces: Vec<SavedSpace> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSpace { pub id: u64, pub name: String, pub theme: String, pub store: StoreRef,
                        pub active_index: Option<usize>, pub tabs: Vec<SpaceTab> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRef { Default, Identified(String) }      // Identified is used from stage 2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceTab { pub kind: TabKind, pub url: String, pub title: String, pub scroll_y: f64,
                      #[serde(default)] pub saved_url: Option<String>, #[serde(default)] pub saved_title: String }
pub enum LoadedSpaces { Loaded(SpacesFile), Missing, Newer(u32) }   // corrupt => set aside as .bad, then Missing
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceMeta { pub id: u64, pub name: String, pub theme: String, pub store: StoreRef }
impl SpacesFile {
    pub fn load(path: &Path) -> LoadedSpaces;                 // reads {"version"} first; leaves a newer file untouched
    pub fn save(&self, path: &Path) -> io::Result<()>;        // persist::write_private (0600)
    pub fn from_session(s: &SessionFile) -> SpacesFile;       // one Space: id 1, "Personal", theme "personal", StoreRef::Default, Today tabs
    pub fn personal_default() -> SpacesFile;                  // same Space, no tabs
}
impl TabList {
    pub fn from_space(s: &SavedSpace, now: Instant) -> TabList;    // all asleep; invalid URLs skipped; no tabs => one blank tab
    pub fn to_space(&self, meta: &SpaceMeta) -> SavedSpace;        // blank tabs skipped
}

// browser.rs
pub fn new(engine: E, settings: Settings, spaces: &SpacesFile, history: History, sinks: SinkFactory, now: Instant) -> Self;
    // uses spaces.spaces[active_space] (falls back to the first, then to personal_default())
pub fn space(&self) -> &SpaceMeta;
pub fn session_if_dirty(&mut self) -> Option<SpacesFile>;
pub fn spaces_file(&self) -> SpacesFile;

// settings.rs (struct is #[serde(default)])
pub sidebar_width: f64,     // default 240.0
pub sidebar_visible: bool,  // default true

// app persistence.rs
pub struct Loaded { pub settings: Settings, pub spaces: SpacesFile, pub newer_spaces: Option<u32>, pub history: History }
pub fn load() -> Loaded;              // = load_from(&data_dir())
pub fn load_from(dir: &Path) -> Loaded;
pub fn save_spaces(file: &SpacesFile); // no-op is decided by the caller when newer_spaces is Some
```

- **Load order in `load_from`:**
  - `spaces.json` `Loaded` → use it.
  - `Missing` → `SessionFile::load(session.json)` → `from_session`, else `personal_default()`.
  - `Newer(v)` → the same fallback, and `newer_spaces = Some(v)`.
- **`AppState` when `newer_spaces` is set:** it never saves `spaces.json` (neither in `tick` nor in `save_all`). At startup it shows the notice: `Tabs won't be saved: spaces.json was written by a newer version of my-browser.`
- **`last_active`** (spec §11) is not part of this stage. Stage 3 adds it as a `#[serde(default)]` field together with the archiving grace period, so `SPACES_VERSION` stays 1.

- [ ] **Step 1: Write the failing tests**

In `spaces_file.rs`:

```rust
#[test] fn round_trip_keeps_kinds_saved_urls_and_active_index()
#[test] fn saved_file_is_private_0600()                        // metadata().permissions().mode() & 0o777 == 0o600
#[test] fn missing_file_is_missing()
#[test] fn corrupt_file_is_set_aside_and_missing()             // "<path>.bad" exists afterwards
#[test] fn newer_version_is_reported_and_left_untouched()      // {"version":2,...} => Newer(2); bytes identical
#[test] fn import_from_session_puts_tabs_in_personal_today()   // name "Personal", theme "personal", Default, active_index kept
#[test] fn import_skips_invalid_urls_and_bad_active_index()    // ["not a url", "https://a.example/"], active_index 7 => 1 tab, active first
#[test] fn empty_space_restores_one_blank_tab()
#[test] fn blank_tabs_are_not_saved()
```

In `browser/tests.rs`:

```rust
#[test] fn new_restores_kinds_and_wakes_only_active()          // pinned + 2 today, active_index 2 => one page created, kinds restored
```

In `settings.rs`:

```rust
#[test] fn old_settings_without_sidebar_fields_get_defaults()  // 240.0, true
```

In `crates/app/src/persistence.rs` (`#[cfg(test)]`, `tempfile`):

```rust
#[test] fn session_json_is_imported_and_left_untouched()       // load_from(dir) has the tabs; session.json bytes identical; no spaces.json written by load
#[test] fn newer_spaces_file_sets_newer_spaces()               // Some(2)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser-core -- spaces_file:: settings:: browser:: && cargo test -p my-browser persistence::`
Expected: FAIL (types missing).

- [ ] **Step 3: Implement `spaces_file.rs` and the `Browser` constructor, `space`, `session_if_dirty`, and `spaces_file`.**
  - `Browser` stores the `SpaceMeta` of the loaded Space.
  - For `LoadedSpaces::load`, deserialize a small `struct VersionOnly { version: u32 }` first. Use `read_json`'s `.bad` behavior for corrupt files.

- [ ] **Step 4: Switch the app to the new persistence.**
  - `main.rs` calls `persistence::load()` and passes `Loaded` into `AppState::new`.
  - In `state.rs`, `tick` and `save_all` call `save_spaces` unless `newer_spaces` is set.
  - `save_session` and every `session.json` write are deleted.

- [ ] **Step 5: Run all tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: E2E migration check**

In a test instance whose `MYBROWSER_DATA_DIR` contains a copied `session.json` with three `example.com`/`example.org` tabs:
- Launch it, then quit through `debug_ui quit`.
- `spaces.json` exists with mode `-rw-------` and three Today tabs.
- `cmp` shows `session.json` unchanged.
- A second launch restores the same three tabs.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src crates/app/src
git commit -m "feat: spaces.json (satu Space Personal), impor session.json, pengaturan sidebar"
```

---

### Task 5: Favicon selection, disk cache, and isolated evaluation

**Files:**
- Create: `crates/core/src/favicon.rs`
- Modify:
  - `crates/core/src/lib.rs`;
  - `crates/core/src/engine.rs` (`Page::evaluate_isolated`);
  - `crates/engine-webkit/src/page.rs`;
  - `crates/core/src/browser/tests.rs` (`FakePage` impl);
  - `crates/engine-webkit/tests/smoke.rs`.

**Interfaces:**
- Consumes: `sidebar::site_host` (Task 2).
- Produces:

```rust
// engine.rs, trait Page
/// Like `evaluate`, but in the isolated content world shared with the password scripts,
/// so page scripts cannot override DOM functions it uses. Reply never synchronous.
fn evaluate_isolated(&self, script: &str, reply: Box<dyn FnOnce(Result<String, String>)>);

// favicon.rs
pub const ICON_CANDIDATES_JS: &str;   // returns JSON: [{"href": absolute URL, "rel": "...", "sizes": "..."}]
pub fn pick_candidates(json: &str, page: &Url) -> Vec<Url>;
pub fn host_key(host: &str) -> String;               // FNV-1a 64 of the lowercased host, 16 lowercase hex chars
pub fn monogram(host: &str) -> (char, Rgb);          // first alphanumeric of host without "www.", uppercased; color from a fixed palette by host_key
pub const MAX_FILES: usize = 2000;
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
pub const MAX_ICON_BYTES: usize = 512 * 1024;
pub struct FaviconCache { /* dir */ }
impl FaviconCache {
    pub fn new(dir: PathBuf) -> Self;
    pub fn load(&self, host: &str) -> Option<Vec<u8>>;
    pub fn needs_refresh(&self, host: &str, now: SystemTime) -> bool;   // missing, or mtime older than REFRESH_AFTER
    pub fn store(&self, host: &str, png: &[u8]) -> io::Result<()>;     // write_private, then prune(MAX_FILES, MAX_BYTES)
    pub fn prune(&self, max_files: usize, max_bytes: u64) -> io::Result<()>; // delete oldest mtime first
    pub fn retain_hosts(&self, hosts: &HashSet<String>) -> io::Result<usize>; // delete *.png whose key is not a key of `hosts`
}
```

- **`pick_candidates` order:**
  1. `rel~=icon` with a known size ≥ 32 px (largest side), smallest first;
  2. `rel~=icon` without a size;
  3. `apple-touch-icon`;
  4. `<origin>/favicon.ico`.
- **Filtering:** https only (App Transport Security blocks plain http in the app fetcher), de-duplicated. A non-https page with no https candidates gives an empty list.

- [ ] **Step 1: Write the failing tests**

In `favicon.rs`:

```rust
#[test] fn pick_prefers_smallest_icon_of_at_least_32px()          // 16, 32, 192 => 32 first, then 192, ..., favicon.ico last
#[test] fn pick_falls_back_to_apple_touch_then_favicon_ico()
#[test] fn pick_keeps_https_only_and_resolves_relative_hrefs()
#[test] fn pick_survives_garbage_json()                           // "nope" on https page => [origin/favicon.ico]; on http page => []
#[test] fn host_key_is_16_hex_and_case_insensitive()
#[test] fn monogram_strips_www_and_uppercases()                   // "www.example.com" => 'E'; same color twice
#[test] fn cache_store_then_load_round_trip()
#[test] fn needs_refresh_after_seven_days()                       // File::set_modified(now - 8 days) => true; fresh => false
#[test] fn prune_removes_oldest_beyond_limits()
#[test] fn retain_hosts_deletes_icons_of_other_hosts()
```

In `smoke.rs` (real WebKit):

```rust
#[test] fn evaluate_isolated_reads_icons_despite_page_overrides()
// load_html with <link rel=icon sizes=32x32 href=/a.png> and a page script setting
// document.querySelectorAll = () => []; evaluate_isolated(ICON_CANDIDATES_JS) => JSON contains "/a.png"
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser-core favicon:: && cargo test -p engine-webkit --test smoke evaluate_isolated`
Expected: FAIL (missing items).

- [ ] **Step 3: Implement `favicon.rs` and `evaluate_isolated`.**
  - In the WebKit implementation, use `evaluateJavaScript_inFrame_inContentWorld_completionHandler` with `WKContentWorld::worldWithName(PASSWORD_WORLD)`, as `request_activity` does (`page.rs:188`). A non-string result is `Err`.
  - `FakePage` replies with `Err("fake")` on the next turn.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src crates/engine-webkit/src/page.rs crates/engine-webkit/tests/smoke.rs
git commit -m "feat: pemilihan favicon, cache disk, dan evaluate_isolated"
```

---

### Task 6: Favicon fetcher in the app

**Files:**
- Create: `crates/app/src/favicons.rs`
- Modify:
  - `crates/app/src/main.rs` (module);
  - `crates/app/src/state.rs` (`favicons` field; handle `UiChange::Loaded`; retain after Clear History);
  - `crates/app/src/history_window.rs` (after a clear, call `state.favicons.forget_unused(state)`);
  - `crates/app/src/debug_ui.rs` (action `favicons`);
  - `crates/app/Cargo.toml` and root `Cargo.toml` (only if `NSURLSession` needs no extra features: none expected).

**Interfaces:**
- Consumes: `favicon::{ICON_CANDIDATES_JS, pick_candidates, FaviconCache, monogram, MAX_ICON_BYTES}`, `sidebar::site_host`, `Page::evaluate_isolated` (Task 5), `UiChange::Loaded` (Task 2).
- Produces:

```rust
pub struct Favicons { /* FaviconCache, HashMap<String, Retained<NSImage>>, in-flight HashSet<String>, NSURLSession */ }
impl Favicons {
    pub fn new(dir: PathBuf) -> Self;                         // dir = data_dir()/favicons
    pub fn image(&self, host: &str) -> Option<Retained<NSImage>>; // memory, then disk; None => caller draws monogram
    pub fn monogram_image(host: &str, size: f64) -> Retained<NSImage>; // rounded tile + letter, from favicon::monogram
    pub fn page_loaded(&self, state: &Rc<AppState>, id: TabId);  // if needs_refresh: evaluate_isolated -> pick -> fetch first that works
    pub fn forget_unused(&self, state: &AppState);            // retain_hosts(hosts of history + tabs + saved URLs), clear memory map
}
pub fn normalize_icon(bytes: &[u8]) -> Option<Vec<u8>>;      // ≤ MAX_ICON_BYTES, decodable by NSImage, non-zero size => 32×32 px PNG
```

- **Session configuration:** `NSURLSessionConfiguration::ephemeralSessionConfiguration()` with:
  - `HTTPCookieStorage = nil`, `HTTPShouldSetCookies = false`, `URLCache = nil`;
  - `timeoutIntervalForRequest = 10`.
- **Completion:** hop to the main queue (`dispatch2`).
- **Rejected responses** (try the next candidate): anything other than HTTP 200, a body larger than `MAX_ICON_BYTES`, or `normalize_icon` returning `None`.
- **Success:** `store`, insert into memory, `state.layout_sidebar()` (Task 7 provides it; in this task, call `state.refresh()`).

- [ ] **Step 1: Write the failing tests** (`crates/app/src/favicons.rs`, `#[cfg(test)]`; AppKit image classes work in unit tests without `NSApplication`):

```rust
#[test] fn normalize_scales_png_to_32_px()        // 64×64 PNG generated with NSBitmapImageRep => output decodes to 32×32
#[test] fn normalize_rejects_garbage()            // b"<html>not an icon</html>" => None
#[test] fn normalize_rejects_oversized_input()    // MAX_ICON_BYTES + 1 bytes => None
#[test] fn normalize_rejects_zero_size_image()
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser favicons::`
Expected: FAIL.

- [ ] **Step 3: Implement `favicons.rs` and wire `UiChange::Loaded(id)` in `AppState::refresh` to `favicons.page_loaded`.** Add `debug_ui` action `favicons`, returning `{"hosts": [cached hosts in memory and on disk]}`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p my-browser favicons::`
Expected: PASS.

- [ ] **Step 5: E2E check** (network): in a test instance, open `https://www.rust-lang.org/`, then wait for `LoadFinished`.
  - `debug_ui favicons` lists `rust-lang.org`.
  - `sips -g pixelWidth -g pixelHeight` on the cached file prints 32 and 32.
  - Opening a local `http://127.0.0.1:<port>/` page caches nothing.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): pengambil favicon tanpa cookie dengan cache 32 px"
```

---

### Task 7: New window shell: gradient, card, and sidebar with the tab list

**Files:**
- Create:
  - `crates/app/src/layout.rs` (pure layout math);
  - `crates/app/src/background.rs` (gradient view);
  - `crates/app/src/card.rs` (shadow and clip container, empty state, toast, interact outline);
  - `crates/app/src/sidebar.rs` (top row, address field, scrolling list);
  - `crates/app/src/sidebar_row.rs` (`TabRowView`).
- Delete: `crates/app/src/nav_bar.rs`, `crates/app/src/tab_strip.rs`.
- Modify:
  - `crates/app/src/window.rs` (rewrite), `crates/app/src/state.rs`, `crates/app/src/actions.rs`;
  - `crates/app/src/address_bar.rs` and `crates/app/src/debug_ui.rs` (`ui.nav.address` → `ui.sidebar.address`);
  - `crates/app/src/suggest_popup.rs` (minimum width 480);
  - every `ui.nav.show_notice` caller (`chat_account.rs`, `control.rs`, `history_window.rs`, `mcp_access.rs`, `passwords.rs`, `state.rs`) → `ui.show_notice`;
  - `crates/app/src/main.rs`, root and app `Cargo.toml` (`objc2-quartz-core`);
  - `.github/workflows/ci.yml` (add `sidebar_state` to the grep).

**Interfaces:**
- Consumes: `sidebar_model`, `RowInfo`, `RowInfo::accessibility_label` (Task 2); `theme::{theme, Theme, SECONDARY_ALPHA}` (Task 3); `Settings::{sidebar_width, sidebar_visible}`, `Browser::space()` (Task 4); `Favicons::{image, monogram_image}` (Task 6).
- Produces:

```rust
// layout.rs (AppKit coordinates, origin bottom-left)
#[derive(Debug, Clone, Copy, PartialEq)] pub struct Frame { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SidebarMode { Docked, Hidden, Overlay }
#[derive(Debug, Clone, Copy, PartialEq)] pub struct Frames { pub sidebar: Frame, pub card: Frame, pub panel: Option<Frame> }
pub const INSET: f64 = 8.0;
pub const SIDEBAR_MIN: f64 = 200.0;
pub const SIDEBAR_MAX: f64 = 360.0;
pub const SIDEBAR_DEFAULT: f64 = 240.0;
pub const PANEL_WIDTH: f64 = 380.0;
pub const CARD_MIN: f64 = 320.0;
pub const CARD_RADIUS: f64 = 10.0;
pub fn layout(window_w: f64, window_h: f64, mode: SidebarMode, sidebar_w: f64, panel: bool) -> Frames;

// card.rs
pub struct Card { pub view: Retained<NSView>, pub content: Retained<NSView>, /* empty, toast, outline */ }
impl Card {
    pub fn build(mtm: MainThreadMarker) -> Card;
    pub fn set_frame(&self, f: Frame);                // also updates the shadow path
    pub fn show_empty(&self, space_name: Option<&str>); // Some => "<name>" + "Press ⌘L to type an address"; None hides
    pub fn show_toast(&self, text: &str);             // top center, hides after 6 s (generation counter like NavBar today)
    pub fn set_outline(&self, on: bool);              // 2 pt orange border; hitTest returns nil
}

// sidebar.rs
pub struct Sidebar { pub view: Retained<NSView>, pub back: Retained<NSButton>, pub forward: Retained<NSButton>,
                     pub reload: Retained<NSButton>, pub address: Retained<NSTextField>, /* list, icons, rows */ }
impl Sidebar {
    pub fn build(mtm: MainThreadMarker, target: &ActionTarget, theme: &Theme) -> Sidebar;
    pub fn set_frame(&self, f: Frame);
    pub fn update_nav(&self, can_back: bool, can_forward: bool, loading: bool, url: &str);
    pub fn is_editing(&self) -> bool;
    pub fn render(&self, model: &SidebarModel, favicons: &Favicons, target: &ActionTarget);
}

// sidebar_row.rs: define_class! NSView subclass
pub struct TabRowView;    // ivars: tab id, close button; tracking area shows the close button on hover
impl TabRowView { pub fn new(mtm: MainThreadMarker, row: &RowInfo, icon: &NSImage, theme: &Theme, target: &ActionTarget) -> Retained<Self> }

// window.rs
pub struct Ui { pub window: Retained<NSWindow>, pub sidebar: Sidebar, pub card: Card, pub target: Retained<ActionTarget>, /* background, panel card, mode, width */ }
impl Ui {
    pub fn build(mtm: MainThreadMarker, target: Retained<ActionTarget>, theme: &Theme, sidebar_width: f64, sidebar_visible: bool) -> Ui;
    pub fn relayout(&self);
    pub fn show_content(&self, view: Option<Retained<NSView>>, viewport: Option<(f64, f64)>);
    pub fn layout_content(&self, viewport: Option<(f64, f64)>);
    pub fn set_panel(&self, view: Option<Retained<NSView>>);   // panel inside its own Card at Frames.panel
    pub fn content_size(&self) -> (f64, f64);
    pub fn shown_view(&self) -> Option<Retained<NSView>>;
    pub fn show_notice(&self, text: &str);                     // = card.show_toast
}

// state.rs
pub fn layout_sidebar(&self);   // replaces layout_tabs; same deferred performBlock pattern
```

**Layout rules for `layout()`:**
- **Docked:**
  - Sidebar at x 0, full height, effective width `clamp(sidebar_w, 200, 360)`, reduced toward 200 while the card would be narrower than `CARD_MIN`.
  - Card at `x = sidebar width`, `y = 8`, `w = W − sidebar − 8 − (panel ? 380 + 8 : 0)`, `h = H − 16`.
  - Panel at `x = W − 8 − 380`, `y = 8`, `h = H − 16`.
- **Hidden:** the card starts at x 8.
- **Overlay:** the card is as in Hidden; the sidebar frame is `(8, 8, width, H − 16)`.
- No width or height is ever negative (floor 0).

**Window and view details:**
- **Window:**
  - Add `FullSizeContentView` to the style mask, `titlebarAppearsTransparent(true)`, `titleVisibility(Hidden)`.
  - The title is still set (`update_window_title` unchanged, including the "— Claude can interact" suffix).
  - Minimum content size stays 800×500.
- **Background:** a `CAGradientLayer` with `theme.stops` at `angle_deg`. When `NSWorkspace.accessibilityDisplayShouldIncreaseContrast` is true, use solid `stops[0]`. It returns YES from `mouseDownCanMoveWindow`, and so does the empty sidebar container; rows and tiles return NO.
- **Sidebar top row:**
  - Vertically centered on the close button (read `standardWindowButton(NSWindowButton::CloseButton)` on each relayout).
  - Back, forward, and reload/stop are right-aligned before ✦ (`toggleChat:`), using the same SF Symbols as today's nav bar.
- **Address field:**
  - Borderless, no background, white text, placeholder "Search or enter address".
  - Inside a 30 pt rounded container (white 14% alpha, radius 9).
  - Keeps `addressEntered:` and the delegate. The interim suggestion popup anchors to it, at least 480 pt wide.
- **List:** one scroll view, with rows in sidebar order under the labels `PINNED` and `TODAY`. A 1 pt divider (white 14% alpha) separates Pinned from Today.
  - Favorites render as plain rows in this task; Task 8 turns them into tiles.
  - A `＋ New Tab` row (`newTab:`) sits at the top of Today.
- **Row:**
  - 28 pt; favicon 16 pt (`Favicons::image(host)` or the monogram); title on one line, truncated at the tail.
  - Active row: white 24% alpha background, radius 8.
  - Asleep row: alpha `SECONDARY_ALPHA` plus a trailing `moon.fill`. Playing row: trailing `speaker.wave.2.fill`.
  - Click → `selectTab:` (tag = id). Hovering shows ✕ (`closeTabButton:`).
  - Accessibility: role button, label from `accessibility_label()`, selected when active.
- **Card:**
  - Outer view with a layer shadow (opacity 0.35, radius 12, offset (0, −4), `shadowPath` of the rounded rect).
  - Inner clip view: `cornerRadius 10`, `cornerCurve continuous`, `masksToBounds`.
  - `content` holds the web view as today. The empty state shows while the active tab is blank.
- **Claude interaction:** `toggle_claude_interact` also calls `ui.card.set_outline(on)`.
- **`debug_ui` actions:**
  - `sidebar_state` → `{"favorites": [...], "pinned": [...], "today": [...]}`. Each row is `{"id", "title", "url", "active", "asleep", "playing", "icon": "favicon"|"monogram", "frame": [x,y,w,h], "title_lines": n}`.
  - `sidebar_click` with `text` `"<id>"` or `"<id>:close"` → `performClick` on that row's real button.
  - `window_layout` → `{"mode", "sidebar", "card", "panel", "traffic_lights_hidden", "outline", "toast", "empty_state", "theme"}`.
  - The existing `windows` action adds `"number"` (`windowNumber`) for `screencapture -l`.

- [ ] **Step 1: Write the failing tests** in `layout.rs`:

```rust
#[test] fn docked_default_frames()                          // 1280×800, 240, no panel => sidebar (0,0,240,800), card (240,8,1032,784)
#[test] fn hidden_card_fills_window_with_inset()            // card (8,8,1264,784)
#[test] fn overlay_sidebar_floats_over_card()               // sidebar (8,8,240,784), card as hidden
#[test] fn panel_takes_right_side_with_gap()                // panel (892,8,380,784), card w = 1280-240-8-388
#[test] fn narrow_window_with_panel_shrinks_sidebar_first_and_never_negative() // 800 wide, 360 sidebar, panel => sidebar 200, all w >= 0
#[test] fn sidebar_width_is_clamped()                        // 100 => 200, 500 => 360
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser layout::`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement `layout.rs`.** Run `cargo test -p my-browser layout::`. Expected: PASS.

- [ ] **Step 4: Implement `background.rs`, `card.rs`, `sidebar.rs`, `sidebar_row.rs`, and the new `window.rs`.**
  - Delete `nav_bar.rs` and `tab_strip.rs`. Rewire `state.rs`:
    - `refresh` → `layout_sidebar` on `Tabs`/`Active`;
    - `sidebar.update_nav` on `Nav`/`Active`;
    - `card.show_empty(blank.then(|| space name))` on `Active`;
    - `ui.show_notice` for notices.
  - Rewire `actions.rs`: window resize → `ui.relayout()`.
  - Update the `show_notice` callers and the `debug_ui` references.

- [ ] **Step 5: Add the `debug_ui` actions and the CI grep entry, then run all tests**

Run: `scripts/check.sh`
Expected: PASS.

- [ ] **Step 6: E2E and visual check** in a test instance:
  - **Tab list:** launch with `https://example.com/ https://example.org/` plus a local page whose `<title>` is 300 × "🦀". `sidebar_state` lists 3 Today rows; the emoji row has `title_lines == 1` and frame height 28.
  - **Rapid close:** open 10 more tabs (MCP `open_tab`), then send 10 `sidebar_click "<id>:close"` calls back-to-back without waiting. The process is alive, and `sidebar_state` has 3 rows.
  - **Empty state:** close every tab. `window_layout.empty_state == true`, and the address field is focused (`address_state.editing`).
  - **Interaction outline:** `debug_ui menu toggleClaudeInteract:` makes `window_layout.outline` true.
  - **Visual:** `screencapture -x -l <number>` in light and dark system appearance, at 1280×800 and at the 800×500 minimum, with the panel open (⌘J). Compare with the approved mockup layout.
  - **No extra processes:** with 1 awake tab, the WebContent process count is 1.

- [ ] **Step 7: Commit**

```bash
git add -A crates/app .github/workflows/ci.yml Cargo.toml Cargo.lock
git commit -m "feat(app): jendela baru: gradasi, kartu halaman, dan sidebar daftar tab"
```

---

### Task 8: Favorites grid, slot actions, context menu, and the interact pill

**Files:**
- Modify: `crates/app/src/sidebar.rs`, `crates/app/src/sidebar_row.rs`, `crates/app/src/layout.rs`, `crates/app/src/actions.rs`, `crates/app/src/menu.rs`, `crates/app/src/state.rs`, `crates/app/src/debug_ui.rs`

**Interfaces:**
- Consumes: `Browser::{toggle_pin, toggle_favorite, set_pinned_url, request_sleep, close_tab}` (Tasks 1–2); `TabRowView`, `Sidebar::render`, `layout.rs` (Task 7).
- Produces:

```rust
// layout.rs
pub const TILE_MIN: f64 = 44.0;
pub const TILE_GAP: f64 = 6.0;
pub const TILE_HEIGHT: f64 = 40.0;
pub fn tile_columns(inner_width: f64) -> usize;   // max(1, floor((inner + GAP) / (TILE_MIN + GAP)))

// sidebar_row.rs
pub enum RowStyle { Row, Tile }
impl TabRowView { pub fn new(mtm, row: &RowInfo, style: RowStyle, icon: &NSImage, theme: &Theme, target: &ActionTarget) -> Retained<Self> }

// actions.rs: new selectors (sender tag = tab id; tag 0 from the menu bar = active tab)
togglePin:  toggleFavorite:  setPinnedURL:  sleepTab:  copyTabLink:

// state.rs
pub fn tab_action(&self, id: Option<u64>, action: TabAction);
pub enum TabAction { TogglePin, ToggleFavorite, SetPinnedUrl, Sleep, CopyLink, Close }
```

**Favorites, rows, and menus:**
- **Favorites grid:** tiles of `tile_columns(sidebar_width − 16)` columns, wrapping onto more rows.
  - Each tile shows the favicon at 20 pt and a tooltip with the title. The active tile is highlighted (white 28% alpha); an asleep tile shows a small `moon.fill` badge.
  - The grid appears only when there is at least one favorite.
  - The grid sits between the address field and the scroll view and does not scroll. Only Pinned and Today scroll (spec §2).
- **Pinned rows:** the hover button is `minus` (same `closeTabButton:`, which unloads), with the tooltip "Unload Tab".
- **Context menu** (row and tile; `menuForEvent`), in this order:
  - Pin Tab / Unpin Tab
  - Add to Favorites / Remove from Favorites
  - Set Current Page as Pinned URL (Pinned/Favorite only, enabled when `url != saved_url`)
  - Copy Link
  - Sleep Tab (awake only)
  - separator
  - Close Tab (Today) / Unload Tab (Pinned/Favorite)
  - Every item carries tag = tab id.
- **File menu additions** after "Reopen Closed Tab": "Pin Tab" ⌘D (title toggles to "Unpin Tab" in `validateMenuItem`), "Add to Favorites" (toggles "Remove from Favorites"), "Set Current Page as Pinned URL". All are disabled for a blank active tab.
- **Interact pill:** under the top row, visible only while interaction is on. It shows "Claude can interact" and a "Turn Off" button (`toggleClaudeInteract:`). Hiding it relayouts the list.
- **`debug_ui sidebar_menu`** with `text` `"<id>:<item title>"`: builds that row's real context menu and calls `performActionForItemAtIndex` on the matching item.

- [ ] **Step 1: Write the failing tests** in `layout.rs`:

```rust
#[test] fn tile_columns_fit_sidebar_width()   // 224 => 4, 184 => 3, 344 => 6, 10 => 1
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser layout::tile_columns`
Expected: FAIL.

- [ ] **Step 3: Implement the grid, `RowStyle::Tile`, the context menu, the menu items with validation, `tab_action`, the pill, and `sidebar_menu`.**

- [ ] **Step 4: Run all checks**

Run: `scripts/check.sh`
Expected: PASS.

- [ ] **Step 5: E2E** in a test instance with three `example.*` tabs:
  - **Pin:** `sidebar_menu "<A>:Pin Tab"` moves A to `pinned`. ⌘D through `debug_ui menu togglePin:` on B moves B to `pinned` too.
  - **Favorite:** `sidebar_menu "<C>:Add to Favorites"` moves C to `favorites`.
  - **Unload:** `sidebar_click "<A>:close"` keeps A in `pinned` with `asleep: true` and its URL equal to the pinned URL, after navigating A elsewhere first.
  - **Restart:** quit and relaunch. The sections are restored, and only the active tab is awake.
  - **Pill:** `debug_ui menu toggleClaudeInteract:`. The pill is visible in `sidebar_state` (add `"interact_pill": bool`), and `sidebar_click`-ing "Turn Off" hides it and the outline.
  - **Visual:** screenshot with favorites, pinned, and today populated.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): grid favorit, aksi pin/unload, menu konteks, dan penanda interaksi Claude"
```

---

### Task 9: Drag and drop in the sidebar

**Files:**
- Modify: `crates/app/src/sidebar.rs`, `crates/app/src/sidebar_row.rs`, `crates/app/src/layout.rs`, `crates/app/src/state.rs`, `crates/app/src/debug_ui.rs`

**Interfaces:**
- Consumes: `Browser::place_tab(id, kind, at)` (Task 2); `TabRowView`, `tile_columns` (Tasks 7–8).
- Produces:

```rust
// layout.rs
pub fn insertion_index(mids: &[f64], y: f64) -> usize;  // mids = row centers top-to-bottom in flipped coordinates; first index whose mid > y

// state.rs
pub fn place_tab(&self, id: u64, kind: TabKind, at: usize);   // calls Browser::place_tab, then refresh
```

**Drag and drop:**
- **Pasteboard type:** `dev.my-browser.tab` (string tab id).
- **Dragging:** a row or tile starts a dragging session after the pointer moves more than 3 pt with the button down. The source operation mask is Move; the image is the row snapshot.
- **Drop targets:** the list document view (Pinned and Today regions) and the favorites grid (also accepted when it is empty, as a 40 pt drop zone shown only during a drag). Both are registered for the type.
- **During a drag:** a 2 pt white insertion line at `insertion_index`. In the grid, the insertion index is `row * columns + column` from the point.
- **On drop:** `state.place_tab(id, kind_of_region, index)`.
- **`debug_ui sidebar_drop`** with `text` `"<id>:<favorite|pinned|today>:<at>"`: calls the same handler that `performDragOperation` calls.

- [ ] **Step 1: Write the failing tests** in `layout.rs`:

```rust
#[test] fn insertion_index_picks_gap_under_pointer()   // mids [14, 42, 70]: y 0 => 0, y 30 => 1, y 69 => 2, y 200 => 3
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p my-browser layout::insertion_index`
Expected: FAIL.

- [ ] **Step 3: Implement the dragging source, the destinations, the insertion line, `place_tab`, and `sidebar_drop`.**

- [ ] **Step 4: Run all checks**

Run: `scripts/check.sh`
Expected: PASS.

- [ ] **Step 5: E2E plus a manual check:**
  - `sidebar_drop "<C>:pinned:0"` makes C the first pinned tab.
  - `sidebar_drop "<A>:today:1"` makes A the second Today tab, with no saved URL.
  - `sidebar_drop "<B>:favorite:0"` makes B the first favorite.
  - Manually drag a row with the mouse once in each direction (Today → Pinned → Favorites → Today) and confirm the line and the result.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): seret dan lepas tab antarbagian sidebar"
```

---

### Task 10: Hidden sidebar, hover overlay, and resizing

**Files:**
- Modify: `crates/app/src/window.rs`, `crates/app/src/sidebar.rs`, `crates/app/src/actions.rs`, `crates/app/src/menu.rs`, `crates/app/src/state.rs`, `crates/app/src/debug_ui.rs`

**Interfaces:**
- Consumes: `layout(…, SidebarMode, …)`, `SIDEBAR_MIN/MAX` (Task 7); `Settings::{sidebar_width, sidebar_visible}` and `Browser::update_settings` (Task 4).
- Produces:

```rust
// window.rs
impl Ui {
    pub fn set_sidebar_visible(&self, visible: bool);   // Docked <-> Hidden; traffic lights hidden when Hidden
    pub fn show_overlay(&self);                          // Hidden -> Overlay (traffic lights shown)
    pub fn hide_overlay(&self);                          // Overlay -> Hidden
    pub fn sidebar_mode(&self) -> SidebarMode;
}
// actions.rs: new selector toggleSidebar:  (View menu "Hide Sidebar"/"Show Sidebar", ⌘S, title toggled in validateMenuItem)
```

**Behavior:**
- **Hover edge:** a tracking area over the leftmost 8 pt of the window (only in Hidden mode). After 150 ms of hover, `show_overlay()`.
- **Overlay:** the sidebar view is re-parented above the card, with its own gradient layer, radius 10, and a shadow. It slides in over 180 ms, or appears instantly when `accessibilityDisplayShouldReduceMotion` is true. `mouseExited` of the overlay → `hide_overlay()`.
- **Resize handle:** a 4 pt view on the sidebar's right edge (Docked only), with the `resizeLeftRight` cursor. Dragging sets the width live (clamped 200–360). `mouseUp` persists `sidebar_width` through `update_settings`.
- **Persistence:** ⌘S toggles and persists `sidebar_visible`. Launch honors both settings.
- **`debug_ui` actions:** `sidebar_hover_edge` (runs the same handler as the tracking area after the delay), `sidebar_leave_overlay`, and `sidebar_resize` with `text` `"<width>"` (runs the same code as `mouseUp` at that width). `window_layout.mode` reports the mode.

- [ ] **Step 1: Write the failing test** in `crates/core/src/settings.rs`:

```rust
#[test] fn sidebar_settings_round_trip()   // save with 300.0/false, load => 300.0/false
```

Layout is already covered by Task 7.

- [ ] **Step 2: Run the test to verify it fails or passes**

Run: `cargo test -p my-browser-core settings::sidebar_settings_round_trip`
Expected: PASS is acceptable if Task 4 already covered serialization. The test pins it either way. If it fails, fix the setting serialization.

- [ ] **Step 3: Implement the modes, the tracking area, the overlay, the resize handle, the menu item, and the `debug_ui` actions.**

- [ ] **Step 4: Run all checks**

Run: `scripts/check.sh`
Expected: PASS.

- [ ] **Step 5: E2E:**
  - **Hide:** `debug_ui menu toggleSidebar:` gives `mode == "hidden"`, `traffic_lights_hidden == true`, and card x == 8.
  - **Overlay:** `sidebar_hover_edge` gives `mode == "overlay"` with the card unchanged. `sidebar_leave_overlay` returns to hidden.
  - **Show again:** toggling again gives docked.
  - **Resize:** `sidebar_resize "320"` sets the sidebar width to 320. After a relaunch, both the width (320) and the visibility are restored.
  - **Manual:** check the hover slide-in with the mouse, and with Reduce Motion on.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src crates/core/src/settings.rs
git commit -m "feat(app): sembunyikan sidebar (⌘S), overlay saat hover, dan ubah lebar"
```

---

### Task 11: MCP `kind`, documentation, the memory and CPU gate, and the version

**Files:**
- Modify:
  - `crates/app/src/control.rs` (`TabInfo` gets `kind`), `crates/mcp/src/tools.rs` (the `list_tabs` description mentions sections, and that `close_tab` on a pinned or favorite tab unloads it);
  - `docs/mcp.md`, `README.md` (Features: sidebar, favorites, Pinned/Today, favicons; Roadmap: replace "Bookmarks" with the redesign stages 2–4);
  - `docs/testing/manual-checklist.md`, `docs/testing/measurements.md`;
  - `resources/Info.plist` (0.2.0, build 3), `Cargo.toml` (workspace version 0.2.0), `Cargo.lock`.
- Create: `docs/sidebar.md` (English user doc: layout, sections, unload versus sleep, shortcuts ⌘D/⌘S/⌘L, migration note that `session.json` is kept for older builds).

**Interfaces:**
- Consumes: everything above.
- Produces: `list_tabs` entries with `"kind": "favorite" | "pinned" | "today"`.

- [ ] **Step 1: Write the failing test** for `TabInfo` serialization in `crates/app/src/control.rs` tests (or the existing control test module):

```rust
#[test] fn tab_info_includes_kind()   // serde_json of a pinned tab info contains "\"kind\":\"pinned\""
```

- [ ] **Step 2: Run the test to verify it fails, implement `kind`, and run it again**

Run: `cargo test -p my-browser tab_info_includes_kind`
Expected: FAIL, then PASS.

- [ ] **Step 3: Measure** with the method from `docs/testing/measurements.md` (`scripts/ukur-memory.sh --detail my-browser`):
  - **Baseline:** build `main` in a temporary worktree (`git worktree add "$TMPDIR/mb-base" main`).
  - **(a) Memory:** 20 `example.*`/Wikipedia tabs, `sleep_idle_secs` 20, measured after the sleep settles. App-process footprint, new build ≤ baseline + 15 MB.
  - **(b) Processes:** the WebContent count equals the number of awake tabs.
  - **(c) CPU:** the same 1080p YouTube video for 2 minutes in each build; average CPU of app + WebContent + GPU from `top -l 10 -s 2`. Flag anything above +10% relative.
  - Append the table and the date to `measurements.md`. Remove the worktree afterwards.
  - If a gate fails, stop and report. Do not tune the gate.

- [ ] **Step 4: Write the docs and bump the version** (Info.plist `CFBundleShortVersionString` 0.2.0, `CFBundleVersion` 3, workspace `version = "0.2.0"`, `cargo update --workspace --offline`).

- [ ] **Step 5: Final verification**

Run: `scripts/check.sh && scripts/bundle.sh && ! strings dist/my-browser.app/Contents/MacOS/my-browser | grep -qE 'debug_ui|SELFTEST|sheet_button|sidebar_state'`
Expected: all pass, and grep finds nothing.

- [ ] **Step 6: Commit** (no tag; releasing is the owner's call)

```bash
git add -A docs README.md crates resources/Info.plist Cargo.toml Cargo.lock
git commit -m "docs: sidebar dan tahap 1, kind di list_tabs, pengukuran, versi 0.2.0"
```

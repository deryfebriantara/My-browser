# Manual Checklist

Run against the bundle build (`scripts/run.sh`). Failed checkboxes get a note explaining why.

## Task 1: Prototype and Google login GATE

Date: 2026-11-15. macOS 15.7.7, Safari 18.6, arm64.

- [x] The window opens and the YouTube page appears
- [x] Signing in through YouTube's "Sign in" button (password and phone code) is accepted by Google
- [x] Videos play
- [x] The YouTube full screen button works
- [x] After quitting (⌘Q) and reopening (`open dist/my-browser.app`), the user is still signed in

Note: checked by the user on 2026-11-15, everything worked. GATE passed.

## Task 8: App UI

- [ ] The window opens at 1280x800; it cannot be resized below 800x500
- [ ] Sidebar on the left (window buttons, back/forward/reload, ✦, address, tabs), page card on the right (see "Sidebar and page card" below)
- [ ] Closing the window quits the app
- [ ] The back/forward buttons are enabled only when there is history
- [ ] The reload icon changes to a cross while the page is loading
- [ ] The address field shows the active tab's URL, and is empty for a blank tab
- [ ] Typing `youtube.com` and pressing Enter loads YouTube
- [ ] Typing `resep nasi goreng` and pressing Enter opens a Google search
- [ ] Escape in the address field restores the active tab's URL
- [ ] An active blank tab focuses the address field immediately
- [ ] ⌘V in the address field pastes text
- [ ] Tab strip: a title per tab (truncated when long), a close button per tab, a plus button at the end
- [ ] The active tab is marked
- [ ] Many tabs: the strip scrolls horizontally
- [ ] The window title follows the active tab's title
- [ ] A `target=_blank` link opens a new tab next to the current one and activates it
- [ ] The YouTube full screen button works
- [ ] File menu: New Tab ⌘T, Close Tab ⌘W, Reopen Closed Tab ⇧⌘T, Open Location ⌘L
- [ ] View menu: Reload Page ⌘R, Enter Full Screen ⌃⌘F
- [ ] History menu: Back ⌘[, Forward ⌘]
- [ ] Window menu: Next Tab ⌃⇥ and ⇧⌘], Previous Tab ⌃⇧⇥ and ⇧⌘[, ⌘1 through ⌘9

## Task 9: Tab sleep, memory pressure, session

For manual testing, quit the browser first (⌘Q), set `"sleep_idle_secs": 20` in `~/Library/Application Support/my-browser/settings.json`, then reopen it. When done, set it back to 1800 (the default, 30 minutes) the same way.

- [ ] (a) Open 4 tabs with URLs, activate one, wait 60 seconds: background tabs appear asleep (dimmed, tooltip "Sleeping; reloads when opened")
- [ ] (b) A YouTube video playing in a background tab: that tab never sleeps
- [ ] (c) A YouTube video paused at minute 2, put to sleep, then opened: it resumes from around minute 2
- [ ] (d) A page scrolled far down, put to sleep, then opened: the scroll position is restored
- [ ] (e) The user runs `sudo memory_pressure -S -l warn`: all background tabs go to sleep within a few seconds, except tabs with unsent text
- [ ] (f) ⌘Q and reopen: all tabs come back, and only the active tab loads
- [ ] (g) Type a comment on YouTube or in any form without submitting it, switch tabs, wait past the sleep limit: that tab stays awake and the comment is intact (fixed 2026-11-21)
- [ ] (h) The comment from (g) is submitted or deleted: that tab goes to sleep again on the next round
- [ ] (i) Type in the YouTube search box and pick a suggestion (without Enter): the tab can still sleep
- [ ] (j) `sudo memory_pressure -S -l critical`: the tab from (g) is put to sleep as well

## Task 10: Ad blocker

- [ ] https://www.detik.com shows fewer ads than Safari without a content blocker
- [ ] View > Disable Blocker for This Site: the page reloads and ads appear again; the menu checkmark changes too
- [ ] That menu item is disabled on a blank tab
- [ ] YouTube still plays videos and the sign-in persists
- [ ] Skippable YouTube video ads show the "Skip" button (fixed 2026-11-20)
- [ ] github.com and Gmail render fully (generic hide rules do not apply on generichide sites)
- [ ] Automatic YouTube ad skipping: skippable ads disappear in about 1 second; unskippable ads are muted and finish in 1-2 seconds
- [ ] After an ad, the video speed (e.g. 1.5x) and sound return to what they were before
- [ ] View > Disable Blocker for This Site on YouTube: ads play normally again; turn it back on: ads are skipped again

## Task 11: Downloads

Already verified automatically (temporary Downloads folder): two simultaneous downloads with the same name become `gitignore-main.zip` and `gitignore-main (1).zip`; a tab whose navigation turns into a download is not saved in the session and does not download again when the app reopens.

- [ ] Downloading a PDF through a `download` link twice produces `nama.pdf` and `nama (1).pdf` in `~/Downloads`
- [ ] The "Downloaded: ..." notice appears at the top of the page card and disappears after about 6 seconds

## Fixes after the final review

- [ ] Blank tab (the first tab or ⌘T): typing a URL and pressing Enter shows the page immediately
- [ ] Click tabs repeatedly (including the already active tab) and close tabs with the cross button: no crash, and the active tab marker stays correct
- [ ] File upload (e.g. an image upload form on any site): the file picker appears
- [ ] `alert`, `confirm`, and `prompt` dialogs appear (e.g. typing `javascript` does not work in the address bar; use a test site such as https://www.w3schools.com/js/tryit.asp?filename=tryjs_alert)
- [ ] Opening `localhost:59999` (server down): "Couldn't load page: ..." appears at the top of the page card
- [ ] Start a download and close its tab immediately: the "Downloaded" notice still appears
- [ ] Blocker exception from a subdomain (e.g. enable it on www.detik.com, open news.detik.com, disable it again): the blocker is really active again

## Claude MCP

Reopen the browser with the new build (⌘Q, then `open dist/my-browser.app`), then start a new Claude Code session.

- [ ] `/mcp` in Claude Code shows my-browser as connected
- [ ] Ask Claude to "open youtube.com in a new tab": the tab opens in the browser
- [ ] Ask Claude to read the active page and take a screenshot
- [ ] Ask Claude to click something: it is refused with a message to turn on the permission
- [ ] Turn on Claude > Allow Claude to Interact: the window title is marked, the click succeeds, and the notice "Claude: clicking element ..." appears
- [ ] Turn it off again: clicks are refused again
- [ ] Quit the browser (⌘Q), then ask Claude to list the tabs: the browser opens automatically
- [ ] Ask Claude to build a small website, run it on localhost, and check it: Claude uses console_logs, network_log, and screenshot, then fixes the errors it finds
- [ ] Choose Claude > Pick Element for Claude (⌘⇧E) and move the cursor: elements are highlighted with a size label; click a button: the button does not fire, and a "Selected" marker appears briefly. Then ask Claude to "look at the element I selected": Claude names the correct element and can see its image
- [ ] Pick Element for Claude mode, then press Esc: the highlight disappears and the next click works normally
- [ ] Ask Claude to "check the phone view and dark mode": the page appears phone-width at the top center of the window, and the notice "Claude: phone 390×844, dark mode" appears; after Claude is done (or through the Claude > Reset Device Emulation menu) the view returns to full size
- [ ] The Reset Device Emulation menu item is disabled (grayed out) on tabs that are not emulated
- [ ] Ask Claude to "screenshot the whole page" on a long page: Claude receives several images and can describe the bottom of the page
- [ ] Ask Claude to use check_page after changing a local site's code: one call returns an error summary, failed requests, and a screenshot

## Ask Claude

Reopen the browser with the new build (⌘Q, then `open dist/my-browser.app`).

- [ ] The ✦ button at the top right of the sidebar and ⌘J open the panel as a second card on the right; the page narrows (it is not covered by the panel); ⌘J again closes it
- [ ] On YouTube or an article, ask "summarize this page": the answer streams in, and the label "Reading page" appears above the answer
- [ ] Point at an element (⌘⇧E), then ask "explain the element I pointed at": Claude discusses the correct element
- [ ] Ask "open youtube.com": Claude refuses, and no new tab opens
- [ ] Close the panel while Claude is answering: the answer stops; reopen it: the history is still there and the next question continues the previous conversation
- [ ] Activity Monitor: the `claude` process exists only while the panel is open and has been used
- [ ] Click a link in Claude's answer: it opens in a new tab
- [ ] macOS light and dark mode: the panel changes color to match

## Password manager

Reopen the browser with the new build (⌘Q, then `open dist/my-browser.app`).

- [ ] Save on the save offer: the next dialog (Create Master Password or Unlock Passwords) appears immediately, and the window is not locked without a dialog (fixed 2026-11-25)
- [ ] Sign in on a site: "Save Password for …?" appears; Save asks you to create a master password (refused if < 10 characters or the two fields differ)
- [ ] Sign out and open the login page again, press ⌘\: the username and password are filled in, and the site accepts the login
- [ ] Passwords > Lock Passwords, then ⌘\: the master password is requested; a wrong master password is refused with a message, and the correct one fills immediately
- [ ] Sign in with a different password: "Update Password for …?" appears
- [ ] "Never for This Site": the next sign-in on that site does not ask again
- [ ] Passwords > Manage Passwords…: search, Show (hidden again after 20 seconds), Copy (clipboard cleared again after 30 seconds), Edit, Delete (asks for confirmation), Add with Generate
- [ ] Import CSV from Chrome or the Passwords app: the imported count is correct, and a reminder to delete the CSV file appears
- [ ] Export CSV: asks for the master password again; a wrong master password cancels the export
- [ ] Change the master password: a wrong old master password is refused; after the change, locking and unlocking with the new one works and the old one is refused
- [ ] Lock the Mac screen (Ctrl+Cmd+Q) and unlock it: passwords are locked
- [ ] Leave passwords unused for 10 minutes: they lock on their own
- [ ] Google two-step sign-in: ⌘\ at the email step, then at the password step

## MCP program permissions

Reopen the browser with the new build (⌘Q, then `open dist/my-browser.app`).

- [ ] Start a new Claude Code session and ask Claude to list the tabs: my-browser comes to the front with the dialog "Allow Claude Code to control my-browser?", which shows that session's project folder
- [ ] Always Allow: Claude gets the tab list immediately; the next Claude Code session is not asked again
- [ ] Claude > Revoke MCP Client Access: the next request is asked again
- [ ] Deny: Claude gets a refusal message; subsequent requests are refused immediately until the browser is reopened
- [ ] The Ask Claude panel can still read the page without a permission dialog
- [ ] The panel shows the email, plan, and profile below the title; choosing a profile switches the account and starts a new conversation
- [ ] Choose a profile that is not signed in (e.g. ~/.claude-flash): the Sign In to Claude card appears; Sign In with Claude Subscription opens the login page in a my-browser tab; after approval the login tab closes on its own and the panel is ready to use
- [ ] Cancel while waiting for login: back to the Sign In to Claude card
- [ ] Sign Out: asks for confirmation with a warning that Claude Code in Terminal with the same profile is signed out too

## History and address bar suggestions

Reopen the browser with the new build (⌘Q, then `open dist/my-browser.app`). History is recorded starting with this build.

- [ ] Open a few sites, then type the beginning of an address you typed earlier (e.g. `you`): the address is completed with the added part selected; typing further replaces it, and Backspace deletes it
- [ ] The suggestion list appears neatly below the address bar in light and dark mode, and moves along when the window is moved
- [ ] ↑ ↓ select a suggestion and fill the address bar; Enter opens it; a mouse click on a row also opens it
- [ ] The first Escape closes the list, the second Escape restores the tab's address
- [ ] Type the title of another open tab: "Switch to tab" appears; choosing it switches to that tab without reloading
- [ ] Click several videos on YouTube (without reloading the page): each video appears in History with the correct title
- [ ] A video put to sleep and opened again does not create a new entry (no `&t=` in History)
- [ ] ⌘Y opens the History window grouped by day; search works, ⌘-click opens in a new tab, ✕ deletes a single page
- [ ] History > Clear History… > Last hour: the pages from earlier disappear from History and from the address bar suggestions
- [ ] A Japanese IME (or another input method that composes characters) in the address bar is not autocompleted while composing
- [ ] VoiceOver reads the suggestion rows aloud
- [ ] Ask Claude "what YouTube videos did I watch today?": Claude uses search_history

## Sidebar and page card

- [ ] The window has no title bar; the window buttons sit at the top of the sidebar, level with back/forward/reload and ✦
- [ ] The empty areas of the sidebar and the margins around the card drag the window; double-click zooms
- [ ] The page card has rounded corners and a soft shadow; video in the card (1080p YouTube) plays smoothly
- [ ] CPU check: play the same 1080p YouTube video for 2 minutes in 0.1.1 and in this build (window visible, no other load) and compare my-browser + WebContent + WindowServer CPU in Activity Monitor; flag more than +10%
- [ ] System appearance light and dark: the card follows the system, the sidebar stays readable on the gradient
- [ ] Increase Contrast on: the gradient becomes a solid dark color
- [ ] Tab rows: favicon (or a colored letter), one-line title truncated at the end, the active row highlighted, sleeping rows dimmed with a moon, a speaker icon while audio plays
- [ ] Hovering a row shows ✕ (Today) or – (Pinned); clicking it closes or unloads the tab
- [ ] With a real mouse, click ✕ several times in a row without moving the pointer: each click closes the next tab (none gets selected)
- [ ] Full screen (⌃⌘F): the sidebar keeps its layout (buttons, address, list in place)
- [ ] Right-click a row or favorite tile: Pin/Unpin, Add to/Remove from Favorites, Set Current Page as Pinned URL, Copy Link, Sleep Tab, Close/Unload
- [ ] ⌘D pins and unpins the current tab; the File menu titles change accordingly
- [ ] Drag a row with the mouse: Today → Pinned → Favorites → Today, and reorder within a section; a white line shows where it lands; with no favorites yet, an empty drop zone appears above the list while dragging
- [ ] The sidebar button in the top row hides the sidebar; in the floating overlay the same button keeps it open
- [ ] Clicking the address in the sidebar opens the wide address bar over the page with the full URL and suggestions below; it looks right on light and dark pages and closes after Enter, a click anywhere outside it (page, sidebar, back/reload), or a tab switch; reopening it shows the tab's address, not abandoned text
- [ ] With the sidebar hidden, ⌘L and ⌘T bring it in as an overlay with the wide address bar focused; it stays while typing and hides after Enter or a click outside the bar (Escape alone keeps editing)
- [ ] ⌘S hides the sidebar (window buttons hide too); resting the pointer on the left edge slides it in as a floating panel; moving away hides it again; with Reduce Motion on, it appears without sliding
- [ ] Dragging the sidebar's right edge resizes it (200–360 pt); the width and the hidden state survive a relaunch
- [ ] Allow Claude to Interact on: orange outline around the card and a "Claude can interact" pill with Turn Off in the sidebar
- [ ] VoiceOver reads a row as "title, host[, asleep][, playing audio]"
- [ ] First launch after upgrading: all previous tabs appear under Today; `session.json` is unchanged so an older build still opens it


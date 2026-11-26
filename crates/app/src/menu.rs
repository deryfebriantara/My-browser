//! Menu utama aplikasi (teks Inggris).

use crate::actions::ActionTarget;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

struct Item<'a> {
    title: &'a str,
    action: Sel,
    key: &'a str,
    mods: NSEventModifierFlags,
    /// `None`: dikirim ke first responder (Edit, Keluar, Layar Penuh).
    target: Option<&'a ActionTarget>,
    tag: isize,
}

fn make(mtm: MainThreadMarker, spec: &Item) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(spec.title),
            Some(spec.action),
            &NSString::from_str(spec.key),
        )
    };
    item.setKeyEquivalentModifierMask(spec.mods);
    if let Some(target) = spec.target {
        unsafe { item.setTarget(Some(target)) };
    }
    item.setTag(spec.tag);
    item
}

/// Item tanpa judul menjadi garis pemisah.
fn submenu(mtm: MainThreadMarker, title: &str, items: &[Item]) -> Retained<NSMenuItem> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for spec in items {
        if spec.title.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        } else {
            menu.addItem(&make(mtm, spec));
        }
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    holder
}

pub fn build(mtm: MainThreadMarker, target: &ActionTarget) -> Retained<NSMenu> {
    let cmd = NSEventModifierFlags::Command;
    let cmd_shift = NSEventModifierFlags::Command | NSEventModifierFlags::Shift;
    let ctrl = NSEventModifierFlags::Control;
    let ctrl_shift = NSEventModifierFlags::Control | NSEventModifierFlags::Shift;
    let ctrl_cmd = NSEventModifierFlags::Control | NSEventModifierFlags::Command;
    let t = Some(target);
    let it = |title, action, key, mods, target, tag| Item { title, action, key, mods, target, tag };

    let main = NSMenu::new(mtm);
    main.addItem(&submenu(mtm, "my-browser", &[it("Quit my-browser", sel!(terminate:), "q", cmd, None, 0)]));
    main.addItem(&submenu(
        mtm,
        "File",
        &[
            it("New Tab", sel!(newTab:), "t", cmd, t, 0),
            it("Close Tab", sel!(closeActiveTab:), "w", cmd, t, 0),
            it("Reopen Closed Tab", sel!(reopenClosedTab:), "t", cmd_shift, t, 0),
            it("", sel!(goBack:), "", cmd, t, 0),
            it("Pin Tab", sel!(togglePin:), "d", cmd, t, 0),
            it("Add to Favorites", sel!(toggleFavorite:), "", cmd, t, 0),
            it("Set Current Page as Pinned URL", sel!(setPinnedURL:), "", cmd, t, 0),
            it("", sel!(goBack:), "", cmd, t, 0),
            it("Open Location", sel!(focusAddress:), "l", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Edit",
        &[
            it("Undo", sel!(undo:), "z", cmd, None, 0),
            it("Redo", sel!(redo:), "z", cmd_shift, None, 0),
            it("Cut", sel!(cut:), "x", cmd, None, 0),
            it("Copy", sel!(copy:), "c", cmd, None, 0),
            it("Paste", sel!(paste:), "v", cmd, None, 0),
            it("Select All", sel!(selectAll:), "a", cmd, None, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "View",
        &[
            it("Reload Page", sel!(reloadPage:), "r", cmd, t, 0),
            it("Hide Sidebar", sel!(toggleSidebar:), "s", cmd, t, 0),
            it("Enter Full Screen", sel!(toggleFullScreen:), "f", ctrl_cmd, None, 0),
            it("Disable Blocker for This Site", sel!(toggleBlocker:), "", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "History",
        &[
            it("Back", sel!(goBack:), "[", cmd, t, 0),
            it("Forward", sel!(goForward:), "]", cmd, t, 0),
            it("", sel!(goBack:), "", cmd, t, 0),
            it("Show All History", sel!(showHistory:), "y", cmd, t, 0),
            it("Clear History…", sel!(clearHistory:), "", cmd, t, 0),
        ],
    ));

    let opt_cmd = NSEventModifierFlags::Option | NSEventModifierFlags::Command;
    main.addItem(&submenu(
        mtm,
        "Passwords",
        &[
            it("Fill Password", sel!(fillPassword:), "\\", cmd, t, 0),
            it("Manage Passwords…", sel!(openPasswordManager:), "p", opt_cmd, t, 0),
            it("Copy Random Password", sel!(copyGeneratedPassword:), "", cmd, t, 0),
            it("Lock Passwords", sel!(lockPasswords:), "", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Claude",
        &[
            it("Ask Claude", sel!(toggleChat:), "j", cmd, t, 0),
            it("Pick Element for Claude", sel!(pickElement:), "e", cmd_shift, t, 0),
            it("Reset Device Emulation", sel!(resetEmulation:), "", cmd, t, 0),
            it("Allow Claude to Interact", sel!(toggleClaudeInteract:), "", cmd, t, 0),
            it("Revoke MCP Client Access", sel!(revokeMcpClients:), "", cmd, t, 0),
        ],
    ));

    let mut window_items = vec![
        it("Next Tab", sel!(nextTab:), "\t", ctrl, t, 0),
        it("Next Tab", sel!(nextTab:), "]", cmd_shift, t, 0),
        it("Previous Tab", sel!(previousTab:), "\t", ctrl_shift, t, 0),
        it("Previous Tab", sel!(previousTab:), "[", cmd_shift, t, 0),
    ];
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const TITLES: [&str; 9] = ["Tab 1", "Tab 2", "Tab 3", "Tab 4", "Tab 5", "Tab 6", "Tab 7", "Tab 8", "Last Tab"];
    for i in 0..9 {
        window_items.push(it(TITLES[i], sel!(selectTabIndex:), DIGITS[i], cmd, t, i as isize));
    }
    main.addItem(&submenu(mtm, "Window", &window_items));
    main
}

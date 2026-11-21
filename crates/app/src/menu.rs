//! Menu utama aplikasi (teks Indonesia).

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

fn submenu(mtm: MainThreadMarker, title: &str, items: &[Item]) -> Retained<NSMenuItem> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for spec in items {
        menu.addItem(&make(mtm, spec));
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
    main.addItem(&submenu(mtm, "my-browser", &[it("Keluar my-browser", sel!(terminate:), "q", cmd, None, 0)]));
    main.addItem(&submenu(
        mtm,
        "Berkas",
        &[
            it("Tab Baru", sel!(newTab:), "t", cmd, t, 0),
            it("Tutup Tab", sel!(closeActiveTab:), "w", cmd, t, 0),
            it("Buka Lagi Tab yang Ditutup", sel!(reopenClosedTab:), "t", cmd_shift, t, 0),
            it("Buka Alamat", sel!(focusAddress:), "l", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Edit",
        &[
            it("Urungkan", sel!(undo:), "z", cmd, None, 0),
            it("Ulangi", sel!(redo:), "z", cmd_shift, None, 0),
            it("Potong", sel!(cut:), "x", cmd, None, 0),
            it("Salin", sel!(copy:), "c", cmd, None, 0),
            it("Tempel", sel!(paste:), "v", cmd, None, 0),
            it("Pilih Semua", sel!(selectAll:), "a", cmd, None, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Tampilan",
        &[
            it("Muat Ulang", sel!(reloadPage:), "r", cmd, t, 0),
            it("Layar Penuh", sel!(toggleFullScreen:), "f", ctrl_cmd, None, 0),
            it("Matikan Pemblokir untuk Situs Ini", sel!(toggleBlocker:), "", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Riwayat",
        &[it("Mundur", sel!(goBack:), "[", cmd, t, 0), it("Maju", sel!(goForward:), "]", cmd, t, 0)],
    ));

    let opt_cmd = NSEventModifierFlags::Option | NSEventModifierFlags::Command;
    main.addItem(&submenu(
        mtm,
        "Password",
        &[
            it("Isi Password", sel!(fillPassword:), "\\", cmd, t, 0),
            it("Kelola Password…", sel!(openPasswordManager:), "p", opt_cmd, t, 0),
            it("Salin Password Acak", sel!(copyGeneratedPassword:), "", cmd, t, 0),
            it("Kunci Password", sel!(lockPasswords:), "", cmd, t, 0),
        ],
    ));
    main.addItem(&submenu(
        mtm,
        "Claude",
        &[
            it("Tanya Claude", sel!(toggleChat:), "j", cmd, t, 0),
            it("Tunjuk Elemen ke Claude", sel!(pickElement:), "e", cmd_shift, t, 0),
            it("Kembalikan Tampilan Normal", sel!(resetEmulation:), "", cmd, t, 0),
            it("Izinkan Claude Berinteraksi", sel!(toggleClaudeInteract:), "", cmd, t, 0),
            it("Cabut Izin Program MCP", sel!(revokeMcpClients:), "", cmd, t, 0),
        ],
    ));

    let mut window_items = vec![
        it("Tab Berikutnya", sel!(nextTab:), "\t", ctrl, t, 0),
        it("Tab Berikutnya", sel!(nextTab:), "]", cmd_shift, t, 0),
        it("Tab Sebelumnya", sel!(previousTab:), "\t", ctrl_shift, t, 0),
        it("Tab Sebelumnya", sel!(previousTab:), "[", cmd_shift, t, 0),
    ];
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const TITLES: [&str; 9] = ["Tab 1", "Tab 2", "Tab 3", "Tab 4", "Tab 5", "Tab 6", "Tab 7", "Tab 8", "Tab Terakhir"];
    for i in 0..9 {
        window_items.push(it(TITLES[i], sel!(selectTabIndex:), DIGITS[i], cmd, t, i as isize));
    }
    main.addItem(&submenu(mtm, "Jendela", &window_items));
    main
}

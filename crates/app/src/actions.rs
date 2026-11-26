//! Target aksi Objective-C untuk tombol, menu, address field, dan window.
//! Semua handler hanya meneruskan ke `AppState`.

use crate::state::{AppState, TabAction};
use browser_core::tabs::TabKind;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSControl, NSControlStateValueOff, NSControlStateValueOn, NSControlTextEditingDelegate, NSMenuItem,
    NSMenuItemValidation, NSTextFieldDelegate, NSTextView, NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use std::rc::{Rc, Weak};

pub struct TargetIvars {
    state: Weak<AppState>,
}

/// Tab dari tag pengirim (menu konteks baris); tag 0 (menu bar) berarti tab aktif.
fn tab_from(sender: Option<&AnyObject>) -> Option<u64> {
    let tag = tag_of(sender);
    (tag > 0).then_some(tag as u64)
}

fn tag_of(sender: Option<&AnyObject>) -> isize {
    match sender {
        Some(s) => unsafe { msg_send![s, tag] },
        None => 0,
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBActionTarget"]
    #[ivars = TargetIvars]
    pub struct ActionTarget;

    unsafe impl NSObjectProtocol for ActionTarget {}

    impl ActionTarget {
        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.new_tab());
        }

        #[unsafe(method(closeActiveTab:))]
        fn close_active_tab(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.close_active_tab());
        }

        #[unsafe(method(closeTabButton:))]
        fn close_tab_button(&self, sender: Option<&AnyObject>) {
            let id = tag_of(sender) as u64;
            self.with(|s| s.close_tab(id));
        }

        #[unsafe(method(togglePin:))]
        fn toggle_pin(&self, sender: Option<&AnyObject>) {
            let id = tab_from(sender);
            self.with(|s| s.tab_action(id, TabAction::TogglePin));
        }

        #[unsafe(method(toggleFavorite:))]
        fn toggle_favorite(&self, sender: Option<&AnyObject>) {
            let id = tab_from(sender);
            self.with(|s| s.tab_action(id, TabAction::ToggleFavorite));
        }

        #[unsafe(method(setPinnedURL:))]
        fn set_pinned_url(&self, sender: Option<&AnyObject>) {
            let id = tab_from(sender);
            self.with(|s| s.tab_action(id, TabAction::SetPinnedUrl));
        }

        #[unsafe(method(sleepTab:))]
        fn sleep_tab(&self, sender: Option<&AnyObject>) {
            let id = tab_from(sender);
            self.with(|s| s.tab_action(id, TabAction::Sleep));
        }

        #[unsafe(method(copyTabLink:))]
        fn copy_tab_link(&self, sender: Option<&AnyObject>) {
            let id = tab_from(sender);
            self.with(|s| s.tab_action(id, TabAction::CopyLink));
        }

        #[unsafe(method(selectTab:))]
        fn select_tab(&self, sender: Option<&AnyObject>) {
            let id = tag_of(sender) as u64;
            self.with(|s| s.select_tab(id));
        }

        #[unsafe(method(selectTabIndex:))]
        fn select_tab_index(&self, sender: Option<&AnyObject>) {
            let index = tag_of(sender) as usize;
            self.with(|s| s.select_tab_index(index));
        }

        #[unsafe(method(nextTab:))]
        fn next_tab(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.next_tab());
        }

        #[unsafe(method(previousTab:))]
        fn previous_tab(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.previous_tab());
        }

        #[unsafe(method(reopenClosedTab:))]
        fn reopen_closed_tab(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.reopen_closed_tab());
        }

        #[unsafe(method(showHistory:))]
        fn show_history(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.history_window.open(s));
        }

        #[unsafe(method(clearHistory:))]
        fn clear_history(&self, _sender: Option<&AnyObject>) {
            self.with(crate::history_window::clear_dialog);
        }

        /// Baris dropdown saran address bar diklik (tag = nomor baris).
        #[unsafe(method(suggestionClicked:))]
        fn suggestion_clicked(&self, sender: Option<&AnyObject>) {
            let index = tag_of(sender) as usize;
            self.with(|s| s.address.clicked(s, index));
        }

        #[unsafe(method(focusAddress:))]
        fn focus_address(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.focus_address());
        }

        #[unsafe(method(addressEntered:))]
        fn address_entered(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.navigate_from_address());
        }

        #[unsafe(method(reloadOrStop:))]
        fn reload_or_stop(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.reload_or_stop());
        }

        #[unsafe(method(reloadPage:))]
        fn reload_page(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.reload_or_stop());
        }

        #[unsafe(method(goBack:))]
        fn go_back(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.go_back());
        }

        #[unsafe(method(goForward:))]
        fn go_forward(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.go_forward());
        }

        #[unsafe(method(toggleBlocker:))]
        fn toggle_blocker(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.toggle_blocker());
        }

        #[unsafe(method(fillPassword:))]
        fn fill_password(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.passwords.fill_active(s));
        }

        #[unsafe(method(openPasswordManager:))]
        fn open_password_manager(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.password_manager.open(s));
        }

        #[unsafe(method(copyGeneratedPassword:))]
        fn copy_generated_password(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.passwords.copy_generated(s));
        }

        #[unsafe(method(lockPasswords:))]
        fn lock_passwords(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.passwords.lock(s, true));
        }

        #[unsafe(method(revokeMcpClients:))]
        fn revoke_mcp_clients(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.mcp_access.revoke_all(s));
        }

        #[unsafe(method(toggleChat:))]
        fn toggle_chat(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.chat.toggle(s));
        }

        #[unsafe(method(pickElement:))]
        fn pick_element(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.start_pick());
        }

        #[unsafe(method(resetEmulation:))]
        fn reset_emulation(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.reset_emulation());
        }

        #[unsafe(method(toggleClaudeInteract:))]
        fn toggle_claude_interact(&self, _sender: Option<&AnyObject>) {
            self.with(|s| s.toggle_claude_interact());
        }
    }

    unsafe impl NSMenuItemValidation for ActionTarget {
        /// Menu pemblokir bercentang sesuai situs aktif dan nonaktif untuk tab kosong.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> Bool {
            if item.action() == Some(sel!(toggleClaudeInteract:)) {
                let on = self.ivars().state.upgrade().is_some_and(|s| s.claude_interact.get());
                item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
                return Bool::YES;
            }
            if item.action() == Some(sel!(clearHistory:)) {
                return Bool::new(self.ivars().state.upgrade().is_some_and(|s| !s.browser.borrow().history().is_empty()));
            }
            if item.action() == Some(sel!(lockPasswords:)) {
                return Bool::new(self.ivars().state.upgrade().is_some_and(|s| s.passwords.is_unlocked()));
            }
            if item.action() == Some(sel!(toggleChat:)) {
                let open = self.ivars().state.upgrade().is_some_and(|s| s.chat.is_open());
                item.setState(if open { NSControlStateValueOn } else { NSControlStateValueOff });
                return Bool::YES;
            }
            for (action, on, off) in [
                (sel!(togglePin:), "Unpin Tab", "Pin Tab"),
                (sel!(toggleFavorite:), "Remove from Favorites", "Add to Favorites"),
            ] {
                if item.action() == Some(action) {
                    let Some(state) = self.ivars().state.upgrade() else { return Bool::NO };
                    let Some((kind, blank)) = state.active_tab_kind() else { return Bool::NO };
                    let current = if action == sel!(togglePin:) { TabKind::Pinned } else { TabKind::Favorite };
                    item.setTitle(&objc2_foundation::NSString::from_str(if kind == current { on } else { off }));
                    return Bool::new(!blank);
                }
            }
            if item.action() == Some(sel!(setPinnedURL:)) {
                let differs = self.ivars().state.upgrade().is_some_and(|s| s.active_saved_differs());
                return Bool::new(differs);
            }
            if item.action() == Some(sel!(resetEmulation:)) {
                let emulated = self.ivars().state.upgrade().is_some_and(|s| s.active_emulated());
                return Bool::new(emulated);
            }
            if item.action() != Some(sel!(toggleBlocker:)) {
                return Bool::YES;
            }
            let state = self.ivars().state.upgrade().and_then(|s| s.blocker_menu_state());
            item.setState(if state == Some(true) { NSControlStateValueOn } else { NSControlStateValueOff });
            Bool::new(state.is_some())
        }
    }

    unsafe impl NSControlTextEditingDelegate for ActionTarget {
        /// Panah, Enter, Escape, Tab, dan hapus di address field: dropdown
        /// saran dan pelengkapan inline (lihat `AddressBar::command`).
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _text_view: &NSTextView, command: Sel) -> Bool {
            let Some(state) = self.ivars().state.upgrade() else { return Bool::NO };
            Bool::new(state.address.command(&state, command))
        }

        #[unsafe(method(controlTextDidBeginEditing:))]
        fn text_did_begin(&self, notification: &NSNotification) {
            self.with_address(notification, |s| {
                s.ui.sidebar.style_editor();
                s.address.begin();
            });
        }

        #[unsafe(method(controlTextDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            self.with_address(notification, |s| s.address.text_did_change(s));
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_did_end(&self, notification: &NSNotification) {
            self.with_address(notification, |s| s.address.end());
        }
    }

    unsafe impl NSTextFieldDelegate for ActionTarget {}

    unsafe impl NSWindowDelegate for ActionTarget {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, notification: &NSNotification) {
            self.with_main_window(notification, |s| {
                s.ui.relayout();
                s.layout_sidebar();
            });
            self.with_main_window(notification, |s| s.address.hide_popup());
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, notification: &NSNotification) {
            self.with_main_window(notification, |s| s.address.hide_popup());
        }

        #[unsafe(method(windowWillMove:))]
        fn window_will_move(&self, notification: &NSNotification) {
            self.with_main_window(notification, |s| s.address.hide_popup());
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, notification: &NSNotification) {
            let window = notification.object().map_or(std::ptr::null(), |o| Retained::as_ptr(&o).cast());
            self.with(|s| {
                s.password_manager.window_will_close(s, window);
                s.history_window.window_will_close(s, window);
            });
        }
    }
);

impl ActionTarget {
    pub fn new(mtm: MainThreadMarker, state: Weak<AppState>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { state });
        unsafe { msg_send![super(this), init] }
    }

    fn with(&self, f: impl FnOnce(&Rc<AppState>)) {
        if let Some(state) = self.ivars().state.upgrade() {
            f(&state);
        }
    }

    /// Seperti `with`, hanya untuk notifikasi dari address field.
    fn with_address(&self, notification: &NSNotification, f: impl FnOnce(&Rc<AppState>)) {
        self.with(|s| {
            if notification.object().is_some_and(|o| {
                std::ptr::eq(Retained::as_ptr(&o).cast::<u8>(), Retained::as_ptr(&s.ui.sidebar.address).cast())
            }) {
                f(s);
            }
        });
    }

    /// Seperti `with`, hanya untuk notifikasi dari jendela utama.
    fn with_main_window(&self, notification: &NSNotification, f: impl FnOnce(&Rc<AppState>)) {
        self.with(|s| {
            if notification
                .object()
                .is_some_and(|o| std::ptr::eq(Retained::as_ptr(&o).cast::<u8>(), Retained::as_ptr(&s.ui.window).cast()))
            {
                f(s);
            }
        });
    }
}

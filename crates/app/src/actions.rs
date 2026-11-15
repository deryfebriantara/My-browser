//! Target aksi Objective-C untuk tombol, menu, address field, dan window.
//! Semua handler hanya meneruskan ke `AppState`.

use crate::state::AppState;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSControl, NSControlTextEditingDelegate, NSTextFieldDelegate, NSTextView, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use std::rc::{Rc, Weak};

pub struct TargetIvars {
    state: Weak<AppState>,
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
    }

    unsafe impl NSControlTextEditingDelegate for ActionTarget {
        /// Escape di address field mengembalikan URL tab aktif.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _text_view: &NSTextView, command: Sel) -> Bool {
            if command == sel!(cancelOperation:) {
                self.with(|s| s.revert_address());
                return Bool::YES;
            }
            Bool::NO
        }
    }

    unsafe impl NSTextFieldDelegate for ActionTarget {}

    unsafe impl NSWindowDelegate for ActionTarget {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            self.with(|s| s.layout_tabs());
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
}

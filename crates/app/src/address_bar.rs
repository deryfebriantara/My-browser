//! Address bar: meneruskan ketikan dan tombol dari field editor ke
//! `AddressEdit` (core), lalu menjalankan efeknya di AppKit (pelengkapan
//! inline lewat seleksi teks, dan dropdown saran).

use crate::state::AppState;
use crate::suggest_popup::SuggestPopup;
use block2::RcBlock;
use browser_core::address_edit::{AddressEdit, Effect, Key};
use browser_core::suggest::Suggestion;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{msg_send, sel};
use objc2_app_kit::NSText;
use objc2_foundation::{NSRange, NSRunLoop, NSString};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
pub struct AddressBar {
    edit: RefCell<AddressEdit>,
    popup: RefCell<Option<SuggestPopup>>,
}

/// Panjang UTF-16, satuan yang dipakai NSString dan NSRange.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

impl AddressBar {
    /// Field mulai diedit.
    pub fn begin(&self) {
        self.edit.borrow_mut().begin();
    }

    /// Field selesai diedit: dropdown ditutup.
    pub fn end(&self) {
        self.edit.borrow_mut().end();
        self.hide_popup();
    }

    pub fn hide_popup(&self) {
        self.edit.borrow_mut().visible = false;
        if let Some(popup) = self.popup.borrow().as_ref() {
            popup.hide();
        }
    }

    pub fn popup_visible(&self) -> bool {
        self.popup.borrow().as_ref().is_some_and(SuggestPopup::is_visible)
    }

    /// Baris saran sekarang dan yang disorot (untuk pengujian).
    pub fn rows(&self) -> (Vec<Suggestion>, usize) {
        let edit = self.edit.borrow();
        (edit.rows.clone(), edit.selected)
    }

    /// Frame dropdown di layar (untuk pengujian).
    pub fn popup_frame(&self) -> Option<objc2_foundation::NSRect> {
        self.popup.borrow().as_ref().filter(|p| p.is_visible()).map(SuggestPopup::frame)
    }

    fn editor(state: &AppState) -> Option<Retained<NSText>> {
        state.ui.nav.address.currentEditor()
    }

    /// Isi field berubah karena pengguna mengetik, menempel, atau menghapus.
    pub fn text_did_change(&self, state: &Rc<AppState>) {
        let Some(editor) = Self::editor(state) else { return };
        let text = editor.string();
        let length = text.length();
        let range = editor.selectedRange();
        let caret_at_end = range.location == length && range.length == 0;
        let composing: bool = unsafe { msg_send![&*editor, hasMarkedText] };
        let text = text.to_string();
        let effects = self
            .edit
            .borrow_mut()
            .text_changed(&text, caret_at_end, composing, |t, inline| state.browser.borrow().suggest(t, inline));
        self.apply(state, effects);
    }

    /// Tombol khusus dari field editor. `true` kalau sudah ditangani.
    pub fn command(&self, state: &Rc<AppState>, command: Sel) -> bool {
        let key = if command == sel!(moveUp:) {
            Key::Up
        } else if command == sel!(moveDown:) {
            Key::Down
        } else if command == sel!(insertNewline:) {
            Key::Enter
        } else if command == sel!(cancelOperation:) {
            Key::Escape
        } else if command == sel!(insertTab:) {
            Key::Tab
        } else if [
            sel!(deleteBackward:),
            sel!(deleteForward:),
            sel!(deleteWordBackward:),
            sel!(deleteWordForward:),
            sel!(deleteToBeginningOfLine:),
            sel!(deleteToEndOfLine:),
            sel!(deleteBackwardByDecomposingPreviousCharacter:),
        ]
        .contains(&command)
        {
            Key::Delete
        } else {
            return false;
        };
        let (handled, effects) = self.edit.borrow_mut().key(key);
        self.apply(state, effects);
        handled
    }

    /// Baris dropdown diklik. Ditunda ke putaran run loop berikutnya supaya
    /// dropdown tidak ditutup di tengah event klik miliknya sendiri.
    pub fn clicked(&self, state: &Rc<AppState>, index: usize) {
        let Some(row) = self.edit.borrow().rows.get(index).cloned() else { return };
        let weak = Rc::downgrade(state);
        let block = RcBlock::new(move || {
            if let Some(state) = weak.upgrade() {
                state.activate_suggestion(&row);
            }
        });
        // SAFETY: dari main thread ke run loop main thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    }

    fn apply(&self, state: &Rc<AppState>, effects: Vec<Effect>) {
        let mut render = false;
        for effect in effects {
            match effect {
                Effect::AppendInline(tail) => {
                    if let Some(editor) = Self::editor(state) {
                        let start = editor.string().length();
                        editor.replaceCharactersInRange_withString(NSRange::new(start, 0), &NSString::from_str(&tail));
                        editor.setSelectedRange(NSRange::new(start, utf16_len(&tail)));
                    }
                }
                Effect::ReplaceText(text) => {
                    if let Some(editor) = Self::editor(state) {
                        editor.setString(&NSString::from_str(&text));
                        editor.setSelectedRange(NSRange::new(utf16_len(&text), 0));
                    }
                }
                Effect::RestoreTyped { typed, tail } => {
                    if let Some(editor) = Self::editor(state) {
                        editor.setString(&NSString::from_str(&format!("{typed}{tail}")));
                        editor.setSelectedRange(NSRange::new(utf16_len(&typed), utf16_len(&tail)));
                    }
                }
                Effect::Show | Effect::Select(_) => render = true,
                Effect::Hide => {
                    render = false;
                    self.hide_popup();
                }
                Effect::Activate(index) => {
                    let row = self.edit.borrow().rows.get(index).cloned();
                    if let Some(row) = row {
                        state.activate_suggestion(&row);
                    }
                    return;
                }
                Effect::NavigateTyped => {
                    state.navigate_from_address();
                    return;
                }
                Effect::Revert => {
                    state.revert_address();
                    return;
                }
            }
        }
        if render {
            self.render(state);
        }
    }

    fn render(&self, state: &AppState) {
        let (rows, selected) = self.rows();
        if rows.is_empty() {
            self.hide_popup();
            return;
        }
        let mut popup = self.popup.borrow_mut();
        let popup = popup.get_or_insert_with(|| SuggestPopup::new(state.mtm()));
        popup.show(&rows, selected, &state.ui.nav.address, &state.ui.window, &state.ui.target);
    }
}

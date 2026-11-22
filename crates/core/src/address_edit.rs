//! Perilaku keyboard address bar: pelengkapan inline, dropdown saran, dan
//! tombol panah/Enter/Escape/Tab. Murni, tanpa AppKit: lapisan UI hanya
//! menerjemahkan selector menjadi `Key` dan menjalankan `Effect` yang
//! dikembalikan.

use crate::suggest::{Suggestion, Suggestions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Escape,
    Tab,
    /// Semua jenis hapus (Backspace, Delete, hapus kata, hapus baris).
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Tempel teks di akhir field dan seleksi teks itu.
    AppendInline(String),
    /// Ganti isi field, kursor di akhir.
    ReplaceText(String),
    /// Kembalikan teks ketikan pengguna, dengan `tail` terseleksi di akhirnya.
    RestoreTyped {
        typed: String,
        tail: String,
    },
    Show,
    Select(usize),
    Hide,
    /// Buka baris ke-n.
    Activate(usize),
    /// Muat isi field seperti biasa (tanpa dropdown).
    NavigateTyped,
    /// Kembalikan URL tab aktif.
    Revert,
}

#[derive(Debug, Default)]
pub struct AddressEdit {
    pub rows: Vec<Suggestion>,
    pub selected: usize,
    pub visible: bool,
    /// Teks yang diketik pengguna, tanpa ekor pelengkapan.
    typed: Option<String>,
    inline_tail: Option<String>,
    /// Tombol hapus baru ditekan: perubahan berikutnya tidak dilengkapi.
    suppress_inline: bool,
}

impl AddressEdit {
    /// Field mulai diedit.
    pub fn begin(&mut self) {
        *self = AddressEdit::default();
    }

    /// Field selesai diedit (fokus pindah, halaman dibuka).
    pub fn end(&mut self) {
        *self = AddressEdit::default();
    }

    /// Isi field berubah karena pengguna. `caret_at_end`: kursor di akhir
    /// tanpa seleksi. `composing`: IME sedang menyusun huruf.
    pub fn text_changed(
        &mut self,
        text: &str,
        caret_at_end: bool,
        composing: bool,
        suggest: impl FnOnce(&str, bool) -> Suggestions,
    ) -> Vec<Effect> {
        let grew = self.typed.as_ref().is_none_or(|prev| text.len() > prev.len());
        let allow_inline = !self.suppress_inline && caret_at_end && !composing && grew;
        self.suppress_inline = false;
        self.typed = Some(text.to_string());
        let found = suggest(text, allow_inline);
        self.rows = found.rows;
        self.selected = 0;
        self.inline_tail = None;
        let mut effects = Vec::new();
        if allow_inline && let Some(tail) = found.inline_tail.filter(|t| !t.is_empty()) {
            effects.push(Effect::AppendInline(tail.clone()));
            self.inline_tail = Some(tail);
        }
        if self.rows.is_empty() {
            self.visible = false;
            effects.push(Effect::Hide);
        } else {
            self.visible = true;
            effects.push(Effect::Show);
            effects.push(Effect::Select(0));
        }
        effects
    }

    /// Tombol khusus. `true` kalau tombolnya ditangani di sini (AppKit tidak
    /// perlu menjalankan perilaku bawaannya).
    pub fn key(&mut self, key: Key) -> (bool, Vec<Effect>) {
        match key {
            Key::Delete => {
                self.suppress_inline = true;
                (false, Vec::new())
            }
            Key::Down => {
                if self.rows.is_empty() {
                    return (false, Vec::new());
                }
                if !self.visible {
                    self.visible = true;
                    return (true, vec![Effect::Show, Effect::Select(self.selected)]);
                }
                if self.selected + 1 >= self.rows.len() {
                    return (true, Vec::new());
                }
                self.selected += 1;
                (true, vec![Effect::Select(self.selected), Effect::ReplaceText(self.rows[self.selected].fill.clone())])
            }
            Key::Up => {
                if !self.visible || self.selected == 0 {
                    return (self.visible, Vec::new());
                }
                self.selected -= 1;
                let text = if self.selected == 0 {
                    self.restore_typed()
                } else {
                    Effect::ReplaceText(self.rows[self.selected].fill.clone())
                };
                (true, vec![Effect::Select(self.selected), text])
            }
            Key::Enter => {
                if self.visible && !self.rows.is_empty() {
                    self.visible = false;
                    (true, vec![Effect::Hide, Effect::Activate(self.selected)])
                } else {
                    (true, vec![Effect::NavigateTyped])
                }
            }
            Key::Escape => {
                if self.visible {
                    self.visible = false;
                    self.selected = 0;
                    self.inline_tail = None;
                    let typed = self.typed.clone().unwrap_or_default();
                    (true, vec![Effect::Hide, Effect::RestoreTyped { typed, tail: String::new() }])
                } else {
                    (true, vec![Effect::Revert])
                }
            }
            Key::Tab => match self.inline_tail.take() {
                Some(tail) if self.visible && self.selected == 0 => {
                    let full = format!("{}{tail}", self.typed.clone().unwrap_or_default());
                    self.typed = Some(full.clone());
                    (true, vec![Effect::ReplaceText(full)])
                }
                _ => {
                    self.visible = false;
                    (false, vec![Effect::Hide])
                }
            },
        }
    }

    fn restore_typed(&self) -> Effect {
        Effect::RestoreTyped {
            typed: self.typed.clone().unwrap_or_default(),
            tail: self.inline_tail.clone().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::SuggestionKind;
    use url::Url;

    fn row(fill: &str) -> Suggestion {
        Suggestion {
            kind: SuggestionKind::History,
            title: fill.into(),
            detail: String::new(),
            url: Url::parse("https://a.com/").unwrap(),
            fill: fill.into(),
        }
    }

    /// Saran palsu: tiga baris, dan ekor inline "tube.com" untuk awalan "you".
    fn fake(text: &str, allow: bool) -> Suggestions {
        let tail = "youtube.com".strip_prefix(text).filter(|_| allow && !text.is_empty()).map(str::to_string);
        Suggestions { rows: vec![row(text), row("https://b.com/"), row("https://c.com/")], inline_tail: tail }
    }

    fn typed(edit: &mut AddressEdit, text: &str) -> Vec<Effect> {
        edit.text_changed(text, true, false, fake)
    }

    #[test]
    fn typing_completes_inline_and_shows_rows() {
        let mut e = AddressEdit::default();
        assert_eq!(
            typed(&mut e, "yo"),
            vec![Effect::AppendInline("utube.com".into()), Effect::Show, Effect::Select(0)]
        );
        // Mengetik di atas ekor yang terseleksi melengkapi lagi.
        assert_eq!(typed(&mut e, "you")[0], Effect::AppendInline("tube.com".into()));
    }

    #[test]
    fn delete_stops_inline_until_next_typing() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        assert_eq!(e.key(Key::Delete), (false, vec![]));
        assert_eq!(typed(&mut e, "you"), vec![Effect::Show, Effect::Select(0)], "setelah hapus tidak dilengkapi");
        assert_eq!(typed(&mut e, "yo"), vec![Effect::Show, Effect::Select(0)], "teks memendek tidak dilengkapi");
        assert_eq!(typed(&mut e, "you")[0], Effect::AppendInline("tube.com".into()));
    }

    #[test]
    fn caret_not_at_end_or_composing_never_completes() {
        let mut e = AddressEdit::default();
        assert!(!matches!(e.text_changed("you", false, false, fake)[0], Effect::AppendInline(_)));
        let mut e = AddressEdit::default();
        assert!(!matches!(e.text_changed("you", true, true, fake)[0], Effect::AppendInline(_)));
    }

    #[test]
    fn arrows_fill_rows_and_restore_typed_text() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        assert_eq!(e.key(Key::Down), (true, vec![Effect::Select(1), Effect::ReplaceText("https://b.com/".into())]));
        assert_eq!(e.key(Key::Down), (true, vec![Effect::Select(2), Effect::ReplaceText("https://c.com/".into())]));
        assert_eq!(e.key(Key::Down), (true, vec![]), "baris terakhir");
        e.key(Key::Up);
        assert_eq!(
            e.key(Key::Up),
            (true, vec![Effect::Select(0), Effect::RestoreTyped { typed: "you".into(), tail: "tube.com".into() }])
        );
        assert_eq!(e.key(Key::Up), (true, vec![]));
    }

    #[test]
    fn enter_activates_selected_row_or_navigates() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        e.key(Key::Down);
        assert_eq!(e.key(Key::Enter), (true, vec![Effect::Hide, Effect::Activate(1)]));
        let mut e = AddressEdit::default();
        assert_eq!(e.key(Key::Enter), (true, vec![Effect::NavigateTyped]));
    }

    #[test]
    fn escape_twice_hides_then_reverts() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        assert_eq!(
            e.key(Key::Escape),
            (true, vec![Effect::Hide, Effect::RestoreTyped { typed: "you".into(), tail: String::new() }])
        );
        assert_eq!(e.key(Key::Escape), (true, vec![Effect::Revert]));
    }

    #[test]
    fn down_reopens_hidden_dropdown() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        e.key(Key::Escape);
        assert_eq!(e.key(Key::Down), (true, vec![Effect::Show, Effect::Select(0)]));
    }

    #[test]
    fn tab_accepts_inline_completion_otherwise_moves_focus() {
        let mut e = AddressEdit::default();
        typed(&mut e, "you");
        assert_eq!(e.key(Key::Tab), (true, vec![Effect::ReplaceText("youtube.com".into())]));
        assert_eq!(e.key(Key::Tab), (false, vec![Effect::Hide]));
    }

    #[test]
    fn empty_text_hides() {
        let mut e = AddressEdit::default();
        let effects = e.text_changed("", true, false, |_, _| Suggestions::default());
        assert_eq!(effects, vec![Effect::Hide]);
        assert!(!e.visible);
    }
}

//! Sidebar: baris atas (navigasi dan Tanya Claude), kolom alamat, lalu daftar
//! tab yang bisa digulir. Selalu memakai tampilan gelap karena berada di atas
//! gradasi tema.

use crate::actions::ActionTarget;
use crate::favicons::Favicons;
use crate::layout::{Frame, TILE_GAP, TILE_HEIGHT, insertion_index, tile_columns};
use crate::paint::{DropPhase, PaintView};
use crate::sidebar_row::{ROW_HEIGHT, RowStyle, TabRowView, symbol};
use crate::state::AppState;
use browser_core::sidebar::{RowInfo, SidebarModel};
use browser_core::tabs::TabKind;
use browser_core::theme::SECONDARY_ALPHA;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSButton, NSColor, NSFocusRingType, NSFont,
    NSImage, NSLineBreakMode, NSScrollView, NSTextField, NSTextView,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

const PAD: f64 = 8.0;
const BUTTON: f64 = 24.0;
const ADDRESS_HEIGHT: f64 = 30.0;
const HEADER_HEIGHT: f64 = 24.0;

/// Area sidebar yang menerima lepasan tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropRegion {
    List,
    Favorites,
}

/// Satu baris yang sedang tampil (untuk pengujian).
pub struct ShownRow {
    pub info: RowInfo,
    pub view: Retained<TabRowView>,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub favicon: bool,
}

pub struct Sidebar {
    pub view: Retained<PaintView>,
    pub back: Retained<NSButton>,
    pub forward: Retained<NSButton>,
    pub reload: Retained<NSButton>,
    claude: Retained<NSButton>,
    pub address: Retained<NSTextField>,
    address_box: Retained<PaintView>,
    scroll: Retained<NSScrollView>,
    doc: Retained<PaintView>,
    reload_icon: Retained<NSImage>,
    stop_icon: Retained<NSImage>,
    rows: RefCell<Vec<ShownRow>>,
    /// Jarak pusat tombol jendela (merah/kuning/hijau) dari tepi atas.
    top_center: Cell<f64>,
    frame: Cell<Frame>,
    /// Grid ubin favorit (tidak ikut bergulir).
    favs: Retained<PaintView>,
    favs_height: Cell<f64>,
    /// Kolom dan lebar ubin favorit dari render terakhir.
    grid: Cell<(usize, f64)>,
    fav_count: Cell<usize>,
    /// Ada tab yang sedang diseret (grid favorit kosong menampilkan zona lepas).
    dragging: Cell<bool>,
    drop_zone: Retained<PaintView>,
    drop_line: Retained<PaintView>,
    /// Posisi y label TODAY dan baris Today pertama di daftar.
    today_top: Cell<f64>,
    today_rows_top: Cell<f64>,
    /// Penanda "Claude can interact" di bawah baris atas.
    interact: Retained<PaintView>,
    interact_off: Retained<NSButton>,
}

fn icon_button(mtm: MainThreadMarker, image: &NSImage, target: &ActionTarget, action: Sel) -> Retained<NSButton> {
    let button =
        unsafe { NSButton::buttonWithImage_target_action(image, Some(target as &AnyObject), Some(action), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.85)));
    button
}

fn header(mtm: MainThreadMarker, text: &str, width: f64, y: f64) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize_weight(10.0, unsafe { objc2_app_kit::NSFontWeightSemibold })));
    field.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, SECONDARY_ALPHA)));
    field.setFrame(NSRect::new(NSPoint::new(PAD + 6.0, y + 6.0), NSSize::new(width - 2.0 * PAD, 14.0)));
    field
}

/// Favicon host baris (true), atau monogram / ikon globe (false).
fn row_icon(row: &RowInfo, favicons: &Favicons, size: f64) -> (Retained<NSImage>, bool) {
    if let Some(image) = row.host.as_deref().and_then(|h| favicons.image(h)) {
        return (image, true);
    }
    let icon = match row.host.as_deref() {
        Some(host) => Favicons::monogram_image(host, size),
        None => symbol("globe", "Page"),
    };
    (icon, false)
}

impl Sidebar {
    pub fn build(mtm: MainThreadMarker, target: &ActionTarget) -> Sidebar {
        let view = PaintView::container(mtm, true, false);
        view.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref());

        let reload_icon = symbol("arrow.clockwise", "Reload");
        let stop_icon = symbol("xmark", "Stop");
        let back = icon_button(mtm, &symbol("chevron.left", "Back"), target, sel!(goBack:));
        let forward = icon_button(mtm, &symbol("chevron.right", "Forward"), target, sel!(goForward:));
        let reload = icon_button(mtm, &reload_icon, target, sel!(reloadOrStop:));
        let claude = icon_button(mtm, &symbol("sparkles", "Ask Claude"), target, sel!(toggleChat:));
        claude.setContentTintColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.82, 0.54, 1.0)));
        claude.setToolTip(Some(&NSString::from_str("Ask Claude (⌘J)")));
        for button in [&back, &forward, &reload, &claude] {
            view.addSubview(button);
        }

        let address_box = PaintView::fill(mtm, &NSColor::colorWithWhite_alpha(1.0, 0.14), 9.0);
        let address = NSTextField::new(mtm);
        address.setBezeled(false);
        address.setDrawsBackground(false);
        address.setFocusRingType(NSFocusRingType::None);
        address.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        address.setTextColor(Some(&NSColor::whiteColor()));
        address.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        address.setUsesSingleLineMode(true);
        let placeholder = NSString::from_str("Search or enter address");
        // SAFETY: kunci atribut warna dengan nilai NSColor.
        let attrs = unsafe {
            NSDictionary::<NSString, AnyObject>::from_slices(
                &[objc2_app_kit::NSForegroundColorAttributeName],
                &[&*NSColor::colorWithWhite_alpha(1.0, 0.6) as &AnyObject],
            )
        };
        let placeholder = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &placeholder, Some(&attrs))
        };
        address.setPlaceholderAttributedString(Some(&placeholder));
        unsafe {
            address.setTarget(Some(target as &AnyObject));
            address.setAction(Some(sel!(addressEntered:)));
            address.setDelegate(Some(ProtocolObject::from_ref(target)));
        }
        address_box.addSubview(&address);
        view.addSubview(&address_box);

        let interact = PaintView::fill(mtm, &NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.58, 0.0, 0.32), 8.0);
        let interact_label = NSTextField::labelWithString(&NSString::from_str("Claude can interact"), mtm);
        interact_label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        interact_label.setTextColor(Some(&NSColor::whiteColor()));
        interact_label.setFrame(NSRect::new(NSPoint::new(10.0, 5.0), NSSize::new(130.0, 16.0)));
        interact.addSubview(&interact_label);
        let interact_off = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Turn Off"),
                Some(target as &AnyObject),
                Some(sel!(toggleClaudeInteract:)),
                mtm,
            )
        };
        interact_off.setBordered(false);
        interact_off.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
        interact.addSubview(&interact_off);
        interact.setHidden(true);
        view.addSubview(&interact);

        let favs = PaintView::container(mtm, true, false);
        view.addSubview(&favs);

        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), NSRect::ZERO);
        scroll.setHasVerticalScroller(true);
        scroll.setHasHorizontalScroller(false);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        let doc = PaintView::container(mtm, true, false);
        scroll.setDocumentView(Some(&doc));
        view.addSubview(&scroll);

        Sidebar {
            view,
            back,
            forward,
            reload,
            claude,
            address,
            address_box,
            scroll,
            doc,
            reload_icon,
            stop_icon,
            rows: RefCell::default(),
            top_center: Cell::new(14.0),
            frame: Cell::new(Frame { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }),
            favs,
            favs_height: Cell::new(0.0),
            interact,
            interact_off,
            grid: Cell::new((1, 0.0)),
            fav_count: Cell::new(0),
            dragging: Cell::new(false),
            drop_zone: PaintView::outline(mtm, &NSColor::colorWithWhite_alpha(1.0, 0.35), 1.5, 10.0),
            drop_line: PaintView::fill(mtm, &NSColor::whiteColor(), 1.0),
            today_top: Cell::new(0.0),
            today_rows_top: Cell::new(0.0),
        }
    }

    /// `f` dalam koordinat induk; `top_center` adalah jarak pusat tombol
    /// jendela dari tepi atas sidebar.
    pub fn set_frame(&self, f: Frame, top_center: f64) {
        self.top_center.set(top_center);
        self.frame.set(f);
        self.view.setFrame(NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.w, f.h)));
        self.layout_inner();
    }

    /// Menata isi sidebar dari atas ke bawah: baris atas, penanda interaksi
    /// Claude, kolom alamat, grid favorit, lalu daftar yang bisa digulir.
    fn layout_inner(&self) {
        let f = self.frame.get();
        let w = f.w;
        let row_y = (self.top_center.get() - BUTTON / 2.0).max(0.0);
        let buttons = [&self.claude, &self.reload, &self.forward, &self.back];
        for (i, button) in buttons.into_iter().enumerate() {
            let x = w - PAD - BUTTON - i as f64 * (BUTTON + 4.0);
            button.setFrame(NSRect::new(NSPoint::new(x, row_y), NSSize::new(BUTTON, BUTTON)));
        }
        let mut address_y = row_y + BUTTON + 8.0;
        if !self.interact.isHidden() {
            self.interact.setFrame(NSRect::new(NSPoint::new(PAD, address_y - 2.0), NSSize::new(w - 2.0 * PAD, 26.0)));
            self.interact_off.setFrame(NSRect::new(NSPoint::new(w - 2.0 * PAD - 72.0, 3.0), NSSize::new(64.0, 20.0)));
            address_y += 30.0;
        }
        self.address_box
            .setFrame(NSRect::new(NSPoint::new(PAD, address_y), NSSize::new(w - 2.0 * PAD, ADDRESS_HEIGHT)));
        self.address.setFrame(NSRect::new(NSPoint::new(10.0, 4.0), NSSize::new(w - 2.0 * PAD - 20.0, 22.0)));
        let mut list_y = address_y + ADDRESS_HEIGHT + 8.0;
        let favs_height = self.favs_height.get();
        self.favs.setFrame(NSRect::new(NSPoint::new(0.0, list_y), NSSize::new(w, favs_height)));
        if favs_height > 0.0 {
            list_y += favs_height + 8.0;
        }
        self.scroll.setFrame(NSRect::new(NSPoint::new(0.0, list_y), NSSize::new(w, (f.h - list_y - PAD).max(0.0))));
    }

    pub fn update_nav(&self, can_back: bool, can_forward: bool, loading: bool, url: &str) {
        self.back.setEnabled(can_back);
        self.forward.setEnabled(can_forward);
        self.reload.setImage(Some(if loading { &self.stop_icon } else { &self.reload_icon }));
        if !self.is_editing() {
            self.address.setStringValue(&NSString::from_str(url));
        }
    }

    pub fn is_editing(&self) -> bool {
        self.address.currentEditor().is_some()
    }

    /// Kursor teks putih di field editor (bawaannya hitam, tidak terlihat di atas gradasi).
    pub fn style_editor(&self) {
        if let Some(editor) = self.address.currentEditor()
            && let Ok(text_view) = editor.downcast::<NSTextView>()
        {
            text_view.setInsertionPointColor(Some(&NSColor::whiteColor()));
        }
    }

    /// Ubin favorit di grid di atas daftar. Mengubah tinggi grid lalu menata ulang sidebar.
    fn render_favorites(
        &self,
        model: &SidebarModel,
        favicons: &Favicons,
        target: &ActionTarget,
        shown: &mut Vec<ShownRow>,
    ) {
        let mtm = MainThreadMarker::from(&*self.view);
        for old in self.favs.subviews().iter() {
            old.removeFromSuperview();
        }
        let inner = (self.frame.get().w - 2.0 * PAD).max(0.0);
        let columns = tile_columns(inner);
        let tile_w = ((inner - TILE_GAP * (columns as f64 - 1.0)) / columns as f64).max(0.0);
        for (i, row) in model.favorites.iter().enumerate() {
            let (icon, favicon) = row_icon(row, favicons, 20.0);
            let view = TabRowView::new(mtm, row, RowStyle::Tile, &icon, tile_w, target);
            let (r, c) = (i / columns, i % columns);
            view.setFrameOrigin(NSPoint::new(
                PAD + c as f64 * (tile_w + TILE_GAP),
                r as f64 * (TILE_HEIGHT + TILE_GAP),
            ));
            self.favs.addSubview(&view);
            shown.push(ShownRow { info: row.clone(), view, favicon });
        }
        self.grid.set((columns, tile_w));
        self.fav_count.set(model.favorites.len());
        self.update_favs_height();
        self.layout_inner();
    }

    /// Tinggi grid favorit: baris ubin, atau zona lepas 40 pt saat menyeret
    /// ke grid yang masih kosong.
    fn update_favs_height(&self) {
        let (columns, _) = self.grid.get();
        let count = self.fav_count.get();
        let rows = count.div_ceil(columns.max(1));
        let height = if rows > 0 {
            rows as f64 * (TILE_HEIGHT + TILE_GAP) - TILE_GAP
        } else if self.dragging.get() {
            TILE_HEIGHT
        } else {
            0.0
        };
        self.drop_zone.removeFromSuperview();
        if count == 0 && self.dragging.get() {
            let inner = (self.frame.get().w - 2.0 * PAD).max(0.0);
            self.drop_zone.setFrame(NSRect::new(NSPoint::new(PAD, 0.0), NSSize::new(inner, TILE_HEIGHT)));
            self.favs.addSubview(&self.drop_zone);
        }
        self.favs_height.set(height);
        self.layout_inner();
    }

    /// Seret tab dimulai atau selesai (dari `TabRowView` lewat responder chain).
    pub fn set_dragging(&self, on: bool) {
        self.dragging.set(on);
        if !on {
            self.drop_line.removeFromSuperview();
        }
        self.update_favs_height();
    }

    /// Daftar tab dan grid favorit menjadi tujuan lepasan.
    pub fn install_drops(&self, state: Weak<AppState>) {
        for (view, region) in [(&self.doc, DropRegion::List), (&self.favs, DropRegion::Favorites)] {
            let weak = state.clone();
            view.set_drop_handler(Rc::new(move |phase, point| {
                let Some(state) = weak.upgrade() else { return false };
                state.ui.sidebar.drop_event(&state, region, phase, point)
            }));
        }
    }

    fn drop_event(&self, state: &Rc<AppState>, region: DropRegion, phase: DropPhase, point: NSPoint) -> bool {
        let (kind, at) = self.drop_target(region, point);
        match phase {
            DropPhase::Update => {
                self.show_drop_line(region, kind, at);
                true
            }
            DropPhase::Exit => {
                self.drop_line.removeFromSuperview();
                false
            }
            DropPhase::Perform(id) => {
                self.drop_line.removeFromSuperview();
                // Pindah ke bawah di bagian yang sama: posisi tab itu sendiri
                // ikut terhitung sebelum ia diangkat.
                let current = self.rows.borrow().iter().filter(|r| r.info.kind == kind).position(|r| r.info.id.0 == id);
                let at = match current {
                    Some(current) if current < at => at - 1,
                    _ => at,
                };
                state.place_tab(id, kind, at);
                true
            }
        }
    }

    /// Bagian dan posisi sisip untuk titik `point` (koordinat region).
    fn drop_target(&self, region: DropRegion, point: NSPoint) -> (TabKind, usize) {
        match region {
            DropRegion::Favorites => {
                let (columns, tile_w) = self.grid.get();
                let col = (((point.x - PAD) / (tile_w + TILE_GAP)).floor().max(0.0) as usize).min(columns - 1);
                let row = (point.y / (TILE_HEIGHT + TILE_GAP)).floor().max(0.0) as usize;
                (TabKind::Favorite, (row * columns + col).min(self.fav_count.get()))
            }
            DropRegion::List => {
                let kind = if point.y < self.today_top.get() { TabKind::Pinned } else { TabKind::Today };
                let mids: Vec<f64> = self
                    .rows
                    .borrow()
                    .iter()
                    .filter(|r| r.info.kind == kind)
                    .map(|r| {
                        let f = r.view.frame();
                        f.origin.y + f.size.height / 2.0
                    })
                    .collect();
                (kind, insertion_index(&mids, point.y))
            }
        }
    }

    fn show_drop_line(&self, region: DropRegion, kind: TabKind, at: usize) {
        self.drop_line.removeFromSuperview();
        match region {
            DropRegion::Favorites => {
                let (columns, tile_w) = self.grid.get();
                let (row, col) = (at / columns, at % columns);
                let x = PAD + col as f64 * (tile_w + TILE_GAP) - TILE_GAP / 2.0 - 1.0;
                let y = row as f64 * (TILE_HEIGHT + TILE_GAP);
                self.drop_line.setFrame(NSRect::new(NSPoint::new(x.max(0.0), y), NSSize::new(2.0, TILE_HEIGHT)));
                self.favs.addSubview(&self.drop_line);
            }
            DropRegion::List => {
                let frames: Vec<NSRect> =
                    self.rows.borrow().iter().filter(|r| r.info.kind == kind).map(|r| r.view.frame()).collect();
                let y = match (frames.get(at), frames.last()) {
                    (Some(f), _) => f.origin.y - 1.0,
                    (None, Some(last)) => last.origin.y + last.size.height + 1.0,
                    (None, None) if kind == TabKind::Pinned => (self.today_top.get() - 4.0).max(0.0),
                    (None, None) => self.today_rows_top.get(),
                };
                let width = (self.scroll.contentSize().width - 2.0 * PAD).max(0.0);
                self.drop_line.setFrame(NSRect::new(NSPoint::new(PAD, y - 1.0), NSSize::new(width, 2.0)));
                self.doc.addSubview(&self.drop_line);
            }
        }
    }

    /// Melepas tab `id` di setengah atas baris `target` lewat jalur lepasan
    /// sungguhan (titik, posisi sisip, koreksi geser ke bawah).
    #[cfg(debug_assertions)]
    pub fn debug_drop_before(&self, state: &Rc<AppState>, id: u64, target: u64) -> bool {
        let found = self.rows.borrow().iter().find(|r| r.info.id.0 == target).map(|r| (r.info.kind, r.view.frame()));
        let Some((kind, frame)) = found else { return false };
        let region = if kind == TabKind::Favorite { DropRegion::Favorites } else { DropRegion::List };
        let point = NSPoint::new(frame.origin.x + 4.0, frame.origin.y + 2.0);
        self.drop_event(state, region, DropPhase::Update, point);
        self.drop_event(state, region, DropPhase::Perform(id), point)
    }

    pub fn set_interact(&self, on: bool) {
        self.interact.setHidden(!on);
        self.layout_inner();
    }

    #[cfg(debug_assertions)]
    pub fn interact_visible(&self) -> bool {
        !self.interact.isHidden()
    }

    #[cfg(debug_assertions)]
    pub fn interact_off_button(&self) -> Retained<NSButton> {
        self.interact_off.clone()
    }

    /// Menggambar ulang daftar tab. Dipanggil dari putaran run loop berikutnya
    /// (lihat `AppState::layout_sidebar`), tidak dari dalam aksi baris.
    pub fn render(&self, model: &SidebarModel, favicons: &Favicons, target: &ActionTarget) {
        let mtm = MainThreadMarker::from(&*self.view);
        for old in self.doc.subviews().iter() {
            old.removeFromSuperview();
        }
        let mut shown = Vec::new();
        self.render_favorites(model, favicons, target, &mut shown);
        let width = self.scroll.contentSize().width;
        let row_w = (width - 2.0 * PAD).max(0.0);
        let mut y = 0.0;
        let add_rows = |rows: &[RowInfo], y: &mut f64, shown: &mut Vec<ShownRow>| {
            for row in rows {
                let (icon, favicon) = row_icon(row, favicons, 16.0);
                let view = TabRowView::new(mtm, row, RowStyle::Row, &icon, row_w, target);
                view.setFrameOrigin(NSPoint::new(PAD, *y));
                self.doc.addSubview(&view);
                shown.push(ShownRow { info: row.clone(), view, favicon });
                *y += ROW_HEIGHT + 2.0;
            }
        };
        if !model.pinned.is_empty() {
            self.doc.addSubview(&header(mtm, "PINNED", width, y));
            y += HEADER_HEIGHT;
            add_rows(&model.pinned, &mut y, &mut shown);
            let divider = PaintView::fill(mtm, &NSColor::colorWithWhite_alpha(1.0, 0.14), 0.0);
            divider.setFrame(NSRect::new(NSPoint::new(PAD + 6.0, y + 4.0), NSSize::new(row_w - 12.0, 1.0)));
            self.doc.addSubview(&divider);
            y += 9.0;
        }
        self.today_top.set(y);
        self.doc.addSubview(&header(mtm, "TODAY", width, y));
        y += HEADER_HEIGHT;
        let new_tab = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("＋  New Tab"),
                Some(target as &AnyObject),
                Some(sel!(newTab:)),
                mtm,
            )
        };
        new_tab.setBordered(false);
        new_tab.setAlignment(objc2_app_kit::NSTextAlignment::Left);
        new_tab.setAlphaValue(0.7);
        new_tab.setFrame(NSRect::new(NSPoint::new(PAD + 6.0, y), NSSize::new(row_w - 6.0, 26.0)));
        self.doc.addSubview(&new_tab);
        y += 28.0;
        self.today_rows_top.set(y);
        add_rows(&model.today, &mut y, &mut shown);

        let visible = self.scroll.contentSize().height;
        self.doc.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, y.max(visible))));
        *self.rows.borrow_mut() = shown;
    }

    /// Baris yang sedang tampil, dalam urutan sidebar.
    #[cfg(debug_assertions)]
    pub fn rows(&self) -> std::cell::Ref<'_, Vec<ShownRow>> {
        self.rows.borrow()
    }
}

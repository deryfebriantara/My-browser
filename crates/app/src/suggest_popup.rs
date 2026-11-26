//! Dropdown saran address bar: jendela anak tanpa bingkai tepat di bawah
//! address field, berisi paling banyak 8 baris. Native, tanpa WebView, jadi
//! tidak menambah proses WebContent. Jendela tanpa bingkai tidak pernah
//! menjadi key window, jadi fokus dan kursor tetap di address field.

use crate::actions::ActionTarget;
use browser_core::suggest::{Suggestion, SuggestionKind};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSEvent, NSFont, NSImage,
    NSImageView, NSLineBreakMode, NSTextField, NSTitlePosition, NSView, NSWindow, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const ROW_HEIGHT: f64 = 36.0;
const PADDING: f64 = 4.0;
/// Lebar paling kecil: address field di sidebar lebih sempit dari isi saran.
const MIN_WIDTH: f64 = 480.0;

define_class!(
    /// Tombol transparan di atas satu baris. Menerima klik pertama walaupun
    /// jendelanya bukan key window.
    #[unsafe(super(NSButton, objc2_app_kit::NSControl, NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBSuggestRowButton"]
    struct RowButton;

    impl RowButton {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> Bool {
            Bool::YES
        }
    }
);

fn icon_for(kind: &SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Search => "magnifyingglass",
        SuggestionKind::Url => "globe",
        SuggestionKind::History => "clock",
        SuggestionKind::SwitchTab(_) => "square.on.square",
    }
}

fn label(mtm: MainThreadMarker, text: &str, size: f64, color: &NSColor, frame: NSRect) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize(size)));
    field.setTextColor(Some(color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setUsesSingleLineMode(true);
    field
}

pub struct SuggestPopup {
    window: Retained<NSWindow>,
    rows_view: Retained<NSView>,
}

impl SuggestPopup {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 100.0));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(true);

        let content = NSView::initWithFrame(NSView::alloc(mtm), rect);
        let background = NSBox::initWithFrame(NSBox::alloc(mtm), rect);
        background.setBoxType(NSBoxType::Custom);
        background.setTitlePosition(NSTitlePosition::NoTitle);
        background.setCornerRadius(8.0);
        background.setBorderWidth(1.0);
        background.setBorderColor(&NSColor::separatorColor());
        background.setFillColor(&NSColor::windowBackgroundColor());
        background.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let rows_view = NSView::initWithFrame(NSView::alloc(mtm), rect);
        rows_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&background);
        content.addSubview(&rows_view);
        window.setContentView(Some(&content));
        SuggestPopup { window, rows_view }
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    /// Frame jendela (koordinat layar), untuk pengujian.
    pub fn frame(&self) -> NSRect {
        self.window.frame()
    }

    /// Menampilkan `rows` di bawah `anchor`, dengan baris `selected` disorot.
    pub fn show(
        &self,
        rows: &[Suggestion],
        selected: usize,
        anchor: &NSTextField,
        parent: &NSWindow,
        target: &ActionTarget,
    ) {
        let field = anchor.convertRect_toView(anchor.bounds(), None);
        let field = parent.convertRectToScreen(field);
        let width = (field.size.width + 8.0).max(MIN_WIDTH);
        let height = rows.len() as f64 * ROW_HEIGHT + 2.0 * PADDING;
        let frame =
            NSRect::new(NSPoint::new(field.origin.x - 4.0, field.origin.y - 3.0 - height), NSSize::new(width, height));
        self.window.setFrame_display(frame, false);
        self.render(rows, selected, width, height, target);
        if self.window.parentWindow().is_none() {
            unsafe { parent.addChildWindow_ordered(&self.window, NSWindowOrderingMode::Above) };
        }
        self.window.orderFront(None);
    }

    pub fn hide(&self) {
        if let Some(parent) = self.window.parentWindow() {
            parent.removeChildWindow(&self.window);
        }
        self.window.orderOut(None);
    }

    fn render(&self, rows: &[Suggestion], selected: usize, width: f64, height: f64, target: &ActionTarget) {
        let mtm = MainThreadMarker::from(&*self.window);
        for view in self.rows_view.subviews().iter() {
            view.removeFromSuperview();
        }
        let inner = width - 2.0 * PADDING;
        for (i, row) in rows.iter().enumerate() {
            let y = height - PADDING - (i + 1) as f64 * ROW_HEIGHT;
            let chosen = i == selected;
            if chosen {
                let highlight = NSBox::initWithFrame(
                    NSBox::alloc(mtm),
                    NSRect::new(NSPoint::new(PADDING, y), NSSize::new(inner, ROW_HEIGHT)),
                );
                highlight.setBoxType(NSBoxType::Custom);
                highlight.setTitlePosition(NSTitlePosition::NoTitle);
                highlight.setCornerRadius(5.0);
                highlight.setBorderWidth(0.0);
                highlight.setFillColor(&NSColor::selectedContentBackgroundColor());
                self.rows_view.addSubview(&highlight);
            }
            let (title_color, detail_color, icon_color) = if chosen {
                (NSColor::whiteColor(), NSColor::colorWithWhite_alpha(1.0, 0.85), NSColor::whiteColor())
            } else {
                (NSColor::labelColor(), NSColor::secondaryLabelColor(), NSColor::secondaryLabelColor())
            };
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(icon_for(&row.kind)),
                None,
            ) {
                let icon = NSImageView::imageViewWithImage(&image, mtm);
                icon.setFrame(NSRect::new(NSPoint::new(PADDING + 10.0, y + 10.0), NSSize::new(16.0, 16.0)));
                icon.setContentTintColor(Some(&icon_color));
                self.rows_view.addSubview(&icon);
            }
            let text_x = PADDING + 36.0;
            let text_w = inner - 44.0;
            // Halaman tanpa judul: alamatnya cukup ditulis sekali, di tengah baris.
            let single = row.detail.is_empty() || row.detail == row.title;
            let title_y = if single { y + 10.0 } else { y + 17.0 };
            let title = label(
                mtm,
                &row.title,
                13.0,
                &title_color,
                NSRect::new(NSPoint::new(text_x, title_y), NSSize::new(text_w, 16.0)),
            );
            self.rows_view.addSubview(&title);
            if !single {
                let detail = label(
                    mtm,
                    &row.detail,
                    11.0,
                    &detail_color,
                    NSRect::new(NSPoint::new(text_x, y + 3.0), NSSize::new(text_w, 14.0)),
                );
                self.rows_view.addSubview(&detail);
            }

            let button: Retained<RowButton> = unsafe {
                msg_send![RowButton::alloc(mtm), initWithFrame: NSRect::new(NSPoint::new(PADDING, y), NSSize::new(inner, ROW_HEIGHT))]
            };
            button.setTransparent(true);
            button.setBordered(false);
            button.setTitle(&NSString::new());
            button.setTag(i as isize);
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(sel!(suggestionClicked:)));
                let description = NSString::from_str(&format!("{}, {}", row.title, row.detail));
                let _: () = msg_send![&*button, setAccessibilityLabel: &*description];
            }
            self.rows_view.addSubview(&button);
        }
    }
}

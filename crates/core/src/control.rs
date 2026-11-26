//! Protokol kontrol antara browser yang sedang berjalan dan server MCP
//! (`my-browser --mcp`). Satu baris JSON per permintaan dan per balasan,
//! lewat file socket pribadi di folder data.

use crate::engine::{ColorScheme, Emulation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Lokasi socket kontrol di folder data browser.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("control.sock")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeAction {
    Status,
    Play,
    Pause,
    Seek,
    Rate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    All,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitState {
    Visible,
    Present,
    Hidden,
}

/// Preset perangkat untuk `emulate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    /// iPhone 15: 390×844, user agent Safari iPhone.
    Mobile,
    /// iPad Air: 820×1180. iPadOS Safari memakai user agent desktop.
    Tablet,
    /// Laptop umum: 1440×900.
    Desktop,
    /// Kembali normal: seukuran jendela, user agent bawaan, warna sistem.
    Reset,
}

/// Batas ukuran viewport `emulate`, dalam poin CSS.
pub const VIEWPORT_RANGE: std::ops::RangeInclusive<u32> = 200..=3840;

/// Emulasi baru dari argumen `emulate`, dimulai dari emulasi tab sekarang.
pub fn emulation_for(
    current: Emulation,
    device: Option<Device>,
    width: Option<u32>,
    height: Option<u32>,
    color_scheme: Option<ColorScheme>,
) -> Result<Emulation, String> {
    if device.is_none() && width.is_none() && height.is_none() && color_scheme.is_none() {
        return Err("emulate butuh device, width/height, atau color_scheme.".into());
    }
    if let Some(bad) = [width, height].into_iter().flatten().find(|v| !VIEWPORT_RANGE.contains(v)) {
        return Err(format!(
            "Ukuran viewport {bad} di luar batas {}–{} poin.",
            VIEWPORT_RANGE.start(),
            VIEWPORT_RANGE.end()
        ));
    }
    let mut next = current;
    match device {
        Some(Device::Mobile) => (next.viewport, next.mobile) = (Some((390.0, 844.0)), true),
        Some(Device::Tablet) => (next.viewport, next.mobile) = (Some((820.0, 1180.0)), false),
        Some(Device::Desktop) => (next.viewport, next.mobile) = (Some((1440.0, 900.0)), false),
        Some(Device::Reset) => next = Emulation::default(),
        None => {}
    }
    if width.is_some() || height.is_some() {
        let (w, h) = next.viewport.unwrap_or((1440.0, 900.0));
        next.viewport = Some((width.map_or(w, f64::from), height.map_or(h, f64::from)));
    }
    if let Some(scheme) = color_scheme {
        next.color_scheme = scheme;
    }
    Ok(next)
}

fn yes() -> bool {
    true
}

/// Permintaan dari server MCP. Nama `cmd` sama dengan nama tool MCP, dan
/// argumen tool sama dengan field-nya.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    ListTabs,
    OpenTab {
        url: String,
        #[serde(default = "yes")]
        activate: bool,
    },
    CloseTab {
        tab_id: u64,
    },
    ActivateTab {
        tab_id: u64,
    },
    Navigate {
        url: String,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    GoBack {
        #[serde(default)]
        tab_id: Option<u64>,
    },
    GoForward {
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Reload {
        #[serde(default)]
        tab_id: Option<u64>,
    },
    SleepTab {
        tab_id: u64,
    },
    MemoryUsage,
    /// Mencari riwayat halaman yang pernah dibuka (baca saja).
    SearchHistory {
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        days: Option<u32>,
    },
    /// Pengait uji khusus build debug: menggerakkan address bar, menu, dan
    /// jendela lewat jalur AppKit sungguhan. Tidak ada di build release.
    #[cfg(debug_assertions)]
    DebugUi {
        action: String,
        #[serde(default)]
        text: Option<String>,
    },
    ReadPage {
        #[serde(default)]
        tab_id: Option<u64>,
        #[serde(default)]
        max_chars: Option<usize>,
    },
    ListElements {
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Scroll {
        direction: ScrollDirection,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Youtube {
        action: YoutubeAction,
        #[serde(default)]
        value: Option<f64>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Screenshot {
        #[serde(default)]
        tab_id: Option<u64>,
        /// Seluruh halaman, dipotong menjadi beberapa gambar.
        #[serde(default)]
        full_page: bool,
        /// Hanya elemen ini (selector CSS).
        #[serde(default)]
        selector: Option<String>,
    },
    Emulate {
        #[serde(default)]
        device: Option<Device>,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        color_scheme: Option<ColorScheme>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    /// Elemen yang terakhir ditunjuk pengguna lewat menu Pick Element for Claude.
    GetSelection {
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Click {
        element: u32,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    Type {
        element: u32,
        text: String,
        #[serde(default)]
        submit: bool,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    RunJs {
        code: String,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    ConsoleLogs {
        #[serde(default)]
        level: Option<LogLevel>,
        #[serde(default)]
        since: Option<u64>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    NetworkLog {
        #[serde(default)]
        failed_only: bool,
        #[serde(default)]
        url_contains: Option<String>,
        #[serde(default)]
        include_resources: bool,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    WaitFor {
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        state: Option<WaitState>,
        #[serde(default)]
        timeout_ms: Option<u64>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
    InspectElement {
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        element: Option<u32>,
        #[serde(default)]
        styles: Option<Vec<String>>,
        #[serde(default)]
        tab_id: Option<u64>,
    },
}

impl ControlRequest {
    /// Aksi yang mengubah isi halaman atas nama pengguna. Hanya dijalankan
    /// kalau pengguna menyalakan "Allow Claude to Interact".
    pub fn needs_interaction(&self) -> bool {
        matches!(self, Self::Click { .. } | Self::Type { .. } | Self::RunJs { .. })
    }

    /// Hanya membaca halaman atau keadaan browser. Panel Tanya Claude hanya
    /// boleh mengirim permintaan seperti ini.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ListTabs
                | Self::ReadPage { .. }
                | Self::ListElements { .. }
                | Self::Scroll { .. }
                | Self::Screenshot { .. }
                | Self::Youtube { .. }
                | Self::MemoryUsage
                | Self::SearchHistory { .. }
                | Self::ConsoleLogs { .. }
                | Self::NetworkLog { .. }
                | Self::WaitFor { .. }
                | Self::InspectElement { .. }
                | Self::GetSelection { .. }
        )
    }

    /// Tab yang dituju permintaan, kalau ada.
    pub fn tab_id(&self) -> Option<u64> {
        match self {
            Self::CloseTab { tab_id } | Self::ActivateTab { tab_id } | Self::SleepTab { tab_id } => Some(*tab_id),
            Self::Navigate { tab_id, .. }
            | Self::GoBack { tab_id }
            | Self::GoForward { tab_id }
            | Self::Reload { tab_id }
            | Self::ReadPage { tab_id, .. }
            | Self::ListElements { tab_id }
            | Self::Scroll { tab_id, .. }
            | Self::Youtube { tab_id, .. }
            | Self::Screenshot { tab_id, .. }
            | Self::Emulate { tab_id, .. }
            | Self::GetSelection { tab_id }
            | Self::Click { tab_id, .. }
            | Self::Type { tab_id, .. }
            | Self::RunJs { tab_id, .. }
            | Self::ConsoleLogs { tab_id, .. }
            | Self::NetworkLog { tab_id, .. }
            | Self::WaitFor { tab_id, .. }
            | Self::InspectElement { tab_id, .. } => *tab_id,
            Self::ListTabs | Self::OpenTab { .. } | Self::MemoryUsage | Self::SearchHistory { .. } => None,
            #[cfg(debug_assertions)]
            Self::DebugUi { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlResponse {
    Ok(Value),
    Error(String),
}

/// Satu tab dalam balasan `list_tabs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub url: Option<String>,
    pub active: bool,
    pub asleep: bool,
    pub loading: bool,
    pub loads: u64,
    /// Bagian sidebar: favorite, pinned, atau today.
    pub kind: crate::tabs::TabKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tab_info_includes_kind() {
        let info = TabInfo {
            id: 3,
            title: "P".into(),
            url: Some("https://p.example/".into()),
            active: false,
            asleep: true,
            loading: false,
            loads: 1,
            kind: crate::tabs::TabKind::Pinned,
        };
        assert!(serde_json::to_string(&info).unwrap().contains("\"kind\":\"pinned\""));
    }

    fn parse(v: Value) -> ControlRequest {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn tool_arguments_map_to_requests_with_defaults() {
        assert_eq!(parse(json!({"cmd": "list_tabs"})), ControlRequest::ListTabs);
        assert_eq!(
            parse(json!({"cmd": "open_tab", "url": "example.com"})),
            ControlRequest::OpenTab { url: "example.com".into(), activate: true }
        );
        assert_eq!(
            parse(json!({"cmd": "navigate", "url": "x.com"})),
            ControlRequest::Navigate { url: "x.com".into(), tab_id: None }
        );
        assert_eq!(
            parse(json!({"cmd": "type", "element": 3, "text": "halo"})),
            ControlRequest::Type { element: 3, text: "halo".into(), submit: false, tab_id: None }
        );
        assert_eq!(
            parse(json!({"cmd": "youtube", "action": "seek", "value": 90.0})),
            ControlRequest::Youtube { action: YoutubeAction::Seek, value: Some(90.0), tab_id: None }
        );
    }

    #[test]
    fn developer_tool_arguments_have_defaults() {
        assert_eq!(
            parse(json!({"cmd": "console_logs", "level": "error"})),
            ControlRequest::ConsoleLogs { level: Some(LogLevel::Error), since: None, limit: None, tab_id: None }
        );
        assert_eq!(
            parse(json!({"cmd": "network_log", "failed_only": true})),
            ControlRequest::NetworkLog {
                failed_only: true,
                url_contains: None,
                include_resources: false,
                limit: None,
                tab_id: None
            }
        );
        assert_eq!(
            parse(json!({"cmd": "wait_for", "selector": "#app", "state": "hidden"})),
            ControlRequest::WaitFor {
                selector: Some("#app".into()),
                text: None,
                state: Some(WaitState::Hidden),
                timeout_ms: None,
                tab_id: None
            }
        );
        assert_eq!(
            parse(json!({"cmd": "inspect_element", "element": 4, "styles": ["color"]})),
            ControlRequest::InspectElement {
                selector: None,
                element: Some(4),
                styles: Some(vec!["color".into()]),
                tab_id: None
            }
        );
    }

    #[test]
    fn visual_tool_arguments_have_defaults() {
        assert_eq!(
            parse(json!({"cmd": "screenshot"})),
            ControlRequest::Screenshot { tab_id: None, full_page: false, selector: None }
        );
        assert_eq!(
            parse(json!({"cmd": "screenshot", "selector": ".hero"})),
            ControlRequest::Screenshot { tab_id: None, full_page: false, selector: Some(".hero".into()) }
        );
        assert_eq!(
            parse(json!({"cmd": "emulate", "device": "mobile", "color_scheme": "dark"})),
            ControlRequest::Emulate {
                device: Some(Device::Mobile),
                width: None,
                height: None,
                color_scheme: Some(ColorScheme::Dark),
                tab_id: None
            }
        );
        assert_eq!(parse(json!({"cmd": "get_selection"})), ControlRequest::GetSelection { tab_id: None });
    }

    #[test]
    fn emulation_presets_and_overrides() {
        let none = Emulation::default();
        let phone = emulation_for(none, Some(Device::Mobile), None, None, None).unwrap();
        assert_eq!(
            phone,
            Emulation { viewport: Some((390.0, 844.0)), mobile: true, color_scheme: ColorScheme::System }
        );
        let dark = emulation_for(phone, None, None, None, Some(ColorScheme::Dark)).unwrap();
        assert_eq!(dark, Emulation { color_scheme: ColorScheme::Dark, ..phone }, "warna saja: perangkat tetap");
        let narrow = emulation_for(dark, None, Some(360), None, None).unwrap();
        assert_eq!(narrow.viewport, Some((360.0, 844.0)), "lebar saja: tinggi tetap");
        assert!(narrow.mobile);
        let custom = emulation_for(none, None, Some(1024), Some(768), None).unwrap();
        assert_eq!(
            custom,
            Emulation { viewport: Some((1024.0, 768.0)), mobile: false, color_scheme: ColorScheme::System }
        );
        let tablet = emulation_for(phone, Some(Device::Tablet), None, None, None).unwrap();
        assert_eq!((tablet.viewport, tablet.mobile), (Some((820.0, 1180.0)), false));
    }

    #[test]
    fn emulation_reset_and_invalid_input() {
        let dark_phone = Emulation { viewport: Some((390.0, 844.0)), mobile: true, color_scheme: ColorScheme::Dark };
        assert_eq!(emulation_for(dark_phone, Some(Device::Reset), None, None, None).unwrap(), Emulation::default());
        assert_eq!(
            emulation_for(dark_phone, Some(Device::Reset), None, None, Some(ColorScheme::Light)).unwrap().color_scheme,
            ColorScheme::Light
        );
        assert!(emulation_for(dark_phone, None, None, None, None).is_err());
        assert!(emulation_for(dark_phone, None, Some(50), None, None).unwrap_err().contains("50"));
        assert!(emulation_for(dark_phone, None, None, Some(10_000), None).is_err());
    }

    #[test]
    fn unknown_or_invalid_arguments_are_rejected() {
        assert!(serde_json::from_value::<ControlRequest>(json!({"cmd": "navigate"})).is_err());
        assert!(serde_json::from_value::<ControlRequest>(json!({"cmd": "hapus_semua"})).is_err());
        assert!(serde_json::from_value::<ControlRequest>(json!({"cmd": "click", "element": 1, "x": 2})).is_err());
    }

    #[test]
    fn only_page_changing_actions_need_interaction() {
        assert!(parse(json!({"cmd": "click", "element": 1})).needs_interaction());
        assert!(parse(json!({"cmd": "type", "element": 1, "text": "a"})).needs_interaction());
        assert!(parse(json!({"cmd": "run_js", "code": "1"})).needs_interaction());
        for cmd in [
            "list_tabs",
            "memory_usage",
            "read_page",
            "list_elements",
            "screenshot",
            "console_logs",
            "network_log",
            "wait_for",
            "inspect_element",
            "get_selection",
            "search_history",
        ] {
            assert!(!parse(json!({"cmd": cmd})).needs_interaction(), "{cmd}");
        }
        assert!(!parse(json!({"cmd": "scroll", "direction": "down"})).needs_interaction());
        assert!(!parse(json!({"cmd": "emulate", "device": "mobile"})).needs_interaction());
    }

    #[test]
    fn search_history_parses_and_is_read_only() {
        assert_eq!(
            parse(json!({"cmd": "search_history"})),
            ControlRequest::SearchHistory { query: None, limit: None, days: None }
        );
        let r = parse(json!({"cmd": "search_history", "query": "youtube", "limit": 5, "days": 7}));
        assert_eq!(r, ControlRequest::SearchHistory { query: Some("youtube".into()), limit: Some(5), days: Some(7) });
        assert!(r.is_read_only());
        assert_eq!(r.tab_id(), None);
        assert!(serde_json::from_value::<ControlRequest>(json!({"cmd": "search_history", "url": "x"})).is_err());
    }

    #[test]
    fn navigation_and_interaction_are_not_read_only() {
        for (cmd, extra) in [
            ("navigate", json!({"url": "x"})),
            ("open_tab", json!({"url": "x"})),
            ("close_tab", json!({"tab_id": 1})),
            ("click", json!({"element": 1})),
            ("run_js", json!({"code": "1"})),
            ("emulate", json!({"device": "mobile"})),
            ("reload", json!({})),
        ] {
            let mut v = extra;
            v["cmd"] = json!(cmd);
            assert!(!parse(v).is_read_only(), "{cmd}");
        }
    }

    #[test]
    fn responses_are_tagged_ok_or_error() {
        assert_eq!(serde_json::to_value(ControlResponse::Ok(json!(1))).unwrap(), json!({"ok": 1}));
        assert_eq!(serde_json::to_value(ControlResponse::Error("x".into())).unwrap(), json!({"error": "x"}));
    }
}

//! Protokol kontrol antara browser yang sedang berjalan dan server MCP
//! (`my-browser --mcp`). Satu baris JSON per permintaan dan per balasan,
//! lewat file socket pribadi di folder data.

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
    /// kalau pengguna menyalakan "Izinkan Claude Berinteraksi".
    pub fn needs_interaction(&self) -> bool {
        matches!(self, Self::Click { .. } | Self::Type { .. } | Self::RunJs { .. })
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
            | Self::Screenshot { tab_id }
            | Self::Click { tab_id, .. }
            | Self::Type { tab_id, .. }
            | Self::RunJs { tab_id, .. }
            | Self::ConsoleLogs { tab_id, .. }
            | Self::NetworkLog { tab_id, .. }
            | Self::WaitFor { tab_id, .. }
            | Self::InspectElement { tab_id, .. } => *tab_id,
            Self::ListTabs | Self::OpenTab { .. } | Self::MemoryUsage => None,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            ControlRequest::InspectElement { selector: None, element: Some(4), styles: Some(vec!["color".into()]), tab_id: None }
        );
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
            "list_tabs", "memory_usage", "read_page", "list_elements", "screenshot",
            "console_logs", "network_log", "wait_for", "inspect_element",
        ] {
            assert!(!parse(json!({"cmd": cmd})).needs_interaction(), "{cmd}");
        }
        assert!(!parse(json!({"cmd": "scroll", "direction": "down"})).needs_interaction());
    }

    #[test]
    fn responses_are_tagged_ok_or_error() {
        assert_eq!(serde_json::to_value(ControlResponse::Ok(json!(1))).unwrap(), json!({"ok": 1}));
        assert_eq!(serde_json::to_value(ControlResponse::Error("x".into())).unwrap(), json!({"error": "x"}));
    }
}

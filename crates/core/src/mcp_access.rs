//! Siapa yang boleh mengontrol browser lewat socket kontrol.
//!
//! Socket hanya bisa dibuka pengguna yang sama, tetapi itu berarti program
//! apa pun yang pengguna jalankan. Karena itu setiap program diminta izin
//! dulu. Identitasnya diambil dari kernel (pid peer socket dan lokasi
//! executable), bukan dari pengakuan program itu sendiri.

/// Jenis program di ujung lain socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientKind {
    /// Server MCP milik panel Tanya Claude: proses Claude yang dijalankan
    /// browser ini sendiri. Disetujui otomatis, tetapi hanya boleh membaca.
    Panel,
    /// Program lain (mis. Claude Code), dikenali dari lokasi executable-nya.
    External { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    /// Tanyakan ke pengguna.
    Ask,
}

/// Keputusan untuk sebuah program: `always` dari pengaturan (Selalu
/// Izinkan), `denied` dari penolakan selama browser terbuka.
pub fn decide(kind: &ClientKind, always: &[String], denied: &[String]) -> Access {
    match kind {
        ClientKind::Panel => Access::Allowed,
        ClientKind::External { path } if always.contains(path) => Access::Allowed,
        ClientKind::External { path } if denied.contains(path) => Access::Denied,
        ClientKind::External { .. } => Access::Ask,
    }
}

/// Nama yang mudah dikenali untuk dialog izin.
pub fn display_name(path: &str) -> String {
    if path.contains("@anthropic-ai/claude-code") || path.ends_with("/claude") || path.ends_with("/claude.exe") {
        return "Claude Code".into();
    }
    // Bundle terdalam: Python.app di dalam Xcode.app adalah Python.
    if let Some(app) = path.split('/').rfind(|part| part.ends_with(".app")) {
        return app.trim_end_matches(".app").to_string();
    }
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(path: &str) -> ClientKind {
        ClientKind::External { path: path.into() }
    }

    #[test]
    fn panel_is_always_allowed_and_others_need_permission() {
        assert_eq!(decide(&ClientKind::Panel, &[], &[]), Access::Allowed);
        assert_eq!(decide(&ext("/bin/claude"), &[], &[]), Access::Ask);
        assert_eq!(decide(&ext("/bin/claude"), &["/bin/claude".into()], &[]), Access::Allowed);
        assert_eq!(decide(&ext("/tmp/jahat"), &["/bin/claude".into()], &["/tmp/jahat".into()]), Access::Denied);
        assert_eq!(decide(&ext("/tmp/lain"), &["/bin/claude".into()], &[]), Access::Ask, "izin per lokasi program");
    }

    #[test]
    fn names_are_friendly() {
        let claude = "/Users/a/.nvm/versions/node/v22/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe";
        assert_eq!(display_name(claude), "Claude Code");
        assert_eq!(display_name("/Applications/Cursor.app/Contents/MacOS/Cursor"), "Cursor");
        assert_eq!(display_name("/usr/bin/python3"), "python3");
        let python = "/Applications/Xcode.app/Contents/Developer/Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python";
        assert_eq!(display_name(python), "Python");
    }
}

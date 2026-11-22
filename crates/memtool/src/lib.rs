//! Mengelompokkan physical footprint proses per aplikasi.

pub mod sys;

/// Satu proses beserta pid aplikasi yang bertanggung jawab atasnya.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub name: String,
    pub responsible_pid: i32,
    /// Physical footprint dalam byte (sama dengan kolom Memory di Activity Monitor).
    pub footprint: u64,
}

/// Total pemakaian satu aplikasi beserta semua proses pembantunya.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUsage {
    pub app: String,
    pub processes: Vec<ProcInfo>,
    pub footprint: u64,
}

/// Untuk setiap nama aplikasi, cari proses utamanya (nama sama dan bertanggung
/// jawab atas dirinya sendiri), lalu kumpulkan semua proses yang responsible
/// pid-nya menunjuk ke proses utama itu.
pub fn group(procs: &[ProcInfo], apps: &[&str]) -> Vec<AppUsage> {
    apps.iter()
        .map(|&app| {
            let mains: Vec<i32> =
                procs.iter().filter(|p| p.name == app && p.responsible_pid == p.pid).map(|p| p.pid).collect();
            let processes: Vec<ProcInfo> =
                procs.iter().filter(|p| mains.contains(&p.responsible_pid)).cloned().collect();
            let footprint = processes.iter().map(|p| p.footprint).sum();
            AppUsage { app: app.to_string(), processes, footprint }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, name: &str, resp: i32, mb: u64) -> ProcInfo {
        ProcInfo { pid, name: name.into(), responsible_pid: resp, footprint: mb * 1024 * 1024 }
    }

    #[test]
    fn groups_helpers_under_responsible_app() {
        let procs = vec![
            p(10, "Safari", 10, 200),
            p(11, "com.apple.WebKit.WebContent", 10, 300),
            p(12, "com.apple.WebKit.Networking", 10, 50),
            p(20, "Mail", 20, 100),
            p(21, "com.apple.WebKit.WebContent", 20, 80),
        ];
        let out = group(&procs, &["Safari"]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].processes.len(), 3);
        assert_eq!(out[0].footprint, 550 * 1024 * 1024);
    }

    #[test]
    fn missing_app_reports_zero() {
        let out = group(&[], &["Safari"]);
        assert_eq!(out[0].processes.len(), 0);
        assert_eq!(out[0].footprint, 0);
    }
}

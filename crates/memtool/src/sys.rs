//! Akses libproc untuk membaca daftar proses, nama, responsible pid, dan
//! physical footprint.

use crate::ProcInfo;
use libc::{c_int, c_void, pid_t};

unsafe extern "C" {
    // Tidak ada di header publik, tetapi diekspor libSystem dan dipakai Activity
    // Monitor untuk mengatribusikan proses XPC (misalnya WebContent) ke aplikasi.
    fn responsibility_get_pid_responsible_for_pid(pid: pid_t) -> pid_t;
}

fn all_pids() -> Vec<pid_t> {
    let mut pids: Vec<pid_t> = vec![0; 8192];
    let bytes = (pids.len() * size_of::<pid_t>()) as c_int;
    let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr() as *mut c_void, bytes) };
    if n <= 0 {
        return Vec::new();
    }
    pids.truncate(n as usize);
    pids.retain(|&p| p > 0);
    pids
}

fn name_of(pid: pid_t) -> Option<String> {
    let mut buf = [0u8; 256];
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

fn footprint_of(pid: pid_t) -> Option<u64> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let rc =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, &mut info as *mut _ as *mut libc::rusage_info_t) };
    (rc == 0).then_some(info.ri_phys_footprint)
}

/// Semua proses yang bisa dibaca oleh pengguna saat ini.
pub fn list_processes() -> Vec<ProcInfo> {
    all_pids()
        .into_iter()
        .filter_map(|pid| {
            let name = name_of(pid)?;
            let footprint = footprint_of(pid)?;
            let responsible_pid = unsafe { responsibility_get_pid_responsible_for_pid(pid) };
            let responsible_pid = if responsible_pid > 0 { responsible_pid } else { pid };
            Some(ProcInfo { pid, name, responsible_pid, footprint })
        })
        .collect()
}

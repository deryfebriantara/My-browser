//! Identitas proses di ujung lain socket kontrol, dari kernel (libproc),
//! bukan dari pengakuan proses itu sendiri.

use browser_core::mcp_access::ClientKind;
use libc::{c_char, c_int, c_void, pid_t};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Pid proses yang membuka koneksi socket.
pub fn peer_pid(conn: &UnixStream) -> Option<pid_t> {
    let mut pid: pid_t = 0;
    let mut len = size_of::<pid_t>() as libc::socklen_t;
    // SAFETY: buffer seukuran pid_t, panjang diberikan.
    let rc = unsafe {
        libc::getsockopt(
            conn.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            &mut pid as *mut pid_t as *mut c_void,
            &mut len,
        )
    };
    (rc == 0 && pid > 0).then_some(pid)
}

/// Lokasi executable proses (symlink sudah diselesaikan kernel).
pub fn exe_path(pid: pid_t) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: buffer dan ukurannya cocok.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

pub fn parent_pid(pid: pid_t) -> Option<pid_t> {
    // SAFETY: struct POD diisi kernel; ukuran dicek dari nilai kembalian.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut c_void, size) };
    (n == size && info.pbi_ppid > 0).then_some(info.pbi_ppid as pid_t)
}

/// Folder kerja proses, untuk membantu pengguna mengenali sesi Claude Code.
pub fn working_dir(pid: pid_t) -> Option<String> {
    // SAFETY: seperti parent_pid.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_vnodepathinfo>() as c_int;
    let n =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, &mut info as *mut _ as *mut c_void, size) };
    if n != size {
        return None;
    }
    let raw: Vec<u8> =
        info.pvi_cdir.vip_path.iter().flatten().map(|&c: &c_char| c as u8).take_while(|&b| b != 0).collect();
    (!raw.is_empty()).then(|| String::from_utf8_lossy(&raw).into_owned())
}

/// Program yang meminta mengontrol browser.
#[derive(Debug, Clone)]
pub struct Client {
    /// Nomor koneksi (unik selama browser hidup).
    pub id: u64,
    pub kind: ClientKind,
    pub pid: pid_t,
    pub path: String,
    pub cwd: Option<String>,
}

/// `peer` adalah proses yang membuka socket. Server MCP (`my-browser --mcp`)
/// hanya perantara, jadi yang meminta akses adalah induknya (mis. Claude
/// Code). Kalau induk peminta adalah browser ini sendiri, itu panel Tanya
/// Claude; silsilah ini tidak bisa dipalsukan program lain.
pub fn identify(peer: pid_t, me: pid_t, id: u64) -> Option<Client> {
    let peer_path = exe_path(peer)?;
    let relay = peer != me && Path::new(&peer_path).file_name().is_some_and(|n| n == "my-browser");
    let requester = if relay { parent_pid(peer)? } else { peer };
    let path = exe_path(requester)?;
    let kind = if requester != me && parent_pid(requester) == Some(me) {
        ClientKind::Panel
    } else {
        ClientKind::External { path: path.clone() }
    };
    Some(Client { id, kind, pid: requester, path, cwd: working_dir(requester) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn own_process_is_identified_from_the_kernel() {
        let me = std::process::id() as pid_t;
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert_eq!(exe_path(me).map(std::path::PathBuf::from).map(|p| p.canonicalize().unwrap()), Some(exe.clone()));
        assert!(parent_pid(me).is_some_and(|p| p > 0));
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(working_dir(me).map(|d| std::path::PathBuf::from(d).canonicalize().unwrap()), Some(cwd));
        assert_eq!(exe_path(-5), None);
    }

    #[test]
    fn socket_peer_is_this_process_and_not_a_panel() {
        let dir = std::env::temp_dir().join(format!("mb-peer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        let me = std::process::id() as pid_t;
        assert_eq!(peer_pid(&server), Some(me));
        // Dilihat dari "browser" lain (pid 1): proses test ini program biasa.
        let identified = identify(me, 1, 7).unwrap();
        assert_eq!(identified.id, 7);
        assert!(
            matches!(identified.kind, ClientKind::External { ref path } if path.contains("my_browser")),
            "{identified:?}"
        );
        // Dilihat dari induknya sendiri: proses anak browser = panel.
        let parent = parent_pid(me).unwrap();
        assert_eq!(identify(me, parent, 8).unwrap().kind, ClientKind::Panel);
        drop(client);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

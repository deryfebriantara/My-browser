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

/// Lokasi executable proses (symlink sudah diselesaikan kernel). Kalau berkas
/// programnya sudah diganti sejak proses berjalan, lihat `launch_path`.
pub fn exe_path(pid: pid_t) -> Option<String> {
    kernel_path(pid).or_else(|| launch_path(pid))
}

/// `proc_pidpath`: jalur berkas yang sedang dijalankan proses. Gagal kalau
/// berkas itu sudah dihapus atau diganti (mis. Claude Code memperbarui dirinya
/// sendiri saat sesinya masih berjalan).
fn kernel_path(pid: pid_t) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: buffer dan ukurannya cocok.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

/// Jalur yang dipakai saat proses dijalankan (`KERN_PROCARGS2`), dengan
/// symlink diselesaikan seperti `proc_pidpath` (mis. `~/.nvm/.../bin/claude`
/// menjadi `.../claude-code/bin/claude.exe`, sama dengan daftar izin).
/// Cadangan untuk `kernel_path`: kurang kuat karena disimpan di memory proses
/// itu, jadi hanya diterima kalau jalurnya absolut dan berkas program memang
/// ada di sana sekarang (versi baru yang menggantikan berkas lama).
fn launch_path(pid: pid_t) -> Option<String> {
    let mut argmax: c_int = 0;
    let mut len = size_of::<c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: mib dua elemen, keluaran satu c_int dengan panjangnya.
    let rc = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 2, &mut argmax as *mut c_int as *mut c_void, &mut len, std::ptr::null_mut(), 0)
    };
    if rc != 0 || argmax <= 0 {
        return None;
    }
    let mut buf = vec![0u8; argmax as usize];
    let mut size = buf.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: buffer seukuran KERN_ARGMAX; kernel menulis paling banyak `size` byte.
    let rc = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0)
    };
    if rc != 0 || size <= size_of::<c_int>() {
        return None;
    }
    // Susunannya: argc (c_int), lalu jalur program diakhiri NUL.
    let rest = &buf[size_of::<c_int>()..size];
    let end = rest.iter().position(|&b| b == 0)?;
    let path = std::str::from_utf8(&rest[..end]).ok()?;
    if !path.starts_with('/') {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    real.is_file().then(|| real.to_string_lossy().into_owned())
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
    fn launch_path_matches_this_executable() {
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        let launched =
            launch_path(std::process::id() as pid_t).map(|p| std::path::PathBuf::from(p).canonicalize().unwrap());
        assert_eq!(launched, Some(exe));
    }

    /// Seperti Claude Code yang memperbarui dirinya saat sesinya berjalan:
    /// berkas program diganti, proc_pidpath gagal, tetapi proses tetap
    /// dikenali dari jalur saat dijalankan selama berkas baru ada di sana.
    /// Dipanggil sebagai proses anak oleh test di bawah: hanya tidur.
    #[test]
    #[ignore = "pembantu: dijalankan sebagai proses anak oleh test lain"]
    fn helper_sleeps() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    /// Menjalankan `path` (salinan biner test ini) sebagai proses yang tidur.
    /// Salinan biner Apple (mis. /bin/sleep) tidak bisa dipakai: macOS menolak
    /// KERN_PROCARGS2 untuk biner sistem yang dijalankan dari luar lokasinya.
    fn sleeper(path: &Path) -> std::process::Child {
        let child = std::process::Command::new(path)
            .args(["--ignored", "--exact", "procinfo::tests::helper_sleeps"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        child
    }

    fn stop(mut child: std::process::Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn launch_path_resolves_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("program-asli");
        let link = dir.path().join("program");
        std::fs::copy(std::env::current_exe().unwrap(), &real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let child = sleeper(&link);
        let found = launch_path(child.id() as pid_t);
        stop(child);
        assert_eq!(found.map(std::path::PathBuf::from), Some(real.canonicalize().unwrap()));
    }

    /// Seperti Claude Code yang memperbarui dirinya saat sesinya berjalan:
    /// berkas program diganti, dan proses tetap dikenali dari jalur saat
    /// dijalankan selama ada berkas program di jalur itu.
    #[test]
    fn replaced_executable_is_still_identified() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("program-uji");
        std::fs::copy(std::env::current_exe().unwrap(), &bin).unwrap();
        let expected = bin.canonicalize().unwrap();
        let child = sleeper(&bin);
        let pid = child.id() as pid_t;

        std::fs::remove_file(&bin).unwrap();
        let without_file = (kernel_path(pid), launch_path(pid), exe_path(pid));
        std::fs::copy(std::env::current_exe().unwrap(), &bin).unwrap();
        let replaced = (launch_path(pid), exe_path(pid));
        stop(child);

        assert_eq!(without_file, (None, None, None), "berkas dihapus tanpa pengganti: tidak dikenali");
        let as_path = |p: Option<String>| p.map(std::path::PathBuf::from);
        assert_eq!(as_path(replaced.0), Some(expected.clone()), "jalur saat dijalankan menunjuk berkas baru");
        assert_eq!(as_path(replaced.1), Some(expected));
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

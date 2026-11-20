//! Sambungan ke socket kontrol browser. Kalau browser belum berjalan,
//! bundle aplikasinya dibuka sekali lalu sambungan dicoba lagi.

use crate::server::Transport;
use browser_core::control::{ControlRequest, ControlResponse};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const REPLY_TIMEOUT: Duration = Duration::from_secs(45);

pub struct SocketTransport {
    path: PathBuf,
    app: Option<PathBuf>,
    conn: Option<(UnixStream, BufReader<UnixStream>)>,
    launched: bool,
}

impl SocketTransport {
    pub fn new(path: PathBuf, app: Option<PathBuf>) -> Self {
        Self { path, app, conn: None, launched: false }
    }

    fn try_connect(&mut self) -> bool {
        let Ok(stream) = UnixStream::connect(&self.path) else { return false };
        let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
        let Ok(reader) = stream.try_clone() else { return false };
        self.conn = Some((stream, BufReader::new(reader)));
        true
    }

    fn connect(&mut self) -> Result<(), String> {
        if self.try_connect() {
            return Ok(());
        }
        let Some(app) = self.app.clone().filter(|_| !self.launched) else {
            return Err("Browser my-browser tidak berjalan. Buka my-browser dulu.".into());
        };
        self.launched = true;
        std::process::Command::new("open")
            .arg(&app)
            .status()
            .map_err(|e| format!("Gagal membuka my-browser: {e}"))?;
        let start = Instant::now();
        while start.elapsed() < LAUNCH_TIMEOUT {
            std::thread::sleep(Duration::from_millis(250));
            if self.try_connect() {
                return Ok(());
            }
        }
        Err("my-browser sudah dibuka tetapi belum menjawab. Coba lagi sebentar lagi.".into())
    }
}

impl Transport for SocketTransport {
    fn call(&mut self, request: &ControlRequest) -> Result<ControlResponse, String> {
        let line = serde_json::to_string(request).map_err(|e| e.to_string())?;
        // Sambungan lama bisa putus kalau browser ditutup lalu dibuka lagi.
        // Permintaan hanya dikirim ulang kalau gagal saat menulis, supaya aksi
        // tidak pernah dijalankan dua kali.
        for attempt in 0..2 {
            if self.conn.is_none() {
                self.connect()?;
            }
            let (stream, reader) = self.conn.as_mut().expect("baru tersambung");
            if writeln!(stream, "{line}").and_then(|_| stream.flush()).is_err() {
                self.conn = None;
                if attempt == 0 {
                    continue;
                }
                return Err("Sambungan ke my-browser putus.".into());
            }
            let mut reply = String::new();
            return match reader.read_line(&mut reply) {
                Ok(n) if n > 0 => serde_json::from_str(&reply).map_err(|e| format!("Balasan browser tidak valid: {e}")),
                _ => {
                    self.conn = None;
                    Err("my-browser tidak menjawab (mungkin sedang ditutup).".into())
                }
            };
        }
        unreachable!("loop selalu kembali")
    }
}

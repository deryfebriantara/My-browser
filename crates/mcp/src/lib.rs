//! Server MCP untuk Claude Code (`my-browser --mcp`). Menerima JSON-RPC lewat
//! stdin/stdout dan meneruskan setiap panggilan tool ke browser yang sedang
//! berjalan lewat socket kontrol.

mod server;
mod socket;
mod tools;

pub use server::{Server, Transport};
pub use socket::SocketTransport;

use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Menjalankan server MCP sampai stdin ditutup. `app` adalah bundle
/// aplikasi yang dibuka otomatis kalau browser belum berjalan.
pub fn run_stdio(socket: PathBuf, app: Option<PathBuf>) {
    let mut server = Server::new(SocketTransport::new(socket, app));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&line)
            && writeln!(out, "{response}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
}

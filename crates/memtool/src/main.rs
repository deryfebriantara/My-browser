//! Pemakaian: memtool [--detail] <nama-app>...
//! Mencetak total physical footprint per aplikasi beserta proses pembantunya.

use memtool::{group, sys};

const MB: f64 = 1024.0 * 1024.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let detail = args.iter().any(|a| a == "--detail");
    let apps: Vec<&str> = args.iter().filter(|a| *a != "--detail").map(String::as_str).collect();
    if apps.is_empty() {
        eprintln!("pemakaian: memtool [--detail] <nama-app>...");
        std::process::exit(2);
    }

    let procs = sys::list_processes();
    for usage in group(&procs, &apps) {
        println!(
            "{:<20} {:>3} proses  {:>9.1} MB",
            usage.app,
            usage.processes.len(),
            usage.footprint as f64 / MB
        );
        if detail {
            let mut list = usage.processes.clone();
            list.sort_by(|a, b| b.footprint.cmp(&a.footprint));
            for p in list {
                println!("    {:>6}  {:<40} {:>9.1} MB", p.pid, p.name, p.footprint as f64 / MB);
            }
        }
    }
}

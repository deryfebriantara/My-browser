//! Pemakaian: blocklist-gen <folder-filter> <folder-keluaran>
//! Membaca semua *.txt di folder filter, menulis blocklist-<i>.json.

use blocklist_gen::{MAX_RULES_PER_LIST, build_lists};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("pemakaian: blocklist-gen <folder-filter> <folder-keluaran>");
        std::process::exit(2);
    };
    let mut files: Vec<_> = std::fs::read_dir(input)
        .expect("folder filter bisa dibaca")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let text: String = files.iter().map(|p| std::fs::read_to_string(p).expect("filter bisa dibaca") + "\n").collect();

    let (chunks, skipped) = build_lists(&text, MAX_RULES_PER_LIST);
    let sizes: Vec<usize> = chunks.iter().map(Vec::len).collect();

    let out = Path::new(output);
    std::fs::create_dir_all(out).expect("folder keluaran bisa dibuat");
    for old in std::fs::read_dir(out).expect("folder keluaran bisa dibaca").flatten() {
        if old.file_name().to_string_lossy().starts_with("blocklist-") {
            let _ = std::fs::remove_file(old.path());
        }
    }
    for (i, list) in chunks.iter().enumerate() {
        let path = out.join(format!("blocklist-{i}.json"));
        std::fs::write(&path, serde_json::to_vec(list).expect("JSON")).expect("bisa menulis");
    }
    println!("{} berkas filter, {skipped} filter dilewati, {} daftar berukuran {sizes:?}", files.len(), chunks.len());
}

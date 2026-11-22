//! Impor dan ekspor CSV password (Chrome, Safari/Passwords, Firefox,
//! Bitwarden). Berkas CSV berisi password dalam teks terang: setelah impor
//! sebaiknya dihapus, dan ekspor hanya dilakukan kalau benar-benar perlu.

use crate::entries::{Entry, NewEntry, origin_of};

/// Baris dan kolom CSV (RFC 4180: kutip, kutip ganda, baris baru di dalam kutip).
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (mut rows, mut row, mut field) = (Vec::new(), Vec::new(), String::new());
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r: &Vec<String>| !(r.len() == 1 && r[0].trim().is_empty()));
    rows
}

#[derive(Debug, PartialEq)]
pub struct Imported {
    pub entries: Vec<NewEntry>,
    /// Baris tanpa URL web atau tanpa password.
    pub skipped: usize,
}

fn column(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| names.contains(&h.trim().to_lowercase().as_str()))
}

/// Entri dari CSV ekspor browser atau password manager lain.
pub fn import(text: &str) -> Result<Imported, String> {
    let rows = parse(text);
    let Some((header, body)) = rows.split_first() else { return Err("The CSV file is empty.".into()) };
    let url = column(header, &["url", "login_uri", "website", "origin"]);
    let password = column(header, &["password", "login_password"]);
    let (Some(url), Some(password)) = (url, password) else {
        return Err("Couldn't find url and password columns. Supported: exports from Chrome, Safari/Passwords, \
                    Firefox, and Bitwarden."
            .into());
    };
    let username = column(header, &["username", "login_username", "user", "email", "login"]);
    let note = column(header, &["note", "notes", "extra"]);
    let cell = |row: &Vec<String>, i: Option<usize>| i.and_then(|i| row.get(i)).cloned().unwrap_or_default();
    let mut imported = Imported { entries: Vec::new(), skipped: 0 };
    for row in body {
        let pass = cell(row, Some(password));
        match origin_of(&cell(row, Some(url))) {
            Some(origin) if !pass.is_empty() => imported.entries.push(NewEntry {
                origin,
                username: cell(row, username),
                password: pass,
                note: cell(row, note),
            }),
            _ => imported.skipped += 1,
        }
    }
    Ok(imported)
}

fn quote(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// CSV format Chrome (`name,url,username,password,note`), bisa diimpor
/// browser dan password manager lain.
pub fn export(entries: &[Entry]) -> String {
    let mut out = String::from("name,url,username,password,note\n");
    for e in entries {
        let fields = [e.host(), &e.origin, &e.username, &e.password, &e.note];
        out.push_str(&fields.iter().map(|f| quote(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quotes_commas_and_newlines() {
        let rows = parse("\u{feff}a,b\r\n\"x,1\",\"say \"\"hi\"\"\"\n\"multi\nline\",z\n\n");
        assert_eq!(rows, vec![vec!["a", "b"], vec!["x,1", "say \"hi\""], vec!["multi\nline", "z"]]);
        assert_eq!(parse("tanpa,akhir"), vec![vec!["tanpa", "akhir"]]);
    }

    #[test]
    fn imports_common_formats() {
        let chrome = "name,url,username,password,note\ngithub.com,https://github.com/login,dery,pw1,\n";
        let safari = "Title,URL,Username,Password,Notes,OTPAuth\nMail,https://mail.example/,d@mail,pw2,catatan,\n";
        let firefox = "\"url\",\"username\",\"password\",\"httpRealm\"\n\"https://a.example\",\"u\",\"pw3\",\"\"\n";
        let bitwarden = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n,,login,B,n,,0,https://b.example,bu,pw4,\n";
        let get = |text| import(text).unwrap().entries;
        assert_eq!(get(chrome)[0].origin, "https://github.com");
        assert_eq!((get(safari)[0].username.clone(), get(safari)[0].note.clone()), ("d@mail".into(), "catatan".into()));
        assert_eq!(get(firefox)[0].password, "pw3");
        assert_eq!(get(bitwarden)[0].origin, "https://b.example");
    }

    #[test]
    fn skips_rows_without_web_url_or_password_and_rejects_unknown_files() {
        let text = "url,username,password\nhttps://ok.example,u,p\nandroid://app,u,p\nhttps://kosong.example,u,\n";
        let imported = import(text).unwrap();
        assert_eq!((imported.entries.len(), imported.skipped), (1, 2));
        assert!(import("nama,umur\nx,1\n").is_err());
        assert!(import("").is_err());
    }

    #[test]
    fn export_round_trips_through_import() {
        let entry = Entry {
            id: "1".into(),
            origin: "https://github.com".into(),
            username: "dery".into(),
            password: "a,b\"c\nd".into(),
            note: "n".into(),
            created: 0,
            updated: 0,
            last_used: None,
        };
        let back = import(&export(std::slice::from_ref(&entry))).unwrap().entries;
        assert_eq!(back[0].password, entry.password);
        assert_eq!(back[0].origin, entry.origin);
        assert_eq!(back[0].username, "dery");
    }
}

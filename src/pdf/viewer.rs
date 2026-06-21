use std::path::Path;

use crate::config;
use crate::util::expand_home;

pub fn find_and_open(message_id: &str, pdf_path: Option<&str>) {
    if let Some(path) = pdf_path {
        let p = Path::new(path);
        if p.exists() {
            open_file(p);
            return;
        }
    }

    let pdf_dir = expand_home(&config::get().app.pdf_dir);
    let prefix = &message_id[..message_id.len().min(16)];
    if let Ok(entries) = std::fs::read_dir(&pdf_dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(prefix) {
                open_file(&entry.path());
                return;
            }
        }
    }
}

fn open_file(path: &Path) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

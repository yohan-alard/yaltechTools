use std::path::{Path, PathBuf};

use crate::config;
use crate::util::expand_home;

pub fn path_for(message_id: &str, filename: &str) -> PathBuf {
    let dir = expand_home(&config::get().app.pdf_dir);
    let safe_name: String = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    Path::new(&dir).join(format!(
        "{}_{}",
        &message_id[..message_id.len().min(16)],
        safe_name
    ))
}

pub fn archive(message_id: &str, pdf_path: Option<&str>) {
    let pdf_dir = expand_home(&config::get().app.pdf_dir);
    let archive_dir = format!("{}/archives", pdf_dir);
    let _ = std::fs::create_dir_all(&archive_dir);

    let move_file = |src: &Path| {
        if let Some(name) = src.file_name() {
            let _ = std::fs::rename(src, Path::new(&archive_dir).join(name));
        }
    };

    if let Some(path) = pdf_path {
        let src = Path::new(path);
        if src.exists() {
            move_file(src);
            return;
        }
    }

    let prefix = &message_id[..message_id.len().min(16)];
    if let Ok(entries) = std::fs::read_dir(&pdf_dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(prefix) {
                move_file(&entry.path());
            }
        }
    }
}

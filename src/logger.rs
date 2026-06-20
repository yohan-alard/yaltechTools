use std::io::Write;
use std::sync::{Mutex, OnceLock};
#[cfg(unix)]
use std::os::unix::io::IntoRawFd;

static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

pub fn init() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let path = format!("{}/.local/share/yaltech-tools/yaltech.log", home);
    if let Some(p) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = FILE.set(Mutex::new(f));
    }

    // Redirige stdout (fd 1) ET stderr (fd 2) vers le fichier de log.
    // pdf-extract utilise println! (stdout) pour ses warnings "unknown glyph name".
    // Ratatui utilise /dev/tty directement, donc rediriger stdout est sans danger.
    #[cfg(unix)]
    {
        redirect_fd_to_log(1, &path);
        redirect_fd_to_log(2, &path);
    }

    std::panic::set_hook(Box::new(|info| {
        write(&format!("PANIC: {}", info));
    }));
}

#[cfg(unix)]
fn redirect_fd_to_log(target_fd: libc::c_int, path: &str) {
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let fd = f.into_raw_fd();
        unsafe {
            libc::dup2(fd, target_fd);
            libc::close(fd);
        }
    }
}

pub fn write(msg: &str) {
    if let Some(m) = FILE.get() {
        if let Ok(mut f) = m.lock() {
            let _ = writeln!(f, "{}", msg);
        }
    }
}

macro_rules! tlog {
    ($($arg:tt)*) => {
        $crate::logger::write(&format!($($arg)*))
    };
}
pub(crate) use tlog;

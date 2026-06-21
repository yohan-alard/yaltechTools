use anyhow::Context;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub async fn wait_for_code(port: u16, label: &str) -> anyhow::Result<String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .context(format!("Impossible d'écouter sur le port {}", port))?;

    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let request = std::str::from_utf8(&buf[..n]).unwrap_or("");
        crate::logger::tlog!("[{} callback] {}", label, request.lines().next().unwrap_or(""));

        match parse_callback(request) {
            CallbackResult::Code(code) => {
                stream.write_all(success_page()).await?;
                return Ok(code);
            }
            CallbackResult::Error(msg) => {
                stream.write_all(error_page(&msg).as_bytes()).await?;
                return Err(anyhow::anyhow!("{} OAuth erreur: {}", label, msg));
            }
            CallbackResult::Ignore => {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await?;
            }
        }
    }
}

enum CallbackResult {
    Code(String),
    Error(String),
    Ignore,
}

fn parse_callback(request: &str) -> CallbackResult {
    let path = match request.split_whitespace().nth(1) {
        Some(p) => p,
        None => return CallbackResult::Ignore,
    };
    let query = match path.split('?').nth(1) {
        Some(q) => q,
        None => return CallbackResult::Ignore,
    };
    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|p| {
            let mut it = p.splitn(2, '=');
            Some((it.next()?, it.next().unwrap_or("")))
        })
        .collect();

    if let Some(&code) = params.get("code") {
        return CallbackResult::Code(url_decode(code));
    }
    if let Some(&error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(|d| url_decode(d).replace('+', " "))
            .unwrap_or_default();
        return CallbackResult::Error(format!("{}: {}", error, desc));
    }
    CallbackResult::Ignore
}

pub fn open_browser(url: &str) {
    let ok = std::process::Command::new("open")
        .args(["-a", "Google Chrome", url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("Chrome ouvert pour l'authentification.");
    } else {
        println!("Ouvre ce lien dans ton navigateur :\n\n  {}", url);
    }
    println!();
}

pub fn success_page() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <!DOCTYPE html><html><body style='font-family:sans-serif;padding:2em;text-align:center'>\
    <h2>Autorisation re\xE7ue !</h2><p>Tu peux fermer cet onglet.</p></body></html>"
}

pub fn error_page(msg: &str) -> String {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
        <h2>Erreur OAuth</h2><pre>{}</pre>",
        msg
    )
}

pub fn save_tokens<T: Serialize>(path: &std::path::Path, tokens: &T) -> anyhow::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(tokens)?)?;
    Ok(())
}

pub fn generate_state() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    unix_now().hash(&mut h);
    std::process::id().hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            b => format!("%{:02X}", b),
        })
        .collect()
}

pub fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut bytes = s.bytes().peekable();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let h1 = bytes.next().unwrap_or(b'0') as char;
            let h2 = bytes.next().unwrap_or(b'0') as char;
            if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                result.push(byte as char);
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

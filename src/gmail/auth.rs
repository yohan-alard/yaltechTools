use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config;

#[derive(Serialize, Deserialize, Clone)]
pub struct Tokens {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

pub async fn ensure_access_token() -> anyhow::Result<String> {
    let path = token_path()?;

    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(mut stored) = serde_json::from_str::<Tokens>(&data) {
                if stored.expires_at > unix_now() + 300 {
                    return Ok(stored.access_token);
                }
                if let Some(ref rt) = stored.refresh_token.clone() {
                    match do_refresh(rt).await {
                        Ok(new) => {
                            stored.access_token = new.access_token;
                            stored.expires_at = new.expires_at;
                            save_tokens(&path, &stored)?;
                            return Ok(stored.access_token);
                        }
                        Err(e) => eprintln!("[gmail] refresh échoué: {}", e),
                    }
                }
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let tokens = auth_code_flow().await?;
    save_tokens(&path, &tokens)?;
    Ok(tokens.access_token)
}

async fn do_refresh(refresh_token: &str) -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let client_id = std::env::var("gmail.client_id").context("gmail.client_id manquant")?;
    let client_secret = std::env::var("gmail.client_secret").context("gmail.client_secret manquant")?;

    let resp = Client::new()
        .post(&cfg.token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("refresh_token", refresh_token),
        ])
        .send().await?;

    if !resp.status().is_success() {
        return Err(anyhow!("Refresh échoué: {}", resp.text().await.unwrap_or_default()));
    }
    parse_token_resp(resp).await
}

async fn auth_code_flow() -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let client_id = std::env::var("gmail.client_id").context("gmail.client_id manquant dans .env")?;
    let client_secret = std::env::var("gmail.client_secret").context("gmail.client_secret manquant dans .env")?;
    let state = generate_state();

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}&access_type=offline&prompt=consent",
        cfg.auth_base,
        url_encode(&client_id),
        url_encode(&cfg.redirect_uri),
        url_encode(&cfg.scope),
        state,
    );

    println!();
    println!("──────────────────────────────────────────");
    println!("  Authentification Google Gmail requise");
    println!("──────────────────────────────────────────");
    println!();

    open_browser(&auth_url);

    println!("En attente du callback sur {}...", cfg.redirect_uri);
    println!();

    let code = wait_for_code(cfg.redirect_port).await?;
    eprintln!("[gmail] code reçu, échange en cours...");
    exchange_code(&code, &client_id, &client_secret).await
}

async fn wait_for_code(port: u16) -> anyhow::Result<String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .context(format!("Impossible d'écouter sur le port {}", port))?;

    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let request = std::str::from_utf8(&buf[..n]).unwrap_or("");
        eprintln!("[gmail callback] {}", request.lines().next().unwrap_or(""));

        match parse_callback(request) {
            CallbackResult::Code(code) => {
                stream.write_all(success_page()).await?;
                return Ok(code);
            }
            CallbackResult::Error(msg) => {
                stream.write_all(error_page(&msg).as_bytes()).await?;
                return Err(anyhow!("Gmail OAuth erreur: {}", msg));
            }
            CallbackResult::Ignore => {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await?;
            }
        }
    }
}

async fn exchange_code(code: &str, client_id: &str, client_secret: &str) -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let resp = Client::new()
        .post(&cfg.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", cfg.redirect_uri.as_str()),
            ("code", code),
        ])
        .send().await.context("Erreur réseau échange code Gmail")?;

    let status = resp.status();
    eprintln!("[gmail] token endpoint: HTTP {}", status);
    if !status.is_success() {
        return Err(anyhow!("Gmail token HTTP {}: {}", status, resp.text().await.unwrap_or_default()));
    }
    parse_token_resp(resp).await
}

async fn parse_token_resp(resp: reqwest::Response) -> anyhow::Result<Tokens> {
    #[derive(Deserialize)]
    struct Resp { access_token: String, refresh_token: Option<String>, expires_in: Option<u64> }
    let r: Resp = resp.json().await.context("Réponse token Gmail invalide")?;
    Ok(Tokens {
        access_token:  r.access_token,
        refresh_token: r.refresh_token,
        expires_at:    unix_now() + r.expires_in.unwrap_or(3600),
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

enum CallbackResult { Code(String), Error(String), Ignore }

fn parse_callback(request: &str) -> CallbackResult {
    let path = match request.split_whitespace().nth(1) { Some(p) => p, None => return CallbackResult::Ignore };
    let query = match path.split('?').nth(1) { Some(q) => q, None => return CallbackResult::Ignore };
    let params: std::collections::HashMap<&str, &str> = query.split('&')
        .filter_map(|p| { let mut it = p.splitn(2, '='); Some((it.next()?, it.next().unwrap_or(""))) })
        .collect();
    if let Some(&code)  = params.get("code")  { return CallbackResult::Code(url_decode(code)); }
    if let Some(&error) = params.get("error") {
        let desc = params.get("error_description").map(|d| url_decode(d).replace('+', " ")).unwrap_or_default();
        return CallbackResult::Error(format!("{}: {}", error, desc));
    }
    CallbackResult::Ignore
}

fn open_browser(url: &str) {
    let ok = std::process::Command::new("open")
        .args(["-a", "Google Chrome", url])
        .status().map(|s| s.success()).unwrap_or(false);
    if ok { println!("Chrome ouvert pour l'authentification Google."); }
    else  { println!("Ouvre ce lien dans ton navigateur :\n\n  {}", url); }
    println!();
}

fn success_page() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <!DOCTYPE html><html><body style='font-family:sans-serif;padding:2em;text-align:center'>\
    <h2>Google authentifie !</h2><p>Tu peux fermer cet onglet.</p></body></html>"
}

fn error_page(msg: &str) -> String {
    format!("HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<h2>Erreur OAuth Gmail</h2><pre>{}</pre>", msg)
}

fn save_tokens(path: &std::path::Path, tokens: &Tokens) -> anyhow::Result<()> {
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(path, serde_json::to_string_pretty(tokens)?)?;
    Ok(())
}

fn token_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(expand_home(&config::get().app.google_token_store)))
}

fn generate_state() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    unix_now().hash(&mut h);
    std::process::id().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", std::env::var("HOME").unwrap_or_else(|_| ".".into()), rest)
    } else { path.to_string() }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "%20".to_string(),
        b => format!("%{:02X}", b),
    }).collect()
}

fn url_decode(s: &str) -> String {
    let mut r = String::new();
    let mut bytes = s.bytes().peekable();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let h1 = bytes.next().unwrap_or(b'0') as char;
            let h2 = bytes.next().unwrap_or(b'0') as char;
            if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) { r.push(byte as char); }
        } else if b == b'+' { r.push(' '); } else { r.push(b as char); }
    }
    r
}

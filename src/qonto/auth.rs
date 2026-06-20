use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config;

#[derive(Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    expires_at: u64,
}

pub async fn ensure_access_token() -> anyhow::Result<String> {
    let path = token_path()?;

    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(stored) = serde_json::from_str::<StoredTokens>(&data) {
                if stored.expires_at > unix_now() + 300 {
                    return Ok(stored.access_token);
                }
                crate::logger::tlog!("[qonto] token expiré, re-authentification...");
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let tokens = auth_code_flow().await?;
    save_tokens(&path, &tokens)?;
    Ok(tokens.access_token)
}

async fn auth_code_flow() -> anyhow::Result<StoredTokens> {
    let client_id =
        std::env::var("qonto.client_id").context("qonto.client_id manquant dans .env")?;
    let client_secret =
        std::env::var("qonto.client_secret").context("qonto.client_secret manquant dans .env")?;
    let staging_token =
        std::env::var("qonto.header_staging").context("qonto.header_staging manquant dans .env")?;

    crate::logger::tlog!("[qonto] staging token : {}...", &staging_token[..8.min(staging_token.len())]);

    let cfg = &config::get().qonto;
    let state = generate_state();
    let auth_url = format!(
        "{}/oauth2/auth?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}",
        cfg.oauth_base,
        url_encode(&client_id),
        url_encode(&cfg.redirect_uri),
        url_encode(&cfg.scope),
        state,
    );

    println!();
    println!("─────────────────────────────────────────");
    println!("  Authentification Qonto requise");
    println!("─────────────────────────────────────────");
    println!();

    open_browser(&auth_url);

    println!("En attente du callback sur {}...", cfg.redirect_uri);
    println!();

    let code = wait_for_code(cfg.redirect_port).await?;
    crate::logger::tlog!("[qonto] code reçu, échange en cours...");
    exchange_code(&code, &client_id, &client_secret, &staging_token).await
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
        crate::logger::tlog!("[qonto callback] {}", request.lines().next().unwrap_or(""));

        match parse_callback(request) {
            CallbackResult::Code(code) => {
                stream.write_all(success_page()).await?;
                return Ok(code);
            }
            CallbackResult::Error(msg) => {
                stream
                    .write_all(error_page(&msg).as_bytes())
                    .await?;
                return Err(anyhow!("Qonto OAuth erreur: {}", msg));
            }
            CallbackResult::Ignore => {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await?;
            }
        }
    }
}

async fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
    staging_token: &str,
) -> anyhow::Result<StoredTokens> {
    let cfg = &config::get().qonto;
    let resp = Client::new()
        .post(format!("{}/oauth2/token", cfg.oauth_base))
        .header("X-Qonto-Staging-Token", staging_token)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", cfg.redirect_uri.as_str()),
            ("code", code),
        ])
        .send()
        .await
        .context("Erreur réseau échange code Qonto")?;

    let status = resp.status();
    crate::logger::tlog!("[qonto] token endpoint: HTTP {}", status);
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Token endpoint HTTP {} {}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            &body[..body.len().min(400)]
        ));
    }

    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    }
    let r: Resp = resp.json().await.context("Réponse token Qonto invalide")?;
    Ok(StoredTokens {
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_at: unix_now() + r.expires_in.unwrap_or(3600),
    })
}

// ── Helpers partagés ──────────────────────────────────────────────────────────

enum CallbackResult { Code(String), Error(String), Ignore }

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
        .filter_map(|p| { let mut it = p.splitn(2, '='); Some((it.next()?, it.next().unwrap_or(""))) })
        .collect();

    if let Some(&code) = params.get("code") {
        return CallbackResult::Code(url_decode(code));
    }
    if let Some(&error) = params.get("error") {
        let desc = params.get("error_description")
            .map(|d| url_decode(d).replace('+', " "))
            .unwrap_or_default();
        return CallbackResult::Error(format!("{}: {}", error, desc));
    }
    CallbackResult::Ignore
}

fn open_browser(url: &str) {
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

fn success_page() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <!DOCTYPE html><html><body style='font-family:sans-serif;padding:2em;text-align:center'>\
    <h2>Autorisation re\xE7ue !</h2>\
    <p>Tu peux fermer cet onglet.</p></body></html>"
}

fn error_page(msg: &str) -> String {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
        <h2>Erreur OAuth</h2><pre>{}</pre>",
        msg
    )
}

fn save_tokens(path: &std::path::Path, tokens: &StoredTokens) -> anyhow::Result<()> {
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(path, serde_json::to_string_pretty(tokens)?)?;
    Ok(())
}

fn token_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(expand_home(&config::get().app.token_store)))
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

pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{}/{}", home, rest)
    } else {
        path.to_string()
    }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "%20".to_string(),
        b => format!("%{:02X}", b),
    }).collect()
}

fn url_decode(s: &str) -> String {
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

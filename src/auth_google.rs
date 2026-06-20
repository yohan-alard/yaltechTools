use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config;

#[derive(Serialize, Deserialize, Clone)]
pub struct GoogleTokens {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

pub async fn ensure_access_token() -> anyhow::Result<String> {
    let path = token_path()?;

    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(mut stored) = serde_json::from_str::<GoogleTokens>(&data) {
                let now = unix_now();
                if stored.expires_at > now + 300 {
                    return Ok(stored.access_token);
                }
                // Essai de refresh
                if let Some(ref rt) = stored.refresh_token.clone() {
                    match refresh_token(rt).await {
                        Ok(new_tokens) => {
                            stored.access_token = new_tokens.access_token;
                            stored.expires_at = new_tokens.expires_at;
                            save_tokens(&path, &stored)?;
                            return Ok(stored.access_token);
                        }
                        Err(e) => eprintln!("[google] refresh échoué: {}", e),
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

async fn refresh_token(refresh_token: &str) -> anyhow::Result<GoogleTokens> {
    let cfg = &config::get().google;
    let client_id = std::env::var("gmail.client_id").context("gmail.client_id manquant")?;
    let client_secret =
        std::env::var("gmail.client_secret").context("gmail.client_secret manquant")?;

    let resp = Client::new()
        .post(&cfg.token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Refresh token échoué: {}", body));
    }
    parse_token_response(resp).await
}

async fn auth_code_flow() -> anyhow::Result<GoogleTokens> {
    let cfg = &config::get().google;
    let client_id = std::env::var("gmail.client_id").context("gmail.client_id manquant dans .env")?;
    let client_secret =
        std::env::var("gmail.client_secret").context("gmail.client_secret manquant dans .env")?;
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

    let opened = std::process::Command::new("open")
        .args(["-a", "Google Chrome", &auth_url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if opened {
        println!("Chrome ouvert pour l'authentification Google.");
    } else {
        println!("Ouvre ce lien dans ton navigateur :");
        println!();
        println!("  {}", auth_url);
    }
    println!();
    println!("En attente du callback sur {}...", cfg.redirect_uri);
    println!();

    let code = wait_for_code(cfg.redirect_port).await?;
    eprintln!("[google] code reçu, échange en cours...");
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
        let first_line = request.lines().next().unwrap_or("");
        eprintln!("[google callback] {}", first_line);

        match parse_callback(request) {
            CallbackResult::Code(code) => {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
                        <!DOCTYPE html><html><body style='font-family:sans-serif;padding:2em;text-align:center'>\
                        <h2>Google authentifie !</h2>\
                        <p>Tu peux fermer cet onglet.</p></body></html>",
                    )
                    .await?;
                return Ok(code);
            }
            CallbackResult::Error(msg) => {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                            <h2>Erreur OAuth Google</h2><pre>{}</pre>",
                            msg
                        )
                        .as_bytes(),
                    )
                    .await?;
                return Err(anyhow!("Google OAuth erreur: {}", msg));
            }
            CallbackResult::Ignore => {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await?;
            }
        }
    }
}

async fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<GoogleTokens> {
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
        .send()
        .await
        .context("Erreur réseau échange code Google")?;

    let status = resp.status();
    eprintln!("[google] token endpoint: HTTP {}", status);
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Google token HTTP {}: {}", status, body));
    }
    parse_token_response(resp).await
}

async fn parse_token_response(resp: reqwest::Response) -> anyhow::Result<GoogleTokens> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    }
    let r: Resp = resp.json().await.context("Réponse token Google invalide")?;
    Ok(GoogleTokens {
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_at: unix_now() + r.expires_in.unwrap_or(3600),
    })
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

fn save_tokens(path: &std::path::Path, tokens: &GoogleTokens) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(tokens)?)?;
    Ok(())
}

fn token_path() -> anyhow::Result<std::path::PathBuf> {
    let raw = &config::get().app.google_token_store;
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/{}", home, rest)
    } else {
        raw.clone()
    };
    Ok(std::path::PathBuf::from(expanded))
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn url_encode(s: &str) -> String {
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

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::oauth;
use crate::util::expand_home;

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
                if stored.expires_at > oauth::unix_now() + 300 {
                    return Ok(stored.access_token);
                }
                if let Some(ref rt) = stored.refresh_token.clone() {
                    match do_refresh(rt).await {
                        Ok(new) => {
                            stored.access_token = new.access_token;
                            stored.expires_at = new.expires_at;
                            oauth::save_tokens(&path, &stored)?;
                            return Ok(stored.access_token);
                        }
                        Err(e) => crate::logger::tlog!("[gmail] refresh échoué: {}", e),
                    }
                }
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let tokens = auth_code_flow().await?;
    oauth::save_tokens(&path, &tokens)?;
    Ok(tokens.access_token)
}

async fn do_refresh(refresh_token: &str) -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let client_id = std::env::var("gmail.client_id").context("gmail.client_id manquant")?;
    let client_secret =
        std::env::var("gmail.client_secret").context("gmail.client_secret manquant")?;

    let resp = reqwest::Client::new()
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
        return Err(anyhow::anyhow!(
            "Refresh échoué: {}",
            resp.text().await.unwrap_or_default()
        ));
    }
    parse_token_resp(resp).await
}

async fn auth_code_flow() -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let client_id =
        std::env::var("gmail.client_id").context("gmail.client_id manquant dans .env")?;
    let client_secret =
        std::env::var("gmail.client_secret").context("gmail.client_secret manquant dans .env")?;
    let state = oauth::generate_state();

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}&access_type=offline&prompt=consent",
        cfg.auth_base,
        oauth::url_encode(&client_id),
        oauth::url_encode(&cfg.redirect_uri),
        oauth::url_encode(&cfg.scope),
        state,
    );

    println!();
    println!("──────────────────────────────────────────");
    println!("  Authentification Google Gmail requise");
    println!("──────────────────────────────────────────");
    println!();

    oauth::open_browser(&auth_url);

    println!("En attente du callback sur {}...", cfg.redirect_uri);
    println!();

    let code = oauth::wait_for_code(cfg.redirect_port, "gmail").await?;
    crate::logger::tlog!("[gmail] code reçu, échange en cours...");
    exchange_code(&code, &client_id, &client_secret).await
}

async fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<Tokens> {
    let cfg = &config::get().google;
    let resp = reqwest::Client::new()
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
        .context("Erreur réseau échange code Gmail")?;

    let status = resp.status();
    crate::logger::tlog!("[gmail] token endpoint: HTTP {}", status);
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "Gmail token HTTP {}: {}",
            status,
            resp.text().await.unwrap_or_default()
        ));
    }
    parse_token_resp(resp).await
}

async fn parse_token_resp(resp: reqwest::Response) -> anyhow::Result<Tokens> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    }
    let r: Resp = resp.json().await.context("Réponse token Gmail invalide")?;
    Ok(Tokens {
        access_token: r.access_token,
        refresh_token: r.refresh_token,
        expires_at: oauth::unix_now() + r.expires_in.unwrap_or(3600),
    })
}

fn token_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(expand_home(
        &config::get().app.google_token_store,
    )))
}

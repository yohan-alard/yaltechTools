use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::oauth;
use crate::util::expand_home;

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
                if stored.expires_at > oauth::unix_now() + 300 {
                    return Ok(stored.access_token);
                }
                crate::logger::tlog!("[qonto] token expiré, re-authentification...");
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let tokens = auth_code_flow().await?;
    oauth::save_tokens(&path, &tokens)?;
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
    let state = oauth::generate_state();
    let auth_url = format!(
        "{}/oauth2/auth?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}",
        cfg.oauth_base,
        oauth::url_encode(&client_id),
        oauth::url_encode(&cfg.redirect_uri),
        oauth::url_encode(&cfg.scope),
        state,
    );

    println!();
    println!("─────────────────────────────────────────");
    println!("  Authentification Qonto requise");
    println!("─────────────────────────────────────────");
    println!();

    oauth::open_browser(&auth_url);

    println!("En attente du callback sur {}...", cfg.redirect_uri);
    println!();

    let code = oauth::wait_for_code(cfg.redirect_port, "qonto").await?;
    crate::logger::tlog!("[qonto] code reçu, échange en cours...");
    exchange_code(&code, &client_id, &client_secret, &staging_token).await
}

async fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
    staging_token: &str,
) -> anyhow::Result<StoredTokens> {
    let cfg = &config::get().qonto;
    let resp = reqwest::Client::new()
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
        return Err(anyhow::anyhow!(
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
        expires_at: oauth::unix_now() + r.expires_in.unwrap_or(3600),
    })
}

fn token_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(expand_home(&config::get().app.token_store)))
}

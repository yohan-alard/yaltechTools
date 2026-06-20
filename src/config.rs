use anyhow::Context;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize, Clone)]
pub struct QontoConfig {
    pub oauth_base: String,
    pub api_base: String,
    pub redirect_uri: String,
    pub redirect_port: u16,
    pub scope: String,
}

#[derive(Deserialize, Clone)]
pub struct GoogleConfig {
    pub auth_base: String,
    pub token_url: String,
    pub api_base: String,
    pub redirect_uri: String,
    pub redirect_port: u16,
    pub scope: String,
    pub mail_query: String,
    pub max_results: u32,
}

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub token_store: String,
    pub google_token_store: String,
    pub cache_db: String,
    pub pdf_dir: String,
    pub auto_refresh_secs: u64,
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub qonto: QontoConfig,
    pub google: GoogleConfig,
    pub app: AppConfig,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn load() -> anyhow::Result<()> {
    let path = std::path::Path::new("config.toml");
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Impossible de lire {}", path.display()))?;
    let cfg: Config = toml::from_str(&raw).context("config.toml invalide")?;
    CONFIG.set(cfg).ok();
    Ok(())
}

pub fn get() -> &'static Config {
    CONFIG.get().expect("config::load() non appelé")
}

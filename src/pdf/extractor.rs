use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

pub async fn fetch_and_save(
    client: &Client,
    access_token: &str,
    msg_id: &str,
    att_id: &str,
    pdf_path: &std::path::Path,
) -> anyhow::Result<Vec<u8>> {
    if pdf_path.exists() {
        return std::fs::read(pdf_path).context("lecture PDF cache");
    }

    let api_base = &crate::config::get().google.api_base;

    #[derive(Deserialize)]
    struct AttachmentBody {
        data: String,
    }

    let att: AttachmentBody = client
        .get(format!(
            "{}/users/me/messages/{}/attachments/{}",
            api_base, msg_id, att_id
        ))
        .bearer_auth(access_token)
        .send()
        .await?
        .json()
        .await?;

    let bytes = URL_SAFE.decode(&att.data).context("base64 decode PDF")?;

    if let Some(parent) = pdf_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(pdf_path, &bytes);

    Ok(bytes)
}

pub fn extract_text(bytes: &[u8]) -> String {
    std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes).unwrap_or_default())
        .unwrap_or_default()
}

pub fn extract_amount(text: &str) -> Option<String> {
    let re = Regex::new(
        r"(?i)(?:total|montant|amount|ttc|net\s+à\s+payer)[^\d]{0,20}([\d\s]+[.,]\d{2})\s*(?:€|eur)",
    )
    .ok()?;

    if let Some(cap) = re.captures(text) {
        return Some(format!("{} €", cap[1].trim().replace(' ', "\u{202F}")));
    }

    let re2 = Regex::new(r"([\d\s]{1,10}[.,]\d{2})\s*(?:€|EUR)").ok()?;
    re2.captures(text)
        .map(|c| format!("{} €", c[1].trim().replace(' ', "\u{202F}")))
}

use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::app::MailInvoice;
use crate::cache;
use crate::config;

// ── Structures API Gmail ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MessageList {
    #[serde(default)]
    messages: Vec<MessageRef>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
struct Message {
    id: String,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    headers: Vec<Header>,
    #[serde(default)]
    parts: Vec<Part>,
    body: Option<Body>,
    #[serde(rename = "mimeType", default)]
    mime_type: String,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct Part {
    #[serde(rename = "mimeType", default)]
    mime_type: String,
    filename: Option<String>,
    body: Option<Body>,
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
struct Body {
    #[serde(rename = "attachmentId")]
    attachment_id: Option<String>,
    #[serde(default)]
    size: u64,
    data: Option<String>,
}

#[derive(Deserialize)]
struct AttachmentBody {
    data: String,
}

// ── Point d'entrée principal ──────────────────────────────────────────────────

pub async fn fetch_mail_invoices(access_token: &str) -> anyhow::Result<Vec<MailInvoice>> {
    let cfg = &config::get();
    let client = Client::new();

    let list: MessageList = client
        .get(format!("{}/users/me/messages", cfg.google.api_base))
        .bearer_auth(access_token)
        .query(&[
            ("q", cfg.google.mail_query.as_str()),
            ("maxResults", &cfg.google.max_results.to_string()),
        ])
        .send()
        .await
        .context("Gmail list messages")?
        .json()
        .await
        .context("Gmail list parse")?;

    // Commence par charger tout le cache existant
    let mut cached: std::collections::HashMap<String, MailInvoice> = cache::load_all()
        .into_iter()
        .collect();

    let mut results: Vec<MailInvoice> = Vec::new();

    for msg_ref in list.messages.iter().take(cfg.google.max_results as usize) {
        // Message déjà en cache → on l'utilise directement
        if let Some(inv) = cached.remove(&msg_ref.id) {
            results.push(inv);
            continue;
        }

        // Nouveau message → fetch + cache
        match fetch_message(&client, access_token, &msg_ref.id).await {
            Ok(Some(inv)) => {
                cache::upsert(&msg_ref.id, &inv, None);
                results.push(inv);
            }
            Ok(None) => {}
            Err(e) => eprintln!("[gmail] message {} ignoré: {}", msg_ref.id, e),
        }
    }

    Ok(results)
}

async fn fetch_message(
    client: &Client,
    access_token: &str,
    id: &str,
) -> anyhow::Result<Option<MailInvoice>> {
    let cfg = &config::get().google;

    let msg: Message = client
        .get(format!("{}/users/me/messages/{}", cfg.api_base, id))
        .bearer_auth(access_token)
        .query(&[("format", "full")])
        .send()
        .await?
        .json()
        .await
        .context("parse message")?;

    let payload = match msg.payload {
        Some(p) => p,
        None => return Ok(None),
    };

    let subject = payload
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("subject"))
        .map(|h| h.value.clone())
        .unwrap_or_default();

    let date = payload
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("date"))
        .map(|h| parse_date(&h.value))
        .unwrap_or_default();

    let from = payload
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("from"))
        .map(|h| h.value.clone())
        .unwrap_or_default();

    // Cherche les pièces jointes PDF et le corps du message
    let mut pdf_attachments: Vec<(String, Option<String>)> = Vec::new(); // (filename, attachment_id)
    let mut body_text = String::new();

    collect_parts(
        &payload.parts,
        payload.body.as_ref(),
        &payload.mime_type,
        &mut pdf_attachments,
        &mut body_text,
    );

    // Traitement des pièces jointes PDF
    if !pdf_attachments.is_empty() {
        let mut amount: Option<String> = None;
        let mut filenames: Vec<String> = Vec::new();

        for (filename, att_id) in &pdf_attachments {
            filenames.push(filename.clone());
            if let Some(id_str) = att_id {
                let pdf_path = cache::pdf_path(&msg.id, filename);
                if let Ok(text) =
                    fetch_and_extract_pdf(client, access_token, &msg.id, id_str, &pdf_path).await
                {
                    if amount.is_none() {
                        amount = extract_amount(&text);
                    }
                }
            }
        }

        return Ok(Some(MailInvoice {
            subject,
            from: extract_name(&from),
            date,
            amount: amount.unwrap_or_else(|| "—".to_string()),
            kind: format!("PDF ({})", filenames.join(", ")),
            link: None,
        }));
    }

    // Pas de PDF joint — cherche des liens de téléchargement dans le corps
    if let Some(link) = extract_invoice_link(&body_text) {
        return Ok(Some(MailInvoice {
            subject,
            from: extract_name(&from),
            date,
            amount: "—".to_string(),
            kind: "Lien".to_string(),
            link: Some(link),
        }));
    }

    // Mail avec "facture" dans le sujet mais sans PDF ni lien détecté
    if subject.to_lowercase().contains("facture")
        || subject.to_lowercase().contains("invoice")
    {
        return Ok(Some(MailInvoice {
            subject,
            from: extract_name(&from),
            date,
            amount: "—".to_string(),
            kind: "Mail".to_string(),
            link: None,
        }));
    }

    Ok(None)
}

fn collect_parts(
    parts: &[Part],
    body: Option<&Body>,
    mime_type: &str,
    pdfs: &mut Vec<(String, Option<String>)>,
    text: &mut String,
) {
    // Corps direct (message sans parts)
    if parts.is_empty() {
        if let Some(b) = body {
            if mime_type.contains("text/plain") || mime_type.contains("text/html") {
                if let Some(ref data) = b.data {
                    if let Ok(decoded) = URL_SAFE.decode(data) {
                        text.push_str(&String::from_utf8_lossy(&decoded));
                    }
                }
            }
        }
        return;
    }

    for part in parts {
        if part.mime_type == "application/pdf"
            || part
                .filename
                .as_deref()
                .map(|f| f.to_lowercase().ends_with(".pdf"))
                .unwrap_or(false)
        {
            let filename = part
                .filename
                .clone()
                .unwrap_or_else(|| "facture.pdf".to_string());
            let att_id = part.body.as_ref().and_then(|b| b.attachment_id.clone());
            if att_id.is_some() || part.body.as_ref().map(|b| b.size > 0).unwrap_or(false) {
                pdfs.push((filename, att_id));
            }
        } else if part.mime_type.starts_with("text/plain") || part.mime_type.starts_with("text/html") {
            if let Some(b) = &part.body {
                if let Some(ref data) = b.data {
                    if let Ok(decoded) = URL_SAFE.decode(data) {
                        text.push_str(&String::from_utf8_lossy(&decoded));
                    }
                }
            }
        } else if part.mime_type.starts_with("multipart/") {
            collect_parts(&part.parts, None, "", pdfs, text);
        }
    }
}

async fn fetch_and_extract_pdf(
    client: &Client,
    access_token: &str,
    msg_id: &str,
    att_id: &str,
    pdf_path: &std::path::Path,
) -> anyhow::Result<String> {
    let cfg = &config::get().google;

    // Si le PDF est déjà sur disque, on le relit directement
    let bytes = if pdf_path.exists() {
        std::fs::read(pdf_path).context("lecture PDF cache")?
    } else {
        let att: AttachmentBody = client
            .get(format!(
                "{}/users/me/messages/{}/attachments/{}",
                cfg.api_base, msg_id, att_id
            ))
            .bearer_auth(access_token)
            .send()
            .await?
            .json()
            .await?;

        let decoded = URL_SAFE.decode(&att.data).context("base64 decode PDF")?;

        // Sauvegarde sur disque
        if let Some(parent) = pdf_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(pdf_path, &decoded);

        decoded
    };

    // pdf-extract peut paniquer sur certains color spaces (DeviceN, etc.)
    let text = std::panic::catch_unwind(|| {
        pdf_extract::extract_text_from_mem(&bytes).unwrap_or_default()
    })
    .unwrap_or_default();

    Ok(text)
}

// ── Utilitaires d'extraction ──────────────────────────────────────────────────

fn extract_amount(text: &str) -> Option<String> {
    // Cherche des montants au format : 1 234,56 € / 1234.56 EUR / €1234 etc.
    let re = Regex::new(
        r"(?i)(?:total|montant|amount|ttc|net\s+à\s+payer)[^\d]{0,20}([\d\s]+[.,]\d{2})\s*(?:€|eur)",
    )
    .ok()?;

    if let Some(cap) = re.captures(text) {
        return Some(format!("{} €", cap[1].trim().replace(' ', "\u{202F}")));
    }

    // Fallback : premier montant en euros dans le texte
    let re2 = Regex::new(r"([\d\s]{1,10}[.,]\d{2})\s*(?:€|EUR)").ok()?;
    re2.captures(text)
        .map(|c| format!("{} €", c[1].trim().replace(' ', "\u{202F}")))
}

fn extract_invoice_link(text: &str) -> Option<String> {
    // Cherche une URL qui ressemble à un lien de téléchargement de facture
    let re = Regex::new(
        r"https?://\S+(?:facture|invoice|bill|receipt|download)[^\s<>]*",
    )
    .ok()?;
    re.find(text).map(|m| m.as_str().to_string())
}

fn extract_name(from: &str) -> String {
    // "Nom <email@example.com>" -> "Nom"
    if let Some(end) = from.find('<') {
        let name = from[..end].trim().trim_matches('\"').trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    from.to_string()
}

fn parse_date(raw: &str) -> String {
    // Garde juste la partie date (dd Mon yyyy)
    let parts: Vec<&str> = raw.trim().splitn(6, ' ').collect();
    if parts.len() >= 4 {
        return format!("{} {} {}", parts[1], parts[2], parts[3]);
    }
    raw.to_string()
}

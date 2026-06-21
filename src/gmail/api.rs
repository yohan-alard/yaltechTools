use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use reqwest::Client;
use serde::Deserialize;

use crate::config;
use crate::db;
use crate::models::MailInvoice;
use crate::pdf;

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

    let mut cached: std::collections::HashMap<String, MailInvoice> =
        db::load_all().into_iter().collect();

    let mut results: Vec<MailInvoice> = Vec::new();

    for msg_ref in list.messages.iter().take(cfg.google.max_results as usize) {
        if let Some(inv) = cached.remove(&msg_ref.id) {
            results.push(inv);
            continue;
        }

        match fetch_message(&client, access_token, &msg_ref.id).await {
            Ok(Some(inv)) => {
                db::upsert(&msg_ref.id, &inv);
                results.push(inv);
            }
            Ok(None) => {}
            Err(e) => crate::logger::tlog!("[gmail] message {} ignoré: {}", msg_ref.id, e),
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

    let mut pdf_attachments: Vec<(String, Option<String>)> = Vec::new();
    let mut body_text = String::new();

    collect_parts(
        &payload.parts,
        payload.body.as_ref(),
        &payload.mime_type,
        &mut pdf_attachments,
        &mut body_text,
    );

    if !pdf_attachments.is_empty() {
        let mut amount: Option<String> = None;
        let mut filenames: Vec<String> = Vec::new();
        let mut first_pdf_path: Option<String> = None;

        for (filename, att_id) in &pdf_attachments {
            filenames.push(filename.clone());
            if let Some(id_str) = att_id {
                let pdf_path = pdf::storage::path_for(&msg.id, filename);
                if first_pdf_path.is_none() {
                    first_pdf_path = Some(pdf_path.to_string_lossy().into_owned());
                }
                if let Ok(bytes) =
                    pdf::extractor::fetch_and_save(client, access_token, &msg.id, id_str, &pdf_path)
                        .await
                {
                    if amount.is_none() {
                        let text = pdf::extractor::extract_text(&bytes);
                        amount = pdf::extractor::extract_amount(&text);
                    }
                }
            }
        }

        return Ok(Some(MailInvoice {
            message_id: msg.id.clone(),
            subject,
            from: extract_name(&from),
            date,
            amount: amount.unwrap_or_else(|| "—".to_string()),
            kind: format!("PDF ({})", filenames.join(", ")),
            link: None,
            pdf_path: first_pdf_path,
        }));
    }

    if let Some(link) = extract_invoice_link(&body_text) {
        return Ok(Some(MailInvoice {
            message_id: msg.id.clone(),
            subject,
            from: extract_name(&from),
            date,
            amount: "—".to_string(),
            kind: "Lien".to_string(),
            link: Some(link),
            pdf_path: None,
        }));
    }

    if subject.to_lowercase().contains("facture") || subject.to_lowercase().contains("invoice") {
        return Ok(Some(MailInvoice {
            message_id: msg.id.clone(),
            subject,
            from: extract_name(&from),
            date,
            amount: "—".to_string(),
            kind: "Mail".to_string(),
            link: None,
            pdf_path: None,
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
        } else if part.mime_type.starts_with("text/plain")
            || part.mime_type.starts_with("text/html")
        {
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

fn extract_invoice_link(text: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"https?://\S+(?:facture|invoice|bill|receipt|download)[^\s<>]*",
    )
    .ok()?;
    re.find(text).map(|m| m.as_str().to_string())
}

fn extract_name(from: &str) -> String {
    if let Some(end) = from.find('<') {
        let name = from[..end].trim().trim_matches('\"').trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    from.to_string()
}

fn parse_date(raw: &str) -> String {
    let parts: Vec<&str> = raw.trim().splitn(6, ' ').collect();
    if parts.len() >= 4 {
        return format!("{} {} {}", parts[1], parts[2], parts[3]);
    }
    raw.to_string()
}

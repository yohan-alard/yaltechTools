use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::Deserialize;

use crate::app::Invoice;

const API_BASE: &str = "https://thirdparty-sandbox.staging.qonto.co";

#[derive(Deserialize)]
struct ListResponse {
    client_invoices: Vec<QontoInvoice>,
}

#[derive(Deserialize)]
struct QontoInvoice {
    #[serde(default)]
    number: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    total_amount_cents: i64,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    issue_date: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    client: Option<QontoClient>,
}

#[derive(Deserialize)]
struct QontoClient {
    #[serde(default)]
    name: String,
}

pub async fn fetch_invoices(access_token: &str) -> anyhow::Result<Vec<Invoice>> {
    let staging_token = std::env::var("qonto.header_staging").unwrap_or_default();

    let resp = Client::new()
        .get(format!("{}/v2/client_invoices", API_BASE))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Qonto-Staging-Token", &staging_token)
        .query(&[("per_page", "100"), ("sort_by", "created_at:desc")])
        .send()
        .await
        .context("Erreur réseau Qonto API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Qonto API HTTP {}: {}", status, body));
    }

    let body: ListResponse = resp.json().await.context("Réponse Qonto invalide")?;

    Ok(body
        .client_invoices
        .into_iter()
        .map(|i| Invoice {
            number: i.number,
            client: i.client.map(|c| c.name).unwrap_or_default(),
            amount_cents: i.total_amount_cents,
            currency: i.currency,
            status: translate_status(&i.status).to_string(),
            issue_date: i.issue_date.unwrap_or_default(),
            due_date: i.due_date.unwrap_or_default(),
        })
        .collect())
}

fn translate_status(s: &str) -> &'static str {
    match s {
        "draft" => "Brouillon",
        "unpaid" => "Non payée",
        "paid" => "Payée",
        "canceled" => "Annulée",
        _ => "Inconnu",
    }
}

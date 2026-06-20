use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::Deserialize;

use crate::app::{Invoice, SupplierInvoice};

const API_BASE: &str = "https://thirdparty-sandbox.staging.qonto.co";

fn staging_token() -> String {
    std::env::var("qonto.header_staging").unwrap_or_default()
}

// ── Factures clients ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ClientListResponse {
    client_invoices: Vec<QontoClientInvoice>,
}

#[derive(Deserialize)]
struct QontoClientInvoice {
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
    client: Option<QontoParty>,
}

#[derive(Deserialize)]
struct QontoParty {
    #[serde(default)]
    name: String,
}

pub async fn fetch_invoices(access_token: &str) -> anyhow::Result<Vec<Invoice>> {
    let resp = Client::new()
        .get(format!("{}/v2/client_invoices", API_BASE))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Qonto-Staging-Token", staging_token())
        .query(&[("per_page", "100"), ("sort_by", "created_at:desc")])
        .send()
        .await
        .context("Erreur réseau Qonto client invoices")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Qonto client invoices HTTP {}: {}", status, body));
    }

    let body: ClientListResponse = resp.json().await.context("Réponse client invoices invalide")?;

    Ok(body
        .client_invoices
        .into_iter()
        .map(|i| Invoice {
            number: i.number,
            client: i.client.map(|c| c.name).unwrap_or_default(),
            amount_cents: i.total_amount_cents,
            currency: i.currency,
            status: translate_client_status(&i.status),
            issue_date: i.issue_date.unwrap_or_default(),
            due_date: i.due_date.unwrap_or_default(),
        })
        .collect())
}

fn translate_client_status(s: &str) -> String {
    match s {
        "draft" => "Brouillon".into(),
        "unpaid" => "Non payée".into(),
        "paid" => "Payée".into(),
        "canceled" => "Annulée".into(),
        _ => s.to_string(),
    }
}

// ── Factures fournisseurs ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SupplierListResponse {
    supplier_invoices: Vec<QontoSupplierInvoice>,
}

#[derive(Deserialize)]
struct QontoSupplierInvoice {
    #[serde(default)]
    label: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    total_amount_cents: i64,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    supplier: Option<QontoParty>,
}

pub async fn fetch_supplier_invoices(access_token: &str) -> anyhow::Result<Vec<SupplierInvoice>> {
    let resp = Client::new()
        .get(format!("{}/v2/supplier_invoices", API_BASE))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Qonto-Staging-Token", staging_token())
        .query(&[("per_page", "100"), ("sort_by", "created_at:desc")])
        .send()
        .await
        .context("Erreur réseau Qonto supplier invoices")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Qonto supplier invoices HTTP {}: {}", status, body));
    }

    let body: SupplierListResponse =
        resp.json().await.context("Réponse supplier invoices invalide")?;

    Ok(body
        .supplier_invoices
        .into_iter()
        .map(|i| SupplierInvoice {
            label: if i.label.is_empty() {
                i.supplier.as_ref().map(|s| s.name.clone()).unwrap_or_default()
            } else {
                i.label
            },
            supplier: i.supplier.map(|s| s.name).unwrap_or_default(),
            amount_cents: i.total_amount_cents,
            currency: i.currency,
            status: translate_supplier_status(&i.status),
            due_date: i.due_date.unwrap_or_default(),
        })
        .collect())
}

fn translate_supplier_status(s: &str) -> String {
    match s {
        "draft" => "Brouillon".into(),
        "pending" => "En attente".into(),
        "to_review" => "A valider".into(),
        "approved" => "Approuvee".into(),
        "paid" => "Payee".into(),
        "canceled" => "Annulee".into(),
        _ => s.to_string(),
    }
}

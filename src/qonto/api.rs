use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::Deserialize;

use crate::app::{Invoice, SupplierInvoice};
use crate::config;

fn api_base() -> String { config::get().qonto.api_base.clone() }
fn staging_token() -> String { std::env::var("qonto.header_staging").unwrap_or_default() }

// ── Factures clients ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ClientListResponse { client_invoices: Vec<QontoClientInvoice> }

#[derive(Deserialize)]
struct QontoClientInvoice {
    #[serde(default)] number: String,
    #[serde(default)] status: String,
    #[serde(default)] total_amount_cents: i64,
    #[serde(default)] currency: String,
    #[serde(default)] issue_date: Option<String>,
    #[serde(default)] due_date: Option<String>,
    #[serde(default)] client: Option<QontoParty>,
}

#[derive(Deserialize)]
struct QontoParty { #[serde(default)] name: String }

pub async fn fetch_invoices(access_token: &str) -> anyhow::Result<Vec<Invoice>> {
    let resp = Client::new()
        .get(format!("{}/v2/client_invoices", api_base()))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Qonto-Staging-Token", staging_token())
        .query(&[("per_page", "100"), ("sort_by", "created_at:desc")])
        .send().await.context("Qonto client invoices réseau")?;

    if !resp.status().is_success() {
        let s = resp.status();
        return Err(anyhow!("Qonto client invoices HTTP {}: {}", s, resp.text().await.unwrap_or_default()));
    }

    Ok(resp.json::<ClientListResponse>().await.context("parse client invoices")?
        .client_invoices.into_iter().map(|i| Invoice {
            number:     i.number,
            client:     i.client.map(|c| c.name).unwrap_or_default(),
            amount_cents: i.total_amount_cents,
            currency:   i.currency,
            status:     translate_client_status(&i.status),
            issue_date: i.issue_date.unwrap_or_default(),
            due_date:   i.due_date.unwrap_or_default(),
        }).collect())
}

fn translate_client_status(s: &str) -> String {
    match s {
        "draft"    => "Brouillon".into(),
        "unpaid"   => "Non payee".into(),
        "paid"     => "Payee".into(),
        "canceled" => "Annulee".into(),
        _          => s.to_string(),
    }
}

// ── Factures fournisseurs ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SupplierListResponse { supplier_invoices: Vec<QontoSupplierInvoice> }

#[derive(Deserialize)]
struct QontoSupplierInvoice {
    #[serde(default)] invoice_number: String,
    #[serde(default)] supplier_name: String,
    #[serde(default)] status: String,
    total_amount: Option<QontoAmount>,
    #[serde(default)] due_date: Option<String>,
}

#[derive(Deserialize)]
struct QontoAmount {
    #[serde(default)] value: String,
    #[serde(default)] currency: String,
}

pub const SUPPLIER_FORBIDDEN: &str = "__forbidden__";

pub async fn fetch_supplier_invoices(access_token: &str) -> anyhow::Result<Vec<SupplierInvoice>> {
    let resp = Client::new()
        .get(format!("{}/v2/supplier_invoices", api_base()))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Qonto-Staging-Token", staging_token())
        .query(&[("per_page", "100"), ("sort_by", "created_at:desc")])
        .send().await.context("Qonto supplier invoices réseau")?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(anyhow!(SUPPLIER_FORBIDDEN));
    }
    if !resp.status().is_success() {
        let s = resp.status();
        return Err(anyhow!("Qonto supplier invoices HTTP {}: {}", s, resp.text().await.unwrap_or_default()));
    }

    let parsed: SupplierListResponse = resp.json().await.context("parse supplier invoices")?;

    Ok(parsed
        .supplier_invoices.into_iter().map(|i| {
            let (amount_cents, currency) = i.total_amount
                .map(|a| {
                    let cents = (a.value.parse::<f64>().unwrap_or(0.0) * 100.0).round() as i64;
                    (cents, a.currency)
                })
                .unwrap_or((0, "EUR".into()));
            SupplierInvoice {
                label:        i.invoice_number,
                supplier:     i.supplier_name,
                amount_cents,
                currency,
                status:       translate_supplier_status(&i.status),
                due_date:     i.due_date.unwrap_or_default(),
            }
        }).collect())
}

fn translate_supplier_status(s: &str) -> String {
    match s {
        "draft"     => "Brouillon".into(),
        "pending"   => "En attente".into(),
        "to_review" => "A valider".into(),
        "approved"  => "Approuvee".into(),
        "paid"      => "Payee".into(),
        "canceled"  => "Annulee".into(),
        _           => s.to_string(),
    }
}

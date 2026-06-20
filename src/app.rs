use std::time::Instant;

pub struct Invoice {
    pub number: String,
    pub client: String,
    pub amount_cents: i64,
    pub currency: String,
    pub status: String,
    pub issue_date: String,
    pub due_date: String,
}

pub struct SupplierInvoice {
    pub label: String,
    pub supplier: String,
    pub amount_cents: i64,
    pub currency: String,
    pub status: String,
    pub due_date: String,
}

pub struct App {
    pub access_token: String,
    pub invoices: Vec<Invoice>,
    pub supplier_invoices: Vec<SupplierInvoice>,
    pub loading: bool,
    pub error: Option<String>,
    pub last_refresh: Option<Instant>,
}

impl App {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            invoices: Vec::new(),
            supplier_invoices: Vec::new(),
            loading: true,
            error: None,
            last_refresh: None,
        }
    }
}

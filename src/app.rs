use std::time::Instant;
use ratatui::widgets::TableState;

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

pub struct MailInvoice {
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub amount: String,
    pub kind: String,
    pub link: Option<String>,
    pub pdf_path: Option<String>,
}

pub enum AppMode {
    Normal,
    EditAmount(String),
}

pub struct App {
    pub access_token: String,
    pub google_token: String,
    pub invoices: Vec<Invoice>,
    pub supplier_invoices: Vec<SupplierInvoice>,
    pub supplier_unavailable: bool,
    pub mail_invoices: Vec<MailInvoice>,
    pub mail_state: TableState,
    pub mode: AppMode,
    pub loading: bool,
    pub error: Option<String>,
    pub last_refresh: Option<Instant>,
}

impl App {
    pub fn new(access_token: String, google_token: String) -> Self {
        Self {
            access_token,
            google_token,
            invoices: Vec::new(),
            supplier_invoices: Vec::new(),
            supplier_unavailable: false,
            mail_invoices: Vec::new(),
            mail_state: TableState::default(),
            mode: AppMode::Normal,
            loading: true,
            error: None,
            last_refresh: None,
        }
    }

    pub fn mail_select_next(&mut self) {
        let len = self.mail_invoices.len();
        if len == 0 { return; }
        let next = match self.mail_state.selected() {
            Some(i) => (i + 1) % len,
            None => 0,
        };
        self.mail_state.select(Some(next));
    }

    pub fn mail_select_prev(&mut self) {
        let len = self.mail_invoices.len();
        if len == 0 { return; }
        let prev = match self.mail_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.mail_state.select(Some(prev));
    }

    pub fn open_selected_pdf(&self) {
        let Some(i) = self.mail_state.selected() else { return };
        let Some(inv) = self.mail_invoices.get(i) else { return };

        if let Some(ref path) = inv.pdf_path {
            if std::path::Path::new(path).exists() {
                let _ = std::process::Command::new("open").arg(path).spawn();
                return;
            }
        }

        // Fallback : cherche dans le répertoire pdfs par préfixe de message_id
        if let Ok(home) = std::env::var("HOME") {
            let pdf_dir = format!("{}/.local/share/yaltech-tools/pdfs", home);
            let prefix = &inv.message_id[..inv.message_id.len().min(16)];
            if let Ok(entries) = std::fs::read_dir(&pdf_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().starts_with(prefix) {
                        let _ = std::process::Command::new("open").arg(entry.path()).spawn();
                        return;
                    }
                }
            }
        }
    }

    /// Retire la ligne sélectionnée de la liste en mémoire.
    /// Retourne (message_id, pdf_path) pour persister en DB et archiver le PDF.
    pub fn ignore_selected(&mut self) -> Option<(String, Option<String>)> {
        let i = self.mail_state.selected()?;
        if i >= self.mail_invoices.len() { return None; }
        let msg_id = self.mail_invoices[i].message_id.clone();
        let pdf_path = self.mail_invoices[i].pdf_path.clone();
        self.mail_invoices.remove(i);
        let len = self.mail_invoices.len();
        if len == 0 {
            self.mail_state.select(None);
        } else {
            self.mail_state.select(Some(i.min(len - 1)));
        }
        Some((msg_id, pdf_path))
    }

    /// Passe en mode édition du montant de la ligne sélectionnée.
    pub fn start_edit_amount(&mut self) {
        let Some(i) = self.mail_state.selected() else { return };
        let Some(inv) = self.mail_invoices.get(i) else { return };
        let current = if inv.amount == "—" { String::new() } else { inv.amount.clone() };
        self.mode = AppMode::EditAmount(current);
    }

    pub fn push_char(&mut self, c: char) {
        if let AppMode::EditAmount(ref mut buf) = self.mode {
            buf.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if let AppMode::EditAmount(ref mut buf) = self.mode {
            buf.pop();
        }
    }

    /// Confirme la saisie. Retourne (message_id, nouveau_montant) pour persister.
    pub fn confirm_edit_amount(&mut self) -> Option<(String, String)> {
        let buf = match self.mode {
            AppMode::EditAmount(ref b) => b.clone(),
            _ => return None,
        };
        let i = self.mail_state.selected()?;
        let inv = self.mail_invoices.get_mut(i)?;
        let msg_id = inv.message_id.clone();
        inv.amount = if buf.trim().is_empty() { "—".to_string() } else { buf.trim().to_string() };
        self.mode = AppMode::Normal;
        Some((msg_id, inv.amount.clone()))
    }

    pub fn cancel_edit(&mut self) {
        self.mode = AppMode::Normal;
    }

    pub fn is_editing(&self) -> bool {
        matches!(self.mode, AppMode::EditAmount(_))
    }

    pub fn edit_buffer(&self) -> Option<&str> {
        match &self.mode {
            AppMode::EditAmount(buf) => Some(buf),
            AppMode::Normal => None,
        }
    }
}

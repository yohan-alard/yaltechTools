use std::collections::HashSet;
use std::time::Instant;

use chrono::Datelike;
use ratatui::widgets::TableState;

use crate::models::{Invoice, MailInvoice, Reminder, SupplierInvoice, REMINDERS};
use crate::pdf;

#[derive(PartialEq)]
pub enum Panel {
    Reminders,
    Mail,
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
    pub reminder_state: TableState,
    pub active_panel: Panel,
    pub reminder_acks: HashSet<String>,
    pub mode: AppMode,
    pub loading: bool,
    pub error: Option<String>,
    pub last_refresh: Option<Instant>,
}

impl App {
    pub fn new(
        access_token: String,
        google_token: String,
        reminder_acks: HashSet<String>,
    ) -> Self {
        Self {
            access_token,
            google_token,
            invoices: Vec::new(),
            supplier_invoices: Vec::new(),
            supplier_unavailable: false,
            mail_invoices: Vec::new(),
            mail_state: TableState::default(),
            reminder_state: TableState::default(),
            active_panel: Panel::Mail,
            reminder_acks,
            mode: AppMode::Normal,
            loading: true,
            error: None,
            last_refresh: None,
        }
    }

    pub fn visible_reminders(&self) -> Vec<(&'static Reminder, i32)> {
        let today = chrono::Local::now();
        let current_day = today.day();
        let month_key = today.format("%Y-%m").to_string();
        REMINDERS
            .iter()
            .filter_map(|r| {
                let delta = r.day as i32 - current_day as i32;
                if delta >= 7 {
                    return None;
                }
                let ack_key = format!("{}|{}|{}", month_key, r.day, r.label);
                if self.reminder_acks.contains(&ack_key) {
                    return None;
                }
                Some((r, delta))
            })
            .collect()
    }

    pub fn ack_selected_reminder(&mut self) -> Option<String> {
        let idx = self.reminder_state.selected()?;
        let today = chrono::Local::now();
        let month_key = today.format("%Y-%m").to_string();
        let visible = self.visible_reminders();
        let (r, _) = visible.get(idx)?;
        let key = format!("{}|{}|{}", month_key, r.day, r.label);
        self.reminder_acks.insert(key.clone());
        let new_len = self.visible_reminders().len();
        if new_len == 0 {
            self.reminder_state.select(None);
        } else {
            self.reminder_state.select(Some(idx.min(new_len - 1)));
        }
        Some(key)
    }

    pub fn reminder_select_next(&mut self) {
        let len = self.visible_reminders().len();
        if len == 0 {
            return;
        }
        let next = match self.reminder_state.selected() {
            Some(i) => (i + 1) % len,
            None => 0,
        };
        self.reminder_state.select(Some(next));
    }

    pub fn reminder_select_prev(&mut self) {
        let len = self.visible_reminders().len();
        if len == 0 {
            return;
        }
        let prev = match self.reminder_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.reminder_state.select(Some(prev));
    }

    pub fn switch_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Mail => {
                if self.reminder_state.selected().is_none()
                    && !self.visible_reminders().is_empty()
                {
                    self.reminder_state.select(Some(0));
                }
                Panel::Reminders
            }
            Panel::Reminders => Panel::Mail,
        };
    }

    pub fn mail_select_next(&mut self) {
        let len = self.mail_invoices.len();
        if len == 0 {
            return;
        }
        let next = match self.mail_state.selected() {
            Some(i) => (i + 1) % len,
            None => 0,
        };
        self.mail_state.select(Some(next));
    }

    pub fn mail_select_prev(&mut self) {
        let len = self.mail_invoices.len();
        if len == 0 {
            return;
        }
        let prev = match self.mail_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.mail_state.select(Some(prev));
    }

    pub fn open_selected_pdf(&self) {
        let Some(i) = self.mail_state.selected() else { return };
        let Some(inv) = self.mail_invoices.get(i) else { return };
        pdf::viewer::find_and_open(&inv.message_id, inv.pdf_path.as_deref());
    }

    pub fn ignore_selected(&mut self) -> Option<(String, Option<String>)> {
        let i = self.mail_state.selected()?;
        if i >= self.mail_invoices.len() {
            return None;
        }
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

    pub fn start_edit_amount(&mut self) {
        let Some(i) = self.mail_state.selected() else { return };
        let Some(inv) = self.mail_invoices.get(i) else { return };
        let current = if inv.amount == "—" {
            String::new()
        } else {
            inv.amount.clone()
        };
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

    pub fn confirm_edit_amount(&mut self) -> Option<(String, String)> {
        let buf = match self.mode {
            AppMode::EditAmount(ref b) => b.clone(),
            _ => return None,
        };
        let i = self.mail_state.selected()?;
        let inv = self.mail_invoices.get_mut(i)?;
        let msg_id = inv.message_id.clone();
        inv.amount = if buf.trim().is_empty() {
            "—".to_string()
        } else {
            buf.trim().to_string()
        };
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

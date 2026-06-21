pub struct Reminder {
    pub day: u32,
    pub label: &'static str,
}

pub const REMINDERS: &[Reminder] = &[
    Reminder { day: 10, label: "Envoyer l'export comptable" },
    Reminder { day: 25, label: "Saisir temps dans Bound" },
    Reminder { day: 28, label: "Envoyer la facture mensuelle" },
    Reminder { day: 28, label: "Saisir indemnites de route" },
];

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

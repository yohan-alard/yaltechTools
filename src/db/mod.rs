use anyhow::Context;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

use crate::config;
use crate::models::MailInvoice;
use crate::util::expand_home;

static DB: Mutex<Option<Connection>> = Mutex::new(None);

pub fn init() -> anyhow::Result<()> {
    let db_path = expand_home(&config::get().app.cache_db);
    let pdf_dir = expand_home(&config::get().app.pdf_dir);

    if let Some(parent) = Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&pdf_dir)?;

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Impossible d'ouvrir {}", db_path))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reminder_acks (key TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS mail_invoices (
            message_id  TEXT PRIMARY KEY,
            subject     TEXT NOT NULL DEFAULT '',
            from_name   TEXT NOT NULL DEFAULT '',
            date        TEXT NOT NULL DEFAULT '',
            amount      TEXT NOT NULL DEFAULT '—',
            kind        TEXT NOT NULL DEFAULT 'Mail',
            link        TEXT,
            pdf_path    TEXT,
            cached_at   INTEGER NOT NULL,
            ignored     INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    let _ = conn.execute_batch(
        "ALTER TABLE mail_invoices ADD COLUMN ignored INTEGER NOT NULL DEFAULT 0;",
    );

    *DB.lock().unwrap() = Some(conn);
    Ok(())
}

pub fn get(message_id: &str) -> Option<MailInvoice> {
    let guard = DB.lock().unwrap();
    let conn = guard.as_ref()?;
    conn.query_row(
        "SELECT subject, from_name, date, amount, kind, link, pdf_path
         FROM mail_invoices WHERE message_id = ?1",
        params![message_id],
        |row| {
            Ok(MailInvoice {
                message_id: message_id.to_string(),
                subject:  row.get(0)?,
                from:     row.get(1)?,
                date:     row.get(2)?,
                amount:   row.get(3)?,
                kind:     row.get(4)?,
                link:     row.get(5)?,
                pdf_path: row.get(6)?,
            })
        },
    )
    .ok()
}

pub fn upsert(message_id: &str, inv: &MailInvoice) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let _ = conn.execute(
        "INSERT INTO mail_invoices
            (message_id, subject, from_name, date, amount, kind, link, pdf_path, cached_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(message_id) DO UPDATE SET
            amount=excluded.amount, kind=excluded.kind, cached_at=excluded.cached_at,
            pdf_path=COALESCE(excluded.pdf_path, mail_invoices.pdf_path)",
        params![
            message_id, inv.subject, inv.from, inv.date,
            inv.amount, inv.kind, inv.link, inv.pdf_path, now,
        ],
    );
}

pub fn load_all() -> Vec<(String, MailInvoice)> {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return Vec::new() };

    let mut stmt = match conn.prepare(
        "SELECT message_id, subject, from_name, date, amount, kind, link, pdf_path
         FROM mail_invoices WHERE ignored=0 ORDER BY cached_at DESC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map([], |row| {
        let message_id: String = row.get(0)?;
        Ok((
            message_id.clone(),
            MailInvoice {
                message_id,
                subject:  row.get(1)?,
                from:     row.get(2)?,
                date:     row.get(3)?,
                amount:   row.get(4)?,
                kind:     row.get(5)?,
                link:     row.get(6)?,
                pdf_path: row.get(7)?,
            },
        ))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

pub fn load_reminder_acks(month: &str) -> std::collections::HashSet<String> {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return Default::default() };
    let mut stmt = match conn.prepare("SELECT key FROM reminder_acks WHERE key LIKE ?1") {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    stmt.query_map(params![format!("{}|%", month)], |row| row.get::<_, String>(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn save_reminder_ack(key: &str) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let _ = conn.execute("INSERT OR IGNORE INTO reminder_acks(key) VALUES(?1)", params![key]);
}

pub fn set_ignored(message_id: &str) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let _ = conn.execute(
        "UPDATE mail_invoices SET ignored=1 WHERE message_id=?1",
        params![message_id],
    );
}

pub fn set_amount(message_id: &str, amount: &str) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let _ = conn.execute(
        "UPDATE mail_invoices SET amount=?1 WHERE message_id=?2",
        params![amount, message_id],
    );
}

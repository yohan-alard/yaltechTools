use anyhow::Context;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::app::MailInvoice;
use crate::config;

static DB: Mutex<Option<Connection>> = Mutex::new(None);

pub fn init() -> anyhow::Result<()> {
    let db_path = expand(&config::get().app.cache_db);
    let pdf_dir = expand(&config::get().app.pdf_dir);

    if let Some(parent) = Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&pdf_dir)?;

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Impossible d'ouvrir {}", db_path))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mail_invoices (
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
    // Migration : colonne ignored sur tables existantes
    let _ = conn.execute_batch("ALTER TABLE mail_invoices ADD COLUMN ignored INTEGER NOT NULL DEFAULT 0;");

    *DB.lock().unwrap() = Some(conn);
    Ok(())
}

/// Vérifie si un message est déjà en cache.
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

/// Insère ou met à jour une entrée dans le cache.
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
            message_id,
            inv.subject,
            inv.from,
            inv.date,
            inv.amount,
            inv.kind,
            inv.link,
            inv.pdf_path,
            now,
        ],
    );
}

/// Déplace le PDF d'un message vers le sous-dossier archives/.
/// Si pdf_path est None, cherche par préfixe de message_id dans le dossier pdfs.
pub fn archive_pdf(message_id: &str, pdf_path: Option<&str>) {
    let pdf_dir = expand(&config::get().app.pdf_dir);
    let archive_dir = format!("{}/archives", pdf_dir);
    let _ = std::fs::create_dir_all(&archive_dir);

    let move_file = |src: &std::path::Path| {
        if let Some(name) = src.file_name() {
            let dst = std::path::Path::new(&archive_dir).join(name);
            let _ = std::fs::rename(src, &dst);
        }
    };

    if let Some(path) = pdf_path {
        let src = std::path::Path::new(path);
        if src.exists() {
            move_file(src);
            return;
        }
    }

    // Fallback : scan par préfixe de message_id
    let prefix = &message_id[..message_id.len().min(16)];
    if let Ok(entries) = std::fs::read_dir(&pdf_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(prefix) {
                move_file(&entry.path());
            }
        }
    }
}

/// Chemin où sauvegarder un PDF pour ce message.
pub fn pdf_path(message_id: &str, filename: &str) -> PathBuf {
    let dir = expand(&config::get().app.pdf_dir);
    // Sanitise le nom de fichier
    let safe_name: String = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    Path::new(&dir).join(format!("{}_{}", &message_id[..message_id.len().min(16)], safe_name))
}

/// Marque un message comme ignoré (ne sera plus affiché).
pub fn set_ignored(message_id: &str) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let _ = conn.execute(
        "UPDATE mail_invoices SET ignored=1 WHERE message_id=?1",
        params![message_id],
    );
}

/// Met à jour le montant d'un message (saisie manuelle).
pub fn set_amount(message_id: &str, amount: &str) {
    let guard = DB.lock().unwrap();
    let Some(conn) = guard.as_ref() else { return };
    let _ = conn.execute(
        "UPDATE mail_invoices SET amount=?1 WHERE message_id=?2",
        params![amount, message_id],
    );
}

/// Retourne tous les messages déjà en cache (pour le chargement initial rapide).
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

pub fn expand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/{}", home, rest)
    } else {
        path.to_string()
    }
}

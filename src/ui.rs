use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};

use crate::app::{App, Panel};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(f, outer[0], app);
    render_footer(f, outer[2], app);

    // Split du contenu : gauche = Qonto, droite = Gmail
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(outer[1]);

    // Colonne gauche : clients + fournisseurs empilés
    let qonto_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[0]);

    render_client_invoices(f, qonto_rows[0], app);
    render_supplier_invoices(f, qonto_rows[1], app);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(5)])
        .split(cols[1]);

    render_reminders(f, right_rows[0], app);
    render_mail_invoices(f, right_rows[1], app);

    if let Some(buf) = app.edit_buffer() {
        render_edit_popup(f, area, buf);
    }
}

fn render_header(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let title = if app.loading {
        "  YalTech — Comptabilite Qonto  [chargement...]"
    } else {
        "  YalTech — Comptabilite Qonto  "
    };

    let p = Paragraph::new(title)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(p, area);
}

fn render_client_invoices(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(
            " Factures clients ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    if app.loading && app.invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Chargement...").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }
    if app.invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Aucune facture client.").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let header_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("N° Facture").style(header_style),
        Cell::from("Client").style(header_style),
        Cell::from("Montant TTC").style(header_style),
        Cell::from("Statut").style(header_style),
        Cell::from("Emise le").style(header_style),
        Cell::from("Echeance").style(header_style),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .invoices
        .iter()
        .map(|inv| {
            let status_style = match inv.status.as_str() {
                "Payee" | "Payée" => Style::default().fg(Color::Green),
                "Non payee" | "Non payée" => Style::default().fg(Color::Yellow),
                "Brouillon" => Style::default().fg(Color::DarkGray),
                "Annulee" | "Annulée" => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            Row::new(vec![
                Cell::from(inv.number.clone()),
                Cell::from(truncate(&inv.client, 25)),
                Cell::from(format_amount(inv.amount_cents, &inv.currency)),
                Cell::from(inv.status.clone()).style(status_style),
                Cell::from(format_date(&inv.issue_date)),
                Cell::from(format_date(&inv.due_date)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Min(14),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(11),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .block(block)
    .column_spacing(2);

    f.render_widget(table, area);
}

fn render_supplier_invoices(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(
            " Factures fournisseurs ",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    if app.loading && app.supplier_invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Chargement...").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }
    if app.supplier_invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Aucune facture fournisseur.").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let header_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("Intitule").style(header_style),
        Cell::from("Fournisseur").style(header_style),
        Cell::from("Montant TTC").style(header_style),
        Cell::from("Statut").style(header_style),
        Cell::from("Echeance").style(header_style),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .supplier_invoices
        .iter()
        .map(|inv| {
            let status_style = match inv.status.as_str() {
                "Payee" | "Payée" => Style::default().fg(Color::Green),
                "Approuvee" | "Approuvée" => Style::default().fg(Color::Blue),
                "En attente" => Style::default().fg(Color::Yellow),
                "A valider" | "À valider" => Style::default().fg(Color::Yellow),
                "Brouillon" => Style::default().fg(Color::DarkGray),
                "Annulee" | "Annulée" => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            Row::new(vec![
                Cell::from(truncate(&inv.label, 30)),
                Cell::from(truncate(&inv.supplier, 20)),
                Cell::from(format_amount(inv.amount_cents, &inv.currency)),
                Cell::from(inv.status.clone()).style(status_style),
                Cell::from(format_date(&inv.due_date)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(22),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .block(block)
    .column_spacing(2);

    f.render_widget(table, area);
}

fn render_mail_invoices(f: &mut Frame, area: ratatui::layout::Rect, app: &mut App) {
    let has_selection = app.mail_state.selected().is_some();
    let block = Block::default()
        .title(Span::styled(
            " Factures Gmail ",
            Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(if has_selection {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::LightBlue)
        });

    if app.loading && app.mail_invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Chargement...").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }
    if app.mail_invoices.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Aucune facture détectée.").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let header_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("Expéditeur").style(header_style),
        Cell::from("Sujet").style(header_style),
        Cell::from("Montant").style(header_style),
        Cell::from("Type").style(header_style),
        Cell::from("Date").style(header_style),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .mail_invoices
        .iter()
        .map(|m| {
            let kind_style = match m.kind.as_str() {
                k if k.starts_with("PDF") => Style::default().fg(Color::Green),
                "Lien" => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::DarkGray),
            };
            let amount_style = if m.amount == "—" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            Row::new(vec![
                Cell::from(truncate(&m.from, 18)),
                Cell::from(truncate(&m.subject, 24)),
                Cell::from(m.amount.clone()).style(amount_style),
                Cell::from(truncate(&m.kind, 14)).style(kind_style),
                Cell::from(m.date.clone()),
            ])
        })
        .collect();

    let highlight_style = Style::default()
        .bg(Color::DarkGray)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Min(14),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .block(block)
    .row_highlight_style(highlight_style)
    .highlight_symbol("> ")
    .column_spacing(1);

    f.render_stateful_widget(table, area, &mut app.mail_state);
}

fn render_reminders(f: &mut Frame, area: ratatui::layout::Rect, app: &mut App) {
    let is_active = app.active_panel == Panel::Reminders;
    let border_style = if is_active {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let block = Block::default()
        .title(Span::styled(
            " Rappels du mois ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let visible = app.visible_reminders();

    if visible.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Aucun rappel cette semaine")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let rows: Vec<Row> = visible.iter().map(|(r, delta)| {
        let (status, style) = match delta {
            d if *d <= 0 => (
                if *d == 0 { "Aujourd'hui !".to_string() } else { format!("En retard {}j", -d) },
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            1 => ("Demain".to_string(), Style::default().fg(Color::Red)),
            2 => ("Dans 2j".to_string(), Style::default().fg(Color::Red)),
            d => (format!("Dans {}j", d), Style::default().fg(Color::Green)),
        };
        Row::new(vec![
            Cell::from(format!("J{:02}", r.day)).style(Style::default().fg(Color::DarkGray)),
            Cell::from(r.label),
            Cell::from(status).style(style),
        ])
    }).collect();

    let highlight_style = Style::default()
        .bg(Color::DarkGray)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let table = Table::new(
        rows,
        [Constraint::Length(4), Constraint::Min(20), Constraint::Length(14)],
    )
    .block(block)
    .row_highlight_style(highlight_style)
    .highlight_symbol("> ")
    .column_spacing(2);

    f.render_stateful_widget(table, area, &mut app.reminder_state);
}

fn render_footer(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let last = app.last_refresh.map(|t| {
        let s = t.elapsed().as_secs();
        if s < 60 { format!("il y a {}s", s) } else { format!("il y a {}m{}s", s / 60, s % 60) }
    });

    let mut spans = if app.is_editing() {
        vec![
            Span::styled("[Entrée] Confirmer", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  [Esc] Annuler", Style::default().fg(Color::DarkGray)),
        ]
    } else if app.active_panel == Panel::Reminders {
        vec![
            Span::styled("[q] Quitter", Style::default().fg(Color::DarkGray)),
            Span::styled("  [Tab] Factures Gmail", Style::default().fg(Color::DarkGray)),
            Span::styled("  [↑↓] Naviguer", Style::default().fg(Color::DarkGray)),
            Span::styled("  [Entrée] Acquitter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]
    } else {
        vec![
            Span::styled("[q] Quitter", Style::default().fg(Color::DarkGray)),
            Span::styled("  [Tab] Rappels", Style::default().fg(Color::DarkGray)),
            Span::styled("  [r] Rafraichir", Style::default().fg(Color::DarkGray)),
            Span::styled("  [↑↓] Naviguer", Style::default().fg(Color::DarkGray)),
            Span::styled("  [Entrée] PDF", Style::default().fg(Color::DarkGray)),
            Span::styled("  [e] Montant", Style::default().fg(Color::DarkGray)),
            Span::styled("  [d] Ignorer", Style::default().fg(Color::DarkGray)),
        ]
    };

    if let Some(ref t) = last {
        spans.push(Span::styled(
            format!("  |  Derniere MAJ: {}", t),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let total = app.invoices.len() + app.supplier_invoices.len();
    if total > 0 {
        spans.push(Span::styled(
            format!(
                "  |  {} clients / {} fourn. / {} mails",
                app.invoices.len(),
                app.supplier_invoices.len(),
                app.mail_invoices.len()
            ),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if let Some(ref err) = app.error {
        spans.push(Span::styled(
            format!("  |  {} {}", "\u{26A0}", truncate(err, 60)),
            Style::default().fg(Color::Red),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn format_amount(cents: i64, currency: &str) -> String {
    let abs = cents.abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{}{},{:02} {}", sign, format_thousands(abs / 100), abs % 100, currency)
}

fn format_thousands(n: i64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    chars
        .iter()
        .enumerate()
        .flat_map(|(i, &c)| {
            if i > 0 && (len - i) % 3 == 0 { vec![' ', c] } else { vec![c] }
        })
        .collect()
}

fn format_date(date: &str) -> String {
    if date.len() >= 10 {
        let p: Vec<&str> = date[..10].split('-').collect();
        if p.len() == 3 {
            return format!("{}/{}/{}", p[2], p[1], &p[0][2..]);
        }
    }
    date.to_string()
}

fn render_edit_popup(f: &mut Frame, area: Rect, buffer: &str) {
    let popup = centered_rect(50, 5, area);
    f.render_widget(Clear, popup);

    let content = format!("  {}_", buffer);
    let p = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(content, Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
    ])
    .block(
        Block::default()
            .title(Span::styled(
                " Saisir le montant ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(p, popup);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

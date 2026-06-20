use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Percentage(55),
            Constraint::Percentage(45),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(f, chunks[0], app);
    render_client_invoices(f, chunks[1], app);
    render_supplier_invoices(f, chunks[2], app);
    render_footer(f, chunks[3], app);
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

fn render_footer(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let last = app.last_refresh.map(|t| {
        let s = t.elapsed().as_secs();
        if s < 60 { format!("il y a {}s", s) } else { format!("il y a {}m{}s", s / 60, s % 60) }
    });

    let mut spans = vec![
        Span::styled("[q] Quitter", Style::default().fg(Color::DarkGray)),
        Span::styled("  [r] Rafraichir", Style::default().fg(Color::DarkGray)),
    ];

    if let Some(ref t) = last {
        spans.push(Span::styled(
            format!("  |  Derniere MAJ: {}", t),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let total = app.invoices.len() + app.supplier_invoices.len();
    if total > 0 {
        spans.push(Span::styled(
            format!("  |  {} clients / {} fournisseurs", app.invoices.len(), app.supplier_invoices.len()),
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

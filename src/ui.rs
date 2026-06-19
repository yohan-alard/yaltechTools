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
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(f, chunks[0], app);
    render_invoices(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let title = if app.loading {
        "  YalTech — Factures Qonto  ⟳ "
    } else {
        "  YalTech — Factures Qonto  "
    };

    let p = Paragraph::new(title)
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(p, area);
}

fn render_invoices(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(
            " Factures clients ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    if app.loading && app.invoices.is_empty() {
        let p = Paragraph::new("\n  Chargement des factures…")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(p, area);
        return;
    }

    if app.invoices.is_empty() {
        let p = Paragraph::new("\n  Aucune facture trouvée.")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(p, area);
        return;
    }

    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let header = Row::new(vec![
        Cell::from("N° Facture").style(header_style),
        Cell::from("Client").style(header_style),
        Cell::from("Montant TTC").style(header_style),
        Cell::from("Statut").style(header_style),
        Cell::from("Émise le").style(header_style),
        Cell::from("Échéance").style(header_style),
    ])
    .height(1)
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .invoices
        .iter()
        .map(|inv| {
            let status_style = match inv.status.as_str() {
                "Payée" => Style::default().fg(Color::Green),
                "Non payée" => Style::default().fg(Color::Yellow),
                "Brouillon" => Style::default().fg(Color::DarkGray),
                "Annulée" => Style::default().fg(Color::Red),
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
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(16),
        Constraint::Min(14),
        Constraint::Length(16),
        Constraint::Length(12),
        Constraint::Length(11),
        Constraint::Length(11),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(2);

    f.render_widget(table, area);
}

fn render_footer(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let last = app.last_refresh.map(|t| {
        let s = t.elapsed().as_secs();
        if s < 60 {
            format!("il y a {}s", s)
        } else {
            format!("il y a {}m{}s", s / 60, s % 60)
        }
    });

    let mut spans = vec![
        Span::styled("[q] Quitter", Style::default().fg(Color::DarkGray)),
        Span::styled("  [r] Rafraîchir", Style::default().fg(Color::DarkGray)),
    ];

    if let Some(ref t) = last {
        spans.push(Span::styled(
            format!("  │  Dernière MAJ: {}", t),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if let Some(ref err) = app.error {
        spans.push(Span::styled(
            format!("  │  ⚠ {}", truncate(err, 60)),
            Style::default().fg(Color::Red),
        ));
    }

    let count = if app.invoices.is_empty() {
        String::new()
    } else {
        format!("  │  {} facture(s)", app.invoices.len())
    };
    if !count.is_empty() {
        spans.push(Span::styled(count, Style::default().fg(Color::DarkGray)));
    }

    let footer = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}

fn format_amount(cents: i64, currency: &str) -> String {
    let abs = cents.abs();
    let sign = if cents < 0 { "-" } else { "" };
    let euros = abs / 100;
    let cts = abs % 100;
    format!("{}{},{:02} {}", sign, format_thousands(euros), cts, currency)
}

fn format_thousands(n: i64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    chars
        .iter()
        .enumerate()
        .flat_map(|(i, &c)| {
            let needs_sep = i > 0 && (len - i) % 3 == 0;
            if needs_sep {
                vec![' ', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

fn format_date(date: &str) -> String {
    if date.len() >= 10 {
        let parts: Vec<&str> = date[..10].split('-').collect();
        if parts.len() == 3 {
            return format!("{}/{}/{}", parts[2], parts[1], &parts[0][2..]);
        }
    }
    date.to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max - 1).collect();
        format!("{}…", t)
    }
}

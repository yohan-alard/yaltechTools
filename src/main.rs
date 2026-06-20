use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

mod app;
mod cache;
mod config;
mod gmail;
mod qonto;
mod ui;

use app::App;

fn main() -> Result<()> {
    color_eyre::install()?;
    dotenvy::dotenv().ok();
    config::load().map_err(|e| color_eyre::eyre::eyre!("{}", e))?;
    cache::init().map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    let rt = tokio::runtime::Runtime::new()?;

    let qonto_token = rt
        .block_on(qonto::auth::ensure_access_token())
        .map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    let google_token = rt
        .block_on(gmail::auth::ensure_access_token())
        .map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    let app = Arc::new(Mutex::new(App::new(qonto_token, google_token)));

    trigger_refresh(&rt, Arc::clone(&app));

    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, &rt, Arc::clone(&app));
    ratatui::restore();

    result
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    rt: &tokio::runtime::Runtime,
    app: Arc<Mutex<App>>,
) -> Result<()> {
    let poll_interval = Duration::from_millis(200);
    let auto_refresh = Duration::from_secs(config::get().app.auto_refresh_secs);
    let mut last_refresh = Instant::now();

    loop {
        {
            let mut a = app.lock().unwrap();
            terminal.draw(|f| ui::render(f, &mut a))?;
        }

        if event::poll(poll_interval)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.lock().unwrap().is_editing() {
                        match key.code {
                            KeyCode::Esc => app.lock().unwrap().cancel_edit(),
                            KeyCode::Enter => {
                                let result = app.lock().unwrap().confirm_edit_amount();
                                if let Some((msg_id, amount)) = result {
                                    cache::set_amount(&msg_id, &amount);
                                }
                            }
                            KeyCode::Backspace => app.lock().unwrap().pop_char(),
                            KeyCode::Char(c)   => app.lock().unwrap().push_char(c),
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => return Ok(()),
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                trigger_refresh(rt, Arc::clone(&app));
                                last_refresh = Instant::now();
                            }
                            KeyCode::Down  => app.lock().unwrap().mail_select_next(),
                            KeyCode::Up    => app.lock().unwrap().mail_select_prev(),
                            KeyCode::Enter => app.lock().unwrap().open_selected_pdf(),
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                let msg_id = app.lock().unwrap().ignore_selected();
                                if let Some(id) = msg_id {
                                    cache::set_ignored(&id);
                                }
                            }
                            KeyCode::Char('e') | KeyCode::Char('E') => {
                                app.lock().unwrap().start_edit_amount();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if last_refresh.elapsed() >= auto_refresh {
            trigger_refresh(rt, Arc::clone(&app));
            last_refresh = Instant::now();
        }
    }
}

fn trigger_refresh(rt: &tokio::runtime::Runtime, app: Arc<Mutex<App>>) {
    let (qonto_token, google_token) = {
        let a = app.lock().unwrap();
        (a.access_token.clone(), a.google_token.clone())
    };
    {
        let mut a = app.lock().unwrap();
        a.loading = true;
        a.error = None;
    }

    rt.spawn(async move {
        let (client_res, supplier_res, mail_res) = tokio::join!(
            qonto::api::fetch_invoices(&qonto_token),
            qonto::api::fetch_supplier_invoices(&qonto_token),
            gmail::api::fetch_mail_invoices(&google_token),
        );

        let mut a = app.lock().unwrap();
        a.loading = false;
        a.last_refresh = Some(Instant::now());

        let mut errors: Vec<String> = Vec::new();
        match client_res   { Ok(v) => a.invoices = v,           Err(e) => errors.push(format!("Qonto clients: {}", e)) }
        match supplier_res { Ok(v) => a.supplier_invoices = v,  Err(e) => errors.push(format!("Qonto fourn.: {}", e)) }
        match mail_res {
            Ok(v) => {
                a.mail_invoices = v;
                if !a.mail_invoices.is_empty() && a.mail_state.selected().is_none() {
                    a.mail_state.select(Some(0));
                }
            }
            Err(e) => errors.push(format!("Gmail: {}", e)),
        }
        if !errors.is_empty() { a.error = Some(errors.join(" | ")); }
    });
}

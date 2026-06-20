use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

mod api;
mod app;
mod auth;
mod config;
mod ui;

use app::App;

fn main() -> Result<()> {
    color_eyre::install()?;
    dotenvy::dotenv().ok();
    config::load().map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    let rt = tokio::runtime::Runtime::new()?;

    // Auth: get or refresh access token (may prompt browser login on first run)
    let access_token = rt
        .block_on(auth::ensure_access_token())
        .map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    let app = Arc::new(Mutex::new(App::new(access_token)));

    // Kick off initial data fetch before starting TUI
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
            let a = app.lock().unwrap();
            terminal.draw(|f| ui::render(f, &a))?;
        }

        if event::poll(poll_interval)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            return Ok(());
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            trigger_refresh(rt, Arc::clone(&app));
                            last_refresh = Instant::now();
                        }
                        _ => {}
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
    let token = app.lock().unwrap().access_token.clone();

    {
        let mut a = app.lock().unwrap();
        a.loading = true;
        a.error = None;
    }

    rt.spawn(async move {
        let (client_result, supplier_result) = tokio::join!(
            api::qonto::fetch_invoices(&token),
            api::qonto::fetch_supplier_invoices(&token),
        );
        let mut a = app.lock().unwrap();
        a.loading = false;
        a.last_refresh = Some(Instant::now());
        match client_result {
            Ok(invoices) => a.invoices = invoices,
            Err(e) => a.error = Some(e.to_string()),
        }
        match supplier_result {
            Ok(invoices) => a.supplier_invoices = invoices,
            Err(e) => {
                let msg = format!("Fournisseurs: {}", e);
                a.error = Some(a.error.take().map(|prev| format!("{} | {}", prev, msg)).unwrap_or(msg));
            }
        }
    });
}

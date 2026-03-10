mod app;
mod chat;
mod classifier;
mod continuity;
mod db;
mod fast_pair;
mod gatt;
mod scanner;
mod service_uuids;
mod tui;
mod vendor;

use app::App;
use chrono::Local;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use db::PendingObs;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use scanner::ScanMessage;
use std::io;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "bluemon-tui", about = "Interactive BLE scanner with TUI")]
struct Cli {
    /// BLE adapter index
    #[arg(short, long, default_value_t = 0)]
    adapter: usize,

    /// Scan duration per cycle in seconds
    #[arg(short, long, default_value_t = 3)]
    scan_duration: u64,

    /// SQLite database path
    #[arg(long)]
    db: Option<PathBuf>,

    /// Write a template service_uuids.toml to ~/.config/bluemon-tui/ and exit
    #[arg(long)]
    init_service_uuids: bool,
}

fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bluemon-tui")
        .join("bluemon.db")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    if cli.init_service_uuids {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bluemon-tui");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("service_uuids.toml");
        std::fs::write(&path, service_uuids::generate_template())?;
        println!("Wrote template to {}", path.display());
        return Ok(());
    }

    service_uuids::load_user_db();

    let db_path = cli.db.clone().unwrap_or_else(default_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = db::open(db_path.to_str().unwrap_or("bluemon.db"))?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, cli, conn, db_path).await;

    // Restore terminal
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cli: Cli,
    conn: rusqlite::Connection,
    db_path: PathBuf,
) -> anyhow::Result<()> {
    let db_path_str = db_path.to_str().unwrap_or("bluemon.db").to_string();
    let mut app = App::new(db_path_str);

    // Load persisted devices from DB
    match db::load_devices(&conn) {
        Ok(devices) => {
            app.devices = devices;
            app.rebuild_fingerprint_groups();
            app.rebuild_sorted_list();
        }
        Err(e) => eprintln!("Warning: failed to load devices from DB: {e}"),
    }

    // Create BLE adapter (shared between scanner and prober)
    let adapter = scanner::get_adapter(cli.adapter).await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<ScanMessage>();

    let scan_adapter = adapter.clone();
    let scan_duration = Duration::from_secs(cli.scan_duration);
    tokio::spawn(async move {
        scanner::scan_loop(tx, scan_adapter, scan_duration).await;
    });

    // GATT probe task
    let (probe_req_tx, probe_req_rx) = mpsc::unbounded_channel();
    let (probe_res_tx, mut probe_res_rx) = mpsc::unbounded_channel();
    app.probe_tx = Some(probe_req_tx);

    let probe_adapter = adapter.clone();
    tokio::spawn(async move {
        gatt::probe_loop(probe_adapter, probe_req_rx, probe_res_tx).await;
    });

    let mut pending_obs: Vec<PendingObs> = Vec::new();

    loop {
        terminal.draw(|f| tui::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.note_mode {
                    match key.code {
                        KeyCode::Esc => app.cancel_note(),
                        KeyCode::Enter => {
                            if let Some(mac) = app.save_note() {
                                let note = app
                                    .devices
                                    .get(&mac)
                                    .and_then(|d| d.note.as_deref())
                                    .unwrap_or("");
                                let _ = db::update_note(&conn, &mac, note);
                            }
                        }
                        KeyCode::Backspace => {
                            app.note_input.pop();
                        }
                        KeyCode::Char(c) => {
                            app.note_input.push(c);
                        }
                        _ => {}
                    }
                } else if app.filter_mode {
                    match key.code {
                        KeyCode::Esc => {
                            app.filter_mode = false;
                            app.filter.clear();
                            app.rebuild_sorted_list();
                        }
                        KeyCode::Enter => {
                            app.filter_mode = false;
                            app.rebuild_sorted_list();
                        }
                        KeyCode::Backspace => {
                            app.filter.pop();
                            app.rebuild_sorted_list();
                        }
                        KeyCode::Char(c) => {
                            app.filter.push(c);
                            app.rebuild_sorted_list();
                        }
                        _ => {}
                    }
                } else if app.chat_mode {
                    match key.code {
                        KeyCode::Esc => {
                            app.chat_mode = false;
                        }
                        KeyCode::Enter => {
                            app.chat.send_message();
                        }
                        KeyCode::Backspace => {
                            app.chat.input.pop();
                        }
                        KeyCode::Char(c) => {
                            app.chat.input.push(c);
                        }
                        KeyCode::Up => app.chat.scroll_up(),
                        KeyCode::Down => app.chat.scroll_down(),
                        _ => {}
                    }
                } else if app.detail_mode {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('d') => {
                            app.detail_mode = false;
                            app.detail_scroll = 0;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.detail_scroll = app.detail_scroll.saturating_add(1);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.detail_scroll = app.detail_scroll.saturating_sub(1);
                        }
                        KeyCode::Char('p') => {
                            try_probe(&mut app);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') => {
                            app.chat_mode = true;
                        }
                        KeyCode::Char('d') => {
                            if app.table_state.selected().is_some() {
                                app.detail_mode = true;
                                app.detail_scroll = 0;
                            }
                        }
                        KeyCode::Char('p') => {
                            try_probe(&mut app);
                        }
                        KeyCode::Char('s') => app.cycle_sort(),
                        KeyCode::Char('S') => app.reverse_sort(),
                        KeyCode::Char('/') => {
                            app.filter_mode = true;
                        }
                        KeyCode::Enter => app.enter_note_mode(),
                        KeyCode::Esc => {
                            app.filter.clear();
                            app.rebuild_sorted_list();
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
                        KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
                        KeyCode::Home => app.scroll_top(),
                        KeyCode::End => app.scroll_bottom(),
                        _ => {}
                    }
                }
            }
        }

        // Drain chat events
        app.chat.drain_events();

        // Drain scan results
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ScanMessage::Result(result) => {
                    pending_obs.push(PendingObs {
                        mac: result.mac.clone(),
                        rssi: result.rssi,
                        name: result.name.clone(),
                        service_uuids: result.service_uuids.join(","),
                    });
                    app.upsert_device(result);
                }
                ScanMessage::CycleComplete => {
                    app.scan_count += 1;
                    if !pending_obs.is_empty() {
                        let _ = db::write_cycle(&conn, &app.devices, &pending_obs);
                        pending_obs.clear();
                    }
                    app.rebuild_sorted_list();
                }
                ScanMessage::Error(_e) => {
                    // Transient scanner errors are ignored
                }
            }
        }

        // Drain GATT probe results
        while let Ok(result) = probe_res_rx.try_recv() {
            match result {
                gatt::ProbeResult::Success { mac, info } => {
                    if let Some(d) = app.devices.get_mut(&mac) {
                        d.gatt_info = Some(info.clone());
                        let _ = db::update_gatt_info(&conn, &mac, &info);
                    }
                    app.rebuild_sorted_list();
                }
                gatt::ProbeResult::Failed { .. } => {}
            }
        }
    }

    Ok(())
}

/// Send a GATT probe request for the currently selected device (5-min cooldown).
fn try_probe(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        return;
    };
    let Some(fp) = app.display_list.get(idx) else {
        return;
    };
    let Some(agg) = app.aggregated.get(fp) else {
        return;
    };
    let mac = agg.representative_mac.clone();
    let now = Local::now();
    let can_probe = app
        .probe_cooldowns
        .get(&mac)
        .map_or(true, |last| {
            now.signed_duration_since(*last).num_seconds() >= 300
        });
    if can_probe {
        app.probe_cooldowns.insert(mac.clone(), now);
        if let Some(tx) = &app.probe_tx {
            let _ = tx.send(gatt::ProbeRequest::Probe { mac });
        }
    }
}

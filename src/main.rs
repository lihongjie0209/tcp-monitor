use clap::Parser;
use ratatui::widgets::TableState;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time;

mod app;
mod diag;
mod tui;

use app::{AppState, ConnInfo};

/// tcp-monitor — real-time TCP connection quality monitor (Linux only)
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Refresh interval (e.g. 1s, 500ms)
    #[arg(short, long, default_value = "1s")]
    interval: humantime::Duration,

    /// Dump one snapshot to stdout (no TUI) and exit — useful for scripting/CI
    #[arg(long)]
    dump: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let interval: Duration = cli.interval.into();

    // --dump: print one snapshot and exit (no TUI)
    if cli.dump {
        let conns = diag::query_connections()?;
        let mut rows: Vec<&ConnInfo> = conns.values().collect();
        rows.sort_by(|a, b| a.rtt_us.cmp(&b.rtt_us));

        println!(
            "{:<22} {:<22} {:<7} {:<7} {:>8} {:>8} {:>8} {:>7} {:>9} {:>5}",
            "Source",
            "Destination",
            "State",
            "CA",
            "RTT(ms)",
            "Jitter",
            "Retrans",
            "Loss%",
            "Rate MB/s",
            "CWND"
        );
        println!("{}", "-".repeat(115));
        for c in &rows {
            println!(
                "{:<22} {:<22} {:<7} {:<7} {:>8.3} {:>8.3} {:>8} {:>7.2} {:>9.2} {:>5}",
                c.src,
                c.dst,
                format!("{:?}", c.state),
                c.ca_state_str(),
                c.rtt_ms(),
                c.rttvar_ms(),
                c.total_retrans,
                c.loss_pct(),
                c.delivery_rate_mbps(),
                c.cwnd,
            );
            // Print extended metrics as a sub-line
            if c.rto_us > 0 || c.lost > 0 || c.segs_out > 0 {
                println!(
                    "  ↳ RTO:{:.1}ms  lost:{} unacked:{}  segs_out:{} segs_in:{}  retrans%:{:.3}%  bytes_retrans%:{:.3}%",
                    c.rto_ms(),
                    c.lost,
                    c.unacked,
                    c.segs_out,
                    c.segs_in,
                    c.retrans_rate_pct(),
                    c.bytes_retrans_pct(),
                );
            }
        }
        println!("{}", "-".repeat(115));
        println!("Total: {} connections", rows.len());
        return Ok(());
    }

    let state = Arc::new(Mutex::new(AppState::new()));

    // Background task: poll kernel every `interval`
    let state_bg = Arc::clone(&state);
    tokio::spawn(async move {
        let mut ticker = time::interval(interval);
        loop {
            ticker.tick().await;
            match diag::query_connections() {
                Ok(fresh) => {
                    let mut s = state_bg.lock().unwrap();
                    s.merge(fresh);
                }
                Err(e) => {
                    eprintln!("diag error: {e}");
                }
            }
        }
    });

    // TUI runs on the main thread
    let mut terminal = tui::setup_terminal()?;
    let mut table_state = TableState::default();

    let result = run_tui(&mut terminal, &state, &mut table_state, interval).await;

    tui::restore_terminal(&mut terminal)?;
    result
}

async fn run_tui(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    state: &Arc<Mutex<AppState>>,
    table_state: &mut TableState,
    interval: Duration,
) -> anyhow::Result<()> {
    let poll_timeout = Duration::from_millis(100).min(interval / 2);

    loop {
        {
            let mut app = state.lock().unwrap();
            terminal.draw(|f| tui::draw(f, &mut app, table_state))?;
        }

        let should_quit = {
            let mut app = state.lock().unwrap();
            tui::handle_event(&mut app, poll_timeout)?
        };

        if should_quit {
            break;
        }
    }

    Ok(())
}

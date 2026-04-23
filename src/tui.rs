// TUI rendering and keyboard event handling using Ratatui + Crossterm

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use std::{io, time::Duration};

use crate::app::{AppState, FilterMode, SortColumn, TcpState};

const HDR_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

pub fn draw(f: &mut Frame, app: &mut AppState, table_state: &mut TableState) {
    let area = f.area();

    // Layout: header(1) + table(fill) + detail(7) + footer(1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(f, chunks[0], app);
    draw_table(f, chunks[1], app, table_state);
    draw_detail(f, chunks[2], app);
    draw_footer(f, chunks[3], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &AppState) {
    let src_style = if app.filter_mode == FilterMode::Src {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let dst_style = if app.filter_mode == FilterMode::Dst {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let src_filter = if app.filter_src.is_empty() {
        "  -  ".to_string()
    } else {
        format!(" {} ", app.filter_src)
    };
    let dst_filter = if app.filter_dst.is_empty() {
        "  -  ".to_string()
    } else {
        format!(" {} ", app.filter_dst)
    };

    let line = Line::from(vec![
        Span::styled(
            " tcp-monitor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ Src: "),
        Span::styled(src_filter, src_style),
        Span::raw(" │ Dst: "),
        Span::styled(dst_filter, dst_style),
        Span::raw(format!(
            " │ {}/{} connections ",
            app.visible_count(),
            app.total_count()
        )),
    ]);

    let para = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(para, area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &mut AppState, table_state: &mut TableState) {
    let sort_col = &app.sort_col;
    let sort_asc = app.sort_asc;

    let sort_indicator = |col: &SortColumn| {
        if col == sort_col {
            if sort_asc { " ▲" } else { " ▼" }
        } else {
            ""
        }
    };

    let header_cells = [
        format!("Source{}", sort_indicator(&SortColumn::Src)),
        format!("Destination{}", sort_indicator(&SortColumn::Dst)),
        format!("State{}", sort_indicator(&SortColumn::State)),
        format!("CA{}", sort_indicator(&SortColumn::State)), // same col for visual
        format!("RTT ms{}", sort_indicator(&SortColumn::Rtt)),
        format!("Jitter{}", sort_indicator(&SortColumn::Jitter)),
        format!("Retrans{}", sort_indicator(&SortColumn::Retrans)),
        format!("Loss%{}", sort_indicator(&SortColumn::Loss)),
        format!("Rate MB/s{}", sort_indicator(&SortColumn::Rate)),
        format!("CWND{}", sort_indicator(&SortColumn::Cwnd)),
    ]
    .iter()
    .map(|h| Cell::from(h.clone()).style(HDR_STYLE))
    .collect::<Vec<_>>();

    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = app
        .visible
        .iter()
        .map(|c| {
            let state_style = match c.state {
                TcpState::Established => Style::default().fg(Color::Green),
                TcpState::CloseWait | TcpState::TimeWait => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::Red),
            };

            let rtt_style = if c.rtt_us == 0 {
                Style::default().fg(Color::DarkGray)
            } else if c.rtt_us < 5_000 {
                Style::default().fg(Color::Green)
            } else if c.rtt_us < 50_000 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            };

            let retrans_style = if c.retrans_delta > 0 {
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            } else if c.total_retrans > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            };

            let loss_style = if c.lost > 0 {
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };

            let ca_style = if c.ca_is_bad() {
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            } else if c.ca_state > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let rtt_str = if c.rtt_us == 0 {
                "    -  ".to_string()
            } else {
                format!("{:>7.3}", c.rtt_ms())
            };
            let jitter_str = if c.rttvar_us == 0 {
                "    -  ".to_string()
            } else {
                format!("{:>7.3}", c.rttvar_ms())
            };
            let retrans_str = if c.retrans_delta > 0 {
                format!("{} (+{})", c.total_retrans, c.retrans_delta)
            } else {
                format!("{}", c.total_retrans)
            };
            let loss_str = if c.segs_out == 0 {
                "  -  ".to_string()
            } else {
                format!("{:.2}", c.loss_pct())
            };
            let rate_str = if c.delivery_rate_bps == 0 {
                "    -  ".to_string()
            } else {
                format!("{:>7.2}", c.delivery_rate_mbps())
            };

            Row::new(vec![
                Cell::from(c.src.clone()),
                Cell::from(c.dst.clone()),
                Cell::from(c.state.short()).style(state_style),
                Cell::from(c.ca_state_str()).style(ca_style),
                Cell::from(rtt_str).style(rtt_style),
                Cell::from(jitter_str).style(rtt_style),
                Cell::from(retrans_str).style(retrans_style),
                Cell::from(loss_str).style(loss_style),
                Cell::from(rate_str),
                Cell::from(format!("{}", c.cwnd)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Min(20),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(11),
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Length(5),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::NONE))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        );

    table_state.select(if app.visible.is_empty() {
        None
    } else {
        Some(app.selected)
    });
    f.render_stateful_widget(table, area, table_state);
}

/// Render a detail panel for the currently selected connection
fn draw_detail(f: &mut Frame, area: Rect, app: &AppState) {
    let Some(conn) = app.visible.get(app.selected) else {
        let para = Paragraph::new(" No connection selected ")
            .block(Block::default().borders(Borders::ALL).title(" Details "));
        f.render_widget(para, area);
        return;
    };

    let na = |v: f64, unit: &str| -> String {
        if v == 0.0 {
            "  n/a ".to_string()
        } else {
            format!("{v:.3}{unit}")
        }
    };
    let na_u64 = |v: u64| -> String {
        if v == 0 {
            "n/a".to_string()
        } else {
            format_bytes(v)
        }
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  RTT: "),
            Span::styled(
                format!("{:.3}ms", conn.rtt_ms()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(format!("  jitter: {:.3}ms", conn.rttvar_ms())),
            Span::raw(format!("  min(kern): {}", na(conn.kern_min_rtt_ms(), "ms"))),
            Span::raw(format!("  min(seen): {}", na(conn.rtt_min_ms(), "ms"))),
            Span::raw(format!("  max: {:.3}ms", conn.rtt_max_ms())),
            Span::raw(format!("  avg: {:.3}ms", conn.rtt_avg_ms())),
            Span::raw(format!("  RTO: {:.1}ms", conn.rto_ms())),
        ]),
        Line::from(vec![
            Span::raw("  CA: "),
            Span::styled(
                conn.ca_state_str(),
                if conn.ca_is_bad() {
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
            Span::raw(format!(
                "  Unacked: {}  Sacked: {}  Lost: {}  In-flight retrans: {}",
                conn.unacked, conn.lost, conn.lost, conn.retrans_in_flight
            )),
        ]),
        Line::from(vec![
            Span::raw("  Retrans: "),
            Span::styled(
                format!("{}", conn.total_retrans),
                if conn.total_retrans > 0 {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
            Span::raw(format!(
                "  rate: {:.3}%  | Loss%: {:.3}%  (lost pkts: {})",
                conn.retrans_rate_pct(),
                conn.loss_pct(),
                conn.lost
            )),
            Span::raw(format!(
                "  bytes retrans: {} ({:.3}%)",
                na_u64(conn.bytes_retrans),
                conn.bytes_retrans_pct()
            )),
        ]),
        Line::from(vec![
            Span::raw("  Delivery rate: "),
            Span::styled(
                format!("{:.2} MB/s", conn.delivery_rate_mbps()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(format!(
                "  Sent: {}  Recv: {}",
                na_u64(conn.bytes_sent),
                na_u64(0u64)
            )),
            Span::raw(format!(
                "  Segs out: {}  Segs in: {}",
                conn.segs_out, conn.segs_in
            )),
        ]),
        Line::from(vec![
            Span::raw(format!(
                "  CWND: {}  MSS: {}  PMTU: {}",
                conn.cwnd, conn.snd_mss, conn.pmtu
            )),
            Span::raw(format!("  Samples: {}", conn.samples)),
        ]),
    ];

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" ▶ {} → {} ", conn.src, conn.dst))
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(para, area);
}

fn format_bytes(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}GB", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}MB", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}KB", n as f64 / 1e3)
    } else {
        format!("{n}B")
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &AppState) {
    let filter_hint = match app.filter_mode {
        FilterMode::None => {
            " F3 SrcFilter  F4 DstFilter  F6 SortBy(RTT/Jitter/Retrans/Loss/Rate/…)  R Reverse  ESC Clear  q Quit "
        }
        FilterMode::Src => " [Editing Src Filter — type to filter, ESC when done] ",
        FilterMode::Dst => " [Editing Dst Filter — type to filter, ESC when done] ",
    };
    let para =
        Paragraph::new(filter_hint).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(para, area);
}

/// Returns true if the application should quit
pub fn handle_event(app: &mut AppState, timeout: Duration) -> io::Result<bool> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            // In filter-edit mode: most keys go to the filter string
            if app.filter_mode != FilterMode::None {
                match key.code {
                    KeyCode::Esc => {
                        app.filter_mode = FilterMode::None;
                        // don't clear, just stop editing
                        app.recompute_visible();
                    }
                    KeyCode::Backspace => app.filter_backspace(),
                    KeyCode::Char(c) => app.filter_push(c),
                    _ => {}
                }
                return Ok(false);
            }

            // Normal mode
            match key.code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(true);
                }
                KeyCode::Char('R') | KeyCode::Char('r') => {
                    app.sort_asc = !app.sort_asc;
                    app.recompute_visible();
                }
                KeyCode::Up => app.move_up(),
                KeyCode::Down => app.move_down(),
                KeyCode::F(3) => {
                    app.filter_mode = FilterMode::Src;
                }
                KeyCode::F(4) => {
                    app.filter_mode = FilterMode::Dst;
                }
                KeyCode::F(6) => {
                    app.sort_col = app.sort_col.next();
                    app.recompute_visible();
                }
                KeyCode::Esc => {
                    app.clear_active_filter();
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

pub fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

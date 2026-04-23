// TUI rendering and keyboard event handling using Ratatui + Crossterm

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use std::{io, time::Duration};

use crate::app::{AppState, FilterMode, Health, SortColumn, TcpState};

const HDR_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

pub fn draw(f: &mut Frame, app: &mut AppState, table_state: &mut TableState) {
    let area = f.area();

    // Layout: header(1) + table(fill) + detail(9) + footer(1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(9),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(f, chunks[0], app);
    draw_table(f, chunks[1], app, table_state);
    draw_detail(f, chunks[2], app);
    draw_footer(f, chunks[3], app);

    if app.show_help {
        draw_help(f, area);
    }
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
        "●".to_string(),
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
                Cell::from(c.overall_health().dot().to_string())
                    .style(Style::default().fg(c.overall_health().color())),
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
        Constraint::Length(3),
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

    let badge = |h: Health| -> Span {
        Span::styled(format!("[{}]", h.badge()), Style::default().fg(h.color()))
    };
    let dot = |h: Health| -> Span {
        Span::styled(
            h.dot().to_string(),
            Style::default().fg(h.color()).add_modifier(Modifier::BOLD),
        )
    };
    let fmt_ssthresh = |v: u32| -> String {
        if v >= 0x7fff_0000 {
            "∞".to_string()
        } else {
            format_bytes(v as u64)
        }
    };

    // Category health
    let lat_h = [conn.rtt_health(), conn.jitter_health(), conn.rto_health()]
        .iter()
        .copied()
        .max()
        .unwrap_or(Health::Good);
    let cong_h = conn.ca_health();
    let loss_h = [
        conn.loss_health(),
        conn.retrans_health(),
        conn.dsack_health(),
    ]
    .iter()
    .copied()
    .max()
    .unwrap_or(Health::Good);
    let reord_h = [conn.reorder_health(), conn.ooo_health()]
        .iter()
        .copied()
        .max()
        .unwrap_or(Health::Good);
    let buf_h = [
        conn.notsent_health(),
        conn.rwnd_health(),
        conn.sndbuf_health(),
    ]
    .iter()
    .copied()
    .max()
    .unwrap_or(Health::Good);

    let lines = vec![
        // Line 1: LATENCY
        Line::from(vec![
            Span::raw(" "),
            dot(lat_h),
            Span::raw(format!(" {:<12}", "LATENCY")),
            Span::raw(format!("RTT: {:.3}ms ", conn.rtt_ms())),
            badge(conn.rtt_health()),
            Span::raw(format!("  Jitter: {:.3}ms ", conn.rttvar_ms())),
            badge(conn.jitter_health()),
            Span::raw(format!("  MinRTT: {:.3}ms", conn.kern_min_rtt_ms())),
            Span::raw(format!("  RTO: {:.0}ms ", conn.rto_ms())),
            badge(conn.rto_health()),
        ]),
        // Line 2: CONGESTION
        Line::from(vec![
            Span::raw(" "),
            dot(cong_h),
            Span::raw(format!(" {:<12}", "CONGESTION")),
            Span::raw("CA: "),
            Span::styled(conn.ca_state_str(), Style::default().fg(cong_h.color())),
            Span::raw(" "),
            badge(conn.ca_health()),
            Span::raw(format!("  CWND: {}", conn.cwnd)),
            Span::raw(format!("  ssthresh: {}", fmt_ssthresh(conn.snd_ssthresh))),
            Span::raw(format!("  Unacked: {}", conn.unacked)),
            Span::raw(format!("  Retrans: {} ", conn.total_retrans)),
            badge(conn.retrans_health()),
        ]),
        // Line 3: LOSS
        Line::from(vec![
            Span::raw(" "),
            dot(loss_h),
            Span::raw(format!(" {:<12}", "LOSS")),
            Span::raw(format!("Loss%: {:.2}% ", conn.loss_pct())),
            badge(conn.loss_health()),
            Span::raw(format!("  Retrans%: {:.3}% ", conn.retrans_rate_pct())),
            badge(conn.retrans_health()),
            Span::raw(format!("  InFlight: {}", conn.retrans_in_flight)),
            Span::raw(format!("  DSACK: {} ", conn.dsack_dups)),
            badge(conn.dsack_health()),
        ]),
        // Line 4: REORDER
        Line::from(vec![
            Span::raw(" "),
            dot(reord_h),
            Span::raw(format!(" {:<12}", "REORDER")),
            Span::raw(format!("OOO: {} ", conn.rcv_ooopack)),
            badge(conn.ooo_health()),
            Span::raw(format!("  Reorder-events: {} ", conn.reord_seen)),
            badge(conn.reorder_health()),
            Span::raw(format!(
                "  Rcv-RTT: {:.3}ms",
                conn.rcv_rtt_us as f64 / 1000.0
            )),
        ]),
        // Line 5: THROUGHPUT
        Line::from(vec![
            Span::raw(" "),
            dot(Health::Good),
            Span::raw(format!(" {:<12}", "THROUGHPUT")),
            Span::raw(format!("Rate: {:.2} MB/s", conn.delivery_rate_mbps())),
            Span::raw(format!("  Sent: {}", format_bytes(conn.bytes_sent))),
            Span::raw(format!("  Segs ↑{} ↓{}", conn.segs_out, conn.segs_in)),
            Span::raw(format!("  Delivered: {}", conn.delivered)),
        ]),
        // Line 6: BUFFERS
        Line::from(vec![
            Span::raw(" "),
            dot(buf_h),
            Span::raw(format!(" {:<12}", "BUFFERS")),
            Span::raw(format!(
                "Notsent: {} ",
                format_bytes(conn.notsent_bytes as u64)
            )),
            badge(conn.notsent_health()),
            Span::raw(format!(
                "  RcvSpace: {}",
                format_bytes(conn.rcv_space as u64)
            )),
            Span::raw(format!("  RwndLtd: {:.1}% ", conn.rwnd_limited_pct())),
            badge(conn.rwnd_health()),
            Span::raw(format!("  BufLtd: {:.1}% ", conn.sndbuf_limited_pct())),
            badge(conn.sndbuf_health()),
        ]),
        // Line 7: PATH
        Line::from(vec![Span::raw(format!(
            "   {:<12}PMTU: {}  MSS: {}  ECN-CE: {}  Samples: {}",
            "PATH", conn.pmtu, conn.snd_mss, conn.delivered_ce, conn.samples
        ))]),
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
            " ?/h 帮助  F3 源地址过滤  F4 目标过滤  F6 排序切换  R 反转  ESC 清除  q 退出 "
        }
        FilterMode::Src => " [编辑源地址过滤 — 输入关键字，ESC 完成] ",
        FilterMode::Dst => " [编辑目标过滤 — 输入关键字，ESC 完成] ",
    };
    let para =
        Paragraph::new(filter_hint).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(para, area);
}

/// Centered help overlay showing all metrics explained in Chinese
fn draw_help(f: &mut Frame, area: Rect) {
    // Center a 70×44 popup
    let popup_w = 70u16.min(area.width.saturating_sub(4));
    let popup_h = 44u16.min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(popup_w) / 2;
    let y = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup_area = Rect::new(x, y, popup_w, popup_h);

    let title_s = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let key_s = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let good_s = Style::default().fg(Color::Green);
    let warn_s = Style::default().fg(Color::Yellow);
    let bad_s = Style::default().fg(Color::LightRed);
    let dim_s = Style::default().fg(Color::DarkGray);

    macro_rules! sec {
        ($t:expr) => {
            Line::from(vec![Span::styled(format!("  ── {} ", $t), title_s)])
        };
    }
    macro_rules! row {
        ($name:expr, $desc:expr, $good:expr, $warn:expr, $bad:expr) => {
            Line::from(vec![
                Span::raw(format!("  {:12}", $name)),
                Span::raw(format!("{:28}", $desc)),
                Span::styled(format!("✓{:<13}", $good), good_s),
                Span::styled(format!("!{:<13}", $warn), warn_s),
                Span::styled(format!("✗{}", $bad), bad_s),
            ])
        };
    }
    macro_rules! info_row {
        ($name:expr, $desc:expr) => {
            Line::from(vec![
                Span::raw(format!("  {:12}", $name)),
                Span::styled($desc, dim_s),
            ])
        };
    }

    let header = Line::from(vec![
        Span::raw(format!("  {:12}", "指标")),
        Span::raw(format!("{:28}", "说明")),
        Span::styled(format!("✓{:<13}", "正常"), good_s),
        Span::styled(format!("!{:<13}", "告警"), warn_s),
        Span::styled("✗异常", bad_s),
    ]);

    let lines = vec![
        Line::from(vec![Span::styled(" tcp-monitor 指标说明", title_s)]),
        Line::from(""),
        sec!("延迟 LATENCY"),
        header.clone(),
        row!(
            "RTT",
            "往返时延，反映网络基础延迟",
            "<10ms",
            "10~100ms",
            "≥100ms"
        ),
        row!(
            "Jitter",
            "RTT 抖动(变化量/RTT)，反映延迟稳定性",
            "<25%",
            "25~75%",
            "≥75%"
        ),
        row!("MinRTT", "历史最低 RTT（内核记录），仅展示", "—", "—", "—"),
        row!(
            "RTO",
            "重传超时时间，过大说明链路不稳",
            "<500ms",
            "500ms~3s",
            "≥3s"
        ),
        Line::from(""),
        sec!("拥塞 CONGESTION"),
        row!(
            "CA State",
            "拥塞控制状态机",
            "Open",
            "Disorder/CWR",
            "Recovery/Loss"
        ),
        row!("CWND", "拥塞窗口(段)，反映可并发发送量", "—", "—", "—"),
        row!(
            "ssthresh",
            "慢启动阈值，CWND 降至此值说明曾经拥塞",
            "—",
            "—",
            "—"
        ),
        row!("Unacked", "已发出但未收到 ACK 的字节数", "—", "—", "—"),
        Line::from(""),
        sec!("丢包 LOSS"),
        row!(
            "Loss%",
            "估算丢包率 = lost/(segs_out+lost)",
            "0%",
            "<0.1%",
            "≥0.1%"
        ),
        row!(
            "Retrans%",
            "重传率 = bytes_retrans/bytes_sent",
            "0%",
            "<1%",
            "≥1%"
        ),
        row!("InFlight", "当前正在重传的段数", "—", "—", "—"),
        row!("DSACK", "收到 DSACK 块数，指示伪重传次数", "0", "1~5", ">5"),
        Line::from(""),
        sec!("乱序 REORDER"),
        row!(
            "OOO",
            "接收到的乱序包数量（kernel ≥5.4）",
            "0",
            "1~10",
            ">10"
        ),
        row!("Reorder", "检测到的数据段乱序事件次数", "0", "1~3", ">3"),
        row!("Rcv-RTT", "接收侧估算的 RTT（独立于发送侧）", "—", "—", "—"),
        Line::from(""),
        sec!("吞吐 THROUGHPUT"),
        info_row!("Rate", "当前交付速率 MB/s（内核 pacing_rate 估算）"),
        info_row!("Sent", "累计已发送字节数"),
        info_row!("Segs", "↑ 发送段数  ↓ 接收段数"),
        info_row!("Delivered", "成功交付的段数（含 ECN）"),
        Line::from(""),
        sec!("缓冲 BUFFERS"),
        row!(
            "Notsent",
            "已入队但尚未发出的字节（发送积压）",
            "<64KB",
            "64KB~1MB",
            "≥1MB"
        ),
        row!("RcvSpace", "接收缓冲区剩余空间", "—", "—", "—"),
        row!(
            "RwndLtd%",
            "连接繁忙时被接收窗口限制的时间占比",
            "<5%",
            "5~25%",
            "≥25%"
        ),
        row!(
            "BufLtd%",
            "连接繁忙时被发送缓冲限制的时间占比",
            "<5%",
            "5~25%",
            "≥25%"
        ),
        Line::from(""),
        sec!("路径 PATH"),
        info_row!("PMTU", "路径最大传输单元（字节）"),
        info_row!("MSS", "最大报文段大小（字节）"),
        info_row!("ECN-CE", "收到 ECN 拥塞标记的包数"),
        Line::from(""),
        Line::from(vec![
            Span::raw("  快捷键："),
            Span::styled("?/h", key_s),
            Span::raw(" 切换帮助  "),
            Span::styled("↑↓", key_s),
            Span::raw(" 选择连接  "),
            Span::styled("F3/F4", key_s),
            Span::raw(" 过滤  "),
            Span::styled("F6", key_s),
            Span::raw(" 排序  "),
            Span::styled("R", key_s),
            Span::raw(" 反转  "),
            Span::styled("q", key_s),
            Span::raw(" 退出"),
        ]),
    ];

    f.render_widget(Clear, popup_area);
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 帮助 — 按 ? 或 h 关闭 ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Left);
    f.render_widget(para, popup_area);
}

/// Returns true if the application should quit
pub fn handle_event(app: &mut AppState, timeout: Duration) -> io::Result<bool> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()?
    {
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
            KeyCode::Char('?') | KeyCode::Char('h') => {
                app.show_help = !app.show_help;
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
                if app.show_help {
                    app.show_help = false;
                } else {
                    app.clear_active_filter();
                }
            }
            _ => {}
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

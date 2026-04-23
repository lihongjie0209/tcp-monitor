// Application state: connection list, sort/filter state, selected row

use std::collections::HashMap;
use std::fmt;

/// TCP connection state enum
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    Unknown(u8),
}

impl TcpState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Established,
            2 => Self::SynSent,
            3 => Self::SynRecv,
            4 => Self::FinWait1,
            5 => Self::FinWait2,
            6 => Self::TimeWait,
            7 => Self::Close,
            8 => Self::CloseWait,
            9 => Self::LastAck,
            10 => Self::Listen,
            11 => Self::Closing,
            _ => Self::Unknown(v),
        }
    }
    pub fn short(&self) -> &'static str {
        match self {
            Self::Established => "ESTAB",
            Self::SynSent => "SYN-S",
            Self::SynRecv => "SYN-R",
            Self::FinWait1 => "FIN-1",
            Self::FinWait2 => "FIN-2",
            Self::TimeWait => "T-WAIT",
            Self::Close => "CLOSE",
            Self::CloseWait => "C-WAIT",
            Self::LastAck => "L-ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSNG",
            Self::Unknown(_) => "?",
        }
    }
}

impl fmt::Display for TcpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// Per-connection data with TCP_INFO metrics
#[derive(Debug, Clone)]
pub struct ConnInfo {
    pub key: String,
    pub src: String,
    pub dst: String,
    pub state: TcpState,
    // ── Basic RTT ──────────────────────────────────────────────────────────────
    pub rtt_us: u32,        // smoothed RTT (us)
    pub rttvar_us: u32,     // RTT variance / jitter (us)
    pub min_rtt_us: u32,    // kernel-tracked minimum RTT (us, kernel 4.8+; 0 = unavail)
    // ── Retransmit / loss ──────────────────────────────────────────────────────
    pub total_retrans: u32,      // cumulative retransmit count
    pub retrans_in_flight: u32,  // currently in-flight retransmits
    pub lost: u32,               // packets kernel considers lost (current estimate)
    pub segs_out: u32,           // total segments sent (kernel 4.2+)
    pub segs_in: u32,            // total segments received
    pub bytes_sent: u64,         // total bytes sent (kernel 5.1+)
    pub bytes_retrans: u64,      // bytes retransmitted (kernel 5.1+)
    // ── Congestion / window ────────────────────────────────────────────────────
    pub cwnd: u32,          // congestion window (segments)
    pub ca_state: u8,       // TCP_CA_Open/Disorder/CWR/Recovery/Loss
    pub rto_us: u32,        // retransmit timeout (us)
    pub unacked: u32,       // unacknowledged packets
    // ── Throughput ─────────────────────────────────────────────────────────────
    pub delivery_rate_bps: u64,  // bytes/sec delivery rate (kernel 4.9+)
    // ── Path ───────────────────────────────────────────────────────────────────
    pub pmtu: u32,
    pub snd_mss: u32,
    // ── Historical stats (computed across samples) ─────────────────────────────
    pub prev_retrans: u32,
    pub retrans_delta: u32,  // retrans since last sample
    pub samples: u64,
    pub rtt_min_us: u32,     // our tracked min (across all samples)
    pub rtt_max_us: u32,
    pub rtt_sum_us: u64,
}

impl ConnInfo {
    pub fn rtt_ms(&self) -> f64 {
        self.rtt_us as f64 / 1000.0
    }
    pub fn rttvar_ms(&self) -> f64 {
        self.rttvar_us as f64 / 1000.0
    }
    pub fn rto_ms(&self) -> f64 {
        self.rto_us as f64 / 1000.0
    }
    pub fn rtt_avg_ms(&self) -> f64 {
        if self.samples == 0 { 0.0 }
        else { self.rtt_sum_us as f64 / self.samples as f64 / 1000.0 }
    }
    pub fn rtt_min_ms(&self) -> f64 {
        if self.rtt_min_us == u32::MAX { 0.0 }
        else { self.rtt_min_us as f64 / 1000.0 }
    }
    pub fn rtt_max_ms(&self) -> f64 {
        self.rtt_max_us as f64 / 1000.0
    }
    /// Kernel-tracked min RTT in ms (0 if not available)
    pub fn kern_min_rtt_ms(&self) -> f64 {
        self.min_rtt_us as f64 / 1000.0
    }
    /// Retransmit rate % = total_retrans / segs_out
    pub fn retrans_rate_pct(&self) -> f64 {
        if self.segs_out == 0 { return 0.0; }
        self.total_retrans as f64 / self.segs_out as f64 * 100.0
    }
    /// Byte retransmit rate % = bytes_retrans / bytes_sent (kernel 5.1+)
    pub fn bytes_retrans_pct(&self) -> f64 {
        if self.bytes_sent == 0 { return 0.0; }
        self.bytes_retrans as f64 / self.bytes_sent as f64 * 100.0
    }
    /// Packet loss rate % = lost / (segs_out + lost)
    pub fn loss_pct(&self) -> f64 {
        let denom = self.segs_out as f64 + self.lost as f64;
        if denom == 0.0 { return 0.0; }
        self.lost as f64 / denom * 100.0
    }
    /// Delivery rate in MB/s
    pub fn delivery_rate_mbps(&self) -> f64 {
        self.delivery_rate_bps as f64 / 1_000_000.0
    }
    /// CA state label
    pub fn ca_state_str(&self) -> &'static str {
        match self.ca_state {
            0 => "Open",
            1 => "Disord",
            2 => "CWR",
            3 => "Recov",
            4 => "Loss",
            _ => "?",
        }
    }
    /// True if CA state indicates a problem
    pub fn ca_is_bad(&self) -> bool {
        self.ca_state >= 3
    }

    /// Merge fresh data from a new query, preserving historical stats
    pub fn update(&mut self, fresh: &ConnInfo) {
        self.retrans_delta = fresh.total_retrans.saturating_sub(self.total_retrans);
        self.total_retrans = fresh.total_retrans;
        self.state = fresh.state.clone();
        self.rtt_us = fresh.rtt_us;
        self.rttvar_us = fresh.rttvar_us;
        self.cwnd = fresh.cwnd;
        self.ca_state = fresh.ca_state;
        self.rto_us = fresh.rto_us;
        self.unacked = fresh.unacked;
        self.lost = fresh.lost;
        self.retrans_in_flight = fresh.retrans_in_flight;
        self.segs_out = fresh.segs_out;
        self.segs_in = fresh.segs_in;
        self.min_rtt_us = fresh.min_rtt_us;
        self.delivery_rate_bps = fresh.delivery_rate_bps;
        self.bytes_sent = fresh.bytes_sent;
        self.bytes_retrans = fresh.bytes_retrans;
        self.pmtu = fresh.pmtu;
        self.snd_mss = fresh.snd_mss;
        if fresh.rtt_us > 0 {
            self.samples += 1;
            self.rtt_sum_us += fresh.rtt_us as u64;
            if fresh.rtt_us < self.rtt_min_us {
                self.rtt_min_us = fresh.rtt_us;
            }
            if fresh.rtt_us > self.rtt_max_us {
                self.rtt_max_us = fresh.rtt_us;
            }
        }
    }
}

/// Column used for sorting
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortColumn {
    Src,
    Dst,
    State,
    Rtt,
    Jitter,
    Retrans,
    Loss,
    Rate,
    Cwnd,
}

impl SortColumn {
    pub fn next(&self) -> Self {
        match self {
            Self::Src => Self::Dst,
            Self::Dst => Self::State,
            Self::State => Self::Rtt,
            Self::Rtt => Self::Jitter,
            Self::Jitter => Self::Retrans,
            Self::Retrans => Self::Loss,
            Self::Loss => Self::Rate,
            Self::Rate => Self::Cwnd,
            Self::Cwnd => Self::Src,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Src => "Source",
            Self::Dst => "Destination",
            Self::State => "State",
            Self::Rtt => "RTT",
            Self::Jitter => "Jitter",
            Self::Retrans => "Retrans",
            Self::Loss => "Loss%",
            Self::Rate => "Rate",
            Self::Cwnd => "CWND",
        }
    }
}

/// Filter mode — which field is being edited
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterMode {
    None,
    Src,
    Dst,
}

pub struct AppState {
    /// All known connections, keyed by src→dst
    pub connections: HashMap<String, ConnInfo>,
    /// Currently visible (filtered + sorted) list
    pub visible: Vec<ConnInfo>,
    pub sort_col: SortColumn,
    pub sort_asc: bool,
    pub filter_src: String,
    pub filter_dst: String,
    pub filter_mode: FilterMode,
    pub selected: usize,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            visible: Vec::new(),
            sort_col: SortColumn::Rtt,
            sort_asc: false,
            filter_src: String::new(),
            filter_dst: String::new(),
            filter_mode: FilterMode::None,
            selected: 0,
        }
    }

    /// Merge a fresh snapshot of connections into the state
    pub fn merge(&mut self, fresh: HashMap<String, ConnInfo>) {
        // Update existing or insert new
        for (key, fresh_conn) in fresh {
            self.connections
                .entry(key)
                .and_modify(|c| c.update(&fresh_conn))
                .or_insert(fresh_conn);
        }
        self.recompute_visible();
    }

    /// Reapply filter + sort to produce the visible list
    pub fn recompute_visible(&mut self) {
        let src_f = self.filter_src.to_lowercase();
        let dst_f = self.filter_dst.to_lowercase();

        let mut v: Vec<ConnInfo> = self
            .connections
            .values()
            .filter(|c| {
                (src_f.is_empty() || c.src.to_lowercase().contains(&src_f))
                    && (dst_f.is_empty() || c.dst.to_lowercase().contains(&dst_f))
            })
            .cloned()
            .collect();

        v.sort_by(|a, b| {
            use std::cmp::Ordering;
            let ord = match self.sort_col {
                SortColumn::Src => a.src.cmp(&b.src),
                SortColumn::Dst => a.dst.cmp(&b.dst),
                SortColumn::State => a.state.short().cmp(b.state.short()),
                SortColumn::Rtt => a.rtt_us.cmp(&b.rtt_us),
                SortColumn::Jitter => a.rttvar_us.cmp(&b.rttvar_us),
                SortColumn::Retrans => a.total_retrans.cmp(&b.total_retrans),
                SortColumn::Loss => a.lost.cmp(&b.lost),
                SortColumn::Rate => a.delivery_rate_bps.cmp(&b.delivery_rate_bps),
                SortColumn::Cwnd => a.cwnd.cmp(&b.cwnd),
            };
            // Treat Equal with Ordering::Equal to satisfy the type checker
            let _: Ordering = ord;
            if self.sort_asc { ord } else { ord.reverse() }
        });

        self.visible = v;
        if self.selected >= self.visible.len() && !self.visible.is_empty() {
            self.selected = self.visible.len() - 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.visible.len() {
            self.selected += 1;
        }
    }

    pub fn total_count(&self) -> usize {
        self.connections.len()
    }

    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    /// Handle a character typed while in filter mode
    pub fn filter_push(&mut self, c: char) {
        match self.filter_mode {
            FilterMode::Src => self.filter_src.push(c),
            FilterMode::Dst => self.filter_dst.push(c),
            FilterMode::None => {}
        }
        self.recompute_visible();
    }

    pub fn filter_backspace(&mut self) {
        match self.filter_mode {
            FilterMode::Src => {
                self.filter_src.pop();
            }
            FilterMode::Dst => {
                self.filter_dst.pop();
            }
            FilterMode::None => {}
        }
        self.recompute_visible();
    }

    pub fn clear_active_filter(&mut self) {
        match self.filter_mode {
            FilterMode::Src => self.filter_src.clear(),
            FilterMode::Dst => self.filter_dst.clear(),
            FilterMode::None => {
                self.filter_src.clear();
                self.filter_dst.clear();
            }
        }
        self.filter_mode = FilterMode::None;
        self.recompute_visible();
    }
}

<div align="center">

# 🔭 tcp-monitor

**Real-time TCP connection quality monitor for Linux**

[![CI](https://github.com/lihongjie0209/tcp-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/lihongjie0209/tcp-monitor/actions/workflows/ci.yml)
[![Release](https://github.com/lihongjie0209/tcp-monitor/actions/workflows/release.yml/badge.svg)](https://github.com/lihongjie0209/tcp-monitor/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux-lightgrey.svg)](#platform-requirements)

Passively monitors **all TCP connections on the system** by reading kernel metrics via Linux **Netlink INET_DIAG** — the same subsystem used by `ss -tip`. **No active probing. No server-side agent. No root required** (for own connections).

</div>

---

## ✨ Features

- 📡 **Passive kernel monitoring** — reads `tcp_info` structs directly from the kernel via Netlink INET_DIAG; zero network overhead
- 📊 **10 real-time columns** — Source, Destination, State, CA State, RTT, Jitter, Retransmits, Loss%, Delivery Rate, CWND
- 🔍 **Filter** by source (`F3`) or destination (`F4`) — live prefix matching
- 🔀 **Sort** by any column (`F6`), toggle ascending/descending (`R`)
- 📋 **Detail panel** — select any row to see full per-connection kernel metrics
- 🎨 **Color-coded** — RTT green/yellow/red, CA state anomalies in red, high retransmit in orange
- ⚡ **Lightweight** — single static binary, ~4 MB, no runtime dependencies
- 🐧 **Dual-mode** — interactive TUI or `--dump` for scripting/CI pipelines

---

## 📸 Preview

```
tcp-monitor │ Src: [        ] │ Dst: [:6379  ] │ 2/18 connections  interval:1s
┌──────────────────────┬──────────────────────┬──────┬───────┬────────┬────────┬──────────┬───────┬──────────┬──────┐
│ Source               │ Destination          │ State│ CA    │RTT(ms) │Jitter  │ Retrans  │ Loss% │Rate MB/s │ CWND │
├──────────────────────┼──────────────────────┼──────┼───────┼────────┼────────┼──────────┼───────┼──────────┼──────┤
│ 127.0.0.1:58338      │ 127.0.0.1:6379       │ ESTAB│ Open  │  0.171 │  0.113 │        0 │  0.00 │    16.84 │   10 │
│▶127.0.0.1:40322      │ 127.0.0.1:6379       │ ESTAB│ Open  │  0.325 │  0.149 │        0 │  0.00 │    10.27 │   10 │
└──────────────────────┴──────────────────────┴──────┴───────┴────────┴────────┴──────────┴───────┴──────────┴──────┘
┌ Detail: 127.0.0.1:40322 → 127.0.0.1:6379 ──────────────────────────────────────────────────────────────────────┐
│ RTT: 0.325ms  Jitter: 0.149ms  Min RTT: 0.100ms  RTO: 204.0ms                                                  │
│ CWND: 10  Retrans: 0  Total Retrans: 0  Retrans%: 0.000%                                                        │
│ Lost: 0  Loss%: 0.00%  Unacked: 0  CA State: Open                                                               │
│ Segs Out: 721  Segs In: 710  Delivery Rate: 10.27 MB/s                                                          │
│ Bytes Sent: 57.2 KB  Bytes Retrans: 0 B  Byte Retrans%: 0.000%                                                  │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 F3 SrcFilter  F4 DstFilter  F6 SortBy[Rate]  R Reverse  ESC Clear  q Quit
```

---

## 🚀 Installation

### Pre-built binaries (recommended)

Download the latest release from [GitHub Releases](https://github.com/lihongjie0209/tcp-monitor/releases):

```bash
# Linux x86_64 (static musl binary — works on any Linux distro)
curl -L https://github.com/lihongjie0209/tcp-monitor/releases/latest/download/tcp-monitor-x86_64-unknown-linux-musl.tar.gz \
  | tar xz
sudo mv tcp-monitor /usr/local/bin/

# Linux aarch64 (ARM64, e.g. Raspberry Pi 4, AWS Graviton)
curl -L https://github.com/lihongjie0209/tcp-monitor/releases/latest/download/tcp-monitor-aarch64-unknown-linux-musl.tar.gz \
  | tar xz
sudo mv tcp-monitor /usr/local/bin/
```

### Build from source

```bash
# Requires Rust 1.80+
cargo install --git https://github.com/lihongjie0209/tcp-monitor

# Or clone and build
git clone https://github.com/lihongjie0209/tcp-monitor
cd tcp-monitor
cargo build --release
sudo cp target/release/tcp-monitor /usr/local/bin/
```

### Docker (try instantly)

```bash
docker build -t tcp-monitor .
docker run -it --rm --network host --cap-add NET_ADMIN tcp-monitor ./target/debug/tcp-monitor
```

---

## 📖 Usage

```
tcp-monitor [OPTIONS]

Options:
  -i, --interval <INTERVAL>  Refresh interval (e.g. 1s, 500ms) [default: 1s]
      --dump                 Print one snapshot to stdout and exit (no TUI)
  -h, --help                 Print help
  -V, --version              Print version
```

### TUI mode (default)

```bash
# Monitor all connections (own connections, no root needed)
tcp-monitor

# See ALL system connections (requires root or CAP_NET_ADMIN)
sudo tcp-monitor

# Faster refresh
tcp-monitor -i 200ms
```

### Dump mode (scripting / CI)

```bash
# Print snapshot with all metrics
tcp-monitor --dump

# Filter and process with standard tools
tcp-monitor --dump | grep ESTAB | sort -k9 -rn | head -20
```

---

## ⌨️ Key Bindings

| Key | Action |
|-----|--------|
| `↑` / `↓` | Move selection (also updates detail panel) |
| `F3` | Edit **source** filter (type prefix to match) |
| `F4` | Edit **destination** filter |
| `F6` | Cycle sort column |
| `R` | Toggle sort direction (ascending/descending) |
| `ESC` | Stop editing filter / clear all filters |
| `q` / `Ctrl+C` | Quit |

---

## 📊 Metrics Reference

All metrics are read directly from the Linux kernel's `tcp_info` struct via Netlink INET_DIAG. No estimation, no active measurement.

| Metric | Kernel Field | Meaning |
|--------|-------------|---------|
| **RTT** | `tcpi_rtt` | Smoothed RTT (SRTT) in ms, as measured by the kernel TCP stack |
| **Jitter** | `tcpi_rttvar / 4` | RTT variance in ms; high value = unstable latency (like `mdev` in ping) |
| **Retrans** | `tcpi_total_retrans` | Cumulative retransmit count; delta `(+N)` shown for increments |
| **Loss%** | `tcpi_lost / (segs_out + lost)` | Kernel's current estimate of lost segments as a percentage |
| **Rate MB/s** | `tcpi_delivery_rate` | TCP sender delivery rate as measured by the kernel (kernel 4.9+) |
| **CWND** | `tcpi_snd_cwnd` | Congestion window in segments; drops signal congestion |
| **CA State** | `tcpi_ca_state` | Congestion control state: Open / Disorder / CWR / Recovery / **Loss** |
| **RTO** | `tcpi_rto` | Retransmission timeout in ms |
| **Min RTT** | `tcpi_min_rtt` | Minimum RTT observed over the connection lifetime (kernel 4.8+) |
| **Unacked** | `tcpi_unacked` | Packets sent but not yet acknowledged |
| **Segs Out/In** | `tcpi_segs_out/in` | Total TCP segments sent and received (kernel 4.2+) |
| **Retrans%** | `total_retrans / segs_out` | Retransmission rate as percentage of segments sent |
| **Byte Retrans%** | `bytes_retrans / bytes_sent` | Byte-level retransmission rate (kernel 5.1+) |

### CA State legend

| State | Meaning |
|-------|---------|
| `Open` | Normal operation (no congestion) |
| `Disorder` | SACK or duplicate ACKs received |
| `CWR` | Congestion window reduced (ECN / ICMP source quench) |
| `Recovery` | Fast retransmit/recovery in progress ⚠️ |
| `Loss` | Timeout-based loss recovery 🚨 |

States **Recovery** and **Loss** are highlighted in red in the TUI.

---

## 🐧 Platform Requirements

| Requirement | Details |
|-------------|---------|
| **OS** | Linux only (Netlink INET_DIAG is Linux-specific) |
| **Kernel** | 3.3+ (INET_DIAG base); 4.2+ for segs; 4.8+ for min_rtt; 4.9+ for delivery_rate; 5.1+ for byte retransmit stats |
| **Permissions** | No root needed for your own connections. `CAP_NET_ADMIN` or root required to see all system connections |
| **Architecture** | x86_64, aarch64 (pre-built); any Linux arch via `cargo build` |

---

## 🔬 How It Works

```
tcp-monitor
    │
    ├── Netlink socket (AF_NETLINK / SOCK_DGRAM)
    │       │
    │       └── SOCK_DIAG_BY_FAMILY request (INET_DIAG)
    │               │
    │               └── Kernel responds with InetDiagMsg + NLA attributes
    │                       │
    │                       └── INET_DIAG_INFO NLA → tcp_info struct (224 bytes)
    │
    ├── Parse all active TCP connections (IPv4 + IPv6)
    ├── Track deltas (retransmit increments, jitter smoothing)
    └── Render with Ratatui TUI (refresh every interval)
```

This is exactly what `ss -tip` does internally. The key advantage over active probing (e.g., ping or custom TCP prober) is that these are the **actual kernel-measured metrics for real application connections** — not synthetic test traffic.

---

## 🏗️ Building

```bash
# Debug build
cargo build

# Release build (optimized, ~4 MB)
cargo build --release

# Run tests
cargo test -- --nocapture

# Static musl binary (no glibc dependency)
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

---

## 📦 Comparison

| Tool | Method | Metrics | TUI | Root needed |
|------|--------|---------|-----|-------------|
| **tcp-monitor** | Kernel INET_DIAG (passive) | RTT, Jitter, Loss%, Rate, CA state, RTO, CWND, ... | ✅ htop-style | ❌ (own conns) |
| `ss -tip` | Kernel INET_DIAG | All `tcp_info` fields | ❌ text output | ❌ |
| `ping` | ICMP probing (active) | RTT, loss | ❌ | ❌ |
| `iperf3` | Active TCP flooding | Throughput, jitter | ❌ | ❌ |
| `nethogs` | /proc/net (passive) | Bandwidth per process | ✅ | ✅ |
| Wireshark | Packet capture (passive) | Everything | ✅ GUI | ✅ |

---

## 🤝 Contributing

Issues and pull requests are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes
4. Open a Pull Request

For major changes, please open an issue first to discuss.

---

## 📄 License

[MIT](LICENSE) © 2025 lihongjie0209


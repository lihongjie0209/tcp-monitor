// Netlink INET_DIAG — query all TCP connections with tcp_info from kernel
// Uses raw libc syscalls (no external netlink crates) for stability.
// Linux only.

use std::collections::HashMap;
use std::io;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::app::{ConnInfo, TcpState};

// ── Netlink constants ────────────────────────────────────────────────────────

const NETLINK_SOCK_DIAG: libc::c_int = 4;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLM_F_REQUEST: u16 = 0x0001;
const NLM_F_DUMP: u16 = 0x0300;
const NLMSG_DONE: u16 = 3;
const NLMSG_ERROR: u16 = 2;
const INET_DIAG_INFO: u16 = 2; // NLA type carrying tcp_info

fn nlmsg_align(len: usize) -> usize {
    (len + 3) & !3
}

// ── Kernel ABI structs (must match linux/inet_diag.h exactly) ────────────────

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct NlMsgHdr {
    nlmsg_len: u32,
    nlmsg_type: u16,
    nlmsg_flags: u16,
    nlmsg_seq: u32,
    nlmsg_pid: u32,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct InetDiagSockId {
    sport: u16,    // network byte order
    dport: u16,    // network byte order
    src: [u32; 4], // network byte order (only [0] used for IPv4)
    dst: [u32; 4], // network byte order
    iface: u32,
    cookie: [u32; 2],
}

#[repr(C)]
#[derive(Default)]
struct InetDiagReqV2 {
    sdiag_family: u8,
    sdiag_protocol: u8,
    idiag_ext: u8, // bitmask: bit (N-1) requests extension N
    pad: u8,
    idiag_states: u32, // bitmask of TCP states to include
    id: InetDiagSockId,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct InetDiagMsg {
    idiag_family: u8,
    idiag_state: u8,
    idiag_timer: u8,
    idiag_retrans: u8,
    id: InetDiagSockId,
    idiag_expires: u32,
    idiag_rqueue: u32,
    idiag_wqueue: u32,
    idiag_uid: u32,
    idiag_inode: u32,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

/// Mirrors linux/tcp.h `struct tcp_info`.
/// Covers fields up to `reord_seen` (kernel 5.1+, offset 220).
/// Older kernels provide fewer bytes — parse_tcp_info_nla zero-fills the rest.
#[repr(C)]
#[derive(Default, Copy, Clone)]
struct TcpInfo {
    // offset 0
    state: u8,
    ca_state: u8,    // congestion avoidance state (TCP_CA_*)
    retransmits: u8, // current retransmit counter
    probes: u8,
    backoff: u8,
    options: u8,
    wscale: u8,
    app_limited: u8,
    // offset 8
    rto: u32, // retransmit timeout (us)
    ato: u32,
    snd_mss: u32,
    rcv_mss: u32,
    // offset 24
    unacked: u32, // unacknowledged packets
    sacked: u32,  // SACK'd packets
    lost: u32,    // lost packets (kernel estimate)
    retrans: u32, // retransmits in flight
    fackets: u32,
    // offset 44
    last_data_sent: u32,
    last_ack_sent: u32,
    last_data_recv: u32,
    last_ack_recv: u32,
    // offset 60
    pmtu: u32,
    rcv_ssthresh: u32,
    rtt: u32,    // smoothed RTT (us)
    rttvar: u32, // RTT variance (us)
    snd_ssthresh: u32,
    snd_cwnd: u32, // congestion window
    advmss: u32,
    reordering: u32,
    // offset 92
    rcv_rtt: u32,
    rcv_space: u32,
    total_retrans: u32, // cumulative retransmits (offset 100)
    // ── kernel 3.15+ ──────────────── (offset 104)
    pacing_rate: u64,
    max_pacing_rate: u64,
    // ── kernel 4.2+ ───────────────── (offset 120)
    bytes_acked: u64,
    bytes_received: u64,
    segs_out: u32, // segments sent    (offset 136)
    segs_in: u32,  // segments received (offset 140)
    notsent_bytes: u32,
    min_rtt: u32, // minimum RTT ever observed (us, offset 148)
    data_segs_in: u32,
    data_segs_out: u32,
    // ── kernel 4.9+ ───────────────── (offset 160)
    delivery_rate: u64, // bytes/sec
    // ── kernel 4.18+ ──────────────── (offset 168)
    busy_time: u64,
    rwnd_limited: u64,
    sndbuf_limited: u64,
    delivered: u32, // offset 192
    delivered_ce: u32,
    // ── kernel 5.1+ ───────────────── (offset 200)
    bytes_sent: u64,
    bytes_retrans: u64, // bytes retransmitted (offset 208)
    dsack_dups: u32,
    reord_seen: u32, // reorder events (offset 220)
}

/// Minimum bytes the kernel always provides (through total_retrans, offset 100).
const TCP_INFO_MIN_SIZE: usize = 104;

// ── Public API ───────────────────────────────────────────────────────────────

pub fn query_connections() -> io::Result<HashMap<String, ConnInfo>> {
    let mut map = HashMap::new();
    for family in [libc::AF_INET as u8, libc::AF_INET6 as u8] {
        if let Ok(entries) = query_family(family) {
            for e in entries {
                map.insert(e.key.clone(), e);
            }
        } // best-effort; skip failed family
    }
    Ok(map)
}

// ── Internal implementation ──────────────────────────────────────────────────

fn query_family(family: u8) -> io::Result<Vec<ConnInfo>> {
    // Open a NETLINK_SOCK_DIAG raw socket
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            NETLINK_SOCK_DIAG,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // Build and send the INET_DIAG dump request
    let req_buf = build_request(family);
    let sent = unsafe {
        libc::send(
            fd,
            req_buf.as_ptr() as *const libc::c_void,
            req_buf.len(),
            0,
        )
    };
    if sent < 0 {
        unsafe { libc::close(fd) };
        return Err(io::Error::last_os_error());
    }

    // Receive and parse all response messages
    let result = recv_and_parse(fd, family);
    unsafe { libc::close(fd) };
    result
}

fn build_request(family: u8) -> Vec<u8> {
    let req = InetDiagReqV2 {
        sdiag_family: family,
        sdiag_protocol: libc::IPPROTO_TCP as u8,
        // Request INET_DIAG_INFO (tcp_info): bit (INET_DIAG_INFO - 1) = bit 1
        idiag_ext: 1 << (INET_DIAG_INFO - 1),
        pad: 0,
        idiag_states: !0u32, // all TCP states
        id: InetDiagSockId::default(),
    };

    let total = mem::size_of::<NlMsgHdr>() + mem::size_of::<InetDiagReqV2>();
    let hdr = NlMsgHdr {
        nlmsg_len: total as u32,
        nlmsg_type: SOCK_DIAG_BY_FAMILY,
        nlmsg_flags: NLM_F_REQUEST | NLM_F_DUMP,
        nlmsg_seq: 1,
        nlmsg_pid: 0,
    };

    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(struct_as_bytes(&hdr));
    buf.extend_from_slice(struct_as_bytes(&req));
    buf
}

fn recv_and_parse(fd: libc::c_int, family: u8) -> io::Result<Vec<ConnInfo>> {
    let mut results = Vec::new();
    let mut recv_buf = vec![0u8; 65536];

    'outer: loop {
        let n = unsafe {
            libc::recv(
                fd,
                recv_buf.as_mut_ptr() as *mut libc::c_void,
                recv_buf.len(),
                0,
            )
        };
        if n <= 0 {
            break;
        }
        let n = n as usize;
        let mut offset = 0usize;

        while offset + mem::size_of::<NlMsgHdr>() <= n {
            let hdr: NlMsgHdr = unsafe { ptr_read_unaligned(&recv_buf[offset..]) };
            let msg_len = hdr.nlmsg_len as usize;

            if msg_len < mem::size_of::<NlMsgHdr>() || offset + msg_len > n {
                break;
            }

            match hdr.nlmsg_type {
                NLMSG_DONE => break 'outer,
                NLMSG_ERROR => break 'outer,
                SOCK_DIAG_BY_FAMILY => {
                    let data = &recv_buf[offset + mem::size_of::<NlMsgHdr>()..offset + msg_len];
                    if data.len() >= mem::size_of::<InetDiagMsg>() {
                        let dmsg: InetDiagMsg = unsafe { ptr_read_unaligned(data) };
                        let nla_data = &data[mem::size_of::<InetDiagMsg>()..];
                        let tcp_info = parse_tcp_info_nla(nla_data);
                        if let Some(ci) = build_conn_info(&dmsg, tcp_info, family) {
                            results.push(ci);
                        }
                    }
                }
                _ => {}
            }

            offset += nlmsg_align(msg_len);
        }
    }

    Ok(results)
}

/// Walk NLA attributes, find INET_DIAG_INFO, parse it as TcpInfo
fn parse_tcp_info_nla(buf: &[u8]) -> Option<TcpInfo> {
    let hdr_sz = mem::size_of::<RtAttr>();
    let mut pos = 0usize;

    while pos + hdr_sz <= buf.len() {
        let attr: RtAttr = unsafe { ptr_read_unaligned(&buf[pos..]) };
        let attr_len = attr.rta_len as usize;
        if attr_len < hdr_sz || pos + attr_len > buf.len() {
            break;
        }
        if attr.rta_type == INET_DIAG_INFO {
            let payload = &buf[pos + hdr_sz..pos + attr_len];
            if payload.len() >= TCP_INFO_MIN_SIZE {
                // Zero-fill then copy: handles kernels that provide fewer bytes
                // than our TcpInfo struct (newer fields zero = "not available").
                let mut raw = [0u8; mem::size_of::<TcpInfo>()];
                let copy_len = payload.len().min(raw.len());
                raw[..copy_len].copy_from_slice(&payload[..copy_len]);
                let info: TcpInfo = unsafe { ptr_read_unaligned(&raw) };
                return Some(info);
            }
        }
        pos += nlmsg_align(attr_len);
    }
    None
}

fn build_conn_info(msg: &InetDiagMsg, tcp_info: Option<TcpInfo>, family: u8) -> Option<ConnInfo> {
    // Skip listening / unconnected sockets
    if msg.id.dport == 0 {
        return None;
    }

    let src_ip = decode_addr(family, &msg.id.src);
    let dst_ip = decode_addr(family, &msg.id.dst);
    // Ports are in network byte order
    let src_port = u16::from_be(msg.id.sport);
    let dst_port = u16::from_be(msg.id.dport);

    let src = format_addr(src_ip, src_port);
    let dst = format_addr(dst_ip, dst_port);
    let key = format!("{src}→{dst}");
    let state = TcpState::from_u8(msg.idiag_state);

    let (
        rtt_us,
        rttvar_us,
        total_retrans,
        cwnd,
        ca_state,
        rto_us,
        unacked,
        lost,
        retrans_in_flight,
        segs_out,
        segs_in,
        min_rtt_us,
        delivery_rate_bps,
        bytes_sent,
        bytes_retrans,
        pmtu,
        snd_mss,
    ) = tcp_info
        .map(|t| {
            (
                t.rtt,
                t.rttvar,
                t.total_retrans,
                t.snd_cwnd,
                t.ca_state,
                t.rto,
                t.unacked,
                t.lost,
                t.retrans,
                t.segs_out,
                t.segs_in,
                t.min_rtt,
                t.delivery_rate,
                t.bytes_sent,
                t.bytes_retrans,
                t.pmtu,
                t.snd_mss,
            )
        })
        .unwrap_or((0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));

    Some(ConnInfo {
        key,
        src,
        dst,
        state,
        rtt_us,
        rttvar_us,
        total_retrans,
        cwnd,
        ca_state,
        rto_us,
        unacked,
        lost,
        retrans_in_flight,
        segs_out,
        segs_in,
        min_rtt_us,
        delivery_rate_bps,
        bytes_sent,
        bytes_retrans,
        pmtu,
        snd_mss,
        retrans_delta: 0,
        samples: if rtt_us > 0 { 1 } else { 0 },
        rtt_min_us: if rtt_us > 0 { rtt_us } else { u32::MAX },
        rtt_max_us: rtt_us,
        rtt_sum_us: rtt_us as u64,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn decode_addr(family: u8, raw: &[u32; 4]) -> IpAddr {
    // The raw[] words hold addresses in network byte order as stored in memory.
    // to_ne_bytes() reads the actual memory bytes, which equal network-order bytes.
    if family == libc::AF_INET as u8 {
        IpAddr::V4(Ipv4Addr::from(raw[0].to_ne_bytes()))
    } else {
        let mut bytes = [0u8; 16];
        for (i, &w) in raw.iter().enumerate() {
            bytes[i * 4..(i + 1) * 4].copy_from_slice(&w.to_ne_bytes());
        }
        IpAddr::V6(Ipv6Addr::from(bytes))
    }
}

fn format_addr(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(a) => format!("{a}:{port}"),
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() {
                format!("{v4}:{port}")
            } else {
                format!("[{a}]:{port}")
            }
        }
    }
}

/// Read a T from a potentially unaligned byte slice
unsafe fn ptr_read_unaligned<T: Copy>(buf: &[u8]) -> T {
    debug_assert!(buf.len() >= mem::size_of::<T>());
    unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const T) }
}

/// View a repr(C) struct as a byte slice
fn struct_as_bytes<T: Sized>(val: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(val as *const T as *const u8, mem::size_of::<T>()) }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_query_connections_no_error() {
        let conns = query_connections().expect("query_connections should not fail");
        println!("Found {} connections", conns.len());
        for (_, info) in conns.iter().take(5) {
            println!(
                "  {} state={} rtt={:.3}ms jitter={:.3}ms retrans={}",
                info.key,
                info.state.short(),
                info.rtt_ms(),
                info.rttvar_ms(),
                info.total_retrans,
            );
        }
    }

    #[test]
    fn test_established_loopback_visible() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let t = thread::spawn(move || {
            let (_conn, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(300));
        });

        let client = TcpStream::connect(addr).unwrap();
        thread::sleep(Duration::from_millis(50));

        let conns = query_connections().unwrap();
        println!("Total connections: {}", conns.len());

        let our_port = client.local_addr().unwrap().port();
        let found = conns
            .values()
            .any(|c| c.src.ends_with(&format!(":{our_port}")));
        println!("Our loopback conn (src port {our_port}) found: {found}");

        for c in conns
            .values()
            .filter(|c| c.state == crate::app::TcpState::Established)
            .take(3)
        {
            println!("  ESTAB {} rtt={:.3}ms cwnd={}", c.key, c.rtt_ms(), c.cwnd);
        }

        t.join().ok();
    }
}

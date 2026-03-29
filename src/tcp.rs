// tcp.rs -- TCP client protocol (RFC 793, minimal subset).
//
// Client-only: connect, send, recv, close.  No listen/accept.
// No slow start, no congestion control, no SACK, no timestamps.
// Window size is hardcoded to 4096.
// Retransmission: simple 2-second timeout on SYN, 3 retries max.
// Data retransmission not implemented (rely on QEMU's reliable vnet).
//
// TCP checksum uses the standard pseudo-header (src_ip, dst_ip,
// zero, protocol=6, tcp_length) followed by the TCP segment.

use crate::ipv4;
use crate::net::{htonl, htons, ntohl, ntohs, IP_PROTO_TCP, NET_IP_ADDR};
use crate::string;
use crate::timer;
use core::arch::asm;

// TCP flag bits
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;

// TCP states (client-only subset)
const TCP_CLOSED: i32 = 0;
const TCP_SYN_SENT: i32 = 1;
const TCP_ESTABLISHED: i32 = 2;
const TCP_FIN_WAIT_1: i32 = 3;
const TCP_FIN_WAIT_2: i32 = 4;
const TCP_TIME_WAIT: i32 = 5;

const TCP_MAX_CONNS: usize = 4;
const TCP_RX_BUF_SIZE: usize = 4096;
const TCP_WINDOW: u16 = 4096;

/// Pseudo-header for TCP checksum computation.
#[repr(C, packed)]
struct TcpPseudo {
    src_ip: u32,    // network byte order
    dst_ip: u32,    // network byte order
    zero: u8,
    protocol: u8,   // 6
    tcp_len: u16,   // network byte order
}

#[repr(C, packed)]
struct TcpHeader {
    src_port: u16,
    dst_port: u16,
    seq_num: u32,
    ack_num: u32,
    data_offset: u8, // upper 4 bits = header length in 32-bit words
    flags: u8,
    window: u16,
    checksum: u16,
    urgent_ptr: u16,
}

struct TcpConn {
    in_use: bool,
    state: i32,
    remote_ip: u32,
    remote_port: u16,
    local_port: u16,
    send_seq: u32,     // next sequence number to send
    send_ack: u32,     // last ack we sent (= next expected from remote)

    // Receive buffer (circular)
    rx_buf: [u8; TCP_RX_BUF_SIZE],
    rx_head: u16, // write index (set by tcp_rx in IRQ context)
    rx_tail: u16, // read index (set by tcp_recv in user context)

    // Flags
    fin_received: bool,
    connected: bool,
}

impl TcpConn {
    const fn new() -> Self {
        TcpConn {
            in_use: false,
            state: TCP_CLOSED,
            remote_ip: 0,
            remote_port: 0,
            local_port: 0,
            send_seq: 0,
            send_ack: 0,
            rx_buf: [0; TCP_RX_BUF_SIZE],
            rx_head: 0,
            rx_tail: 0,
            fin_received: false,
            connected: false,
        }
    }
}

static mut CONNS: [TcpConn; TCP_MAX_CONNS] = [
    TcpConn::new(),
    TcpConn::new(),
    TcpConn::new(),
    TcpConn::new(),
];

static mut NEXT_LOCAL_PORT: u16 = 49152;

/// Compute the TCP checksum over pseudo-header + TCP header + data.
unsafe fn tcp_checksum(src_ip: u32, dst_ip: u32, tcp_seg: *const u8, tcp_len: u16) -> u16 {
    static mut CSUM_BUF: [u8; 12 + 1480] = [0; 12 + 1480]; // pseudo-header + max TCP segment

    if tcp_len > 1480 {
        return 0;
    }

    let ph = CSUM_BUF.as_mut_ptr() as *mut TcpPseudo;
    core::ptr::write_unaligned(&raw mut (*ph).src_ip, htonl(src_ip));
    core::ptr::write_unaligned(&raw mut (*ph).dst_ip, htonl(dst_ip));
    (*ph).zero = 0;
    (*ph).protocol = 6;
    core::ptr::write_unaligned(&raw mut (*ph).tcp_len, htons(tcp_len));

    string::memcpy(CSUM_BUF.as_mut_ptr().add(12), tcp_seg, tcp_len as usize);

    ipv4::checksum(CSUM_BUF.as_ptr(), 12 + tcp_len)
}

/// Send a TCP segment. Builds the header, computes checksum, and
/// hands off to ipv4::send.
unsafe fn tcp_send_segment(c: &mut TcpConn, flags: u8, data: *const u8, data_len: u16) {
    static mut SEG_BUF: [u8; 1480] = [0; 1480];
    let tcp_len = 20u16 + data_len;

    if tcp_len > 1480 {
        return;
    }

    let hdr = SEG_BUF.as_mut_ptr() as *mut TcpHeader;
    string::memset(SEG_BUF.as_mut_ptr(), 0, 20);
    core::ptr::write_unaligned(&raw mut (*hdr).src_port, htons(c.local_port));
    core::ptr::write_unaligned(&raw mut (*hdr).dst_port, htons(c.remote_port));
    core::ptr::write_unaligned(&raw mut (*hdr).seq_num, htonl(c.send_seq));
    core::ptr::write_unaligned(&raw mut (*hdr).ack_num, htonl(c.send_ack));
    (*hdr).data_offset = 0x50; // 5 dwords = 20 bytes, no options
    (*hdr).flags = flags;
    core::ptr::write_unaligned(&raw mut (*hdr).window, htons(TCP_WINDOW));
    core::ptr::write_unaligned(&raw mut (*hdr).checksum, 0);
    core::ptr::write_unaligned(&raw mut (*hdr).urgent_ptr, 0);

    if data_len > 0 && !data.is_null() {
        string::memcpy(SEG_BUF.as_mut_ptr().add(20), data, data_len as usize);
    }

    core::ptr::write_unaligned(
        &raw mut (*hdr).checksum,
        tcp_checksum(NET_IP_ADDR, c.remote_ip, SEG_BUF.as_ptr(), tcp_len),
    );

    ipv4::send(c.remote_ip, IP_PROTO_TCP, SEG_BUF.as_ptr(), tcp_len);
}

/// Allocate a free connection slot. Returns index or -1.
unsafe fn tcp_alloc_conn() -> i32 {
    for i in 0..TCP_MAX_CONNS {
        if !CONNS[i].in_use {
            return i as i32;
        }
    }
    -1
}

/// Find a connection matching the given remote IP and port pair.
/// Returns index or -1.
unsafe fn tcp_find_conn(remote_ip: u32, remote_port: u16, local_port: u16) -> i32 {
    for i in 0..TCP_MAX_CONNS {
        if CONNS[i].in_use
            && CONNS[i].remote_ip == remote_ip
            && CONNS[i].remote_port == remote_port
            && CONNS[i].local_port == local_port
        {
            return i as i32;
        }
    }
    -1
}

/// Initialize the TCP subsystem.
pub fn init() {
    unsafe {
        for i in 0..TCP_MAX_CONNS {
            CONNS[i] = TcpConn::new();
        }
    }
}

/// Open a TCP connection (blocking -- performs the 3-way handshake).
/// Returns connection id on success, -1 on failure.
pub fn connect(remote_ip: u32, remote_port: u16) -> i32 {
    unsafe {
        let idx = tcp_alloc_conn();
        if idx < 0 {
            return -1;
        }
        let i = idx as usize;

        CONNS[i] = TcpConn::new();
        CONNS[i].in_use = true;
        CONNS[i].state = TCP_SYN_SENT;
        CONNS[i].remote_ip = remote_ip;
        CONNS[i].remote_port = remote_port;
        CONNS[i].local_port = NEXT_LOCAL_PORT;
        NEXT_LOCAL_PORT = NEXT_LOCAL_PORT.wrapping_add(1);
        if NEXT_LOCAL_PORT == 0 {
            NEXT_LOCAL_PORT = 49152;
        }
        CONNS[i].send_seq = timer::get_ticks().wrapping_mul(12345).wrapping_add(1);
        CONNS[i].send_ack = 0;
        CONNS[i].rx_head = 0;
        CONNS[i].rx_tail = 0;
        CONNS[i].fin_received = false;
        CONNS[i].connected = false;

        // Send SYN with retransmission (up to 3 attempts, 2 seconds each)
        for _attempt in 0..3 {
            tcp_send_segment(&mut CONNS[i], TCP_SYN, core::ptr::null(), 0);
            let start = timer::get_ticks();

            // Wait up to 2 seconds (200 ticks at 100 Hz)
            while timer::get_ticks().wrapping_sub(start) < 200 {
                if core::ptr::read_volatile(&raw const CONNS[i].connected) {
                    return idx;
                }
                asm!("hlt");
            }
        }

        // Timeout -- clean up
        CONNS[i].in_use = false;
        CONNS[i].state = TCP_CLOSED;
        -1
    }
}

/// Send data on an established connection.
/// Returns 0 on success, -1 on error.
pub fn send(conn_id: i32, data: *const u8, length: u16) -> i32 {
    unsafe {
        if conn_id < 0 || conn_id as usize >= TCP_MAX_CONNS {
            return -1;
        }

        let i = conn_id as usize;
        if !CONNS[i].in_use || CONNS[i].state != TCP_ESTABLISHED {
            return -1;
        }

        tcp_send_segment(&mut CONNS[i], TCP_ACK | TCP_PSH, data, length);
        CONNS[i].send_seq = CONNS[i].send_seq.wrapping_add(length as u32);
        0
    }
}

/// Receive data (blocking with timeout).
/// Returns number of bytes received, 0 on timeout or connection closed.
pub fn recv(conn_id: i32, buf: *mut u8, max_len: u16, timeout_ticks: u32) -> i32 {
    unsafe {
        if conn_id < 0 || conn_id as usize >= TCP_MAX_CONNS {
            return 0;
        }

        let i = conn_id as usize;
        if !CONNS[i].in_use {
            return 0;
        }

        let start = timer::get_ticks();

        // Block until data is available, FIN received, or timeout
        while core::ptr::read_volatile(&raw const CONNS[i].rx_head)
            == core::ptr::read_volatile(&raw const CONNS[i].rx_tail)
            && !core::ptr::read_volatile(&raw const CONNS[i].fin_received)
        {
            if timer::get_ticks().wrapping_sub(start) >= timeout_ticks {
                return 0;
            }
            asm!("hlt");
        }

        // Copy data from the circular buffer
        let head = core::ptr::read_volatile(&raw const CONNS[i].rx_head);
        let tail = core::ptr::read_volatile(&raw const CONNS[i].rx_tail);

        if head == tail {
            return 0; // FIN with no data
        }

        let avail = if head > tail {
            head - tail
        } else {
            TCP_RX_BUF_SIZE as u16 - tail + head
        };

        let mut count = avail;
        if count > max_len {
            count = max_len;
        }

        for j in 0..count {
            *buf.add(j as usize) =
                CONNS[i].rx_buf[((tail + j) % TCP_RX_BUF_SIZE as u16) as usize];
        }
        core::ptr::write_volatile(
            &raw mut CONNS[i].rx_tail,
            (tail + count) % TCP_RX_BUF_SIZE as u16,
        );

        count as i32
    }
}

/// Close a TCP connection (sends FIN+ACK).
pub fn close(conn_id: i32) {
    unsafe {
        if conn_id < 0 || conn_id as usize >= TCP_MAX_CONNS {
            return;
        }

        let i = conn_id as usize;
        if !CONNS[i].in_use {
            return;
        }

        if CONNS[i].state == TCP_ESTABLISHED || CONNS[i].state == TCP_SYN_SENT {
            tcp_send_segment(&mut CONNS[i], TCP_FIN | TCP_ACK, core::ptr::null(), 0);
            CONNS[i].send_seq = CONNS[i].send_seq.wrapping_add(1);
            CONNS[i].state = TCP_FIN_WAIT_1;

            // Wait briefly (up to 2 seconds) for peer's ACK/FIN
            let start = timer::get_ticks();
            while CONNS[i].state != TCP_CLOSED && CONNS[i].state != TCP_TIME_WAIT {
                if timer::get_ticks().wrapping_sub(start) >= 200 {
                    break;
                }
                asm!("hlt");
            }
        }

        // If we got to TIME_WAIT, wait a short time then close
        if CONNS[i].state == TCP_TIME_WAIT {
            let start = timer::get_ticks();
            while timer::get_ticks().wrapping_sub(start) < 50 {
                asm!("hlt");
            }
        }

        CONNS[i].in_use = false;
        CONNS[i].state = TCP_CLOSED;
    }
}

/// Handle an incoming TCP segment.
/// Called from ipv4::rx (IRQ context).
pub fn rx(src_ip: u32, data: *const u8, length: u16) {
    unsafe {
        if length < 20 {
            return;
        }

        // Verify TCP checksum
        if tcp_checksum(src_ip, NET_IP_ADDR, data, length) != 0 {
            return;
        }

        let hdr = data as *const TcpHeader;
        let src_port = ntohs(core::ptr::read_unaligned(&raw const (*hdr).src_port));
        let dst_port = ntohs(core::ptr::read_unaligned(&raw const (*hdr).dst_port));
        let flags = (*hdr).flags;
        let seq = ntohl(core::ptr::read_unaligned(&raw const (*hdr).seq_num));
        let ack = ntohl(core::ptr::read_unaligned(&raw const (*hdr).ack_num));
        let hdr_len = (((*hdr).data_offset >> 4) * 4) as u16;

        if hdr_len < 20 || hdr_len > length {
            return;
        }

        let payload = data.add(hdr_len as usize);
        let payload_len = length - hdr_len;

        // Find matching connection
        let idx = tcp_find_conn(src_ip, src_port, dst_port);
        if idx < 0 {
            // No connection -- send RST if not already RST
            if (flags & TCP_RST) == 0 {
                let mut tmp = TcpConn::new();
                tmp.remote_ip = src_ip;
                tmp.remote_port = src_port;
                tmp.local_port = dst_port;
                if (flags & TCP_ACK) != 0 {
                    tmp.send_seq = ack;
                    tmp.send_ack = 0;
                    tcp_send_segment(&mut tmp, TCP_RST, core::ptr::null(), 0);
                } else {
                    tmp.send_seq = 0;
                    tmp.send_ack = seq + payload_len as u32;
                    if (flags & TCP_SYN) != 0 {
                        tmp.send_ack = tmp.send_ack.wrapping_add(1);
                    }
                    if (flags & TCP_FIN) != 0 {
                        tmp.send_ack = tmp.send_ack.wrapping_add(1);
                    }
                    tcp_send_segment(&mut tmp, TCP_RST | TCP_ACK, core::ptr::null(), 0);
                }
            }
            return;
        }

        let i = idx as usize;

        // RST in any state: kill connection
        if (flags & TCP_RST) != 0 {
            CONNS[i].state = TCP_CLOSED;
            CONNS[i].in_use = false;
            core::ptr::write_volatile(&raw mut CONNS[i].connected, false);
            core::ptr::write_volatile(&raw mut CONNS[i].fin_received, true);
            return;
        }

        match CONNS[i].state {
            TCP_SYN_SENT => {
                // Expecting SYN+ACK
                if (flags & (TCP_SYN | TCP_ACK)) == (TCP_SYN | TCP_ACK) {
                    if ack != CONNS[i].send_seq.wrapping_add(1) {
                        return; // wrong ack number
                    }
                    CONNS[i].send_seq = ack;
                    CONNS[i].send_ack = seq + 1;
                    tcp_send_segment(&mut CONNS[i], TCP_ACK, core::ptr::null(), 0);
                    CONNS[i].state = TCP_ESTABLISHED;
                    core::ptr::write_volatile(&raw mut CONNS[i].connected, true);
                }
            }

            TCP_ESTABLISHED => {
                // Data and/or FIN
                if payload_len > 0 {
                    // Copy data into circular receive buffer
                    for j in 0..payload_len {
                        let next = (CONNS[i].rx_head + 1) % TCP_RX_BUF_SIZE as u16;
                        if next == CONNS[i].rx_tail {
                            break; // buffer full -- drop remaining
                        }
                        CONNS[i].rx_buf[CONNS[i].rx_head as usize] = *payload.add(j as usize);
                        core::ptr::write_volatile(&raw mut CONNS[i].rx_head, next);
                    }
                    CONNS[i].send_ack = seq.wrapping_add(payload_len as u32);
                    tcp_send_segment(&mut CONNS[i], TCP_ACK, core::ptr::null(), 0);
                }
                if (flags & TCP_FIN) != 0 {
                    CONNS[i].send_ack = seq.wrapping_add(payload_len as u32).wrapping_add(1);
                    tcp_send_segment(&mut CONNS[i], TCP_ACK, core::ptr::null(), 0);
                    core::ptr::write_volatile(&raw mut CONNS[i].fin_received, true);
                    CONNS[i].state = TCP_TIME_WAIT;
                }
            }

            TCP_FIN_WAIT_1 => {
                if (flags & TCP_ACK) != 0 {
                    if (flags & TCP_FIN) != 0 {
                        // Simultaneous close: FIN+ACK from peer
                        CONNS[i].send_ack = seq + 1;
                        tcp_send_segment(&mut CONNS[i], TCP_ACK, core::ptr::null(), 0);
                        CONNS[i].state = TCP_TIME_WAIT;
                    } else {
                        // Just ACK for our FIN
                        CONNS[i].state = TCP_FIN_WAIT_2;
                    }
                }
            }

            TCP_FIN_WAIT_2 => {
                if (flags & TCP_FIN) != 0 {
                    CONNS[i].send_ack = seq + 1;
                    tcp_send_segment(&mut CONNS[i], TCP_ACK, core::ptr::null(), 0);
                    CONNS[i].state = TCP_TIME_WAIT;
                    core::ptr::write_volatile(&raw mut CONNS[i].fin_received, true);
                }
            }

            TCP_TIME_WAIT => {
                // Already closing, ignore
            }

            _ => {}
        }
    }
}

// C-compatible aliases used by the built-in C compiler (cc/sym.rs).
// These are exported as function pointers for compiled C programs.

/// C-ABI wrapper for tcp::connect.
#[unsafe(no_mangle)]
pub extern "C" fn tcp_connect(remote_ip: u32, remote_port: u16) -> i32 {
    connect(remote_ip, remote_port)
}

/// C-ABI wrapper for tcp::send.
#[unsafe(no_mangle)]
pub extern "C" fn tcp_send(conn_id: i32, data: *const u8, length: u16) -> i32 {
    send(conn_id, data, length)
}

/// C-ABI wrapper for tcp::recv.
#[unsafe(no_mangle)]
pub extern "C" fn tcp_recv(conn_id: i32, buf: *mut u8, max_len: u16, timeout_ticks: u32) -> i32 {
    recv(conn_id, buf, max_len, timeout_ticks)
}

/// C-ABI wrapper for tcp::close.
#[unsafe(no_mangle)]
pub extern "C" fn tcp_close(conn_id: i32) {
    close(conn_id)
}

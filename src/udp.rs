// udp.rs -- UDP protocol (RFC 768).
//
// Minimal implementation: no checksum verification, simple port binding table.
// UDP checksum is optional in IPv4, so we set it to 0 on transmit.

use crate::ipv4;
use crate::net::{htons, ntohs, IP_PROTO_UDP};
use crate::string;

#[repr(C, packed)]
struct UdpHeader {
    src_port: u16,
    dst_port: u16,
    length: u16,    // header + data
    checksum: u16,  // optional in IPv4, can be 0
}

const UDP_MAX_BINDS: usize = 16;

/// UDP handler callback type.
pub type UdpHandler = fn(src_ip: u32, src_port: u16, data: *const u8, length: u16);

struct UdpBindEntry {
    port: u16,
    handler: Option<UdpHandler>,
}

static mut BIND_TABLE: [UdpBindEntry; UDP_MAX_BINDS] = {
    const EMPTY: UdpBindEntry = UdpBindEntry {
        port: 0,
        handler: None,
    };
    [EMPTY; UDP_MAX_BINDS]
};

static mut BIND_COUNT: usize = 0;

/// Register a handler for a given UDP port.
/// Returns 1 on success, 0 if the table is full.
pub fn bind(port: u16, handler: UdpHandler) -> i32 {
    unsafe {
        // Check if already bound -- update handler
        for i in 0..BIND_COUNT {
            if BIND_TABLE[i].port == port {
                BIND_TABLE[i].handler = Some(handler);
                return 1;
            }
        }

        if BIND_COUNT >= UDP_MAX_BINDS {
            return 0;
        }

        BIND_TABLE[BIND_COUNT].port = port;
        BIND_TABLE[BIND_COUNT].handler = Some(handler);
        BIND_COUNT += 1;
        1
    }
}

/// Handle an incoming UDP packet.
/// Parses the header, looks up the destination port, and calls the handler.
pub fn rx(src_ip: u32, data: *const u8, length: u16) {
    unsafe {
        if length < 8 {
            return;
        }

        let hdr = data as *const UdpHeader;
        let dst_port = ntohs(core::ptr::read_unaligned(&raw const (*hdr).dst_port));
        let udp_len = ntohs(core::ptr::read_unaligned(&raw const (*hdr).length));

        if udp_len < 8 || udp_len > length {
            return;
        }

        let payload = data.add(8);
        let data_len = udp_len - 8;

        for i in 0..BIND_COUNT {
            if BIND_TABLE[i].port == dst_port {
                if let Some(handler) = BIND_TABLE[i].handler {
                    let src_port = ntohs(core::ptr::read_unaligned(&raw const (*hdr).src_port));
                    handler(src_ip, src_port, payload, data_len);
                    return;
                }
            }
        }
    }
}

/// Send a UDP packet.
/// Builds the UDP header (checksum set to 0) and calls ipv4::send().
/// Returns 0 on success, -1 on failure.
pub fn send(dst_ip: u32, src_port: u16, dst_port: u16, data: *const u8, length: u16) -> i32 {
    unsafe {
        static mut BUF: [u8; 1500] = [0; 1500];
        let udp_len = 8u16 + length;

        if udp_len > 1480 {
            return -1;
        }

        let hdr = BUF.as_mut_ptr() as *mut UdpHeader;
        core::ptr::write_unaligned(&raw mut (*hdr).src_port, htons(src_port));
        core::ptr::write_unaligned(&raw mut (*hdr).dst_port, htons(dst_port));
        core::ptr::write_unaligned(&raw mut (*hdr).length, htons(udp_len));
        core::ptr::write_unaligned(&raw mut (*hdr).checksum, 0); // optional in IPv4

        string::memcpy(BUF.as_mut_ptr().add(8), data, length as usize);

        ipv4::send(dst_ip, IP_PROTO_UDP, BUF.as_ptr(), udp_len)
    }
}

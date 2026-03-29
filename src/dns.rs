// dns.rs -- DNS stub resolver (A records only).
//
// Sends queries to the QEMU default DNS server (10.0.2.3) on port 53.
// Uses a simple volatile flag pattern for async receive, similar to ICMP.

use crate::net::{htons, ntohs};
use crate::string;
use crate::timer;
use crate::udp;
use core::arch::asm;

const DNS_SERVER: u32 = 0x0A000203; // 10.0.2.3
const DNS_PORT: u16 = 53;
const DNS_LOCAL: u16 = 1024;
const DNS_MAX_RESP: usize = 512;

/// DNS header (12 bytes).
#[repr(C, packed)]
struct DnsHeader {
    id: u16,
    flags: u16,
    qdcount: u16,
    ancount: u16,
    nscount: u16,
    arcount: u16,
}

// DNS query types and classes
const DNS_TYPE_A: u16 = 1;
const DNS_CLASS_IN: u16 = 1;

// Async response state
static mut DNS_REPLY_READY: i32 = 0;
static mut DNS_REPLY_IP: u32 = 0;
static mut DNS_REPLY_ID: u16 = 0;
static mut DNS_QUERY_ID: u16 = 0;

/// Encode a hostname into DNS label format.
/// "google.com" -> "\x06google\x03com\x00"
/// Returns the total number of bytes written to dst.
unsafe fn dns_encode_name(name: *const u8, dst: *mut u8) -> i32 {
    let mut total = 0i32;
    let mut p = name;

    while *p != 0 {
        let mut dot = p;

        // Find next dot or end of string
        while *dot != 0 && *dot != b'.' {
            dot = dot.add(1);
        }

        let len = (dot as usize - p as usize) as u8;
        if len == 0 || len > 63 {
            return 0; // invalid label
        }

        *dst.add(total as usize) = len;
        total += 1;
        while p != dot {
            *dst.add(total as usize) = *p;
            total += 1;
            p = p.add(1);
        }

        if *p == b'.' {
            p = p.add(1);
        }
    }

    *dst.add(total as usize) = 0; // null terminator
    total += 1;
    total
}

/// Skip a DNS name in a packet (handles label compression).
/// Returns the number of bytes consumed from the current position.
unsafe fn dns_skip_name(pkt: *const u8, offset: i32, pkt_len: i32) -> i32 {
    let start = offset;
    let mut off = offset;
    let mut jumped = false;
    let mut count = 0i32;
    let mut hops = 0;

    while off < pkt_len {
        let b = *pkt.add(off as usize);

        if b == 0 {
            // End of name
            if !jumped {
                return (off - start) + 1;
            } else {
                return count;
            }
        }

        if (b & 0xC0) == 0xC0 {
            // Compression pointer: 2 bytes
            hops += 1;
            if hops > 64 {
                return -1; // prevent infinite loop on circular pointers
            }
            if !jumped {
                count = (off - start) + 2;
                jumped = true;
            }
            if off + 1 >= pkt_len {
                return -1;
            }
            off = (((b & 0x3F) as i32) << 8) | (*pkt.add(off as usize + 1) as i32);
            continue;
        }

        // Regular label
        off += 1 + b as i32;
    }

    -1 // malformed
}

/// UDP handler for DNS responses.
fn dns_rx(_src_ip: u32, _src_port: u16, data: *const u8, length: u16) {
    unsafe {
        if length < 12 {
            return;
        }

        let hdr = data as *const DnsHeader;
        let id = ntohs(core::ptr::read_unaligned(&raw const (*hdr).id));
        let flags = ntohs(core::ptr::read_unaligned(&raw const (*hdr).flags));
        let qdcount = ntohs(core::ptr::read_unaligned(&raw const (*hdr).qdcount));
        let ancount = ntohs(core::ptr::read_unaligned(&raw const (*hdr).ancount));

        // Check that this is a response (QR=1) and matches our query
        if (flags & 0x8000) == 0 {
            return;
        }
        if id != core::ptr::read_volatile(&raw const DNS_QUERY_ID) {
            return;
        }

        // Skip question section
        let mut offset = 12i32;
        for _ in 0..qdcount {
            let skip = dns_skip_name(data, offset, length as i32);
            if skip < 0 {
                return;
            }
            offset += skip;
            offset += 4; // qtype(2) + qclass(2)
            if offset > length as i32 {
                return;
            }
        }

        // Parse answer section: find first A record
        for _ in 0..ancount {
            let skip = dns_skip_name(data, offset, length as i32);
            if skip < 0 {
                return;
            }
            offset += skip;

            if offset + 10 > length as i32 {
                return;
            }

            let rtype = ((*data.add(offset as usize) as u16) << 8)
                | (*data.add(offset as usize + 1) as u16);
            let rclass = ((*data.add(offset as usize + 2) as u16) << 8)
                | (*data.add(offset as usize + 3) as u16);
            // ttl at offset+4..offset+7 (skip)
            let rdlength = ((*data.add(offset as usize + 8) as u16) << 8)
                | (*data.add(offset as usize + 9) as u16);
            offset += 10;

            if offset + rdlength as i32 > length as i32 {
                return;
            }

            if rtype == DNS_TYPE_A && rclass == DNS_CLASS_IN && rdlength == 4 {
                let ip = ((*data.add(offset as usize) as u32) << 24)
                    | ((*data.add(offset as usize + 1) as u32) << 16)
                    | ((*data.add(offset as usize + 2) as u32) << 8)
                    | (*data.add(offset as usize + 3) as u32);
                core::ptr::write_volatile(&raw mut DNS_REPLY_IP, ip);
                core::ptr::write_volatile(&raw mut DNS_REPLY_ID, id);
                core::ptr::write_volatile(&raw mut DNS_REPLY_READY, 1);
                return;
            }

            offset += rdlength as i32;
        }
    }
}

/// Initialize the DNS resolver.
/// Binds the UDP handler on the local DNS port.
pub fn init() {
    unsafe {
        DNS_QUERY_ID = 0;
        core::ptr::write_volatile(&raw mut DNS_REPLY_READY, 0);
        udp::bind(DNS_LOCAL, dns_rx);
    }
}

/// Resolve a hostname to an IPv4 address.
/// Returns 1 and fills ip_out on success, 0 on timeout or error.
pub fn resolve(hostname: *const u8, ip_out: *mut u32) -> i32 {
    unsafe {
        static mut QUERY_BUF: [u8; DNS_MAX_RESP] = [0; DNS_MAX_RESP];

        // Check hostname length
        if string::strlen(hostname) > 253 {
            return 0;
        }

        // Increment query ID
        DNS_QUERY_ID = DNS_QUERY_ID.wrapping_add(1);
        core::ptr::write_volatile(&raw mut DNS_REPLY_READY, 0);

        // Build DNS header
        string::memset(QUERY_BUF.as_mut_ptr(), 0, DNS_MAX_RESP);
        let hdr = QUERY_BUF.as_mut_ptr() as *mut DnsHeader;
        core::ptr::write_unaligned(&raw mut (*hdr).id, htons(DNS_QUERY_ID));
        core::ptr::write_unaligned(&raw mut (*hdr).flags, htons(0x0100)); // RD = 1
        core::ptr::write_unaligned(&raw mut (*hdr).qdcount, htons(1));
        core::ptr::write_unaligned(&raw mut (*hdr).ancount, 0);
        core::ptr::write_unaligned(&raw mut (*hdr).nscount, 0);
        core::ptr::write_unaligned(&raw mut (*hdr).arcount, 0);

        // Encode the hostname into the question section
        let name_len = dns_encode_name(hostname, QUERY_BUF.as_mut_ptr().add(12));
        if name_len == 0 {
            return 0;
        }

        // Set qtype = A (1), qclass = IN (1)
        *QUERY_BUF.as_mut_ptr().add(12 + name_len as usize) = 0;
        *QUERY_BUF.as_mut_ptr().add(12 + name_len as usize + 1) = DNS_TYPE_A as u8;
        *QUERY_BUF.as_mut_ptr().add(12 + name_len as usize + 2) = 0;
        *QUERY_BUF.as_mut_ptr().add(12 + name_len as usize + 3) = DNS_CLASS_IN as u8;

        let query_len = 12 + name_len + 4;

        // Send the query
        udp::send(
            DNS_SERVER,
            DNS_LOCAL,
            DNS_PORT,
            QUERY_BUF.as_ptr(),
            query_len as u16,
        );

        // Wait up to 2 seconds (200 ticks at 100 Hz) for a response
        let start = timer::get_ticks();
        while timer::get_ticks().wrapping_sub(start) < 200 {
            // Check volatile flag with interrupts disabled to avoid TOCTOU
            asm!("cli");
            let ready = core::ptr::read_volatile(&raw const DNS_REPLY_READY) != 0
                && core::ptr::read_volatile(&raw const DNS_REPLY_ID) == DNS_QUERY_ID;
            if ready {
                let rip = core::ptr::read_volatile(&raw const DNS_REPLY_IP);
                core::ptr::write_volatile(&raw mut DNS_REPLY_READY, 0);
                asm!("sti");
                *ip_out = rip;
                return 1;
            }
            asm!("sti");
            asm!("hlt");
        }

        0
    }
}

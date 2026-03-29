// net.rs -- Ethernet frame dispatch and network initialization.
//
// Shared network types, constants, and byte-order helpers.

use crate::arp;
use crate::ipv4;
use crate::rtl8139;
use crate::string;
use crate::vga;
use core::arch::asm;

// Network configuration (QEMU user-mode networking defaults)
pub const NET_IP_ADDR: u32 = 0x0A00020F;     // 10.0.2.15
pub const NET_GATEWAY: u32 = 0x0A000202;     // 10.0.2.2
pub const NET_SUBNET_MASK: u32 = 0xFFFFFF00; // 255.255.255.0

// EtherType constants (in host byte order, convert with htons before use)
pub const ETH_TYPE_IP: u16 = 0x0800;
pub const ETH_TYPE_ARP: u16 = 0x0806;

// IP protocol numbers
pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_UDP: u8 = 17;

/// Ethernet header (14 bytes).
#[repr(C, packed)]
pub struct EthHeader {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16, // network byte order
}

/// Broadcast MAC address.
pub const ETH_BROADCAST: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

static mut OUR_MAC: [u8; 6] = [0; 6];

// Byte-order conversion (x86 is little-endian, network is big-endian)

#[inline]
pub fn htons(v: u16) -> u16 {
    (v >> 8) | (v << 8)
}

#[inline]
pub fn ntohs(v: u16) -> u16 {
    htons(v)
}

#[inline]
pub fn htonl(v: u32) -> u32 {
    ((v >> 24) & 0xFF)
        | ((v >> 8) & 0xFF00)
        | ((v << 8) & 0xFF_0000)
        | ((v << 24) & 0xFF00_0000)
}

#[inline]
pub fn ntohl(v: u32) -> u32 {
    htonl(v)
}

/// Initialize the network stack.
pub fn init() {
    unsafe {
        OUR_MAC = rtl8139::get_mac();
        arp::init();
    }
}

/// Called by the NIC driver's IRQ handler when a frame is received.
/// Dispatches by EtherType to the appropriate protocol handler.
pub fn rx(frame: *const u8, length: u16) {
    unsafe {
        if length < 14 {
            return;
        }

        let eth = frame as *const EthHeader;
        let ethertype = ntohs(core::ptr::read_unaligned(&raw const (*eth).ethertype));
        let payload = frame.add(14);
        let payload_len = length - 14;

        match ethertype {
            ETH_TYPE_ARP => {
                arp::rx(payload, payload_len);
            }
            ETH_TYPE_IP => {
                ipv4::rx(payload, payload_len);
            }
            _ => {}
        }
    }
}

/// Construct and send an Ethernet frame.
/// Prepends the 14-byte Ethernet header to the payload and hands it to the NIC.
pub fn send_ethernet(dst_mac: &[u8; 6], ethertype: u16, payload: *const u8, payload_len: u16) {
    unsafe {
        static mut FRAME: [u8; 1518] = [0; 1518]; // max Ethernet frame

        if payload_len as u32 + 14 > 1518 {
            return;
        }

        // Disable interrupts to prevent IRQ reentrancy on the static buffer
        let flags: u32;
        asm!("pushfd", "pop {0:e}", "cli", out(reg) flags);

        let eth = FRAME.as_mut_ptr() as *mut EthHeader;
        string::memcpy((*eth).dst.as_mut_ptr(), dst_mac.as_ptr(), 6);
        string::memcpy((*eth).src.as_mut_ptr(), OUR_MAC.as_ptr(), 6);
        core::ptr::write_unaligned(&raw mut (*eth).ethertype, htons(ethertype));
        string::memcpy(FRAME.as_mut_ptr().add(14), payload, payload_len as usize);

        rtl8139::send(FRAME.as_ptr(), 14 + payload_len);

        asm!("push {0:e}", "popfd", in(reg) flags);
    }
}

/// Format a 32-bit IP address (host byte order) as "A.B.C.D" into buf.
pub unsafe fn ip_to_str(ip: u32, buf: *mut u8) {
    let mut pos = 0usize;
    for i in (0..4).rev() {
        let octet = ((ip >> (i * 8)) & 0xFF) as u8;
        if octet >= 100 {
            *buf.add(pos) = b'0' + octet / 100;
            pos += 1;
            *buf.add(pos) = b'0' + (octet / 10) % 10;
            pos += 1;
            *buf.add(pos) = b'0' + octet % 10;
            pos += 1;
        } else if octet >= 10 {
            *buf.add(pos) = b'0' + octet / 10;
            pos += 1;
            *buf.add(pos) = b'0' + octet % 10;
            pos += 1;
        } else {
            *buf.add(pos) = b'0' + octet;
            pos += 1;
        }
        if i > 0 {
            *buf.add(pos) = b'.';
            pos += 1;
        }
    }
    *buf.add(pos) = 0;
}

/// Get the kernel's IP address.
pub fn get_ip() -> u32 {
    NET_IP_ADDR
}

/// Set the kernel's IP address (no-op in this static config, kept for API compat).
pub fn set_ip(_ip: u32) {
    // Static configuration -- IP is a compile-time constant
}

/// Get the gateway address.
pub fn get_gateway() -> u32 {
    NET_GATEWAY
}

/// Set the gateway address (no-op in this static config, kept for API compat).
pub fn set_gateway(_gw: u32) {
    // Static configuration -- gateway is a compile-time constant
}

/// Get the subnet mask.
pub fn get_subnet() -> u32 {
    NET_SUBNET_MASK
}

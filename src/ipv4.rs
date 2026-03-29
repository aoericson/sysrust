// ipv4.rs -- IPv4 protocol (RFC 791).
//
// Minimal implementation: no fragmentation, no options, no routing table.
// Packets to the local subnet go directly; everything else goes to the gateway.

use crate::arp;
use crate::icmp;
use crate::net::{
    self, htons, ntohs, htonl, ntohl, ETH_TYPE_IP, IP_PROTO_ICMP, IP_PROTO_TCP, IP_PROTO_UDP,
    NET_GATEWAY, NET_IP_ADDR, NET_SUBNET_MASK,
};
use crate::string;
use crate::tcp;
use crate::udp;

#[repr(C, packed)]
struct Ipv4Header {
    version_ihl: u8,      // version (4) | IHL (5) = 0x45
    tos: u8,              // type of service
    total_length: u16,    // header + payload
    identification: u16,  // fragment ID
    flags_fragment: u16,  // flags + fragment offset
    ttl: u8,              // time to live
    protocol: u8,         // 1=ICMP, 6=TCP, 17=UDP
    checksum: u16,
    src_ip: u32,          // network byte order
    dst_ip: u32,          // network byte order
}

static mut IP_ID_COUNTER: u16 = 0;

/// Compute the standard Internet checksum (ones-complement sum).
/// Used for IPv4 header and ICMP packets.
pub fn checksum(data: *const u8, length: u16) -> u16 {
    unsafe {
        let mut ptr = data as *const u16;
        let mut sum: u32 = 0;
        let mut remaining = length;

        while remaining > 1 {
            sum += core::ptr::read_unaligned(ptr) as u32;
            ptr = ptr.add(1);
            remaining -= 2;
        }
        if remaining == 1 {
            sum += *(ptr as *const u8) as u32;
        }

        sum = (sum >> 16) + (sum & 0xFFFF);
        sum += sum >> 16;
        (!sum) as u16
    }
}

/// Handle an incoming IPv4 packet.
/// Validates the header and dispatches by protocol number.
pub fn rx(data: *const u8, length: u16) {
    unsafe {
        if length < 20 {
            return;
        }

        let hdr = data as *const Ipv4Header;

        // Check version = 4
        if ((*hdr).version_ihl >> 4) != 4 {
            return;
        }

        let hdr_len = (((*hdr).version_ihl & 0x0F) as u16) * 4;
        let total_len = ntohs(core::ptr::read_unaligned(&raw const (*hdr).total_length));

        if (length as u16) < total_len || total_len < hdr_len {
            return;
        }

        // Verify checksum
        if checksum(data, hdr_len) != 0 {
            return;
        }

        // Check destination is our IP or broadcast
        let dst_ip = ntohl(core::ptr::read_unaligned(&raw const (*hdr).dst_ip));
        if dst_ip != NET_IP_ADDR && dst_ip != 0xFFFFFFFF {
            return;
        }

        let payload = data.add(hdr_len as usize);
        let payload_len = total_len - hdr_len;
        let protocol = (*hdr).protocol;

        match protocol {
            IP_PROTO_ICMP => {
                icmp::rx(
                    ntohl(core::ptr::read_unaligned(&raw const (*hdr).src_ip)),
                    payload,
                    payload_len,
                );
            }
            IP_PROTO_TCP => {
                tcp::rx(
                    ntohl(core::ptr::read_unaligned(&raw const (*hdr).src_ip)),
                    payload,
                    payload_len,
                );
            }
            IP_PROTO_UDP => {
                udp::rx(
                    ntohl(core::ptr::read_unaligned(&raw const (*hdr).src_ip)),
                    payload,
                    payload_len,
                );
            }
            _ => {}
        }
    }
}

/// Send an IPv4 packet.
/// Builds the header, resolves the destination MAC via ARP, and sends.
/// Returns 0 on success, -1 on failure.
pub fn send(dst_ip: u32, protocol: u8, payload: *const u8, length: u16) -> i32 {
    unsafe {
        static mut PKT_BUF: [u8; 1500] = [0; 1500];

        if length as u32 + 20 > 1500 {
            return -1;
        }

        let hdr = PKT_BUF.as_mut_ptr() as *mut Ipv4Header;

        // Build IPv4 header
        (*hdr).version_ihl = 0x45; // IPv4, 5 dwords (20 bytes)
        (*hdr).tos = 0;
        core::ptr::write_unaligned(
            &raw mut (*hdr).total_length,
            htons(20 + length),
        );
        core::ptr::write_unaligned(
            &raw mut (*hdr).identification,
            htons(IP_ID_COUNTER),
        );
        IP_ID_COUNTER = IP_ID_COUNTER.wrapping_add(1);
        core::ptr::write_unaligned(
            &raw mut (*hdr).flags_fragment,
            htons(0x4000), // Don't Fragment
        );
        (*hdr).ttl = 64;
        (*hdr).protocol = protocol;
        core::ptr::write_unaligned(&raw mut (*hdr).checksum, 0);
        core::ptr::write_unaligned(&raw mut (*hdr).src_ip, htonl(NET_IP_ADDR));
        core::ptr::write_unaligned(&raw mut (*hdr).dst_ip, htonl(dst_ip));

        // Compute header checksum
        core::ptr::write_unaligned(
            &raw mut (*hdr).checksum,
            checksum(PKT_BUF.as_ptr(), 20),
        );

        // Copy payload after header
        string::memcpy(PKT_BUF.as_mut_ptr().add(20), payload, length as usize);

        // Determine next-hop: on same subnet -> direct, else -> gateway
        let next_hop = if (dst_ip & NET_SUBNET_MASK) == (NET_IP_ADDR & NET_SUBNET_MASK) {
            dst_ip
        } else {
            NET_GATEWAY
        };

        // Resolve MAC address via ARP
        let mut dst_mac = [0u8; 6];
        if arp::resolve(next_hop, &mut dst_mac) == 0 {
            return -1; // ARP resolution failed
        }

        net::send_ethernet(&dst_mac, ETH_TYPE_IP, PKT_BUF.as_ptr(), 20 + length);
        0
    }
}

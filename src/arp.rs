// arp.rs -- Address Resolution Protocol (RFC 826).
//
// Maps IP addresses to MAC addresses. Maintains a 16-entry static table.
// When a MAC is needed and not in the table, broadcasts an ARP request
// and waits (blocking) for the reply.

use crate::net::{
    self, htons, ntohs, ETH_BROADCAST, ETH_TYPE_ARP, NET_IP_ADDR,
};
use crate::rtl8139;
use crate::string;
use crate::sync::Spinlock;
use crate::timer;
use core::arch::asm;

#[repr(C, packed)]
struct ArpPacket {
    hw_type: u16,       // 1 = Ethernet
    proto_type: u16,    // 0x0800 = IPv4
    hw_len: u8,         // 6 (MAC length)
    proto_len: u8,      // 4 (IPv4 length)
    opcode: u16,        // 1 = request, 2 = reply
    sender_mac: [u8; 6],
    sender_ip: [u8; 4], // byte array to avoid alignment issues
    target_mac: [u8; 6],
    target_ip: [u8; 4],
}

const ARP_TABLE_SIZE: usize = 16;

struct ArpEntry {
    ip: u32,
    mac: [u8; 6],
    valid: bool,
}

static mut ARP_TABLE: [ArpEntry; ARP_TABLE_SIZE] = {
    const EMPTY: ArpEntry = ArpEntry {
        ip: 0,
        mac: [0; 6],
        valid: false,
    };
    [EMPTY; ARP_TABLE_SIZE]
};
static mut ARP_LOCK: Spinlock = Spinlock::new();

/// Initialize the ARP table.
pub fn init() {
    unsafe {
        for i in 0..ARP_TABLE_SIZE {
            ARP_TABLE[i].ip = 0;
            ARP_TABLE[i].mac = [0; 6];
            ARP_TABLE[i].valid = false;
        }
    }
}

/// Store an IP->MAC mapping in the ARP table.
unsafe fn arp_table_update(ip: u32, mac: *const u8) {
    ARP_LOCK.lock();
    let mut free_slot: i32 = -1;

    for i in 0..ARP_TABLE_SIZE {
        if ARP_TABLE[i].valid && ARP_TABLE[i].ip == ip {
            string::memcpy(ARP_TABLE[i].mac.as_mut_ptr(), mac, 6);
            ARP_LOCK.unlock();
            return;
        }
        if !ARP_TABLE[i].valid && free_slot == -1 {
            free_slot = i as i32;
        }
    }

    if free_slot >= 0 {
        let slot = free_slot as usize;
        ARP_TABLE[slot].ip = ip;
        string::memcpy(ARP_TABLE[slot].mac.as_mut_ptr(), mac, 6);
        ARP_TABLE[slot].valid = true;
    }
    ARP_LOCK.unlock();
}

/// Look up an IP in the ARP table. Returns true if found.
unsafe fn arp_table_lookup(ip: u32, mac_out: *mut u8) -> bool {
    ARP_LOCK.lock();
    for i in 0..ARP_TABLE_SIZE {
        if ARP_TABLE[i].valid && ARP_TABLE[i].ip == ip {
            string::memcpy(mac_out, ARP_TABLE[i].mac.as_ptr(), 6);
            ARP_LOCK.unlock();
            return true;
        }
    }
    ARP_LOCK.unlock();
    false
}

/// Read a u32 from a byte array (network byte order -> host).
fn read_ip(p: *const u8) -> u32 {
    unsafe {
        (((*p.add(0)) as u32) << 24)
            | (((*p.add(1)) as u32) << 16)
            | (((*p.add(2)) as u32) << 8)
            | ((*p.add(3)) as u32)
    }
}

/// Write a u32 to a byte array (host -> network byte order).
unsafe fn write_ip(p: *mut u8, ip: u32) {
    *p.add(0) = ((ip >> 24) & 0xFF) as u8;
    *p.add(1) = ((ip >> 16) & 0xFF) as u8;
    *p.add(2) = ((ip >> 8) & 0xFF) as u8;
    *p.add(3) = (ip & 0xFF) as u8;
}

/// Send an ARP request for the given IP.
unsafe fn arp_send_request(target_ip: u32) {
    let our_mac = rtl8139::get_mac();
    let mut pkt = core::mem::MaybeUninit::<ArpPacket>::zeroed().assume_init();

    pkt.hw_type = htons(1);
    pkt.proto_type = htons(0x0800);
    pkt.hw_len = 6;
    pkt.proto_len = 4;
    pkt.opcode = htons(1); // request
    string::memcpy(pkt.sender_mac.as_mut_ptr(), our_mac.as_ptr(), 6);
    write_ip(pkt.sender_ip.as_mut_ptr(), NET_IP_ADDR);
    string::memset(pkt.target_mac.as_mut_ptr(), 0, 6);
    write_ip(pkt.target_ip.as_mut_ptr(), target_ip);

    net::send_ethernet(
        &ETH_BROADCAST,
        ETH_TYPE_ARP,
        &pkt as *const ArpPacket as *const u8,
        core::mem::size_of::<ArpPacket>() as u16,
    );
}

/// Send an ARP reply.
unsafe fn arp_send_reply(dst_mac: *const u8, dst_ip: u32) {
    let our_mac = rtl8139::get_mac();
    let mut pkt = core::mem::MaybeUninit::<ArpPacket>::zeroed().assume_init();

    pkt.hw_type = htons(1);
    pkt.proto_type = htons(0x0800);
    pkt.hw_len = 6;
    pkt.proto_len = 4;
    pkt.opcode = htons(2); // reply
    string::memcpy(pkt.sender_mac.as_mut_ptr(), our_mac.as_ptr(), 6);
    write_ip(pkt.sender_ip.as_mut_ptr(), NET_IP_ADDR);
    string::memcpy(pkt.target_mac.as_mut_ptr(), dst_mac, 6);
    write_ip(pkt.target_ip.as_mut_ptr(), dst_ip);

    let dst_mac_arr = &*(dst_mac as *const [u8; 6]);
    net::send_ethernet(
        dst_mac_arr,
        ETH_TYPE_ARP,
        &pkt as *const ArpPacket as *const u8,
        core::mem::size_of::<ArpPacket>() as u16,
    );
}

/// Handle an incoming ARP packet.
/// If it's a request for our IP, send a reply.
/// If it's a reply, update the ARP table.
pub fn rx(data: *const u8, length: u16) {
    unsafe {
        if (length as usize) < core::mem::size_of::<ArpPacket>() {
            return;
        }

        let pkt = data as *const ArpPacket;
        if ntohs(core::ptr::read_unaligned(&raw const (*pkt).hw_type)) != 1 {
            return;
        }
        if ntohs(core::ptr::read_unaligned(&raw const (*pkt).proto_type)) != 0x0800 {
            return;
        }

        let opcode = ntohs(core::ptr::read_unaligned(&raw const (*pkt).opcode));
        let sender_ip = read_ip((&raw const (*pkt).sender_ip) as *const u8);
        let target_ip = read_ip((&raw const (*pkt).target_ip) as *const u8);

        // Always learn from incoming ARP packets
        arp_table_update(sender_ip, (&raw const (*pkt).sender_mac) as *const u8);

        if opcode == 1 && target_ip == NET_IP_ADDR {
            // ARP request for our IP -- send a reply
            arp_send_reply((&raw const (*pkt).sender_mac) as *const u8, sender_ip);
        }
        // Replies are handled by the table update above
    }
}

/// Resolve an IP address to a MAC address.
/// Checks the ARP table first. If not found, sends a request and waits
/// up to 200ms (20 ticks at 100Hz) for a reply.
/// Returns 1 on success, 0 on timeout.
pub fn resolve(ip: u32, mac_out: &mut [u8; 6]) -> i32 {
    unsafe {
        // Check table first
        if arp_table_lookup(ip, mac_out.as_mut_ptr()) {
            return 1;
        }

        // Send request and wait, retry up to 3 times
        for _attempt in 0..3 {
            arp_send_request(ip);
            let start = timer::get_ticks();

            while timer::get_ticks().wrapping_sub(start) < 20 {
                if arp_table_lookup(ip, mac_out.as_mut_ptr()) {
                    return 1;
                }
                asm!("hlt");
            }
        }
        0
    }
}

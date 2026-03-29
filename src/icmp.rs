// icmp.rs -- ICMP protocol (RFC 792).
//
// Handles echo request (type 8) by sending echo reply (type 0).
// Tracks received echo replies for the shell's ping command.

use crate::ipv4;
use crate::net::{htons, ntohs, IP_PROTO_ICMP};
use crate::string;
use core::arch::asm;

#[repr(C, packed)]
struct IcmpHeader {
    type_: u8,
    code: u8,
    checksum: u16,
    identifier: u16,
    sequence: u16,
}

const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_ECHO_REQUEST: u8 = 8;

// State for tracking ping replies
static mut REPLY_RECEIVED: i32 = 0;
static mut REPLY_ID: u16 = 0;
static mut REPLY_SEQ: u16 = 0;
static mut REPLY_SRC_IP: u32 = 0;

/// Handle an incoming ICMP packet.
pub fn rx(src_ip: u32, data: *const u8, length: u16) {
    unsafe {
        if (length as usize) < core::mem::size_of::<IcmpHeader>() {
            return;
        }

        // Verify checksum over the entire ICMP message
        if ipv4::checksum(data, length) != 0 {
            return;
        }

        let hdr = data as *const IcmpHeader;
        let type_ = (*hdr).type_;
        let code = (*hdr).code;

        if type_ == ICMP_ECHO_REQUEST && code == 0 {
            // Reply to ping: send back the same data with type 0
            static mut REPLY_BUF: [u8; 1500] = [0; 1500];

            if length > 1500 {
                return;
            }

            string::memcpy(REPLY_BUF.as_mut_ptr(), data, length as usize);
            let reply_hdr = REPLY_BUF.as_mut_ptr() as *mut IcmpHeader;
            (*reply_hdr).type_ = ICMP_ECHO_REPLY;
            core::ptr::write_unaligned(&raw mut (*reply_hdr).checksum, 0);
            core::ptr::write_unaligned(
                &raw mut (*reply_hdr).checksum,
                ipv4::checksum(REPLY_BUF.as_ptr(), length),
            );

            ipv4::send(src_ip, IP_PROTO_ICMP, REPLY_BUF.as_ptr(), length);
        } else if type_ == ICMP_ECHO_REPLY && code == 0 {
            // Record the reply for the ping command
            core::ptr::write_volatile(
                &raw mut REPLY_ID,
                ntohs(core::ptr::read_unaligned(&raw const (*hdr).identifier)),
            );
            core::ptr::write_volatile(
                &raw mut REPLY_SEQ,
                ntohs(core::ptr::read_unaligned(&raw const (*hdr).sequence)),
            );
            core::ptr::write_volatile(&raw mut REPLY_SRC_IP, src_ip);
            core::ptr::write_volatile(&raw mut REPLY_RECEIVED, 1);
        }
    }
}

/// Send an ICMP echo request (ping) to the given IP.
/// Returns 0 on success, -1 on failure.
pub fn send_echo_request(dst_ip: u32, id: u16, seq: u16) -> i32 {
    unsafe {
        let mut pkt = core::mem::MaybeUninit::<IcmpHeader>::zeroed().assume_init();

        pkt.type_ = ICMP_ECHO_REQUEST;
        pkt.code = 0;
        pkt.checksum = 0;
        pkt.identifier = htons(id);
        pkt.sequence = htons(seq);
        pkt.checksum = ipv4::checksum(
            &pkt as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>() as u16,
        );

        ipv4::send(
            dst_ip,
            IP_PROTO_ICMP,
            &pkt as *const IcmpHeader as *const u8,
            core::mem::size_of::<IcmpHeader>() as u16,
        )
    }
}

/// Check if an echo reply has been received.
/// Returns 1 if a reply is pending (fills output params), 0 otherwise.
/// Clears the reply flag when read.
///
/// Disables interrupts around the volatile flag check-and-clear to avoid
/// a TOCTOU race where the IRQ handler sets reply_received between our test
/// and our clear.
pub fn got_reply(out_id: &mut u16, out_seq: &mut u16, out_src_ip: &mut u32) -> i32 {
    unsafe {
        asm!("cli");
        let got = core::ptr::read_volatile(&raw const REPLY_RECEIVED);
        if got != 0 {
            *out_id = core::ptr::read_volatile(&raw const REPLY_ID);
            *out_seq = core::ptr::read_volatile(&raw const REPLY_SEQ);
            *out_src_ip = core::ptr::read_volatile(&raw const REPLY_SRC_IP);
            core::ptr::write_volatile(&raw mut REPLY_RECEIVED, 0);
        }
        asm!("sti");
        got
    }
}

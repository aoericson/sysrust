// rtl8139.rs -- RTL8139 PCI Ethernet controller driver.
//
// The RTL8139 is a common 10/100 Mbit NIC found in many systems and emulated
// by QEMU. This driver:
//   1. Locates the card on the PCI bus (vendor 0x10EC, device 0x8139).
//   2. Enables PCI bus mastering for DMA transfers.
//   3. Performs a software reset, then configures the RX ring buffer and
//      four round-robin TX descriptor slots.
//   4. Registers an IRQ handler for receive-complete and transmit-complete
//      interrupts.
//
// Physical addresses equal virtual addresses in this kernel (flat model,
// no paging), so buffer pointers can be passed directly to the NIC.

use crate::idt;
use crate::io::{inb, inl, inw, outb, outl, outw};
use crate::net;
use crate::pci;
use crate::pic;
use crate::string;
use crate::vga;

// RTL8139 register offsets (relative to I/O base from PCI BAR0)
const REG_IDR0: u16 = 0x00;    // MAC address bytes 0-5 (6 bytes)
const REG_TSD0: u16 = 0x10;    // TX status descriptor 0 (32-bit)
const REG_TSAD0: u16 = 0x20;   // TX start address 0 (32-bit)
const REG_RBSTART: u16 = 0x30; // RX buffer start address (32-bit)
const REG_CMD: u16 = 0x37;     // Command register (8-bit)
const REG_CAPR: u16 = 0x38;    // Current address of packet read (16-bit)
const REG_IMR: u16 = 0x3C;     // Interrupt mask register (16-bit)
const REG_ISR: u16 = 0x3E;     // Interrupt status register (16-bit)
const REG_RCR: u16 = 0x44;     // RX configuration register (32-bit)
const REG_CONFIG1: u16 = 0x52; // Configuration register 1 (8-bit)

// Command register bits
const CMD_RST: u8 = 0x10;  // software reset
const CMD_RE: u8 = 0x08;   // receiver enable
const CMD_TE: u8 = 0x04;   // transmitter enable
const CMD_BUFE: u8 = 0x01; // RX buffer empty

// Interrupt status / mask bits
const INT_ROK: u16 = 0x0001; // receive OK
const INT_TOK: u16 = 0x0004; // transmit OK

// RX configuration: accept all + physical match + multicast + broadcast + WRAP
const RCR_CONFIG: u32 = 0x0000008F;

// Static DMA buffers (in BSS, aligned for hardware DMA)

// RX buffer: 8K + 16 (header) + 1536 (full max frame size, 4-byte aligned)
#[repr(align(4))]
struct RxBuffer([u8; 8192 + 16 + 1536]);
static mut RX_BUFFER: RxBuffer = RxBuffer([0; 8192 + 16 + 1536]);

// TX buffers: 4 slots, each holds one max-size Ethernet frame
#[repr(align(4))]
struct TxBuffer([u8; 1536]);
static mut TX_BUFFERS: [TxBuffer; 4] = [
    TxBuffer([0; 1536]),
    TxBuffer([0; 1536]),
    TxBuffer([0; 1536]),
    TxBuffer([0; 1536]),
];

static mut IO_BASE: u16 = 0;       // NIC I/O port base from PCI BAR0
static mut MAC_ADDR: [u8; 6] = [0; 6]; // MAC address read from NIC
static mut CURRENT_TX: u8 = 0;     // round-robin TX descriptor index (0-3)
static mut RX_OFFSET: u16 = 0;     // current read position in RX buffer

/// Print a hex byte to VGA for MAC address display.
unsafe fn put_hex_byte(val: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    vga::putchar(HEX[((val >> 4) & 0x0F) as usize]);
    vga::putchar(HEX[(val & 0x0F) as usize]);
}

/// RX packet handler -- called from the IRQ handler.
unsafe fn rtl8139_rx() {
    while (inb(IO_BASE + REG_CMD) & CMD_BUFE) == 0 {
        // Read the 4-byte RX header: low 16 bits = status, high 16 bits = length
        let header_ptr = RX_BUFFER.0.as_ptr().add(RX_OFFSET as usize) as *const u32;
        let header = core::ptr::read_unaligned(header_ptr);
        let status = (header & 0xFFFF) as u16;
        let pkt_length = (header >> 16) as u16;

        if (status & 0x01) == 0 {
            // Packet status ROK bit not set -- skip this packet
            break;
        }

        // The actual Ethernet frame starts at rx_buffer + rx_offset + 4
        // and is pkt_length - 4 bytes (excluding the 4-byte CRC).
        net::rx(
            RX_BUFFER.0.as_ptr().add(RX_OFFSET as usize + 4),
            pkt_length - 4,
        );

        // Advance rx_offset past header + packet, aligned to 4 bytes
        RX_OFFSET = ((RX_OFFSET + pkt_length + 4 + 3) & !3) % 8192;

        // Tell the NIC we've consumed up to this point (-16 hardware quirk)
        outw(IO_BASE + REG_CAPR, RX_OFFSET.wrapping_sub(16));
    }
}

/// IRQ handler.
fn rtl8139_handler(_regs: *mut idt::Registers) {
    unsafe {
        let status = inw(IO_BASE + REG_ISR);
        if status == 0 {
            return; // not our interrupt
        }

        if (status & INT_ROK) != 0 {
            rtl8139_rx();
        }

        // Acknowledge all handled interrupts by writing back the status
        outw(IO_BASE + REG_ISR, status);
    }
}

/// Initialize the RTL8139 NIC.
///
/// Locates the card on the PCI bus, enables bus mastering, performs a
/// software reset, configures the RX buffer and TX descriptors, enables
/// interrupts, and reads the MAC address.
pub fn init() {
    unsafe {
        // Step 1: find the RTL8139 on the PCI bus
        let dev = match pci::find_device(0x10EC, 0x8139) {
            Some(d) => d,
            None => {
                vga::puts(b"RTL8139: not found on PCI bus\n");
                return;
            }
        };

        // Step 2: enable PCI bus mastering (required for DMA)
        pci::enable_bus_mastering(&dev);

        // Step 3: store I/O base address
        IO_BASE = (dev.bar0 & 0xFFFF) as u16;

        // Step 4: power on -- write 0x00 to Config1 to exit low-power mode
        outb(IO_BASE + REG_CONFIG1, 0x00);

        // Step 5: software reset -- set RST bit, then poll until it clears
        outb(IO_BASE + REG_CMD, CMD_RST);
        while (inb(IO_BASE + REG_CMD) & CMD_RST) != 0 {
            // spin until reset completes
        }

        // Step 6: set RX buffer physical address
        outl(IO_BASE + REG_RBSTART, RX_BUFFER.0.as_ptr() as u32);

        // Step 7: enable ROK and TOK interrupts
        outw(IO_BASE + REG_IMR, INT_ROK | INT_TOK);

        // Step 8: configure RX -- accept all, WRAP mode, 8K buffer
        outl(IO_BASE + REG_RCR, RCR_CONFIG);

        // Step 9: enable transmitter and receiver
        outb(IO_BASE + REG_CMD, CMD_TE | CMD_RE);

        // Step 10: read MAC address from IDR0-IDR5
        for i in 0..6 {
            MAC_ADDR[i] = inb(IO_BASE + REG_IDR0 + i as u16);
        }

        // Step 11: register IRQ handler
        idt::register_handler(32 + dev.irq_line, rtl8139_handler);

        // Step 12: unmask the NIC's IRQ line in the PIC
        pic::unmask_irq(dev.irq_line);

        // Step 13: initialize TX and RX state
        CURRENT_TX = 0;
        RX_OFFSET = 0;

        // Step 14: print MAC address for verification
        vga::puts(b"RTL8139: MAC ");
        for i in 0..6 {
            if i > 0 {
                vga::putchar(b':');
            }
            put_hex_byte(MAC_ADDR[i]);
        }
        vga::putchar(b'\n');
    }
}

/// Transmit a raw Ethernet frame.
///
/// Copies the frame into the next TX descriptor buffer, writes the
/// physical address and length to the NIC registers to trigger DMA,
/// then advances to the next descriptor slot (round-robin over 4).
pub fn send(data: *const u8, mut length: u16) {
    unsafe {
        // Clamp to maximum Ethernet frame size
        if length > 1536 {
            length = 1536;
        }

        // Poll the OWN bit (bit 13) of the TX status descriptor.
        // When OWN=1 the NIC owns the descriptor; wait until it clears
        // (OWN=0) before reusing this slot.
        let mut timeout = 10000i32;
        while (inl(IO_BASE + REG_TSD0 + CURRENT_TX as u16 * 4) & (1u32 << 13)) != 0
            && timeout > 0
        {
            timeout -= 1;
        }

        // Copy frame data into the current TX buffer
        string::memcpy(
            TX_BUFFERS[CURRENT_TX as usize].0.as_mut_ptr(),
            data,
            length as usize,
        );

        // Write the physical address of this TX buffer
        outl(
            IO_BASE + REG_TSAD0 + CURRENT_TX as u16 * 4,
            TX_BUFFERS[CURRENT_TX as usize].0.as_ptr() as u32,
        );

        // Write length to TSD register. Bits 12-0 hold the size. Upper bits
        // (including OWN at bit 13) are cleared, which tells the NIC to take
        // ownership and begin transmission.
        outl(
            IO_BASE + REG_TSD0 + CURRENT_TX as u16 * 4,
            length as u32,
        );

        // Advance to next TX descriptor (round-robin)
        CURRENT_TX = (CURRENT_TX + 1) % 4;
    }
}

/// Copy the NIC's MAC address into the caller-provided buffer.
pub fn get_mac() -> [u8; 6] {
    unsafe { MAC_ADDR }
}

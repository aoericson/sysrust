// pic.rs -- 8259 Programmable Interrupt Controller driver.
//
// The 8259 PIC routes hardware interrupt signals (keyboard, timer, disk, etc.)
// to the CPU. There are two PICs: a master (IRQs 0-7) and a slave (IRQs 8-15),
// connected together via IRQ2 ("cascade").
//
// Problem: The BIOS defaults map IRQs 0-7 to interrupt vectors 8-15. But
// vectors 8-15 are already used by CPU exceptions. A keyboard press would be
// misidentified as a Double Fault.
//
// Solution: Send the ICW (Initialization Command Word) sequence to remap:
//   Master PIC: IRQs 0-7  -> vectors 32-39
//   Slave PIC:  IRQs 8-15 -> vectors 40-47

use crate::io;

// PIC I/O port addresses
const PIC1_CMD:  u16 = 0x20;   // master PIC command port
const PIC1_DATA: u16 = 0x21;   // master PIC data port
const PIC2_CMD:  u16 = 0xA0;   // slave PIC command port
const PIC2_DATA: u16 = 0xA1;   // slave PIC data port

// Initialization Command Words
const ICW1_INIT: u8 = 0x10;    // ICW1: begin initialization sequence
const ICW1_ICW4: u8 = 0x01;    // ICW1: ICW4 will be sent
const ICW4_8086: u8 = 0x01;    // ICW4: 8086/88 mode (vs MCS-80/85)

// End-Of-Interrupt command byte
const EOI: u8 = 0x20;

/// Initialize and remap both PICs.
///
/// The 4-step ICW sequence must be sent in order:
///   ICW1: start init + tell PIC that ICW4 is coming
///   ICW2: vector offset (where to map IRQ 0)
///   ICW3: cascade configuration (which IRQ connects master to slave)
///   ICW4: mode selection (8086 mode)
pub unsafe fn init() {
    // ICW1: start initialization (both PICs)
    io::outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
    io::outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);

    // ICW2: set vector offsets
    io::outb(PIC1_DATA, 32);   // master: IRQ 0-7  -> vectors 32-39
    io::outb(PIC2_DATA, 40);   // slave:  IRQ 8-15 -> vectors 40-47

    // ICW3: configure cascade wiring
    io::outb(PIC1_DATA, 4);    // master: slave is connected on IRQ2 (bit 2)
    io::outb(PIC2_DATA, 2);    // slave:  cascade identity = 2

    // ICW4: set 8086 mode
    io::outb(PIC1_DATA, ICW4_8086);
    io::outb(PIC2_DATA, ICW4_8086);

    // Set interrupt masks. A 1 bit = masked (disabled), 0 = enabled.
    // 0xF8 = 1111 1000 -> IRQ0 (timer), IRQ1 (keyboard), IRQ2 (cascade) enabled
    // 0xFF = 1111 1111 -> all slave IRQs disabled
    io::outb(PIC1_DATA, 0xF8);
    io::outb(PIC2_DATA, 0xFF);
}

/// Send End-Of-Interrupt to the PIC.
///
/// The PIC won't deliver the next interrupt on a given line until it receives
/// an EOI signal. For IRQs 8-15 (slave), we must send EOI to BOTH the slave
/// and the master (because the slave is connected through the master).
///
/// irq: the IRQ number (0-15), NOT the interrupt vector.
pub unsafe fn send_eoi(irq: u8) {
    if irq >= 8 {
        io::outb(PIC2_CMD, EOI);    // send EOI to slave
    }
    io::outb(PIC1_CMD, EOI);        // send EOI to master (always)
}

/// Unmask (enable) a specific IRQ line.
///
/// IRQs 0-7 are on the master PIC (data port 0x21).
/// IRQs 8-15 are on the slave PIC (data port 0xA1); for these, also
/// ensure IRQ2 (cascade) is unmasked on the master.
pub unsafe fn unmask_irq(irq: u8) {
    let port;
    let line;

    if irq < 8 {
        port = PIC1_DATA;
        line = irq;
    } else {
        port = PIC2_DATA;
        line = irq - 8;
    }
    let mask = io::inb(port);
    io::outb(port, mask & !(1 << line));
}

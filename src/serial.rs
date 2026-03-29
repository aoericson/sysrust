// serial.rs -- COM1 serial port driver (TX + RX).
//
// Initializes the 16550 UART at COM1 (0x3F8) for 115200 baud, 8N1.
// Used for test automation output and as a remote console input source.
// QEMU can expose this as a TCP socket via -serial tcp::2323,server,nowait.

use crate::io::{inb, outb};
use core::arch::asm;

const COM1: u16 = 0x3F8;

/// Initialize the COM1 serial port for 115200 baud, 8N1.
pub unsafe fn init() {
    outb(COM1 + 1, 0x00); // disable all interrupts
    outb(COM1 + 3, 0x80); // enable DLAB (set baud rate divisor)
    outb(COM1 + 0, 0x01); // divisor low byte: 115200 baud
    outb(COM1 + 1, 0x00); // divisor high byte
    outb(COM1 + 3, 0x03); // 8 bits, no parity, 1 stop bit (8N1)
    outb(COM1 + 2, 0xC7); // enable FIFO, clear, 14-byte threshold
    outb(COM1 + 4, 0x0B); // IRQs enabled, RTS/DSR set
}

/// Write a single character to COM1. Blocks until the transmit holding
/// register is empty (bit 5 of the Line Status Register).
pub unsafe fn putchar(c: u8) {
    while (inb(COM1 + 5) & 0x20) == 0 {}
    outb(COM1, c);
}

/// Write a null-terminated byte string to COM1.
pub unsafe fn puts(s: &[u8]) {
    for &c in s {
        if c == 0 {
            break;
        }
        putchar(c);
    }
}

/// Check if a byte is waiting to be read (bit 0 of LSR = Data Ready).
pub unsafe fn data_ready() -> bool {
    (inb(COM1 + 5) & 0x01) != 0
}

/// Read one byte from COM1 (blocks until data is available).
pub unsafe fn getchar() -> u8 {
    while !data_ready() {
        asm!("hlt");
    }
    inb(COM1)
}

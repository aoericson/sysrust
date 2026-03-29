// io.rs -- x86 port I/O helpers.
//
// x86 CPUs have a separate 64KB I/O address space (ports 0x0000-0xFFFF) used
// to communicate with hardware devices. These functions wrap the 'in' and 'out'
// CPU instructions via inline assembly.

use core::arch::asm;

/// Write a byte to an I/O port.
#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

/// Read a byte from an I/O port.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack, preserves_flags));
    val
}

/// Write a 16-bit value to an I/O port.
#[inline]
pub unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
}

/// Read a 16-bit value from an I/O port.
#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    asm!("in ax, dx", in("dx") port, out("ax") val, options(nomem, nostack, preserves_flags));
    val
}

/// Write a 32-bit value to an I/O port.
#[inline]
pub unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
}

/// Read a 32-bit value from an I/O port.
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    asm!("in eax, dx", in("dx") port, out("eax") val, options(nomem, nostack, preserves_flags));
    val
}

/// Small I/O delay (used when hardware needs time between port operations).
#[inline]
pub unsafe fn io_wait() {
    outb(0x80, 0);
}

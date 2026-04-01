#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

/// Write a byte to COM1 serial port directly (no syscall needed)
unsafe fn serial_putchar(c: u8) {
    // Wait for transmit buffer empty (port 0x3FD bit 5)
    loop {
        let status: u8;
        core::arch::asm!("in al, dx", in("dx") 0x3FDu16, out("al") status);
        if status & 0x20 != 0 { break; }
    }
    // Write byte to COM1 data port 0x3F8
    core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") c);
}

unsafe fn serial_puts(s: &[u8]) {
    for &c in s {
        serial_putchar(c);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        serial_puts(b"[ELF64: alive via serial]\n");

        // Now try int 0x80 syscall for write
        serial_puts(b"[ELF64: trying int 0x80]\n");
        core::arch::asm!(
            "push rbx",
            "mov rbx, 1",      // fd = stdout
            "mov rcx, {buf}",  // buf
            "mov rdx, 14",     // len
            "mov rax, 4",      // sys_write
            "int 0x80",
            "pop rbx",
            buf = in(reg) b"Hello syscall!\n".as_ptr() as u64,
            out("rax") _,
            out("rcx") _,
            out("rdx") _,
        );
        serial_puts(b"[ELF64: after int 0x80]\n");

        // Exit
        core::arch::asm!(
            "push rbx",
            "mov rbx, 0",
            "mov rax, 1",
            "int 0x80",
            options(noreturn),
        );
    }
}
